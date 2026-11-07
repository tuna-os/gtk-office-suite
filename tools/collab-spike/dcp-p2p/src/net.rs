//! The transport: one iroh endpoint per principal, the sync protocol and iroh-blobs on it.
//!
//! - Identity: the iroh secret key IS the principal's Ed25519 key (`SigningKey::as_bytes`), so the
//!   QUIC/TLS handshake authenticates the p2panda author and the p2panda-auth actor.
//! - Sync: a session is one QUIC bidirectional stream. The initiator writes a `Hello` naming the
//!   document, the acceptor answers with one status byte (it applies the membership rule), then both
//!   run p2panda-sync's `LogSync` protocol (Have / PreSync / Operation / Done) over the stream.
//!   p2panda-net's own LogSync driver is not used: see the results document.
//! - Files: iroh-blobs with an in-memory store, behind a provider-side gate (`acl.rs`).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::{presets, Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, SecretKey};
use iroh_blobs::provider::events::{
    AbortReason, ConnectMode, EventMask, EventSender, ObserveMode, ProviderMessage, RequestMode,
};
use iroh_blobs::store::mem::MemStore;
use iroh_blobs::BlobsProtocol;
use p2panda_core::{Hash, Operation, SigningKey, VerifyingKey};
use p2panda_store::SqliteStore;
use p2panda_sync::protocols::{LogSync, LogSyncEvent, LogSyncMessage};
use p2panda_sync::traits::Protocol;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::acl::Acl;
use crate::codec::Codec;
use crate::ext::DcpExt;
use crate::msg::LogId;

pub const SYNC_ALPN: &[u8] = b"dcp/p2p-sync/1";

/// Settings for one endpoint.
#[derive(Clone)]
pub struct NetOptions {
    /// A socket address to bind, for example `127.0.0.1:0` (loopback, no external network).
    pub bind: String,
    /// `None`: no relay at all (direct addresses only). `Some`: use these relays.
    pub relay: Option<iroh::RelayMap>,
    /// DER trust anchors for the relay's TLS certificate; empty means the embedded web roots.
    pub relay_ca_der: Vec<Vec<u8>>,
}

pub struct Net {
    pub endpoint: Endpoint,
    pub router: Router,
    pub blobs: MemStore,
}

#[derive(Debug, Clone)]
struct SyncShim {
    tx: mpsc::UnboundedSender<Connection>,
}

impl ProtocolHandler for SyncShim {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        // The replica task owns the connection from here on; this clone keeps it open.
        let _ = self.tx.send(connection);
        Ok(())
    }
}

impl Net {
    pub async fn spawn(
        key: &SigningKey,
        opts: &NetOptions,
        acl: Arc<Acl>,
        incoming: mpsc::UnboundedSender<Connection>,
    ) -> Result<Net, String> {
        let secret = SecretKey::from_bytes(key.as_bytes());
        let mut builder = Endpoint::builder(presets::Minimal).secret_key(secret);
        builder = match &opts.relay {
            None => builder.relay_mode(RelayMode::Disabled),
            Some(map) => {
                let b = builder.relay_mode(RelayMode::Custom(map.clone()));
                if !opts.relay_ca_der.is_empty() {
                    let roots = opts
                        .relay_ca_der
                        .iter()
                        .map(|d| rustls_pki_types::CertificateDer::from(d.clone()));
                    b.ca_tls_config(iroh::tls::CaTlsConfig::custom_roots(roots))
                } else {
                    #[cfg(feature = "relay-test")]
                    let b = b.ca_tls_config(iroh_relay::tls::CaTlsConfig::insecure_skip_verify());
                    b
                }
            }
        };
        let endpoint = builder
            .clear_ip_transports()
            .bind_addr(opts.bind.as_str())
            .map_err(|e| format!("bind address {}: {e}", opts.bind))?
            .bind()
            .await
            .map_err(|e| format!("binding the iroh endpoint: {e}"))?;

        let blobs = MemStore::new();
        let blobs_protocol = BlobsProtocol::new(&blobs, Some(blob_events(acl)));
        let router = Router::builder(endpoint.clone())
            .accept(SYNC_ALPN, SyncShim { tx: incoming })
            .accept(iroh_blobs::ALPN, blobs_protocol)
            .spawn();
        Ok(Net {
            endpoint,
            router,
            blobs,
        })
    }

