//! The command loop: `hello`, `connect`, `share`, `join`, and the commands that
//! belong to an open document, which it forwards to that document's task.
//!
//! The daemon owns no document state beyond "which documents are open"; a
//! document's task (`crate::doc`) owns its receiver, outbox and caches. Share and
//! join talk to the server before they answer, so they run on this loop and a
//! slow server delays the next command; everything per-document does not.

use std::collections::HashMap;
use std::sync::Arc;

use dcp_core::updates::CrdtSession;
use dcp_core::{DocumentStore, Level, Meta, NewFile, Payload, Role as DcpRole};
use suite_collab::{
    decode_request, negotiate, Binding, Command, ConnState, ConnectConfig, DecodeError, ErrorCode,
    Event, Line, Person, Request, Role, MAX_LINE_BYTES,
};
use tokio::sync::mpsc;

use crate::connector::{code_of, ConnectError, Connector};
use crate::doc::{self, find_genesis, DocHandle, DocMsg, Start};
use crate::invite::{normalise_server, Invite};
use crate::output::Output;
use crate::{log, ALGORITHM, DOC_MODELS};

/// One line of input: a command, or the reason a line was not one.
pub enum Input {
    Request(Request),
    /// A line the reader could not turn into a command. The daemon answers with
    /// an `error` event and carries on.
    Bad { code: ErrorCode, message: String },
}

impl Input {
    /// What one line of stdin is; `None` at the end of the input.
    pub fn from_line(line: Line) -> Option<Input> {
        Some(match line {
            Line::Text(t) => match decode_request(&t) {
                Ok(r) => Input::Request(r),
                Err(DecodeError::Unknown(name)) => Input::Bad {
                    code: ErrorCode::UnknownCommand,
                    message: format!("unknown command {name:?}"),
                },
                Err(e) => Input::Bad {
                    code: ErrorCode::MalformedLine,
                    message: e.to_string(),
                },
            },
            Line::TooLong { limit } => Input::Bad {
                code: ErrorCode::LineTooLong,
                message: format!("a line over {limit} bytes was skipped"),
            },
            Line::NotUtf8 => Input::Bad {
                code: ErrorCode::MalformedLine,
                message: "the line is not UTF-8".into(),
            },
            Line::Eof => return None,
        })
    }
}

struct Conn {
    binding: Binding,
    server: String,
    store: Arc<dyn DocumentStore>,
    principal: String,
}

pub struct Daemon {
    out: Output,
    connector: Arc<dyn Connector>,
    device: String,
    protocol: Option<u32>,
    conn: Option<Conn>,
    docs: HashMap<String, DocHandle>,
}

pub fn to_dcp(r: Role) -> DcpRole {
    match r {
        Role::Viewer => DcpRole::Viewer,
        Role::Commenter => DcpRole::Commenter,
        Role::Editor => DcpRole::Editor,
        Role::Owner => DcpRole::Owner,
    }
}

pub fn from_dcp(r: DcpRole) -> Role {
    match r {
        DcpRole::Viewer => Role::Viewer,
        DcpRole::Commenter => Role::Commenter,
        DcpRole::Editor => Role::Editor,
        DcpRole::Owner => Role::Owner,
    }
}

impl Daemon {
    pub fn new(out: Output, connector: Arc<dyn Connector>, device: String) -> Self {
        Daemon {
            out,
            connector,
            device,
            protocol: None,
            conn: None,
            docs: HashMap::new(),
        }
    }

