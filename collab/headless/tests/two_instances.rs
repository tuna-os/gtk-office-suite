//! RFC-0005 PR 2: two real `collabd` processes and two tables-core `Replica`s
//! converge through a real Durable Streams server, with no GTK.
//!
//! Each [`Shell`] below does what the GTK shell will do: it owns a `Replica` and
//! the workbook, sends `suite_collab` commands to its sidecar over a pipe, answers
//! every `update` with `applied` or `rejected` after a dry run
//! (`Replica::import_updates`), and calls `rebuild` when something changed. The
//! sidecars are the real `collabd` binary (`COLLABD_BIN`, or
//! `collab/target/debug/collabd`); the server is the Node reference server 0.3.9
//! (`DCP_STREAMS_URL`, from `tools/collab-spike/scripts/streams-up.sh node`).
//!
//! A test that needs the server FAILS when `DCP_STREAMS_URL` is not set, unless
//! `DCP_SKIP_STREAMS=1` says to skip it, and then it prints that it skipped.

use std::io::{BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as Proc, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;

use parking_lot::Mutex;
use std::time::{Duration, Instant};

use dcp_core::updates::inline_payload;
use dcp_core::{DocumentStore, Payload, Update};
use dcp_streams::http::{JSON, START};
use dcp_streams::wire::Kind;
use dcp_streams::{Http, Identity, StreamsConfig, StreamsStore};
use suite_collab::{
    decode_message, encode_request, write_line, Binding, Command, ConnState, ConnectConfig,
    DropKind, ErrorCode, Event, Line, LineReader, Message, Person, ProcessClient, PublishKind,
    Role, SessionClient,
};
use tables_core::controller::{apply_all, Op, Replica, WorkbookImage, WorkbookState};

/// How long anything may take before the test says what it was waiting for.
const PATIENCE: Duration = Duration::from_secs(90);

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
                panic!(
                    "DCP_STREAMS_URL is not set: start the reference server (tools/collab-spike/scripts/streams-up.sh node) or set DCP_SKIP_STREAMS=1 to skip these tests explicitly"
                );
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
    let p = std::env::var_os("COLLABD_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/collabd")
        });
    assert!(
        p.exists(),
        "collabd is not built at {}: run `cargo build -p collabd` in collab/, or set COLLABD_BIN",
        p.display()
    );
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
        Sidecar {
            name: name.to_owned(),
            child,
            stdin,
            rx,
            next_req: 1,
        }
    }

    fn send(&mut self, command: Command) -> u64 {
        let req = self.next_req;
        self.next_req += 1;
        let line = encode_request(Some(req), &command);
        write_line(self.stdin.as_mut().expect("stdin open"), &line)
            .unwrap_or_else(|e| panic!("{}: collabd stdin: {e}", self.name));
        req
    }

    fn send_raw(&mut self, line: &str) {
        write_line(self.stdin.as_mut().expect("stdin open"), line)
            .unwrap_or_else(|e| panic!("{}: collabd stdin: {e}", self.name));
    }

    fn recv(&mut self, wait: Duration) -> Option<Message> {
        match self.rx.recv_timeout(wait) {
            Ok(Ok(m)) => Some(m),
            Ok(Err(e)) => panic!("{}: undecodable line from collabd: {e}", self.name),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                panic!("{}: collabd closed its stdout (did it exit?)", self.name)
            }
        }
    }

    /// End the conversation the way a shell does: close stdin and let it finish.
    fn close(&mut self) {
        drop(self.stdin.take());
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ------------------------------------------------------------------ the shell

/// The workbook as the document defines it: sheet ids are local to each peer.
fn image(state: &WorkbookState) -> WorkbookImage {
    let mut image = WorkbookImage::of(state);
    for sheet in &mut image.sheets {
        sheet.sheet_id = 0;
    }
    image
}

fn sample_workbook(rows: usize, cols: usize) -> WorkbookState {
    let mut state = WorkbookState::new(rows, cols).expect("a workbook");
    let sheet = state.sheets[0].borrow().sheet_id;
    let ops: Vec<Op> = [(0, 0, "1"), (1, 0, "2"), (2, 0, "=A1+A2"), (3, 1, "text")]
        .into_iter()
        .map(|(row, col, input)| Op::SetCell { sheet, row, col, input: input.into() })
        .collect();
    apply_all(&mut state, &ops).expect("sample edits");
    state
}

fn xlsx_of(state: &WorkbookState) -> Vec<u8> {
    let sheets: Vec<_> = state.sheets.iter().map(|s| s.borrow().clone()).collect();
    tables_core::io::save_sheets_to_xlsx_bytes(&sheets, Some(&state.engine)).expect("an xlsx")
}

struct Shell {
    name: &'static str,
    sc: Sidecar,
    peer: u64,
    replica: Option<Replica>,
    state: Option<WorkbookState>,
    /// What the other side is known to have: everything this replica had at its
    /// last publish or its last import. A delta is exported from it.
    last_sent: Vec<u8>,
    doc: String,
    principal: String,
    /// Everything the sidecar said, in order.
    seen: Vec<Message>,
    /// The deltas this shell published, with the request that carried each.
    published: Vec<(u64, Vec<u8>)>,
    /// The genesis bytes, once this shell has them.
    genesis: Vec<u8>,
}

impl Shell {
    fn start(name: &'static str, data_dir: &Path, peer: u64) -> Shell {
        let mut s = Shell {
            name,
            sc: Sidecar::spawn(name, data_dir),
            peer,
            replica: None,
            state: None,
            last_sent: Vec::new(),
            doc: String::new(),
            principal: String::new(),
            seen: Vec::new(),
            published: Vec::new(),
            genesis: Vec::new(),
        };
        let req = s.sc.send(Command::Hello {
            protocol: 1,
            min_protocol: Some(1),
            client: format!("two_instances {name}"),
        });
        match s.reply(req) {
            Event::Hello { protocol: 1, bindings, .. } => {
                assert_eq!(bindings, vec![Binding::Streams], "{name}: this build's bindings")
            }
            e => panic!("{name}: {e:?}"),
        }
        s
    }

    fn connect(&mut self, server: &str) -> String {
        let req = self.sc.send(Command::Connect {
            binding: Binding::Streams,
            config: ConnectConfig { server: server.into(), ca_file: None },
        });
        match self.reply(req) {
            Event::Connected { principal, .. } => {
                self.principal = principal.clone();
                principal
            }
            e => panic!("{}: connect: {e:?}", self.name),
        }
    }

    /// Take one message from the sidecar (or none within `wait`) and act on it as the
    /// shell would.
    fn pump(&mut self, wait: Duration) -> bool {
        let Some(m) = self.sc.recv(wait) else { return false };
        self.handle(&m);
        self.seen.push(m);
        true
    }

    fn handle(&mut self, m: &Message) {
        match &m.event {
            Event::Genesis { doc, id, bytes, .. } => {
                self.genesis = bytes.clone();
                let id = id.clone();
                if self.replica.is_none() {
                    let (r, s) = Replica::from_snapshot(self.peer, bytes)
                        .unwrap_or_else(|e| panic!("{}: the genesis does not load: {e}", self.name));
                    self.last_sent = r.version_bytes();
                    self.replica = Some(r);
                    self.state = Some(s);
                }
                self.sc.send(Command::Applied { doc: doc.clone(), id });
            }
            Event::Update { doc, id, bytes, .. } => {
                let r = self.replica.as_mut().expect("an update before the genesis");
                match r.import_updates(bytes) {
                    Ok(report) => {
                        if report.changed() {
                            self.state = Some(r.rebuild().expect("rebuild after an import"));
                        }
                        self.last_sent = r.version_bytes();
                        self.sc.send(Command::Applied { doc: doc.clone(), id: id.clone() });
                    }
                    Err(e) => {
                        self.sc.send(Command::Rejected {
                            doc: doc.clone(),
                            id: id.clone(),
                            why: e.to_string(),
                        });
                    }
                }
            }
            _ => {}
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
            assert!(
                Instant::now() < end,
                "{}: timed out waiting for {what}; saw {:#?}",
                self.name,
                self.seen.iter().map(|m| &m.event).collect::<Vec<_>>()
            );
            self.pump(Duration::from_millis(50));
        }
    }

    /// Make this shell the sharer: `state` is its workbook, the genesis is its snapshot.
    fn share(&mut self, state: WorkbookState, people: Vec<Person>) -> (String, String, String) {
        let xlsx = xlsx_of(&state);
        let replica = Replica::new(self.peer, &state);
        let genesis = replica.export_snapshot();
        self.last_sent = replica.version_bytes();
        self.replica = Some(replica);
        self.state = Some(state);
        self.genesis = genesis.clone();
        let req = self.sc.send(Command::Share {
            title: "Budget".into(),
            mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
            first_version: xlsx,
            genesis: Some(genesis),
            doc_model: "tables/1".into(),
            people,
        });
        match self.reply(req) {
            Event::Shared { doc, invite, base_version, .. } => {
                self.doc = doc.clone();
                (doc, invite, base_version)
            }
            e => panic!("{}: share: {e:?}", self.name),
        }
    }

    fn join(&mut self, invite: &str) -> Event {
        let req = self.sc.send(Command::Join { invite: invite.into() });
        let joined = self.reply(req);
        if let Event::Joined { doc, .. } = &joined {
            self.doc = doc.clone();
        }
        joined
    }

    fn st(&self) -> &WorkbookState {
        self.state.as_ref().expect("this shell has a workbook")
    }

    fn cell(&self, row: usize, col: usize) -> String {
        self.st().engine.input_at(0, row, col)
    }

    /// Edit with ops, record them in the replica and publish the delta. Returns
    /// the delta's size and the request that carries it.
    fn edit(&mut self, ops: Vec<Op>) -> (usize, u64) {
        let state = self.state.as_mut().expect("a workbook");
        apply_all(state, &ops).expect("the edit applies");
        let replica = self.replica.as_mut().expect("a replica");
        replica.record(state, &ops);
        self.publish()
    }

    fn set(&mut self, row: usize, col: usize, input: &str) -> (usize, u64) {
        let sheet = self.st().sheets[0].borrow().sheet_id;
        self.edit(vec![Op::SetCell { sheet, row, col, input: input.into() }])
    }

    fn publish(&mut self) -> (usize, u64) {
        let replica = self.replica.as_ref().expect("a replica");
        let delta = replica.export_since(&self.last_sent).expect("a delta");
        self.last_sent = replica.version_bytes();
        let size = delta.len();
        let req = self.sc.send(Command::PublishUpdate { doc: self.doc.clone(), bytes: delta.clone() });
        self.published.push((req, delta));
        (size, req)
    }

    fn published_ok(&self, req: u64) -> bool {
        self.seen.iter().any(|m| {
            m.req == Some(req) && matches!(m.event, Event::Published { kind: PublishKind::Update, .. })
        })
    }

    fn dropped(&self, kind: DropKind) -> Vec<String> {
        self.seen
            .iter()
            .filter_map(|m| match &m.event {
                Event::Dropped { kind: k, reason, .. } if *k == kind => Some(reason.clone()),
                _ => None,
            })
            .collect()
    }

    fn gaps(&self) -> Vec<(String, u64, u64)> {
        self.seen
            .iter()
            .filter_map(|m| match &m.event {
                Event::Gap { author, expected, found, .. } => {
                    Some((author.principal.clone(), *expected, *found))
                }
                _ => None,
            })
            .collect()
    }
}

/// Pump both shells until `done`, or fail saying what was awaited and what each saw.
fn settle(a: &mut Shell, b: &mut Shell, what: &str, done: impl Fn(&Shell, &Shell) -> bool) {
    let end = Instant::now() + PATIENCE;
    loop {
        a.pump(Duration::from_millis(20));
        b.pump(Duration::from_millis(20));
        if done(a, b) {
            return;
        }
        assert!(
            Instant::now() < end,
            "timed out waiting for {what}\n{}: {:#?}\n{}: {:#?}",
            a.name,
            a.seen.iter().map(|m| &m.event).collect::<Vec<_>>(),
            b.name,
            b.seen.iter().map(|m| &m.event).collect::<Vec<_>>()
        );
    }
}

fn converged(a: &Shell, b: &Shell) -> bool {
    match (&a.state, &b.state) {
        (Some(x), Some(y)) => image(x) == image(y),
        _ => false,
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().expect("a tokio runtime")
}

fn identity_in(dir: &Path) -> Identity {
    let text = std::fs::read_to_string(dir.join("streams-identity")).expect("the sidecar's key file");
    let mut seed = [0u8; 32];
    for (i, b) in seed.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).expect("hex");
    }
    Identity::from_seed(seed)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

// ------------------------------------------------------------------ a link that can be cut

/// A TCP proxy to the server that can be switched off and on, so one sidecar can be
/// made to lose its network without stopping the server the other one is using.
struct Proxy {
    port: u16,
    blocked: Arc<AtomicBool>,
    open: Arc<Mutex<Vec<TcpStream>>>,
}

impl Proxy {
    fn start(target: SocketAddr) -> Proxy {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let port = listener.local_addr().unwrap().port();
        let blocked = Arc::new(AtomicBool::new(false));
        let open: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));
        let (b, o) = (blocked.clone(), open.clone());
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(client) = conn else { continue };
                if b.load(Ordering::SeqCst) {
                    continue; // dropped: the client sees a reset
                }
                let Ok(server) = TcpStream::connect(target) else { continue };
                for (from, to) in [
                    (client.try_clone().unwrap(), server.try_clone().unwrap()),
                    (server.try_clone().unwrap(), client.try_clone().unwrap()),
                ] {
                    std::thread::spawn(move || {
                        let (mut from, mut to) = (from, to);
                        let mut buf = [0u8; 16 * 1024];
                        while let Ok(n) = from.read(&mut buf) {
                            if n == 0 || to.write_all(&buf[..n]).is_err() {
                                break;
                            }
                        }
                        let _ = from.shutdown(Shutdown::Both);
                        let _ = to.shutdown(Shutdown::Both);
                    });
                }
                o.lock().extend([client, server]);
            }
        });
        Proxy { port, blocked, open }
    }

    fn block(&self) {
        self.blocked.store(true, Ordering::SeqCst);
        for s in self.open.lock().drain(..) {
            let _ = s.shutdown(Shutdown::Both);
        }
    }

    fn unblock(&self) {
        self.blocked.store(false, Ordering::SeqCst);
    }
}

