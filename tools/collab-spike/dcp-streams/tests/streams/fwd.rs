//! A TCP forwarder in front of the server under test, standing in for what a real network and
//! a real middlebox do: it counts connections and bytes (TLS records and handshakes
//! included, so the counts are what a firewall sees), delays traffic, stalls a connection the
//! way a lost segment's retransmission does, and closes connections that have been idle.
//!
//! It forwards bytes and understands nothing about TLS or HTTP, so it works the same in front of
//! every server and protocol version. It does **not** lose or reorder packets: a "stall" is a
//! pause of one direction of one connection, which is what TCP makes of a lost segment, as an
//! emulation and not a measurement of TCP.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

/// How an idle connection is ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kill {
    /// Orderly close (FIN) towards both ends.
    Fin,
    /// Abortive close (RST) towards both ends.
    Rst,
    /// Stop forwarding and keep both sockets open: the peers are never told.
    Blackhole,
}

#[derive(Clone, Debug)]
pub struct FwdOpts {
    pub target: SocketAddr,
    /// Close a connection that has carried no byte in either direction for this long.
    pub idle_kill: Option<(Duration, Kill)>,
    /// End a connection this long after it was opened, whatever it carries: a proxy that fails
    /// over, a NAT entry that expires, a restarted middlebox.
    pub age_kill: Option<(Duration, Kill)>,
    /// One-way delay added to every chunk, each direction.
    pub delay: Duration,
    /// `(n, d)`: about one chunk in `n` (per direction and connection) is held back for `d`
    /// before it and everything behind it is written.
    pub stall: Option<(u32, Duration)>,
}

impl FwdOpts {
    pub fn to(target: SocketAddr) -> Self {
        FwdOpts {
            target,
            idle_kill: None,
            age_kill: None,
            delay: Duration::ZERO,
            stall: None,
        }
    }
}

#[derive(Default, Debug)]
pub struct FwdStats {
    pub accepted: AtomicU64,
    /// Bytes client -> server.
    pub up: AtomicU64,
    /// Bytes server -> client.
    pub down: AtomicU64,
    /// Connections ended by the idle rule.
    pub killed: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FwdCounts {
    pub accepted: u64,
    pub up: u64,
    pub down: u64,
    pub killed: u64,
}

pub struct Fwd {
    pub addr: SocketAddr,
    stats: Arc<FwdStats>,
    task: JoinHandle<()>,
}

impl Drop for Fwd {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Fwd {
    pub async fn start(opts: FwdOpts) -> Fwd {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind forwarder");
        let addr = listener.local_addr().unwrap();
        let stats = Arc::new(FwdStats::default());
        let st = stats.clone();
        let task = tokio::spawn(async move {
            let mut seed = 0x9E37_79B9_7F4A_7C15u64;
            loop {
                let Ok((c, _)) = listener.accept().await else {
                    return;
                };
                st.accepted.fetch_add(1, Ordering::Relaxed);
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                tokio::spawn(connection(c, opts.clone(), st.clone(), seed));
            }
        });
        Fwd { addr, stats, task }
    }

    pub fn counts(&self) -> FwdCounts {
        FwdCounts {
            accepted: self.stats.accepted.load(Ordering::Relaxed),
            up: self.stats.up.load(Ordering::Relaxed),
            down: self.stats.down.load(Ordering::Relaxed),
            killed: self.stats.killed.load(Ordering::Relaxed),
        }
    }

