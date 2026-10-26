// SPDX-License-Identifier: GPL-3.0-or-later
//
// collab_session.rs — one shared document's session (RFC-0005 2.1, 2.3, 2.5).
//
// The shell never runs tokio and never links a network stack. `collabd`
// (collab/) does, and is started with `gio::Subprocess`; this talks to it in
// `suite_collab` lines over its pipes. Commands go out through a
// `ProcessClient` over the child's stdin, events come in with
// `DataInputStream::read_line_async` on the main loop: no thread and no
// channel crosses into GTK, and the tab's replica lives in its live model on
// the main thread (`live::collab`).
//
// A sharer sends `connect`, then `share` (its document saved as .docx is
// version zero, a Loro snapshot of it is the genesis) and is live at once. A
// joiner sends `connect` and `join`, is handed the genesis, builds the
// document it holds, checks its text against version zero's file (a join that
// does not match fails visibly) and opens it in a tab of its own; updates that
// arrived meanwhile are applied after that. From then on every edit, undo and
// redo of this person is recorded into the replica by the live model and comes
// here as a delta to publish, and every update of another person is dry-run
// into the replica by the live model, acknowledged, and shown in the buffer
// with the caret kept on its character.
//
// No RefCell of this struct is borrowed across a call into the live model or
// a widget: the model calls back (`publish`, `refused`) while it is borrowed,
// and those take this struct's cells.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use gtk4::prelude::*;
use gtk4::{self as gtk, gio, glib};
use letters_core::collab::{same_text, CollabError, Shared};
use letters_core::Document;
use suite_collab::{
    decode_message, Author, Binding, Command, ConnState, ConnectConfig, ErrorCode, Event, HeadInfo, Message, Person, ProcessClient, PublishKind,
    Role, SessionClient, MIN_PROTOCOL_VERSION, PROTOCOL_VERSION,
};

use crate::collab_actions::Host;
use crate::collab_logic::{connection_words, edit_refusal, invite_server, short, their_head, DOCX_MIME, DOC_MODEL};
use crate::collab_status::StatusModel;
use crate::live::{self, collab::Hooks, collab::Refusal, LiveModel};

type Reply = Box<dyn FnOnce(&Rc<Session>, Event)>;
/// What a step that talks to the sidecar says when it ends.
pub(crate) type Done<T> = Box<dyn FnOnce(&Rc<Session>, Result<T, String>)>;
type Client = ProcessClient<std::io::Empty, gio::OutputStreamWrite<gio::OutputStream>>;

/// What the Share dialog collected.
pub(crate) struct ShareForm {
    pub server: String,
    pub ca_file: Option<String>,
    pub people: Vec<String>,
}

/// What the Join dialog collected.
pub(crate) struct JoinForm {
    pub server: Option<String>,
    pub ca_file: Option<String>,
    pub code: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The sharer has asked for the document and has no answer yet.
    Sharing,
    /// The joiner has joined and has not seen the genesis.
    AwaitingGenesis,
    /// The genesis is here and is being checked against version zero's file.
    Verifying,
    Live,
}

struct Queued {
    id: String,
    author: Author,
    bytes: Vec<u8>,
}

struct DocState {
    id: String,
    title: String,
    invite: String,
    role: Role,
    phase: Phase,
    /// This person's key.
    me: String,
    /// The replica's peer id for this person in this document.
    peer: u64,
    /// The genesis, until version zero's file has come to check it against.
    genesis: Option<Vec<u8>>,
    /// Updates of others that arrived before the document was shown.
    queued: Vec<Queued>,
    /// This person's own deltas made before the document existed (a sharer's
    /// edits while `share` was being answered): sent in order once it does.
    outbox: Vec<Vec<u8>>,
    people: BTreeSet<String>,
    last_update: Option<String>,
    problem: Option<String>,
    heads: Vec<HeadInfo>,
    conflict: bool,
}

impl DocState {
    fn new(id: String, title: String, invite: String, role: Role, phase: Phase, me: String, peer: u64) -> DocState {
        DocState {
            id,
            title,
            invite,
            role,
            phase,
            people: BTreeSet::from([me.clone()]),
            me,
            peer,
            genesis: None,
            queued: Vec::new(),
            outbox: Vec::new(),
            last_update: None,
            problem: None,
            heads: Vec::new(),
            conflict: false,
        }
    }
}

struct Connected {
    principal: String,
    server: String,
    ca_file: Option<String>,
}

struct Proc {
    generation: u64,
    process: gio::Subprocess,
    stdin: gio::OutputStream,
    client: Client,
    pending: HashMap<u64, Reply>,
    connected: Option<Connected>,
    conn: ConnState,
}