fn address_of(url: &str) -> SocketAddr {
    let rest = url.strip_prefix("http://").expect("an http:// server");
    let authority = rest.split('/').next().unwrap();
    authority.to_socket_addrs().expect("an address").next().expect("one address")
}

fn through(url: &str, port: u16) -> String {
    let rest = url.strip_prefix("http://").unwrap();
    let path = &rest[rest.find('/').unwrap()..];
    format!("http://127.0.0.1:{port}{path}")
}

// ------------------------------------------------------------------ tests

#[test]
fn share_join_and_concurrent_edits_converge_deterministically() {
    let server = require_server!();
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut a = Shell::start("A", da.path(), 1);
    let mut b = Shell::start("B", db.path(), 2);
    a.connect(&server);
    let pb = b.connect(&server);
    assert!(pb.starts_with("ed25519:"), "{pb}");

    let (_doc, invite, base) = a.share(
        sample_workbook(40, 8),
        vec![Person { principal: pb, role: Role::Editor }],
    );
    let joined = b.join(&invite);
    match &joined {
        Event::Joined { role, title, base_version, .. } => {
            assert_eq!((*role, title.as_str()), (Role::Editor, "Budget"));
            assert_eq!(base_version.as_deref(), Some(base.as_str()), "the session base is the root version");
        }
        e => panic!("{e:?}"),
    }
    b.until("the genesis", |s| s.replica.is_some());
    assert_eq!(image(b.st()), image(a.st()), "the joiner's rebuilt workbook is the sharer's");
    assert_eq!(b.cell(2, 0), "=A1+A2");

    // A delta of one cell is small: it is an update of about a hundred bytes, inline.
    let (one_cell, req) = a.set(10, 2, "hello");
    assert!(one_cell < 300, "a one-cell delta is {one_cell} bytes");
    settle(&mut a, &mut b, "the one-cell edit", |a, b| a.published_ok(req) && b.cell(10, 2) == "hello");
    eprintln!("one-cell delta: {one_cell} bytes");

    // Concurrent edits: different cells, and one cell both write.
    a.set(2, 3, "a-only");
    a.set(7, 1, "from-a");
    b.set(5, 4, "b-only");
    b.set(7, 1, "from-b");
    settle(&mut a, &mut b, "both replicas to hold all four edits", |a, b| {
        converged(a, b) && a.cell(2, 3) == "a-only" && a.cell(5, 4) == "b-only"
    });
    assert_eq!((a.cell(2, 3), b.cell(2, 3)), ("a-only".into(), "a-only".into()));
    assert_eq!((a.cell(5, 4), b.cell(5, 4)), ("b-only".into(), "b-only".into()));
    let (winner_a, winner_b) = (a.cell(7, 1), b.cell(7, 1));
    assert_eq!(winner_a, winner_b, "the cell both wrote is the same on both");
    assert!(["from-a", "from-b"].contains(&winner_a.as_str()), "{winner_a}");
    eprintln!("the cell both wrote holds {winner_a:?} on both");

    // Determinism through the wire: a fresh replica that imports the genesis and every
    // delta that crossed it, in either order, holds the same workbook.
    let converged_image = image(a.st());
    let deltas = |first: &Shell, second: &Shell| -> Vec<Vec<u8>> {
        first.published.iter().chain(second.published.iter()).map(|(_, d)| d.clone()).collect()
    };
    for (label, order) in [("A then B", deltas(&a, &b)), ("B then A", deltas(&b, &a))] {
        let (mut fresh, _) = Replica::from_snapshot(99, &a.genesis).unwrap();
        for d in &order {
            fresh.import_updates(d).unwrap();
        }
        assert_eq!(image(&fresh.rebuild().unwrap()), converged_image, "a replay in the order {label}");
    }

    // Nothing went wrong on the way.
    for s in [&a, &b] {
        assert!(s.gaps().is_empty(), "{}: {:?}", s.name, s.gaps());
        for kind in [DropKind::Record, DropKind::Update, DropKind::Version] {
            assert!(s.dropped(kind).is_empty(), "{}: {kind:?} {:?}", s.name, s.dropped(kind));
        }
    }
    a.sc.close();
    b.sc.close();
}

