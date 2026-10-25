//! What does an iroh endpoint contact on its own? A probe for the relay spike
//! (`docs/rfc/0004-spike-relay-results.md`, the Flatpak question): two endpoints in one process,
//! one dials the other, and the caller captures the packets (`netns-footprint.sh`).
//!
//!   --variant n0           presets::N0: n0's relays and its DNS/pkarr address lookup. A dials B by
//!                          endpoint id alone, so the lookup has to resolve it.
//!   --variant n0-relay     Minimal preset + n0's default relay map, no address lookup. A dials B
//!                          by id and relay URL.
//!   --variant minimal      Minimal preset, no relay, no address lookup (what dcp-p2p uses with no
//!                          relay configured). A dials B by id and the IP address B is bound to.
//!   --secs N               stay up this long after the connection (default 20)
//!
//! Prints one line per step on stdout.

use std::time::{Duration, Instant};

use iroh::endpoint::{presets, Connection};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, RelayMode};

const ALPN: &[u8] = b"dcp/footprint/1";

#[derive(Debug, Clone)]
struct Echo;

impl ProtocolHandler for Echo {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let (mut send, mut recv) = connection.accept_bi().await?;
        let data = recv.read_to_end(64).await.map_err(AcceptError::from_err)?;
        send.write_all(&data).await.map_err(AcceptError::from_err)?;
        send.finish()?;
        connection.closed().await;
        Ok(())
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn say(what: &str, v: impl std::fmt::Debug) {
    println!("{what}: {v:?}");
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let variant = arg(&args, "--variant").unwrap_or_else(|| "n0".into());
    let secs: u64 = arg(&args, "--secs").and_then(|s| s.parse().ok()).unwrap_or(20);
    say("variant", &variant);

    let build = |v: &str| {
        let b = match v {
            "n0" => Endpoint::builder(presets::N0),
            "n0-relay" => Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Default),
            _ => Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled),
        };
        b.bind()
    };
    let t0 = Instant::now();
    let a = build(&variant).await.expect("endpoint a binds");
    let b = build(&variant).await.expect("endpoint b binds");
    say("bound after ms", t0.elapsed().as_millis());
    let _router = Router::builder(b.clone()).accept(ALPN, Echo).spawn();
    if variant != "minimal" {
        let t = Instant::now();
        let online_a = tokio::time::timeout(Duration::from_secs(30), a.online()).await.is_ok();
        let online_b = tokio::time::timeout(Duration::from_secs(30), b.online()).await.is_ok();
        say("online (home relay reached): a, b, ms", (online_a, online_b, t.elapsed().as_millis()));
    }
    say("a address", a.addr());
    say("b address", b.addr());

    let target = match variant.as_str() {
        "n0" => EndpointAddr::new(b.id()),
        "n0-relay" => {
            let mut t = EndpointAddr::new(b.id());
            for u in b.addr().relay_urls() {
                t = t.with_relay_url(u.clone());
            }
            t
        }
        _ => {
            let mut t = EndpointAddr::new(b.id());
            for s in b.bound_sockets() {
                if s.is_ipv4() {
                    // The unspecified address cannot be dialled: use loopback.
                    let mut s = s;
                    if s.ip().is_unspecified() {
                        s.set_ip(std::net::Ipv4Addr::LOCALHOST.into());
                    }
                    t = t.with_ip_addr(s);
                }
            }
            t
        }
    };
    say("a dials", &target);
    let t = Instant::now();
    match tokio::time::timeout(Duration::from_secs(60), a.connect(target, ALPN)).await {
        Ok(Ok(conn)) => {
            say("connected after ms", t.elapsed().as_millis());
            let (mut send, mut recv) = conn.open_bi().await.expect("open_bi");
            send.write_all(b"footprint").await.expect("write");
            send.finish().expect("finish");
            let echoed = recv.read_to_end(64).await.expect("read");
            say("echo", String::from_utf8_lossy(&echoed).to_string());
            for p in conn.paths().iter() {
                say("path", (format!("{:?}", p.remote_addr()), p.is_selected()));
            }
            tokio::time::sleep(Duration::from_secs(secs)).await;
            for p in conn.paths().iter() {
                say("path after wait", (format!("{:?}", p.remote_addr()), p.is_selected()));
            }
        }
        Ok(Err(e)) => say("connect failed", e.to_string()),
        Err(_) => say("connect failed", "timed out after 60 s"),
    }
    a.close().await;
    b.close().await;
    say("done", "");
}