/// A session: a sidecar process and, once a document is shared or joined, that
/// document's state. It is bound to a tab's buffer for a sharer from the
/// start, for a joiner once the tab exists.
pub(crate) struct Session {
    host: Weak<Host>,
    buf: RefCell<Option<gtk::TextBuffer>>,
    proc: RefCell<Option<Proc>>,
    doc: RefCell<Option<DocState>>,
    generation: Cell<u64>,
}

/// A peer id this document has never seen: a replica's peer is never reused.
fn new_peer() -> u64 {
    let peer = (u64::from(glib::random_int()) << 32) | u64::from(glib::random_int());
    peer.clamp(1, u64::MAX - 1)
}

/// Where `collabd` is: `COLLABD_BIN` (development and tests), else the
/// libexec directory a package installs it in.
fn sidecar_path() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("COLLABD_BIN").filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        return if p.is_file() { Ok(p) } else { Err(format!("{}: {}", p.display(), suite_common::i18n("COLLABD_BIN is not a file"))) };
    }
    for dir in ["/app/libexec", "/usr/libexec", "/usr/local/libexec"] {
        let p = Path::new(dir).join("tuna-collabd");
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(suite_common::i18n("The collaboration helper, tuna-collabd, is not installed."))
}

/// A path in the cache directory for `tag`, for something that can only
/// read or write a path (the .docx reader and writer, a second tab).
fn stage_path(tag: &str) -> Result<PathBuf, String> {
    let dir = glib::user_cache_dir().join("tuna-collab");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir.join(format!("{tag}-{:08x}.docx", glib::random_int())))
}

/// The document as a .docx, in memory.
pub(crate) fn docx_bytes(doc: &Document) -> Result<Vec<u8>, String> {
    let path = stage_path("share")?;
    let made = letters_core::docx::write(doc, &path).and_then(|()| std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display())));
    let _ = std::fs::remove_file(&path);
    made
}

/// The document in `bytes` (a .docx), read the way every file open reads one.
fn read_docx(bytes: &[u8], tag: &str) -> Result<Document, String> {
    let path = stage_path(tag)?;
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let read = match path.to_str() {
        Some(p) => letters_core::docx::read(p),
        None => Err("the cache path is not UTF-8".to_owned()),
    };
    let _ = std::fs::remove_file(&path);
    read
}

fn clock() -> String {
    glib::DateTime::now_local().ok().and_then(|t| t.format("%H:%M:%S").ok()).map(|s| s.to_string()).unwrap_or_default()
}

impl Session {
    pub(crate) fn new(host: &Rc<Host>) -> Rc<Session> {
        Rc::new(Session { host: Rc::downgrade(host), buf: RefCell::new(None), proc: RefCell::new(None), doc: RefCell::new(None), generation: Cell::new(0) })
    }

    /// Tie this session to the tab whose buffer is `buf`.
    pub(crate) fn bind(&self, buf: &gtk::TextBuffer) {
        *self.buf.borrow_mut() = Some(buf.clone());
    }

    pub(crate) fn buffer(&self) -> Option<gtk::TextBuffer> {
        self.buf.borrow().clone()
    }

    fn model(&self) -> Option<Rc<RefCell<LiveModel>>> {
        self.buffer().and_then(|b| live::of(&b))
    }

    pub(crate) fn has_document(&self) -> bool {
        self.doc.borrow().is_some()
    }

    /// Nothing runs and nothing is shared: the host may forget this session.
    pub(crate) fn is_idle(&self) -> bool {
        self.proc.borrow().is_none() && self.doc.borrow().is_none()
    }

    pub(crate) fn invite(&self) -> Option<String> {
        self.doc.borrow().as_ref().map(|d| d.invite.clone()).filter(|i| !i.is_empty())
    }

    pub(crate) fn conflict(&self) -> bool {
        self.doc.borrow().as_ref().is_some_and(|d| d.conflict)
    }

    fn with_doc<R>(&self, f: impl FnOnce(&mut DocState) -> R) -> Option<R> {
        self.doc.borrow_mut().as_mut().map(f)
    }

    fn toast(&self, text: &str) {
        if let Some(host) = self.host.upgrade() {
            host.toast(text);
        }
    }

    fn alert(&self, heading: &str, body: &str) {
        if let Some(host) = self.host.upgrade() {
            host.alert(heading, body);
        }
    }

    fn refresh(&self) {
        if let Some(host) = self.host.upgrade() {
            host.refresh();
        }
    }

    // ------------------------------------------------------------------ the process