#[test]
fn an_offline_gap_catches_up_in_order_and_what_was_typed_offline_is_not_lost() {
    let server = require_server!();
    let proxy = Proxy::start(address_of(&server));
    let via_proxy = through(&server, proxy.port);
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut a = Shell::start("A", da.path(), 1);
    let mut b = Shell::start("B", db.path(), 2);
    a.connect(&server);
    let pb = b.connect(&via_proxy);
    let (_, invite, _) = a.share(
        sample_workbook(40, 8),
        vec![Person { principal: pb, role: Role::Editor }],
    );
    // Same server, reached by B through its own address.
    b.join(&invite.replace(&server, &via_proxy));
    b.until("the genesis", |s| s.replica.is_some());
    let (_, req) = a.set(0, 5, "before");
    settle(&mut a, &mut b, "the first edit", |a, b| a.published_ok(req) && b.cell(0, 5) == "before");
    b.until("B online", |s| {
        s.seen.iter().any(|m| matches!(m.event, Event::State { state: ConnState::Online, doc: Some(_) }))
    });

    // B loses the network.
    proxy.block();
    b.until("B to notice it is offline", |s| {
        s.seen
            .iter()
            .any(|m| matches!(m.event, Event::State { state: ConnState::Offline { .. }, doc: Some(_) }))
    });
    let offline_at = b.seen.len();

    // A keeps working: three edits and a saved version, which B cannot see.
    let mut last = 0;
    for (i, text) in ["one", "two", "three"].iter().enumerate() {
        last = a.set(20 + i, 0, text).1;
    }
    let req = a.sc.send(Command::PublishVersion {
        doc: a.doc.clone(),
        bytes: xlsx_of(a.st()),
        message: Some("saved while B was away".into()),
        resolve: vec![],
    });
    // B types offline too: the sidecar queues it.
    let (_, b_req) = b.set(30, 1, "typed offline");
    let end = Instant::now() + Duration::from_millis(2500);
    while Instant::now() < end {
        a.pump(Duration::from_millis(20));
        b.pump(Duration::from_millis(20));
    }
    assert!(a.published_ok(last), "A is online and its edits are in the log");
    assert!(matches!(a.reply(req), Event::Published { kind: PublishKind::Version, .. }));
    assert_eq!(b.cell(22, 0), "", "B has not seen A's edits");
    assert!(!b.published_ok(b_req), "B's offline edit has not reached the server");

    // The network returns: B reads what it missed, in order, and sends what it queued.
    proxy.unblock();
    settle(&mut a, &mut b, "B to catch up and A to get B's offline edit", |a, b| {
        converged(a, b) && b.cell(22, 0) == "three" && a.cell(30, 1) == "typed offline" && b.published_ok(b_req)
    });
    let online_again = b.seen[offline_at..]
        .iter()
        .any(|m| matches!(m.event, Event::State { state: ConnState::Online, doc: Some(_) }));
    assert!(online_again, "B reports it is online again");
    assert!(b.gaps().is_empty() && a.gaps().is_empty(), "catching up is not a gap");
    // `heads` is told once the version list and the update list of a read are both in (so that a head
    // whose update arrived with it is never shown as a conflict for the length of a read), which is
    // after the updates it came with: wait for it rather than expecting it before the cells.
    b.until("B to see the version A saved while it was away", |s| {
        s.seen.iter().any(|m| matches!(&m.event, Event::Heads { heads, .. } if heads.iter().any(|h| h.message.as_deref() == Some("saved while B was away"))))
    });
    a.sc.close();
    b.sc.close();
}