    /// `base` with its host and port replaced by the forwarder's (`localhost` is kept as the
    /// host name of an https URL so the certificate still matches, and the forwarder's port is used).
    pub fn url_for(&self, base: &str) -> String {
        let (scheme, rest) = base.split_once("://").expect("a URL with a scheme");
        let path = rest.find('/').map_or("", |i| &rest[i..]);
        let host = if scheme == "https" { "localhost" } else { "127.0.0.1" };
        format!("{scheme}://{host}:{}{path}", self.addr.port())
    }
}

struct ConnState {
    t0: Instant,
    last_ms: AtomicU64,
    blackhole: AtomicBool,
}

impl ConnState {
    fn touch(&self) {
        self.last_ms
            .store(self.t0.elapsed().as_millis() as u64, Ordering::Relaxed);
    }
    fn idle(&self) -> Duration {
        let now = self.t0.elapsed().as_millis() as u64;
        Duration::from_millis(now.saturating_sub(self.last_ms.load(Ordering::Relaxed)))
    }
}

async fn connection(c: TcpStream, o: FwdOpts, st: Arc<FwdStats>, seed: u64) {
    let Ok(s) = TcpStream::connect(o.target).await else {
        return;
    };
    let _ = c.set_nodelay(true);
    let _ = s.set_nodelay(true);
    let state = Arc::new(ConnState {
        t0: Instant::now(),
        last_ms: AtomicU64::new(0),
        blackhole: AtomicBool::new(false),
    });
    let (cr, cw) = c.into_split();
    let (sr, sw) = s.into_split();
    let cw = Arc::new(Mutex::new(cw));
    let sw = Arc::new(Mutex::new(sw));
    let up = tokio::spawn(pump(cr, sw.clone(), true, o.clone(), st.clone(), state.clone(), seed));
    let down = tokio::spawn(pump(
        sr,
        cw.clone(),
        false,
        o.clone(),
        st.clone(),
        state.clone(),
        seed ^ 0xA5A5,
    ));
    if o.idle_kill.is_none() && o.age_kill.is_none() {
        let _ = tokio::join!(up, down);
        return;
    }
    // The watch: ends when both pumps are done or a rule fires.
    let how = loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if up.is_finished() && down.is_finished() {
            return;
        }
        if let Some((after, how)) = o.idle_kill {
            if state.idle() >= after {
                break how;
            }
        }
        if let Some((after, how)) = o.age_kill {
            if state.t0.elapsed() >= after {
                break how;
            }
        }
    };
    {
        st.killed.fetch_add(1, Ordering::Relaxed);
        match how {
            Kill::Blackhole => {
                state.blackhole.store(true, Ordering::Relaxed);
                // The pumps keep reading and throw everything away; the sockets stay open.
                let _ = tokio::join!(up, down);
            }
            Kill::Fin => {
                up.abort();
                down.abort();
                let _ = cw.lock().await.shutdown().await;
                let _ = sw.lock().await.shutdown().await;
            }
            Kill::Rst => {
                up.abort();
                down.abort();
                let _ = cw.lock().await.as_ref().set_zero_linger();
                let _ = sw.lock().await.as_ref().set_zero_linger();
            }
        }
    }
}

async fn pump(
    mut r: OwnedReadHalf,
    w: Arc<Mutex<OwnedWriteHalf>>,
    upward: bool,
    o: FwdOpts,
    st: Arc<FwdStats>,
    state: Arc<ConnState>,
    seed: u64,
) {
    let (tx, mut rx) = mpsc::unbounded_channel::<(Instant, Vec<u8>)>();
    let reader = {
        let state = state.clone();
        let st = st.clone();
        let delay = o.delay;
        async move {
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                match r.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        state.touch();
                        if state.blackhole.load(Ordering::Relaxed) {
                            continue;
                        }
                        let ctr = if upward { &st.up } else { &st.down };
                        ctr.fetch_add(n as u64, Ordering::Relaxed);
                        if tx.send((Instant::now() + delay, buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                }
            }
        }
    };
    let writer = async move {
        let mut rng = seed | 1;
        while let Some((due, chunk)) = rx.recv().await {
            tokio::time::sleep_until(tokio::time::Instant::from_std(due)).await;
            if let Some((n, d)) = o.stall {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                if (rng % u64::from(n.max(1))) == 0 {
                    tokio::time::sleep(d).await;
                }
            }
            let mut w = w.lock().await;
            if w.write_all(&chunk).await.is_err() {
                return;
            }
            state.touch();
        }
        // The reader saw EOF: pass it on.
        let _ = w.lock().await.shutdown().await;
    };
    let _ = tokio::join!(reader, writer);
}