    fn ensure_process(self: &Rc<Self>) -> Result<(), String> {
        if self.proc.borrow().is_some() {
            return Ok(());
        }
        let bin = sidecar_path()?;
        let process = gio::Subprocess::newv(&[bin.as_os_str()], gio::SubprocessFlags::STDIN_PIPE | gio::SubprocessFlags::STDOUT_PIPE)
            .map_err(|e| format!("{}: {e}", bin.display()))?;
        let (Some(stdin), Some(stdout)) = (process.stdin_pipe(), process.stdout_pipe()) else {
            process.force_exit();
            return Err(suite_common::i18n("The collaboration helper has no pipes."));
        };
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let mut client: Client = ProcessClient::new(std::io::empty(), stdin.clone().into_write());
        let hello = client
            .send(&Command::Hello { protocol: PROTOCOL_VERSION, min_protocol: Some(MIN_PROTOCOL_VERSION), client: format!("letters {}", env!("CARGO_PKG_VERSION")) })
            .map_err(|e| e.to_string())?;
        let mut pending: HashMap<u64, Reply> = HashMap::new();
        pending.insert(
            hello,
            Box::new(|this, event| match event {
                Event::Hello { bindings, .. } if !bindings.contains(&Binding::Streams) => {
                    this.fail_sidecar(&suite_common::i18n("The collaboration helper cannot speak to Durable Streams."));
                }
                Event::Error { message, .. } => this.fail_sidecar(&message),
                _ => {}
            }),
        );
        *self.proc.borrow_mut() = Some(Proc { generation, process, stdin, client, pending, connected: None, conn: ConnState::Connecting });
        Self::read_loop(self, generation, gio::DataInputStream::new(&stdout));
        Ok(())
    }

    fn read_loop(this: &Rc<Self>, generation: u64, input: gio::DataInputStream) {
        let weak = Rc::downgrade(this);
        let again = input.clone();
        input.read_line_async(glib::Priority::DEFAULT, None::<&gio::Cancellable>, move |line| {
            let Some(this) = weak.upgrade() else { return };
            match line {
                Ok(Some(line)) => {
                    this.on_line(generation, &line);
                    Self::read_loop(&this, generation, again);
                }
                Ok(None) => this.sidecar_ended(generation, None),
                Err(e) => this.sidecar_ended(generation, Some(e.to_string())),
            }
        });
    }

    fn on_line(self: &Rc<Self>, generation: u64, line: &[u8]) {
        if self.proc.borrow().as_ref().map(|p| p.generation) != Some(generation) {
            return;
        }
        let message = std::str::from_utf8(line).map_err(|e| e.to_string()).and_then(|text| decode_message(text).map_err(|e| e.to_string()));
        match message {
            Ok(message) => self.on_message(message),
            // A line this shell cannot read is the sidecar's to explain: it is logged, and the next line is read.
            Err(e) => eprintln!("letters: the collaboration helper sent a line that cannot be read: {e}"),
        }
    }

    fn on_message(self: &Rc<Self>, message: Message) {
        let Message { req, event } = message;
        if let Some(req) = req {
            let reply = self.proc.borrow_mut().as_mut().and_then(|p| p.pending.remove(&req));
            if let Some(reply) = reply {
                reply(self, event);
                return;
            }
        }
        self.on_event(event);
    }

    /// Write `command`; `reply` runs with the event that answers it. When the
    /// pipe is gone the reply is called at once with a transport error.
    fn send(self: &Rc<Self>, command: Command, reply: Option<Reply>) -> bool {
        let failure = {
            let mut guard = self.proc.borrow_mut();
            let Some(proc) = guard.as_mut() else {
                drop(guard);
                if let Some(reply) = reply {
                    reply(self, Event::error(ErrorCode::NotConnected, suite_common::i18n("There is no session.")));
                }
                return false;
            };
            match proc.client.send(&command) {
                Ok(req) => {
                    if let Some(reply) = reply {
                        proc.pending.insert(req, reply);
                    }
                    return true;
                }
                Err(e) => (reply, e.to_string()),
            }
        };
        let (reply, why) = failure;
        if let Some(reply) = reply {
            reply(self, Event::error(ErrorCode::Transport, why.clone()));
        }
        self.fail_sidecar(&why);
        false
    }

    /// The sidecar cannot be written to, or said something that ends the
    /// session. Deferred: this may run inside a call of the live model.
    fn fail_sidecar(self: &Rc<Self>, why: &str) {
        let (this, why) = (self.clone(), why.to_owned());
        glib::idle_add_local_once(move || {
            let had_session = this.proc.borrow().is_some() || this.has_document();
            this.end_session();
            if had_session {
                this.alert(&suite_common::i18n("Sharing Stopped"), &why);
            }
        });
    }