#[test]
fn a_tampered_record_is_dropped_and_reported_and_a_skipped_seq_is_a_gap() {
    let server = require_server!();
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut a = Shell::start("A", da.path(), 1);
    let mut b = Shell::start("B", db.path(), 2);
    a.connect(&server);
    let pb = b.connect(&server);
    // C is a member who does not behave: it is not a sidecar.
    let c_id = Identity::generate();
    let c_principal = c_id.principal();
    let (doc, invite, base) = a.share(
        sample_workbook(40, 8),
        vec![
            Person { principal: pb, role: Role::Editor },
            Person { principal: c_principal.clone(), role: Role::Editor },
        ],
    );
    b.join(&invite);
    b.until("the genesis", |s| s.replica.is_some());
    let (_, req) = a.set(1, 1, "legit");
    settle(&mut a, &mut b, "the first edit", |a, b| a.published_ok(req) && b.cell(1, 1) == "legit");

    let rt = runtime();
    let http = Http::new(&server);

    // 1. A record changed in transit: one bit of A's latest update, appended to the log.
    let (ctl, _, _) = rt.block_on(http.read_all(&format!("dcp/{doc}/ctl"), START)).unwrap();
    let prefix = ctl.last().unwrap()["prefix"].as_str().unwrap().to_owned();
    let upd_path = format!("dcp/{doc}/{prefix}/upd");
    let (items, _, _) = rt.block_on(http.read_all(&upd_path, START)).unwrap();
    let mut forged = items.last().expect("A's update is in the log").clone();
    let c = forged["c"].as_str().unwrap().to_owned();
    let flipped = format!("{}{}", if c.starts_with('A') { "B" } else { "A" }, &c[1..]);
    forged["c"] = serde_json::Value::String(flipped);
    rt.block_on(http.append(&upd_path, JSON, serde_json::to_vec(&forged).unwrap(), None, false)).unwrap();
    settle(&mut a, &mut b, "both sidecars to report the tampered record", |a, b| {
        !a.dropped(DropKind::Record).is_empty() && !b.dropped(DropKind::Record).is_empty()
    });
    for s in [&a, &b] {
        let why = s.dropped(DropKind::Record);
        assert!(
            why.iter().any(|r| r.contains("Signature") || r.contains("Decrypt")),
            "{}: {why:?}",
            s.name
        );
    }

    // 2. C writes an update that is not Loro (the application's dry run refuses it), then
    //    one that skips a seq (the receiver stops that stream and says so).
    let cs = StreamsStore::new(Http::new(&server), c_id, StreamsConfig::new("hostile"));
    let update = |seq: u64, bytes: &[u8]| Update {
        dcp: 1,
        algorithm: "loro/1".into(),
        doc_model: "tables/1".into(),
        base_version: base.clone(),
        seq,
        payload: inline_payload(bytes).unwrap(),
    };
    rt.block_on(cs.publish_update(&doc, update(0, b"this is not a loro update"))).unwrap();
    rt.block_on(cs.publish_update(&doc, update(2, b"seq 1 never existed"))).unwrap();
    settle(&mut a, &mut b, "the shell's refusal and the gap on both sides", |a, b| {
        !a.gaps().is_empty() && !b.gaps().is_empty() && !a.dropped(DropKind::Update).is_empty() && !b.dropped(DropKind::Update).is_empty()
    });
    for s in [&a, &b] {
        assert_eq!(s.gaps(), vec![(c_principal.clone(), 1, 2)], "{}: the gap is C's seq 1", s.name);
        assert!(
            s.dropped(DropKind::Update).iter().any(|r| r.contains("rejected by the application")),
            "{}: {:?}",
            s.name,
            s.dropped(DropKind::Update)
        );
    }
    assert_eq!(image(a.st()), image(b.st()), "nothing of C reached either workbook");

    // 3. The document goes on working.
    let (_, req) = a.set(3, 3, "still works");
    settle(&mut a, &mut b, "an edit after all of that", |a, b| a.published_ok(req) && b.cell(3, 3) == "still works" && converged(a, b));
    a.sc.close();
    b.sc.close();
}

