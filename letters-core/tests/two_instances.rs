//! RFC-0005 P5: two real `collabd` processes and two Letters replicas converge
//! through a real Durable Streams server, with no GTK.
//!
//! Each [`Shell`] does what the Letters shell does (letters/src/collab_session.rs):
//! it owns a `Shared` (the replica, `letters_core::collab`), its document and its
//! undo history, sends `suite_collab` commands to its sidecar over a pipe, answers
//! every `update` with `applied` or `rejected` after the dry run `Shared::import`
//! makes, joins through the genesis and checks it against version zero's .docx.
//! The sidecars are the real `collabd` binary (`COLLABD_BIN`, or
//! `collab/target/debug/collabd`); the server is the Node reference server 0.3.9
//! (`DCP_STREAMS_URL`, from `tools/collab-spike/scripts/streams-up.sh node`).
//!
//! A test that needs the server FAILS when `DCP_STREAMS_URL` is not set, unless
//! `DCP_SKIP_STREAMS=1` says to skip it, and then it prints that it skipped.

#![cfg(feature = "collab")]

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as Proc, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use letters_core::collab::{same_text, Shared};
use letters_core::edit::{self, apply, typing, History, MarkKey, Op};
use letters_core::model::{Document, RunStyle};
use suite_collab::{
    decode_message, encode_request, write_line, Binding, Command, ConnectConfig, Event, HeadInfo, Line, LineReader, Message, Person, PublishKind,
    Role,
};

/// How long anything may take before the test says what it was waiting for.
const PATIENCE: Duration = Duration::from_secs(90);

const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

// ------------------------------------------------------------------ environment

/// `Some(base url)` when a server is configured. Panics when it is not, unless
/// `DCP_SKIP_STREAMS=1`, in which case it says so and returns `None`.
fn server_url(test: &str) -> Option<String> {
    match std::env::var("DCP_STREAMS_URL") {
        Ok(u) if !u.is_empty() => Some(u),
        _ => {
            if std::env::var("DCP_SKIP_STREAMS").as_deref() == Ok("1") {
                eprintln!("SKIPPED {test} (DCP_SKIP_STREAMS=1): no Durable Streams server");
                None
            } else {
                panic!("DCP_STREAMS_URL is not set: start the reference server (tools/collab-spike/scripts/streams-up.sh node) or set DCP_SKIP_STREAMS=1 to skip these tests explicitly");
            }
        }
    }
}

macro_rules! require_server {
    () => {
        match server_url(module_path!()) {
            Some(u) => u,
            None => return,
        }
    };
}

fn collabd_bin() -> PathBuf {
    let p = std::env::var_os("COLLABD_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../collab/target/debug/collabd"));
    assert!(p.exists(), "collabd is not built at {}: run `cargo build -p collabd` in collab/, or set COLLABD_BIN", p.display());
    p
}

// ------------------------------------------------------------------ the sidecar process

struct Sidecar {
    name: String,
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Result<Message, String>>,
    next_req: u64,
}

