//! What iroh itself says about the paths of every connection this node dials or accepts.
//!
//! iroh 1.x (`iroh::endpoint::Connection::paths`, `path_events`) reports, per connection, the open
//! paths (an IP path or a relay path), which one is selected for application data, and the bytes
//! each path carried. The log below is built from nothing else: a task per connection reads the
//! path event stream, after a snapshot of the paths open when the task starts. Timestamps are
//! microseconds since the Unix epoch so that logs of nodes in different processes on one machine
//! can be laid side by side.
//!
//! This is observation for the relay spike (`docs/rfc/0004-spike-relay-results.md`); nothing in the
//! binding reads it.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use iroh::endpoint::{Connection, PathEvent, WeakConnectionHandle};
use iroh::{Endpoint, TransportAddr};
use serde::Serialize;

pub fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

fn kind(a: &TransportAddr) -> &'static str {
    if a.is_ip() {
        "ip"
    } else if a.is_relay() {
        "relay"
    } else {
        "other"
    }
}

/// One connection this node dialled or accepted.
#[derive(Clone, Debug, Serialize)]
pub struct ConnRec {
    /// `Connection::stable_id`, unique in this process.
    pub id: usize,
    /// The remote endpoint id (the principal, 64 hex characters).
    pub peer: String,
    pub alpn: String,
    /// `dial` or `accept`.
    pub side: &'static str,
    /// When `Endpoint::connect` was called (for `accept`: when the connection reached us).
    pub start_us: u64,
    /// When the QUIC handshake had completed.
    pub connected_us: u64,
    pub closed_us: Option<u64>,
    pub close_reason: Option<String>,
}

/// One entry of the path timeline.
#[derive(Clone, Debug, Serialize)]
pub struct PathEv {
    pub t_us: u64,
    pub conn: usize,
    pub peer: String,
    /// `initial` (open when tracking started), `opened`, `selected`, `closed`, `lagged`.
    pub event: &'static str,
    /// `ip`, `relay` or `other`.
    pub transport: &'static str,
    pub remote: String,
    /// For `closed`: the bytes this path sent and received over its life.
    pub tx_bytes: Option<u64>,
    pub rx_bytes: Option<u64>,
}