    fn sidecar_ended(self: &Rc<Self>, generation: u64, why: Option<String>) {
        if self.proc.borrow().as_ref().map(|p| p.generation) != Some(generation) {
            return;
        }
        let why = why.unwrap_or_else(|| suite_common::i18n("The collaboration helper stopped."));
        self.fail_sidecar(&why);
    }

    /// Close the pipe, which ends `collabd`; a helper that has not left in
    /// five seconds is killed. Every command still waiting for an answer is
    /// answered with the error it will now never get a better one for, so a
    /// dialog that waits for it does not wait forever.
    fn stop_process(self: &Rc<Self>) {
        let Some(proc) = self.proc.borrow_mut().take() else { return };
        let Proc { process, stdin, client, pending, .. } = proc;
        let _ = stdin.close(gio::Cancellable::NONE);
        drop(client);
        glib::timeout_add_seconds_local_once(5, move || process.force_exit());
        for (_, reply) in pending {
            reply(self, Event::error(ErrorCode::Transport, suite_common::i18n("The collaboration helper stopped.")));
        }
    }

    /// End the session here: the document stays as it is, editable and
    /// ordinary, and the sidecar goes.
    pub(crate) fn end_session(self: &Rc<Self>) {
        if let Some(model) = self.model() {
            if let Ok(mut m) = model.try_borrow_mut() {
                m.detach_shared();
            }
        }
        *self.doc.borrow_mut() = None;
        self.stop_process();
        self.refresh();
    }

    /// The tab or window is going: leave quietly.
    pub(crate) fn shutdown(self: &Rc<Self>) {
        if let Some(doc) = self.with_doc(|d| d.id.clone()).filter(|d| !d.is_empty()) {
            self.send(Command::Leave { doc }, None);
        }
        self.end_session();
    }

    /// A connection nobody has used (the Join dialog showed a key and was
    /// closed) ends with the dialog.
    pub(crate) fn drop_idle_connection(self: &Rc<Self>) {
        if !self.has_document() {
            self.stop_process();
            self.refresh();
        }
    }

    // ------------------------------------------------------------------ connecting

    /// Be connected to `server`, starting the sidecar if need be; `then` gets
    /// this person's key (what other people put on a roster).
    pub(crate) fn ensure_connected(self: &Rc<Self>, server: String, ca_file: Option<String>, then: Done<String>) {
        if let Err(e) = self.ensure_process() {
            return then(self, Err(e));
        }
        let already = self
            .proc
            .borrow()
            .as_ref()
            .and_then(|p| p.connected.as_ref())
            .and_then(|c| (c.server == server && c.ca_file == ca_file).then(|| c.principal.clone()));
        if let Some(principal) = already {
            return then(self, Ok(principal));
        }
        let (s, ca) = (server.clone(), ca_file.clone());
        self.send(
            Command::Connect { binding: Binding::Streams, config: ConnectConfig { server, ca_file } },
            Some(Box::new(move |this, event| match event {
                Event::Connected { principal, .. } => {
                    if let Some(proc) = this.proc.borrow_mut().as_mut() {
                        proc.connected = Some(Connected { principal: principal.clone(), server: s, ca_file: ca });
                    }
                    then(this, Ok(principal));
                }
                Event::Error { message, .. } => then(this, Err(message)),
                other => then(this, Err(format!("unexpected answer: {other:?}"))),
            })),
        );
    }

    // ------------------------------------------------------------------ sharing

    pub(crate) fn share(self: &Rc<Self>, form: ShareForm, done: Done<String>) {
        if self.has_document() {
            return done(self, Err(suite_common::i18n("This document is already shared.")));
        }
        let ShareForm { server, ca_file, people } = form;
        self.ensure_connected(
            server,
            ca_file,
            Box::new(move |this, connected| match connected {
                Err(e) => {
                    this.drop_idle_connection();
                    done(this, Err(e));
                }
                Ok(principal) => this.send_share(principal, people, done),
            }),
        );
    }