    /// The address another node dials: the endpoint id, the bound loopback/interface sockets and
    /// the relay URL when there is one.
    pub fn addr(&self) -> EndpointAddr {
        let mut addr = EndpointAddr::new(self.endpoint.id());
        for s in self.endpoint.bound_sockets() {
            if s.port() != 0 {
                addr = addr.with_ip_addr(s);
            }
        }
        for r in self.endpoint.addr().relay_urls() {
            addr = addr.with_relay_url(r.clone());
        }
        addr
    }

    pub fn bound_port(&self) -> Option<u16> {
        self.endpoint
            .bound_sockets()
            .iter()
            .find(|s| s.is_ipv4())
            .map(SocketAddr::port)
    }
}

/// The provider-side gate of iroh-blobs. Since the 0.103.1 fix each request type follows its own
/// mode: `get` and `observe` are intercepted here, `get_many` and `push` are refused outright.
fn blob_events(acl: Arc<Acl>) -> EventSender {
    let mask = EventMask {
        connected: ConnectMode::Intercept,
        get: RequestMode::Intercept,
        get_many: RequestMode::Disabled,
        push: RequestMode::Disabled,
        observe: ObserveMode::Intercept,
        ..EventMask::DEFAULT
    };
    let (tx, mut rx) = EventSender::channel(64, mask);
    tokio::spawn(async move {
        let mut conns: HashMap<u64, EndpointId> = HashMap::new();
        while let Some(msg) = rx.recv().await {
            match msg {
                ProviderMessage::ClientConnected(msg) => {
                    let res: Result<(), AbortReason> = match msg.endpoint_id {
                        Some(id) if acl.may_connect(&id) => {
                            conns.insert(msg.connection_id, id);
                            Ok(())
                        }
                        _ => Err(AbortReason::Permission),
                    };
                    msg.tx.send(res).await.ok();
                }
                ProviderMessage::GetRequestReceived(msg) => {
                    let ok = conns.get(&msg.connection_id).is_some_and(|id| {
                        msg.request.ranges.is_blob() && acl.may_get(id, &msg.request.hash)
                    });
                    let res: Result<(), AbortReason> = if ok {
                        Ok(())
                    } else {
                        Err(AbortReason::Permission)
                    };
                    msg.tx.send(res).await.ok();
                }
                ProviderMessage::ObserveRequestReceived(msg) => {
                    msg.tx.send(Err(AbortReason::Permission)).await.ok();
                }
                _ => {}
            }
        }
    });
    tx
}

// ------------------------------------------------------------------ sync session

#[derive(Debug, Serialize, Deserialize)]
pub struct Hello {
    pub doc: String,
    /// `Some(entry id)`: not a sync session but a question: "have you applied this operation?".
    /// The answer waits (bounded) and comes back as the status byte.
    pub barrier: Option<String>,
}

pub const STATUS_OK: u8 = 0;
pub const STATUS_DENIED: u8 = 1;
const MAX_HELLO: usize = 1024;

pub async fn write_hello(send: &mut SendStream, hello: &Hello) -> Result<(), String> {
    let bytes = postcard::to_allocvec(hello).map_err(|e| e.to_string())?;
    let mut frame = Vec::with_capacity(4 + bytes.len());
    frame.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    frame.extend_from_slice(&bytes);
    send.write_all(&frame).await.map_err(|e| e.to_string())
}