impl Sidecar {
    fn spawn(name: &str, data_dir: &Path) -> Sidecar {
        let mut child = Proc::new(collabd_bin())
            .arg("--data-dir")
            .arg(data_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("cannot start collabd");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("stdout");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut lines = LineReader::new(BufReader::new(stdout));
            loop {
                match lines.read_line() {
                    Ok(Line::Text(t)) => {
                        let m = decode_message(&t).map_err(|e| format!("{e}: {t}"));
                        if tx.send(m).is_err() {
                            return;
                        }
                    }
                    Ok(Line::Eof) | Err(_) => return,
                    Ok(other) => {
                        let _ = tx.send(Err(format!("unexpected {other:?} on stdout")));
                    }
                }
            }
        });
        Sidecar { name: name.to_owned(), child, stdin, rx, next_req: 1 }
    }

    fn send(&mut self, command: Command) -> u64 {
        let req = self.next_req;
        self.next_req += 1;
        let line = encode_request(Some(req), &command);
        write_line(self.stdin.as_mut().expect("stdin open"), &line).unwrap_or_else(|e| panic!("{}: collabd stdin: {e}", self.name));
        req
    }

    fn recv(&mut self, wait: Duration) -> Option<Message> {
        match self.rx.recv_timeout(wait) {
            Ok(Ok(m)) => Some(m),
            Ok(Err(e)) => panic!("{}: undecodable line from collabd: {e}", self.name),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => panic!("{}: collabd closed its stdout (did it exit?)", self.name),
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ------------------------------------------------------------------ .docx in memory

fn docx_of(doc: &Document) -> Vec<u8> {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("share.docx");
    letters_core::docx::write(doc, &path).expect("a .docx");
    std::fs::read(&path).expect("the .docx")
}

fn read_docx(bytes: &[u8]) -> Document {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("version.docx");
    std::fs::write(&path, bytes).expect("staged");
    letters_core::docx::read(path.to_str().expect("utf-8")).expect("a readable .docx")
}

// ------------------------------------------------------------------ the shell

struct Shell {
    name: &'static str,
    sc: Sidecar,
    peer: u64,
    shared: Option<Shared>,
    doc: Option<Document>,
    history: History,
    doc_id: String,
    principal: String,
    /// Everything the sidecar said, in order.
    seen: Vec<Message>,
    /// A joiner's genesis, and the request that fetches version zero's file.
    genesis: Option<Vec<u8>>,
    fetch: Option<u64>,
    /// The newest `heads` event.
    heads: Vec<HeadInfo>,
    conflict: bool,
    /// Updates that arrived before the document was shown.
    queued: Vec<(String, Vec<u8>)>,
}

impl Shell {
    fn start(name: &'static str, data_dir: &Path, peer: u64) -> Shell {
        let mut s = Shell {
            name,
            sc: Sidecar::spawn(name, data_dir),
            peer,
            shared: None,
            doc: None,
            history: History::default(),
            doc_id: String::new(),
            principal: String::new(),
            seen: Vec::new(),
            genesis: None,
            fetch: None,
            heads: Vec::new(),
            conflict: false,
            queued: Vec::new(),
        };
        let req = s.sc.send(Command::Hello { protocol: 1, min_protocol: Some(1), client: format!("letters two_instances {name}") });
        match s.reply(req) {
            Event::Hello { protocol: 1, bindings, .. } => assert!(bindings.contains(&Binding::Streams), "{name}: this build's bindings"),
            e => panic!("{name}: {e:?}"),
        }
        s
    }

    fn connect(&mut self, server: &str) -> String {
        let req = self.sc.send(Command::Connect { binding: Binding::Streams, config: ConnectConfig { server: server.into(), ca_file: None } });
        match self.reply(req) {
            Event::Connected { principal, .. } => {
                self.principal = principal.clone();
                principal
            }
            e => panic!("{}: connect: {e:?}", self.name),
        }
    }

    /// Take one message from the sidecar (or none within `wait`) and act on it as the shell would.
    fn pump(&mut self, wait: Duration) -> bool {
        let Some(m) = self.sc.recv(wait) else { return false };
        self.handle(&m);
        self.seen.push(m);
        true
    }

    fn handle(&mut self, m: &Message) {
        match &m.event {
            Event::Genesis { doc, id, base_version, bytes, .. } => {
                if self.genesis.is_none() {
                    self.genesis = Some(bytes.clone());
                    self.fetch = Some(self.sc.send(Command::FetchVersion { doc: doc.clone(), id: base_version.clone() }));
                }
                self.sc.send(Command::Applied { doc: doc.clone(), id: id.clone() });
            }
            Event::VersionFile { bytes, .. } if m.req.is_some() && m.req == self.fetch => {
                let base = read_docx(bytes);
                let genesis = self.genesis.take().expect("the genesis came first");
                let (shared, document) = Shared::join(self.peer, &genesis, &base).unwrap_or_else(|e| panic!("{}: the genesis does not load: {e}", self.name));
                same_text(&document, &base).unwrap_or_else(|e| panic!("{}: the join does not match version zero: {e}", self.name));
                self.shared = Some(shared);
                self.doc = Some(document);
                for (id, bytes) in std::mem::take(&mut self.queued) {
                    self.take(&id, &bytes);
                }
            }
            Event::Update { id, bytes, .. } => {
                if self.shared.is_none() {
                    self.queued.push((id.clone(), bytes.clone()));
                } else {
                    self.take(id, bytes);
                }
            }
            Event::Heads { heads, conflict, .. } => {
                self.heads = heads.clone();
                self.conflict = *conflict;
            }
            _ => {}
        }
    }

    /// A peer's update: the dry run, the history rebased, the document shown, the answer.
    fn take(&mut self, id: &str, bytes: &[u8]) {
        let (shared, doc) = (self.shared.as_mut().expect("a replica"), self.doc.as_mut().expect("a document"));
        match shared.import(bytes, doc) {
            Ok(remote) => {
                remote.report.rebase(&mut self.history);
                if let Some(document) = remote.document {
                    *doc = document;
                }
                let unsent = remote.unsent;
                self.sc.send(Command::Applied { doc: self.doc_id.clone(), id: id.to_owned() });
                if let Some(bytes) = unsent {
                    self.sc.send(Command::PublishUpdate { doc: self.doc_id.clone(), bytes });
                }
            }
            Err(e) => {
                self.sc.send(Command::Rejected { doc: self.doc_id.clone(), id: id.to_owned(), why: e.to_string() });
            }
        }
    }

    fn reply(&mut self, req: u64) -> Event {
        let end = Instant::now() + PATIENCE;
        loop {
            if let Some(m) = self.seen.iter().find(|m| m.req == Some(req)) {
                return m.event.clone();
            }
            assert!(Instant::now() < end, "{}: no answer to request {req}", self.name);
            self.pump(Duration::from_millis(50));
        }
    }

    fn until(&mut self, what: &str, done: impl Fn(&Shell) -> bool) {
        let end = Instant::now() + PATIENCE;
        while !done(self) {
            assert!(Instant::now() < end, "{}: timed out waiting for {what}; saw {:#?}", self.name, self.seen.iter().map(|m| &m.event).collect::<Vec<_>>());
            self.pump(Duration::from_millis(50));
        }
    }

    /// Make this shell the sharer of `doc`; the document's .docx is version zero.
    fn share(&mut self, doc: Document, people: Vec<Person>) -> String {
        let (shared, genesis) = Shared::start(self.peer, &doc).expect("a document this encoding carries");
        let req = self.sc.send(Command::Share {
            title: "Letter".into(),
            mime: DOCX.into(),
            first_version: docx_of(&doc),
            genesis: Some(genesis),
            doc_model: "letters/1".into(),
            people,
        });
        self.shared = Some(shared);
        self.doc = Some(doc);
        match self.reply(req) {
            Event::Shared { doc, invite, .. } => {
                self.doc_id = doc;
                invite
            }
            e => panic!("{}: share: {e:?}", self.name),
        }
    }

    fn join(&mut self, invite: &str) {
        let req = self.sc.send(Command::Join { invite: invite.into() });
        match self.reply(req) {
            Event::Joined { doc, role, .. } => {
                assert!(role >= Role::Editor, "{}: joined as {role:?}", self.name);
                self.doc_id = doc;
            }
            e => panic!("{}: join: {e:?}", self.name),
        }
        self.until("the genesis and version zero", |s| s.shared.is_some());
    }

    fn text(&self) -> String {
        edit::sequence_text(self.doc.as_ref().expect("a document"))
    }

    /// One local action: recorded in the replica, applied, put on the history, and published.
    fn act(&mut self, ops: &[Op]) {
        let (shared, doc) = (self.shared.as_mut().expect("a replica"), self.doc.as_mut().expect("a document"));
        shared.record(doc, ops).expect("recorded");
        let mut inverse = Vec::new();
        for op in ops {
            inverse.push(apply(doc, op).expect("applies"));
        }
        self.history.record(inverse.into_iter().rev().flatten().collect());
        let delta = shared.outgoing().expect("a delta").expect("something to send");
        self.sc.send(Command::PublishUpdate { doc: self.doc_id.clone(), bytes: delta });
    }

    /// Undo this person's last step, as the shell does: on the document, then
    /// recorded in the replica as ordinary ops and published.
    fn undo(&mut self) {
        let (shared, doc) = (self.shared.as_mut().expect("a replica"), self.doc.as_mut().expect("a document"));
        let before = doc.clone();
        let ops = self.history.undo(doc).expect("a step to undo");
        shared.record(&before, &ops).expect("an undo is an ordinary action");
        let delta = shared.outgoing().expect("a delta").expect("something to send");
        self.sc.send(Command::PublishUpdate { doc: self.doc_id.clone(), bytes: delta });
    }

    fn type_at(&mut self, at: usize, text: &str) {
        let op = typing(self.doc.as_ref().expect("a document"), at, text).expect("typing");
        self.act(&[op]);
    }

    /// Save: this document as a .docx, published as a version. The version's id and parents.
    fn save(&mut self) -> (String, Vec<String>) {
        let bytes = docx_of(self.doc.as_ref().expect("a document"));
        let req = self.sc.send(Command::PublishVersion { doc: self.doc_id.clone(), bytes, message: None, resolve: Vec::new() });
        match self.reply(req) {
            Event::Published { kind: PublishKind::Version, id, parents, .. } => (id, parents),
            e => panic!("{}: save: {e:?}", self.name),
        }
    }
}

fn two_sharers(url: &str, dirs: &(tempfile::TempDir, tempfile::TempDir), doc: Document) -> (Shell, Shell) {
    let mut a = Shell::start("A", dirs.0.path(), 1001);
    let mut b = Shell::start("B", dirs.1.path(), 2002);
    a.connect(url);
    let b_key = b.connect(url);
    let invite = a.share(doc, vec![Person { principal: b_key, role: Role::Editor }]);
    b.join(&invite);
    (a, b)
}

fn dirs() -> (tempfile::TempDir, tempfile::TempDir) {
    (tempfile::tempdir().expect("a data dir"), tempfile::tempdir().expect("a data dir"))
}

// ------------------------------------------------------------------ journeys

/// The acceptance journey without the GUI: share, join, both type in one
/// paragraph, the text converges on both, bold applied by one shows on the other.
#[test]
fn two_letters_share_join_type_in_one_paragraph_and_converge() {
    let url = require_server!();
    let dirs = dirs();
    let (mut a, mut b) = two_sharers(&url, &dirs, Document::from_plain_text("Hello"));
    assert_eq!(b.text(), "Hello", "the joiner shows what the sharer shared");

    // Both type in the one paragraph before either has seen the other.
    a.type_at(5, " from A");
    b.type_at(0, "B says ");
    let want = "B says Hello from A";
    a.until("B's text", |s| s.text() == want);
    b.until("A's text", |s| s.text() == want);
    assert_eq!(a.doc.as_ref().unwrap().paragraphs, b.doc.as_ref().unwrap().paragraphs);

    // Bold applied by A over the whole paragraph shows on B.
    let end = edit::doc_len(a.doc.as_ref().unwrap());
    a.act(&[Op::Mark { start: 0, end, key: MarkKey::Bold, value: RunStyle { bold: true, ..Default::default() } }]);
    b.until("A's bold", |s| s.doc.as_ref().is_some_and(|d| d.paragraphs[0].runs.iter().all(|r| r.style.bold) && !d.paragraphs[0].runs.is_empty()));
    assert_eq!(b.text(), want);
    assert_eq!(a.doc.as_ref().unwrap().paragraphs, b.doc.as_ref().unwrap().paragraphs);

    // Undo is mine: B takes back its own typing (its step was rebased over A's
    // edits), and A's text and bold stay, on both.
    b.undo();
    a.until("B's undo", |s| s.text() == "Hello from A");
    assert_eq!(b.text(), "Hello from A");
    assert!(a.doc.as_ref().unwrap().paragraphs[0].runs.iter().all(|r| r.style.bold), "A's bold survived B's undo");
    assert_eq!(a.doc.as_ref().unwrap().paragraphs, b.doc.as_ref().unwrap().paragraphs);
}

/// Two live editors that save one after the other are one history, not a
/// spurious conflict: each version covers what its writer had applied
/// (`covers`), so the second is a child of the first and neither side shows two heads.
#[test]
fn saves_by_two_live_editors_are_not_a_conflict() {
    let url = require_server!();
    let dirs = dirs();
    let (mut a, mut b) = two_sharers(&url, &dirs, Document::from_plain_text("Hello"));
    a.type_at(5, " from A");
    b.until("A's text", |s| s.text() == "Hello from A");
    b.type_at(0, "B: ");
    a.until("B's text", |s| s.text() == "B: Hello from A");

    let (va, _) = a.save();
    // B has the update that version covers, so it holds the version's content.
    b.until("A's version, covered", |s| s.heads.iter().any(|h| h.id == va && h.covered && h.covers.is_some()));
    let (vb, parents) = b.save();
    assert_eq!(parents, vec![va.clone()], "B's save is a child of A's version, not a second head");
    // Coverage is told as it becomes true, so wait for the settled state: B's version is a head, no
    // head is uncovered and there is no conflict, on both sides.
    for shell in [&mut a, &mut b] {
        shell.until("B's version, covered, and no conflict", |s| s.heads.iter().any(|h| h.id == vb) && !s.conflict && s.heads.iter().all(|h| h.covered));
    }
    assert!(a.heads.iter().any(|h| h.id == vb && !h.mine) && b.heads.iter().any(|h| h.id == vb && h.mine), "who published which");
}