    fn document_title(&self) -> String {
        self.host
            .upgrade()
            .zip(self.buffer())
            .and_then(|(host, buf)| host.title_of(&buf))
            .map(|t| t.trim_end_matches(".docx").to_owned())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| suite_common::i18n("Untitled Document"))
    }

    fn send_share(self: &Rc<Self>, principal: String, people: Vec<String>, done: Done<String>) {
        let Some(model) = self.model() else {
            self.drop_idle_connection();
            return done(self, Err(suite_common::i18n("There is no document to share.")));
        };
        // The file and the genesis come from one document at one moment.
        let doc = model.borrow().document().clone();
        let peer = new_peer();
        let made = docx_bytes(&doc).and_then(|docx| Shared::start(peer, &doc).map(|(shared, genesis)| (docx, shared, genesis)).map_err(|e| e.to_string()));
        let (docx, shared, genesis) = match made {
            Ok(m) => m,
            Err(e) => {
                self.drop_idle_connection();
                return done(self, Err(e));
            }
        };
        let title = self.document_title();
        *self.doc.borrow_mut() = Some(DocState::new(String::new(), title.clone(), String::new(), Role::Owner, Phase::Sharing, principal, peer));
        // From now on edits are recorded; they are published once the document exists.
        model.borrow_mut().attach_shared(shared, self.hooks(false));
        self.send(
            Command::Share {
                title,
                mime: DOCX_MIME.to_owned(),
                first_version: docx,
                genesis: Some(genesis),
                doc_model: DOC_MODEL.to_owned(),
                people: people.into_iter().map(|principal| Person { principal, role: Role::Editor }).collect(),
            },
            Some(Box::new(move |this, event| match event {
                Event::Shared { doc, invite, .. } => {
                    this.with_doc(|d| {
                        d.id = doc;
                        d.invite = invite.clone();
                        d.phase = Phase::Live;
                    });
                    // What was edited while the document was being made.
                    this.flush_outbox();
                    this.refresh();
                    done(this, Ok(invite));
                }
                Event::Error { message, .. } => {
                    this.end_session();
                    done(this, Err(message));
                }
                other => {
                    this.end_session();
                    done(this, Err(format!("unexpected answer: {other:?}")));
                }
            })),
        );
        self.refresh();
    }

    /// The hooks the live model calls while it is borrowed: they only take
    /// this session's own cells.
    fn hooks(self: &Rc<Self>, read_only: bool) -> Hooks {
        let (publisher, refuser) = (Rc::downgrade(self), Rc::downgrade(self));
        Hooks {
            read_only,
            publish: Box::new(move |delta| {
                if let Some(this) = publisher.upgrade() {
                    this.publish(delta);
                }
            }),
            refused: Box::new(move |why| {
                if let Some(this) = refuser.upgrade() {
                    this.refused(why);
                }
            }),
        }
    }

    /// One local action's update: out at once, or in the outbox while the
    /// document does not exist yet.
    fn publish(self: &Rc<Self>, delta: Vec<u8>) {
        let live_doc = self.with_doc(|d| match d.phase {
            Phase::Live if d.role >= Role::Editor => Ok(d.id.clone()),
            Phase::Live => Err(false),
            _ => {
                d.outbox.push(delta.clone());
                Err(true)
            }
        });
        if let Some(Ok(doc)) = live_doc {
            self.send(Command::PublishUpdate { doc, bytes: delta }, None);
        }
    }

    fn flush_outbox(self: &Rc<Self>) {
        let Some((doc, outbox)) = self.with_doc(|d| (d.id.clone(), std::mem::take(&mut d.outbox))) else { return };
        for bytes in outbox {
            self.send(Command::PublishUpdate { doc: doc.clone(), bytes }, None);
        }
    }

    /// The live model refused an edit, or the document was replaced.
    fn refused(self: &Rc<Self>, why: &Refusal) {
        self.toast(&edit_refusal(why));
        if *why == Refusal::Replaced {
            let this = self.clone();
            glib::idle_add_local_once(move || {
                if let Some(doc) = this.with_doc(|d| d.id.clone()).filter(|d| !d.is_empty()) {
                    this.send(Command::Leave { doc }, None);
                }
                this.end_session();
            });
        }
    }

    // ------------------------------------------------------------------ joining

    pub(crate) fn join(self: &Rc<Self>, form: JoinForm, done: Done<String>) {
        if self.has_document() {
            return done(self, Err(suite_common::i18n("This session already has a document.")));
        }
        let code = crate::collab_logic::clean_code(&form.code);
        let Some(server) = form.server.filter(|s| !s.trim().is_empty()).or_else(|| invite_server(&code)) else {
            return done(self, Err(suite_common::i18n("That is not a join code.")));
        };
        self.ensure_connected(
            server,
            form.ca_file,
            Box::new(move |this, connected| match connected {
                Err(e) => {
                    this.drop_idle_connection();
                    done(this, Err(e));
                }
                Ok(principal) => {
                    let invite = code.clone();
                    this.send(
                        Command::Join { invite: code },
                        Some(Box::new(move |this, event| match event {
                            Event::Joined { doc, title, role, .. } => {
                                *this.doc.borrow_mut() = Some(DocState::new(doc, title.clone(), invite, role, Phase::AwaitingGenesis, principal, new_peer()));
                                this.refresh();
                                done(this, Ok(title));
                            }
                            Event::Error { message, .. } => {
                                this.drop_idle_connection();
                                done(this, Err(message));
                            }
                            other => {
                                this.drop_idle_connection();
                                done(this, Err(format!("unexpected answer: {other:?}")));
                            }
                        })),
                    );
                }
            }),
        );
    }

    /// The join cannot go on: say why, and leave.
    fn abort_join(self: &Rc<Self>, why: &str) {
        if let Some(doc) = self.with_doc(|d| d.id.clone()) {
            self.send(Command::Leave { doc }, None);
        }
        self.end_session();
        self.alert(&suite_common::i18n("Could Not Join"), why);
    }

    fn on_genesis(self: &Rc<Self>, doc_id: &str, id: String, base_version: String, author: Author, bytes: Vec<u8>) {
        let first = self
            .with_doc(|d| {
                if d.id != doc_id {
                    return None;
                }
                d.people.insert(author.principal.clone());
                if d.genesis.is_some() || d.phase != Phase::AwaitingGenesis {
                    return Some(false);
                }
                d.genesis = Some(bytes);
                d.phase = Phase::Verifying;
                Some(true)
            })
            .flatten();
        let Some(first) = first else { return };
        self.send(Command::Applied { doc: doc_id.to_owned(), id }, None);
        if first {
            // Version zero's file is the record the genesis is checked against.
            self.send(
                Command::FetchVersion { doc: doc_id.to_owned(), id: base_version },
                Some(Box::new(|this, event| match event {
                    Event::VersionFile { bytes, .. } => this.finish_join(&bytes),
                    Event::Error { message, .. } => this.abort_join(&message),
                    _ => {}
                })),
            );
        }
    }

    /// The first rebuild is checked against the file it is based on: the text
    /// must be equal, or the join fails visibly. Then the document opens in a
    /// tab of its own.
    fn finish_join(self: &Rc<Self>, file: &[u8]) {
        let prepared = self.with_doc(|d| (d.genesis.take(), d.peer, d.title.clone(), d.role)).and_then(|(g, p, t, r)| g.map(|g| (g, p, t, r)));
        let Some((genesis, peer, title, role)) = prepared else { return };
        let built = read_docx(file, "version-zero").and_then(|base| {
            let (shared, document) = Shared::join(peer, &genesis, &base).map_err(|e| e.to_string())?;
            same_text(&document, &base)?;
            Ok((shared, document))
        });
        let (shared, document) = match built {
            Ok(b) => b,
            Err(e) => return self.abort_join(&format!("{}: {e}", suite_common::i18n("The shared document cannot be read"))),
        };
        let Some(host) = self.host.upgrade() else { return };
        let Some(buf) = host.open_shared(&document, &title) else {
            return self.abort_join(&suite_common::i18n("The shared document could not be opened in a tab."));
        };
        self.bind(&buf);
        let Some(model) = live::of(&buf) else {
            return self.abort_join(&suite_common::i18n("The shared document could not be opened in a tab."));
        };
        model.borrow_mut().attach_shared(shared, self.hooks(role < Role::Editor));
        let queued = self
            .with_doc(|d| {
                d.phase = Phase::Live;
                std::mem::take(&mut d.queued)
            })
            .unwrap_or_default();
        for update in queued {
            self.apply_update(update.id, update.author, update.bytes);
        }
        self.refresh();
        self.toast(&suite_common::i18n("Joined “%s”").replace("%s", &title));
    }

    // ------------------------------------------------------------------ the live document

    /// One update of another person: a dry run into the replica and the
    /// buffer, then an answer for the sidecar.
    fn apply_update(self: &Rc<Self>, id: String, author: Author, bytes: Vec<u8>) {
        let queued = self.with_doc(|d| {
            if d.phase == Phase::Live {
                false
            } else {
                d.queued.push(Queued { id: id.clone(), author: author.clone(), bytes: bytes.clone() });
                true
            }
        });
        if queued != Some(false) {
            return;
        }
        let (Some(model), Some(buf)) = (self.model(), self.buffer()) else { return };
        let imported = match model.try_borrow_mut() {
            Ok(mut m) => m.import_remote(&buf, &bytes),
            Err(_) => {
                // The model is mid-edit: take the update once that is done.
                let this = self.clone();
                glib::idle_add_local_once(move || this.apply_update(id, author, bytes));
                return;
            }
        };
        let Some(doc_id) = self.with_doc(|d| d.id.clone()) else { return };
        match imported {
            Ok(imported) => {
                self.send(Command::Applied { doc: doc_id, id }, None);
                if let Some(unsent) = imported.unsent {
                    self.publish(unsent);
                }
                let at = clock();
                self.with_doc(|d| {
                    d.people.insert(author.principal.clone());
                    d.last_update = Some(format!("{} {} {}", suite_common::i18n("Last update from"), short(&author.principal), at));
                });
                if imported.changed {
                    crate::live::sync_actions(&buf);
                }
            }
            Err(e) => self.reject(doc_id, id, &e),
        }
        self.refresh();
    }

    fn reject(self: &Rc<Self>, doc: String, id: String, why: &CollabError) {
        self.send(Command::Rejected { doc, id, why: why.to_string() }, None);
        self.with_doc(|d| d.problem = Some(format!("{} {why}", suite_common::i18n("A change was refused:"))));
    }

    fn on_event(self: &Rc<Self>, event: Event) {
        match event {
            Event::State { state, .. } => {
                if let Some(proc) = self.proc.borrow_mut().as_mut() {
                    proc.conn = state;
                }
                self.refresh();
            }
            Event::Genesis { doc, id, base_version, author, bytes } => self.on_genesis(&doc, id, base_version, author, bytes),
            Event::Update { doc, id, author, bytes, .. } => {
                if self.with_doc(|d| d.id == doc) == Some(true) {
                    self.apply_update(id, author, bytes);
                }
            }
            Event::Heads { doc, heads, conflict } => {
                self.with_doc(|d| {
                    if d.id == doc {
                        d.people.extend(heads.iter().map(|h| h.author.principal.clone()));
                        d.heads = heads;
                        d.conflict = conflict;
                    }
                });
                self.refresh();
            }
            Event::Dropped { reason, .. } => {
                self.with_doc(|d| d.problem = Some(format!("{} {reason}", suite_common::i18n("A change was refused:"))));
                self.refresh();
            }
            Event::Gap { author, expected, found, .. } => {
                let why = suite_common::i18n("Updates from %1 are missing: expected number %2, found %3.")
                    .replace("%1", &short(&author.principal))
                    .replace("%2", &expected.to_string())
                    .replace("%3", &found.to_string());
                self.with_doc(|d| d.problem = Some(why));
                self.refresh();
            }
            Event::Error { message, .. } => self.toast(&message),
            // Answers nobody waits for: a published update, a left document.
            Event::Published { .. }
            | Event::Done { .. }
            | Event::Hello { .. }
            | Event::Connected { .. }
            | Event::Shared { .. }
            | Event::Joined { .. }
            | Event::VersionFile { .. } => {}
        }
    }

    // ------------------------------------------------------------------ saving and resolving

    /// A save has been committed to `path`: the file is the record, and a
    /// copy of it is published as a version. A failed publish never fails the
    /// save (RFC-0005 2.3 point 4).
    pub(crate) fn publish_saved(self: &Rc<Self>, path: &Path) {
        let doc = self.with_doc(|d| (d.phase == Phase::Live && d.role >= Role::Editor).then(|| d.id.clone())).flatten();
        let Some(doc) = doc else { return };
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("docx")) {
            return self.toast(&suite_common::i18n("Saved. Only .docx files are shared as versions."));
        }
        match std::fs::read(path) {
            Ok(bytes) => self.publish_version(doc, bytes, None, Vec::new(), suite_common::i18n("Saved and shared")),
            Err(e) => self.toast(&format!("{} {e}", suite_common::i18n("Saved, but not shared yet:"))),
        }
    }

    fn publish_version(self: &Rc<Self>, doc: String, bytes: Vec<u8>, message: Option<String>, resolve: Vec<String>, success: String) {
        self.send(
            Command::PublishVersion { doc, bytes, message, resolve },
            Some(Box::new(move |this, event| match event {
                Event::Published { kind: PublishKind::Version, .. } => this.toast(&success),
                Event::Error { message, .. } => this.toast(&format!("{} {message}", suite_common::i18n("Saved, but not shared yet:"))),
                _ => {}
            })),
        );
    }

    /// The versions, who has them and when, for the Resolve dialog.
    pub(crate) fn heads_text(&self) -> Option<String> {
        let guard = self.doc.borrow();
        let doc = guard.as_ref()?;
        let lines: Vec<String> = doc
            .heads
            .iter()
            .map(|head| {
                let who = if head.mine { suite_common::i18n("You") } else { short(&head.author.principal) };
                let when = glib::DateTime::from_unix_local((head.ts_ms / 1000) as i64)
                    .ok()
                    .and_then(|t| t.format("%H:%M").ok())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                format!("• {who}, {when}")
            })
            .collect();
        Some(lines.join("\n"))
    }

    pub(crate) fn has_other_head(&self) -> bool {
        self.with_doc(|d| their_head(&d.heads).is_some()).unwrap_or(false)
    }

    /// Keep this document: publish it as the merge of every version.
    pub(crate) fn keep_mine(self: &Rc<Self>) {
        self.publish_merge(suite_common::i18n("Kept this version"));
    }

    fn publish_merge(self: &Rc<Self>, message: String) {
        let Some((doc, heads)) = self.with_doc(|d| (d.id.clone(), d.heads.iter().map(|h| h.id.clone()).collect::<Vec<_>>())) else { return };
        let Some(model) = self.model() else { return };
        let current = model.borrow().document().clone();
        match docx_bytes(&current) {
            Ok(bytes) => self.publish_version(doc, bytes, Some(message), heads, suite_common::i18n("The versions are merged")),
            Err(e) => self.alert(&suite_common::i18n("Could Not Merge"), &e),
        }
    }

    /// Fetch the file of the other version and hand it to `then`.
    fn fetch_theirs(self: &Rc<Self>, then: impl FnOnce(&Rc<Session>, String, Vec<u8>) + 'static) {
        let Some((doc, theirs)) = self.with_doc(|d| their_head(&d.heads).map(|h| (d.id.clone(), h.id.clone()))).flatten() else { return };
        let id = theirs.clone();
        self.send(
            Command::FetchVersion { doc, id },
            Some(Box::new(move |this, event| match event {
                Event::VersionFile { bytes, .. } => then(this, theirs, bytes),
                Event::Error { message, .. } => this.alert(&suite_common::i18n("Could Not Read the Other Version"), &message),
                _ => {}
            })),
        );
    }

    /// Take the other version's contents in as one undoable edit, which the
    /// peers receive as an ordinary update, then publish the merge.
    pub(crate) fn keep_theirs(self: &Rc<Self>) {
        self.fetch_theirs(|this, _, bytes| {
            let hint = suite_common::i18n("Open both versions to compare them.");
            let failed = |this: &Rc<Session>, why: &str| this.alert(&suite_common::i18n("Could Not Keep Their Version"), &format!("{why}\n\n{hint}"));
            let (Some(model), Some(buf)) = (this.model(), this.buffer()) else { return };
            let theirs = match read_docx(&bytes, "theirs") {
                Ok(d) => d,
                Err(e) => return failed(this, &e),
            };
            let ops = letters_core::edit::diff(model.borrow().document(), &theirs);
            // A refusal (their version has a table, say) is toasted by the
            // model's hook and nothing changes; the dialog says what to do.
            let applied = ops.is_empty() || model.borrow_mut().apply_user_ops(&buf, &ops, false);
            if !applied {
                return failed(this, &suite_common::i18n("This document refused the change."));
            }
            live::sync_actions(&buf);
            this.publish_merge(suite_common::i18n("Kept the other version"));
        });
    }

    /// Open the other version in a tab of its own; nothing is merged or published.
    pub(crate) fn open_both(self: &Rc<Self>) {
        self.fetch_theirs(|this, id, bytes| {
            let tag = format!("version-{}", id.chars().take(8).collect::<String>());
            let staged = stage_path(&tag).and_then(|path| match std::fs::write(&path, &bytes) {
                Ok(()) => Ok(path),
                Err(e) => Err(format!("{}: {e}", path.display())),
            });
            match staged {
                Ok(path) => {
                    if let Some(app) = gio::Application::default() {
                        app.open(&[gio::File::for_path(path)], "");
                    }
                }
                Err(e) => this.alert(&suite_common::i18n("Could Not Open the Other Version"), &e),
            }
        });
    }

    /// Stop sharing: the document stays in its tab, no longer shared.
    pub(crate) fn leave(self: &Rc<Self>) {
        self.shutdown();
        self.toast(&suite_common::i18n("Stopped sharing"));
    }

    // ------------------------------------------------------------------ what the window shows

    /// What the status popover says, or `None` when no document is shared.
    pub(crate) fn status(&self) -> Option<StatusModel> {
        let proc = self.proc.borrow();
        let guard = self.doc.borrow();
        let doc = guard.as_ref()?;
        let conn = proc.as_ref().map_or(ConnState::Offline { since_ms: 0 }, |p| p.conn.clone());
        let words = connection_words(&conn, doc.people.len());
        let people = doc
            .people
            .iter()
            .map(|p| if *p == doc.me { format!("{} ({})", short(p), suite_common::i18n("you")) } else { short(p) })
            .collect::<Vec<_>>()
            .join("\n");
        let problem = doc.problem.clone().or_else(|| match &conn {
            ConnState::Error { message } => Some(message.clone()),
            _ => None,
        });
        Some(StatusModel { words, people, last_update: doc.last_update.clone(), problem })
    }
}

/// Called by the save path after a save has been committed: publish the file
/// just written as a version, when `buf`'s document is in a live session.
pub(crate) fn after_save(buf: &gtk::TextBuffer, path: &Path) {
    if let Some(session) = Host::session_of(buf) {
        session.publish_saved(path);
    }
}