/// A path that is open now.
#[derive(Clone, Debug, Serialize)]
pub struct LivePath {
    pub conn: usize,
    pub peer: String,
    pub alpn: String,
    pub side: &'static str,
    pub transport: &'static str,
    pub remote: String,
    pub selected: bool,
    pub rtt_us: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

#[derive(Default)]
pub struct PathLog {
    pub conns: Vec<ConnRec>,
    pub events: Vec<PathEv>,
    handles: Vec<(usize, WeakConnectionHandle)>,
}

/// The answer to `P2pStore::net_report`.
#[derive(Clone, Debug, Serialize)]
pub struct NetReport {
    pub now_us: u64,
    pub conns: Vec<ConnRec>,
    pub events: Vec<PathEv>,
    pub live: Vec<LivePath>,
    /// iroh's own socket counters (`Endpoint::metrics().socket`).
    pub metrics: BTreeMap<&'static str, u64>,
}

pub type SharedLog = Rc<RefCell<PathLog>>;

/// Start following `conn`. Call it right after the handshake.
pub fn track(log: &SharedLog, conn: &Connection, side: &'static str, start_us: u64) {
    let id = conn.stable_id();
    let peer = hex::encode(conn.remote_id().as_bytes());
    let alpn = String::from_utf8_lossy(conn.alpn()).into_owned();
    let connected_us = now_us();
    {
        let mut l = log.borrow_mut();
        l.conns.push(ConnRec {
            id,
            peer: peer.clone(),
            alpn,
            side,
            start_us,
            connected_us,
            closed_us: None,
            close_reason: None,
        });
        l.handles.push((id, conn.weak_handle()));
    }
    // Subscribe first, then read the snapshot: a change between the two is seen by the stream.
    let mut events = Box::pin(conn.path_events());
    for p in conn.paths().iter() {
        let st = p.stats();
        let mut l = log.borrow_mut();
        l.events.push(PathEv {
            t_us: connected_us,
            conn: id,
            peer: peer.clone(),
            event: "initial",
            transport: kind(p.remote_addr()),
            remote: format!("{:?}", p.remote_addr()),
            tx_bytes: Some(st.udp_tx.bytes),
            rx_bytes: Some(st.udp_rx.bytes),
        });
        if p.is_selected() {
            l.events.push(PathEv {
                t_us: connected_us,
                conn: id,
                peer: peer.clone(),
                event: "selected",
                transport: kind(p.remote_addr()),
                remote: format!("{:?}", p.remote_addr()),
                tx_bytes: None,
                rx_bytes: None,
            });
        }
    }
    let conn = conn.clone();
    let log = log.clone();
    tokio::task::spawn_local(async move {
        while let Some(ev) = events.next().await {
            let t_us = now_us();
            let rec = match ev {
                PathEvent::Opened { remote_addr, .. } => PathEv {
                    t_us,
                    conn: id,
                    peer: peer.clone(),
                    event: "opened",
                    transport: kind(&remote_addr),
                    remote: format!("{remote_addr:?}"),
                    tx_bytes: None,
                    rx_bytes: None,
                },
                PathEvent::Selected { remote_addr, .. } => PathEv {
                    t_us,
                    conn: id,
                    peer: peer.clone(),
                    event: "selected",
                    transport: kind(&remote_addr),
                    remote: format!("{remote_addr:?}"),
                    tx_bytes: None,
                    rx_bytes: None,
                },
                PathEvent::Closed {
                    remote_addr,
                    last_stats,
                    ..
                } => PathEv {
                    t_us,
                    conn: id,
                    peer: peer.clone(),
                    event: "closed",
                    transport: kind(&remote_addr),
                    remote: format!("{remote_addr:?}"),
                    tx_bytes: Some(last_stats.udp_tx.bytes),
                    rx_bytes: Some(last_stats.udp_rx.bytes),
                },
                PathEvent::Lagged { missed, .. } => PathEv {
                    t_us,
                    conn: id,
                    peer: peer.clone(),
                    event: "lagged",
                    transport: "other",
                    remote: format!("missed {missed}"),
                    tx_bytes: None,
                    rx_bytes: None,
                },
                _ => continue,
            };
            log.borrow_mut().events.push(rec);
        }
        // The stream ends when the connection closes.
        let reason = conn.close_reason().map(|e| e.to_string());
        let mut l = log.borrow_mut();
        if let Some(c) = l.conns.iter_mut().find(|c| c.id == id) {
            c.closed_us = Some(now_us());
            c.close_reason = reason;
        }
        l.handles.retain(|(i, _)| *i != id);
    });
}

/// The log, the paths open now, and iroh's socket counters.
pub fn report(log: &SharedLog, endpoint: &Endpoint) -> NetReport {
    let (conns, events, handles) = {
        let l = log.borrow();
        (l.conns.clone(), l.events.clone(), l.handles.clone())
    };
    let mut live = Vec::new();
    for (id, h) in handles {
        let Some(conn) = h.upgrade() else { continue };
        let Some(rec) = conns.iter().find(|c| c.id == id) else {
            continue;
        };
        for p in conn.paths().iter() {
            let st = p.stats();
            live.push(LivePath {
                conn: id,
                peer: rec.peer.clone(),
                alpn: rec.alpn.clone(),
                side: rec.side,
                transport: kind(p.remote_addr()),
                remote: format!("{:?}", p.remote_addr()),
                selected: p.is_selected(),
                rtt_us: st.rtt.as_micros() as u64,
                tx_bytes: st.udp_tx.bytes,
                rx_bytes: st.udp_rx.bytes,
            });
        }
    }
    let m = &endpoint.metrics().socket;
    let mut metrics = BTreeMap::new();
    macro_rules! put {
        ($($f:ident),* $(,)?) => { $( metrics.insert(stringify!($f), m.$f.get()); )* };
    }
    put!(
        send_ipv4,
        send_ipv6,
        send_relay,
        recv_data_relay,
        recv_data_ipv4,
        recv_data_ipv6,
        holepunch_attempts,
        paths_direct,
        paths_relay,
        num_conns_direct,
        num_conns_opened,
        num_conns_closed,
        transport_ip_paths_added,
        transport_ip_paths_removed,
        transport_relay_paths_added,
        transport_relay_paths_removed,
        relay_home_change,
        relay_conns_success,
        relay_conns_failed,
        relay_conns_closed,
    );
    NetReport {
        now_us: now_us(),
        conns,
        events,
        live,
        metrics,
    }
}