pub async fn read_hello(recv: &mut RecvStream) -> Result<Hello, String> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await.map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_HELLO {
        return Err(format!("hello of {len} bytes"));
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await.map_err(|e| e.to_string())?;
    postcard::from_bytes(&buf).map_err(|e| e.to_string())
}

pub async fn write_status(send: &mut SendStream, status: u8) -> Result<(), String> {
    send.write_all(&[status]).await.map_err(|e| e.to_string())
}

pub async fn read_status(recv: &mut RecvStream) -> Result<u8, String> {
    let mut b = [0u8; 1];
    recv.read_exact(&mut b).await.map_err(|e| e.to_string())?;
    Ok(b[0])
}

#[derive(Debug, Default, Clone)]
pub struct SessionStats {
    pub received_operations: u32,
    pub received_bytes: u32,
    pub sent_operations: u32,
    pub sent_bytes: u32,
}

/// What a session produced. A session that fails part way still returns the operations it had
/// already received: each of them was decoded, and so signature-checked, one by one.
pub struct SessionOutcome {
    pub ops: Vec<Operation<DcpExt>>,
    pub stats: Option<SessionStats>,
    pub error: Option<String>,
}

/// Run p2panda-sync's `LogSync` over one stream pair and collect every operation the remote sent.
///
/// `logs` names the (author, log) pairs this side offers; the protocol compares log heights with
/// the remote's and sends what the remote lacks. The sender's stream is finished (not dropped) so
/// the last `Done` is delivered before the stream closes.
pub async fn run_session(
    store: &SqliteStore,
    logs: std::collections::BTreeMap<VerifyingKey, Vec<LogId>>,
    send: SendStream,
    recv: RecvStream,
) -> SessionOutcome {
    type Evt = LogSyncEvent<DcpExt>;
    let (event_tx, mut event_rx) = broadcast::channel::<Evt>(1 << 17);
    let protocol = LogSync::<LogId, DcpExt, SqliteStore, Evt>::new(store.clone(), logs, event_tx);
    let mut sink = FramedWrite::new(send, Codec::<LogSyncMessage<LogId>>::new());
    let mut stream = FramedRead::new(recv, Codec::<LogSyncMessage<LogId>>::new());

    let result = match tokio::time::timeout(SESSION_TIMEOUT, protocol.run(&mut sink, &mut stream)).await {
        Ok(r) => r.map_err(|e| e.to_string()),
        Err(_) => Err(format!("the session did not finish within {SESSION_TIMEOUT:?}")),
    };
    // Finish the send half before anything else so the remote reads our last message.
    let mut send = sink.into_inner();
    let _ = send.finish();

    let mut ops = Vec::new();
    let mut error = None;
    loop {
        match event_rx.try_recv() {
            Ok(LogSyncEvent::OperationReceived { operation, .. }) => ops.push(*operation),
            Ok(_) => {}
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                error = Some(format!("the session event buffer overflowed by {n} operations"));
            }
            Err(_) => break,
        }
    }
    let stats = match result {
        Ok((_dedup, metrics)) => Some(SessionStats {
            received_operations: metrics.received_operations,
            received_bytes: metrics.received_bytes,
            sent_operations: metrics.sent_operations,
            sent_bytes: metrics.sent_bytes,
        }),
        Err(e) => {
            error = Some(e);
            None
        }
    };
    SessionOutcome { ops, stats, error }
}

pub fn node_id_of(key: &VerifyingKey) -> Result<EndpointId, String> {
    EndpointId::from_bytes(key.as_bytes()).map_err(|e| e.to_string())
}

pub fn key_of(id: &EndpointId) -> Result<VerifyingKey, String> {
    VerifyingKey::from_bytes(id.as_bytes()).map_err(|e| e.to_string())
}

pub fn short(h: &Hash) -> String {
    h.to_hex()[..8].to_string()
}

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A session that has not finished by then is given up (it holds the document's read lock).
pub const SESSION_TIMEOUT: Duration = Duration::from_secs(90);