#[test]
fn a_viewer_cannot_publish_and_what_a_viewer_forges_is_dropped_by_the_readers() {
    let server = require_server!();
    let (da, dv) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut a = Shell::start("A", da.path(), 1);
    let mut v = Shell::start("V", dv.path(), 2);
    a.connect(&server);
    let pv = v.connect(&server);
    let (doc, invite, base) = a.share(
        sample_workbook(40, 8),
        vec![Person { principal: pv.clone(), role: Role::Viewer }],
    );
    match v.join(&invite) {
        Event::Joined { role: Role::Viewer, .. } => {}
        e => panic!("{e:?}"),
    }
    v.until("the genesis", |s| s.replica.is_some());
    assert_eq!(image(v.st()), image(a.st()), "a viewer reads");

    // The sidecar refuses the shell's attempt, both kinds.
    let (_, req) = v.set(0, 7, "viewer edit");
    assert!(matches!(
        v.reply(req),
        Event::Error { code: ErrorCode::Forbidden, .. }
    ));
    let req = v.sc.send(Command::PublishVersion {
        doc: doc.clone(),
        bytes: xlsx_of(v.st()),
        message: None,
        resolve: vec![],
    });
    assert!(matches!(v.reply(req), Event::Error { code: ErrorCode::Forbidden, .. }));

    // A client that is not the sidecar, with the viewer's own key: the store refuses it at
    // the writer ...
    let rt = runtime();
    let key = identity_in(dv.path());
    assert_eq!(key.principal(), pv, "the key file is the principal the sidecar reported");
    let raw = StreamsStore::new(Http::new(&server), key, StreamsConfig::new("viewer-raw"));
    let update = Update {
        dcp: 1,
        algorithm: "loro/1".into(),
        doc_model: "tables/1".into(),
        base_version: base,
        seq: 0,
        payload: inline_payload(b"forged by a viewer").unwrap(),
    };
    let refused = rt.block_on(raw.publish_update(&doc, update.clone()));
    assert!(matches!(refused, Err(dcp_core::StoreError::Forbidden(_))), "{refused:?}");
    // ... and when it signs and sends the record anyway (the server has no access control
    // here), every reader drops it for the author's role, and says so.
    let inner = serde_json::json!({"st": now_ms(), "dev": "viewer-raw", "body": update});
    let sent = rt.block_on(raw.send_unchecked(&doc, Kind::Upd, &inner)).unwrap();
    assert!(sent.is_some(), "the server stored the forged record");
    settle(&mut a, &mut v, "A and V to report the forged record", |a, v| {
        !a.dropped(DropKind::Record).is_empty() && !v.dropped(DropKind::Record).is_empty()
    });
    for s in [&a, &v] {
        assert!(
            s.dropped(DropKind::Record).iter().any(|r| r.contains("Role(Viewer)")),
            "{}: {:?}",
            s.name,
            s.dropped(DropKind::Record)
        );
        assert!(
            !s.seen.iter().any(|m| matches!(&m.event, Event::Update { author, .. } if author.device == "viewer-raw")),
            "{}: the forged update was applied",
            s.name
        );
    }
    a.sc.close();
    v.sc.close();
}