    /// Handle input until it ends, then stop every document task.
    pub async fn run(mut self, mut input: mpsc::UnboundedReceiver<Input>) {
        while let Some(i) = input.recv().await {
            match i {
                Input::Request(r) => self.handle(r).await,
                Input::Bad { code, message } => {
                    log!("bad line: {message}");
                    self.out.send(None, Event::error(code, message));
                }
            }
        }
        let docs = std::mem::take(&mut self.docs);
        for (_, h) in docs {
            drop(h.tx);
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), h.task).await;
        }
    }

    fn reply(&self, req: Option<u64>, event: Event) {
        self.out.send(req, event);
    }

    fn fail(&self, req: Option<u64>, code: ErrorCode, message: impl Into<String>) {
        self.out.send(req, Event::error(code, message));
    }

    pub async fn handle(&mut self, r: Request) {
        let Request { req, command } = r;
        if self.protocol.is_none() && !matches!(command, Command::Hello { .. }) {
            return self.fail(
                req,
                ErrorCode::NotConnected,
                format!("send `hello` before `{}`", command.name()),
            );
        }
        match command {
            Command::Hello {
                protocol,
                min_protocol,
                client,
            } => {
                match negotiate(protocol, min_protocol.unwrap_or(protocol)) {
                    Some(v) => {
                        log!("hello from {client}, protocol {v}");
                        self.protocol = Some(v);
                        self.reply(
                            req,
                            Event::Hello {
                                protocol: v,
                                server: format!("collabd {}", env!("CARGO_PKG_VERSION")),
                                bindings: self.connector.bindings(),
                                max_line: MAX_LINE_BYTES,
                            },
                        );
                    }
                    None => self.fail(
                        req,
                        ErrorCode::UnsupportedProtocol,
                        format!(
                            "this sidecar speaks protocol {} to {}; the client speaks {} to {}",
                            suite_collab::MIN_PROTOCOL_VERSION,
                            suite_collab::PROTOCOL_VERSION,
                            min_protocol.unwrap_or(protocol),
                            protocol
                        ),
                    ),
                }
            }
            Command::Connect { binding, config } => self.connect(req, binding, config).await,
            Command::Share {
                title,
                mime,
                first_version,
                genesis,
                doc_model,
                people,
            } => {
                self.share(req, title, mime, first_version, genesis, doc_model, people)
                    .await
            }
            Command::Join { invite } => self.join(req, &invite).await,
            Command::PublishUpdate { doc, bytes } => {
                self.to_doc(req, &doc, DocMsg::PublishUpdate { req, bytes })
            }
            Command::PublishVersion {
                doc,
                bytes,
                message,
                resolve,
            } => self.to_doc(
                req,
                &doc,
                DocMsg::PublishVersion {
                    req,
                    bytes,
                    message,
                    resolve,
                },
            ),
            Command::FetchVersion { doc, id } => {
                self.to_doc(req, &doc, DocMsg::FetchVersion { req, id })
            }
            Command::Applied { doc, id } => self.to_doc(req, &doc, DocMsg::Applied { id }),
            Command::Rejected { doc, id, why } => {
                self.to_doc(req, &doc, DocMsg::Rejected { id, why })
            }
            Command::Resync { doc } => self.to_doc(req, &doc, DocMsg::Resync { req }),
            Command::Leave { doc } => {
                if self.try_doc(req, &doc, DocMsg::Leave { req }) {
                    self.docs.remove(&doc);
                }
            }
            Command::Grant {
                doc,
                principal,
                role,
            } => {
                let Some(store) = self.store(req) else { return };
                match store.grant(&doc, &principal, to_dcp(role)).await {
                    Ok(()) => self.reply(req, Event::Done { doc: Some(doc) }),
                    Err(e) => self.fail(req, code_of(&e), e.to_string()),
                }
            }
            Command::Revoke { doc, principal } => {
                let Some(store) = self.store(req) else { return };
                match store.revoke(&doc, &principal).await {
                    Ok(()) => self.reply(req, Event::Done { doc: Some(doc) }),
                    Err(e) => self.fail(req, code_of(&e), e.to_string()),
                }
            }
        }
    }

    /// Forward to an open document, or say it is not open.
    fn to_doc(&self, req: Option<u64>, doc: &str, msg: DocMsg) {
        self.try_doc(req, doc, msg);
    }

    /// [`Self::to_doc`], returning whether the document was open.
    fn try_doc(&self, req: Option<u64>, doc: &str, msg: DocMsg) -> bool {
        match self.docs.get(doc) {
            Some(h) if h.tx.send(msg).is_ok() => true,
            _ => {
                self.fail(
                    req,
                    ErrorCode::UnknownDocument,
                    format!("{doc} is not open in this session (share or join it first)"),
                );
                false
            }
        }
    }

    fn store(&self, req: Option<u64>) -> Option<Arc<dyn DocumentStore>> {
        match &self.conn {
            Some(c) => Some(c.store.clone()),
            None => {
                self.fail(req, ErrorCode::NotConnected, "send `connect` first");
                None
            }
        }
    }

    async fn connect(&mut self, req: Option<u64>, binding: Binding, config: ConnectConfig) {
        if !self.connector.bindings().contains(&binding) {
            return self.fail(
                req,
                ErrorCode::UnsupportedBinding,
                format!("this collabd build has no {} binding yet", binding.as_str()),
            );
        }
        if config.server.trim().is_empty() {
            return self.fail(
                req,
                ErrorCode::UnconfiguredBinding,
                format!("the {} binding has no server configured", binding.as_str()),
            );
        }
        self.out.send(
            None,
            Event::State {
                doc: None,
                state: ConnState::Connecting,
            },
        );
        match self.connector.connect(binding, &config, &self.device).await {
            Ok(store) => {
                let principal = store.principal();
                self.conn = Some(Conn {
                    binding,
                    server: normalise_server(&config.server),
                    store,
                    principal: principal.clone(),
                });
                self.out.send(
                    None,
                    Event::State {
                        doc: None,
                        state: ConnState::Online,
                    },
                );
                self.reply(
                    req,
                    Event::Connected {
                        binding,
                        principal,
                        device: self.device.clone(),
                    },
                );
            }
            Err(ConnectError { code, message }) => {
                self.out.send(
                    None,
                    Event::State {
                        doc: None,
                        state: if code == ErrorCode::Forbidden {
                            ConnState::AuthRequired
                        } else {
                            ConnState::Error {
                                message: message.clone(),
                            }
                        },
                    },
                );
                self.fail(req, code, message);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn share(
        &mut self,
        req: Option<u64>,
        title: String,
        mime: String,
        first_version: Vec<u8>,
        genesis: Option<Vec<u8>>,
        doc_model: String,
        people: Vec<Person>,
    ) {
        let Some(conn) = &self.conn else {
            return self.fail(req, ErrorCode::NotConnected, "send `connect` first");
        };
        let (store, binding, server, principal) = (
            conn.store.clone(),
            conn.binding,
            conn.server.clone(),
            conn.principal.clone(),
        );
        if !DOC_MODELS.contains(&doc_model.as_str()) {
            return self.fail(
                req,
                ErrorCode::Invalid,
                format!("this sidecar carries the document models {DOC_MODELS:?}, not {doc_model:?}"),
            );
        }
        // Spec 4.2 version rule 3: nothing is created from a file that does not open.
        if let Err(e) = dcp_core::integrity::check_package(&mime, &first_version) {
            return self.fail(
                req,
                ErrorCode::Invalid,
                format!("refusing to share a file that does not open: {e}"),
            );
        }
        let live = genesis.is_some();
        let meta = Meta {
            dcp: 1,
            title,
            mime: mime.clone(),
            levels: if live {
                vec![Level::L1, Level::L2]
            } else {
                vec![Level::L1]
            },
            algorithms: live.then(|| vec![ALGORITHM.to_owned()]),
            editor_services: None,
        };
        let result = async {
            let doc = store
                .create_document(
                    &meta,
                    Some(NewFile {
                        mime: mime.clone(),
                        bytes: first_version,
                    }),
                )
                .await?;
            // Everyone on the roster before the genesis, so it is written once, into
            // the epoch they will all read.
            for p in &people {
                store.grant(&doc, &p.principal, to_dcp(p.role)).await?;
            }
            let versions = store.versions(&doc).await?;
            let base = versions
                .iter()
                .find(|v| v.body.parents.is_empty())
                .map(|v| v.id.clone())
                .ok_or_else(|| dcp_core::StoreError::Invalid("the new document has no root version".into()))?;
            let mut session = None;
            let mut genesis_id = None;
            if let Some(snapshot) = genesis {
                let mut s = CrdtSession::new(ALGORITHM, doc_model.clone(), base.clone());
                // The genesis is always a file payload, however small (loro/1 profile section 4).
                let file = store
                    .put_update_file(
                        &doc,
                        NewFile {
                            mime: "application/octet-stream".into(),
                            bytes: snapshot,
                        },
                    )
                    .await?;
                let seq = s.reserve();
                let g = store
                    .publish_update(&doc, s.update_at(seq, Payload::File { file }))
                    .await?;
                genesis_id = Some(g.id);
                session = Some(s);
            }
            Ok::<_, dcp_core::StoreError>((doc, base, session, genesis_id))
        }
        .await;
        match result {
            Ok((doc, base, session, genesis_id)) => {
                self.reply(
                    req,
                    Event::Shared {
                        doc: doc.clone(),
                        invite: Invite::new(binding, &server, &doc).encode(),
                        principal: principal.clone(),
                        base_version: base.clone(),
                    },
                );
                let me = suite_collab::Author {
                    principal,
                    device: self.device.clone(),
                };
                self.open(Start {
                    doc,
                    store,
                    out: self.out.clone(),
                    me,
                    role: DcpRole::Owner,
                    session,
                    genesis_id,
                    mine: vec![base.clone()],
                    opened: Some(base),
                    mime,
                });
            }
            Err(e) => self.fail(req, code_of(&e), e.to_string()),
        }
    }

    async fn join(&mut self, req: Option<u64>, text: &str) {
        let Some(conn) = &self.conn else {
            return self.fail(req, ErrorCode::NotConnected, "send `connect` first");
        };
        let invite = match Invite::parse(text) {
            Ok(i) => i,
            Err(e) => return self.fail(req, ErrorCode::InvalidInvite, e.to_string()),
        };
        if invite.binding != conn.binding || invite.server != conn.server {
            return self.fail(
                req,
                ErrorCode::InvalidInvite,
                format!(
                    "the invite is for {} at {}, this session is connected to {} at {}",
                    invite.binding.as_str(),
                    invite.server,
                    conn.binding.as_str(),
                    conn.server
                ),
            );
        }
        let (store, principal) = (conn.store.clone(), conn.principal.clone());
        if self.docs.contains_key(&invite.doc) {
            return self.fail(req, ErrorCode::Conflict, "this document is already open");
        }
        let info = async {
            let meta = store.meta(&invite.doc).await?;
            let role = store.role(&invite.doc).await?;
            let versions = store.versions(&invite.doc).await?;
            // The base is only known once the genesis is visible; a joiner that is
            // early gets `None` here and finds it by reading (RFC-0005 8.1: a join
            // must wait for what it joins to arrive).
            let updates = store.updates(&invite.doc).await.unwrap_or_default();
            let base = find_genesis(&updates, &versions).map(|g| g.body.base_version.clone());
            Ok::<_, dcp_core::StoreError>((meta, role, base))
        }
        .await;
        match info {
            Ok((meta, role, base)) => {
                self.reply(
                    req,
                    Event::Joined {
                        doc: invite.doc.clone(),
                        title: meta.title,
                        mime: meta.mime.clone(),
                        role: from_dcp(role),
                        base_version: base,
                    },
                );
                let me = suite_collab::Author {
                    principal,
                    device: self.device.clone(),
                };
                self.open(Start {
                    doc: invite.doc,
                    store,
                    out: self.out.clone(),
                    me,
                    role,
                    session: None,
                    genesis_id: None,
                    opened: None,
                    mime: meta.mime,
                    mine: Vec::new(),
                });
            }
            Err(e) => self.fail(req, code_of(&e), e.to_string()),
        }
    }

    fn open(&mut self, start: Start) {
        let name = start.doc.clone();
        self.docs.insert(name, doc::spawn(start));
    }
}