#[test]
fn a_large_delta_travels_as_a_file_and_a_second_machine_of_one_person_joins_with_the_copied_key() {
    let server = require_server!();
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut a = Shell::start("A", da.path(), 1);
    let mut b = Shell::start("B", db.path(), 2);
    a.connect(&server);
    let pb = b.connect(&server);
    let (doc, invite, _) = a.share(
        sample_workbook(100, 40),
        vec![Person { principal: pb.clone(), role: Role::Editor }],
    );
    b.join(&invite);
    b.until("the genesis", |s| s.replica.is_some());

    // Four thousand cells of incompressible text, as one paste.
    let sheet = a.st().sheets[0].borrow().sheet_id;
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let cells: Vec<(usize, usize, String)> = (0..100)
        .flat_map(|r| (0..40).map(move |c| (r, c)))
        .map(|(r, c)| {
            let mut s = String::new();
            for _ in 0..3 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                s.push_str(&format!("{x:016x}"));
            }
            (r, c, s)
        })
        .collect();
    let (size, req) = a.edit(vec![Op::SetCells { sheet, cells }]);
    assert!(size > 34_000, "the paste is {size} bytes: over the inline limit");
    settle(&mut a, &mut b, "the big paste", |a, b| a.published_ok(req) && converged(a, b) && b.cell(99, 39).len() == 48);

    // In the log it is a file payload (the genesis too), read with B's key from outside the sidecar.
    let rt = runtime();
    let raw = StreamsStore::new(Http::new(&server), identity_in(db.path()), StreamsConfig::new("probe"));
    let log = rt.block_on(raw.updates(&doc)).unwrap();
    let by_seq = |seq: u64| log.iter().find(|u| u.body.seq == seq && u.sender == a.principal).unwrap();
    assert!(matches!(by_seq(0).body.payload, Payload::File { .. }), "the genesis is always a file");
    assert!(matches!(by_seq(1).body.payload, Payload::File { .. }), "{size} bytes is a file payload");

    // The same person on a second machine: the key file is copied, the device is new. It is
    // the same principal, so it is on the roster already, and it joins from the log.
    let dc = tempfile::tempdir().unwrap();
    std::fs::copy(db.path().join("streams-identity"), dc.path().join("streams-identity")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dc.path().join("streams-identity"), std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut b2 = Shell::start("B2", dc.path(), 3);
    assert_eq!(b2.connect(&server), pb, "a copied key is the same principal");
    b2.join(&invite);
    b2.until("the genesis and the paste", |s| s.state.is_some() && s.cell(99, 39).len() == 48);
    assert_eq!(image(b2.st()), image(a.st()), "the second machine holds the whole workbook");
    a.sc.close();
    b.sc.close();
    b2.sc.close();
}

#[test]
fn the_process_client_speaks_to_the_real_binary_and_a_bad_line_does_not_end_it() {
    // No server needed: everything here is refused before the network.
    let dir = tempfile::tempdir().unwrap();
    let mut child = Proc::new(collabd_bin())
        .arg("--data-dir")
        .arg(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("cannot start collabd");
    let pid = child.id();
    let (stdin, stdout) = (child.stdin.take().unwrap(), child.stdout.take().unwrap());
    // A hung sidecar must not hang the job.
    let done = Arc::new(AtomicBool::new(false));
    let watchdog = done.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        if !watchdog.load(Ordering::SeqCst) {
            let _ = Proc::new("kill").arg("-9").arg(pid.to_string()).status();
        }
    });

    let mut client = ProcessClient::new(stdout, stdin);
    let n = client.handshake("two_instances").expect("the handshake");
    assert_eq!(n.protocol, 1);
    assert_eq!(n.bindings, vec![Binding::Streams], "Matrix is not in this build");
    assert!(n.server.starts_with("collabd "), "{}", n.server);

    // The answer to a command carries its request number; a `state` event before it
    // (connecting, ...) does not and is not the answer.
    let ask = |client: &mut ProcessClient<_, _>, c: Command| -> Message {
        let req = client.send(&c).unwrap();
        loop {
            let m = client.recv().unwrap().expect("an answer");
            if m.req == Some(req) {
                return m;
            }
            assert_eq!(m.req, None, "an answer to another request: {m:?}");
        }
    };
    let m = ask(&mut client, Command::Connect {
        binding: Binding::Matrix,
        config: ConnectConfig { server: "https://matrix.example".into(), ca_file: None },
    });
    assert!(matches!(m.event, Event::Error { code: ErrorCode::UnsupportedBinding, .. }), "{m:?}");
    let m = ask(&mut client, Command::Connect {
        binding: Binding::Streams,
        config: ConnectConfig { server: "".into(), ca_file: None },
    });
    assert!(matches!(m.event, Event::Error { code: ErrorCode::UnconfiguredBinding, .. }), "{m:?}");
    let m = ask(&mut client, Command::Connect {
        binding: Binding::Streams,
        config: ConnectConfig { server: "http://example.org/v1/stream/".into(), ca_file: None },
    });
    assert!(matches!(m.event, Event::Error { code: ErrorCode::Invalid, .. }), "plain http to a remote host: {m:?}");
    let m = ask(&mut client, Command::Join { invite: "streams:http://127.0.0.1:1/#d".into() });
    assert!(matches!(m.event, Event::Error { code: ErrorCode::NotConnected, .. }), "{m:?}");
    // Nothing was stored: nothing was connected.
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "no key is made until a binding connects");

    done.store(true, Ordering::SeqCst);
    drop(client);
    let _ = child.wait();

    // Bad lines on the raw pipe: errors, never an exit.
    let mut raw = Sidecar::spawn("raw", dir.path());
    for line in ["this is not json", r#"{"cmd":"selfdestruct"}"#, r#"{"cmd":"join"}"#, "[1,2,3]"] {
        raw.send_raw(line);
    }
    let mut codes = Vec::new();
    while codes.len() < 4 {
        let m = raw.recv(Duration::from_secs(20)).expect("an error event per bad line");
        match m.event {
            Event::Error { code, .. } => codes.push(code),
            e => panic!("{e:?}"),
        }
    }
    assert_eq!(
        codes,
        [ErrorCode::MalformedLine, ErrorCode::UnknownCommand, ErrorCode::MalformedLine, ErrorCode::MalformedLine]
    );
    let req = raw.send(Command::Hello { protocol: 1, min_protocol: None, client: "after".into() });
    let m = raw.recv(Duration::from_secs(20)).unwrap();
    assert_eq!(m.req, Some(req), "the sidecar still answers");
    raw.close();
}
