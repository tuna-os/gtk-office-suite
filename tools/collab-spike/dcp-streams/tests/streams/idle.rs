//! A proxy, load balancer or firewall between client and server that ends connections. Emulated
//! by `fwd.rs` in front of the TLS port, so it is the same for both servers and both HTTP
//! versions, and a connection can end in three ways: FIN, RST, or silence (the flow is
//! forgotten and nobody is told).
//!
//! Rules: **idle** (nothing in either direction for 60 s, `DCP_IDLE_SECS`: the usual load
//! balancer or NAT timeout) and **age** (the connection is 60 s old, whatever it carries: a proxy
//! that fails over or is restarted).
//!
//! Two questions. What happens to a connection that sits idle *between* requests? And what
//! happens to a long poll or an SSE response that is *in flight* when the connection dies?
//! Whether an idle rule hits an in-flight reader depends on the server's own heartbeat: it does
//! if the server sends nothing for longer than the proxy's timeout. The server's timeouts of the
//! run are `DCP_STREAMS_LONG_POLL_S` (long-poll timeout; Node's SSE keep-alive interval) and
//! `DCP_STREAMS_SSE_RECONNECT_S` (how long the Go server keeps one SSE response).
//!
//! Needs `DCP_STREAMS_URL` (https). Takes about 2.5 minutes.

use std::time::{Duration, Instant};

use dcp_streams::crypto;
use dcp_streams::http::JSON;
use dcp_streams::Http;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::common::*;
use crate::fwd::{Fwd, FwdOpts, Kill};
use crate::live::{follow, Ev, EvKind, Live};
use crate::measure::emit;
use crate::rawtls::*;
use crate::require_tls;

fn secs_env(var: &str, default: u64) -> u64 {
    std::env::var(var).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn stream_path(what: &str) -> String {
    format!("dcp/{what}-{}/s", hex::encode(crypto::random::<6>()))
}

fn addr_of(base: &str) -> std::net::SocketAddr {
    format!("127.0.0.1:{}", Target::parse(base).port).parse().unwrap()
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Rule {
    None,
    Idle(Kill),
    Age(Kill),
}

/// HTTP/2 keep-alive pings at a quarter of the proxy's timeout (15 s for 60 s).
fn ping_every(after: Duration) -> Duration {
    (after / 4).max(Duration::from_secs(1))
}

impl Rule {
    fn name(self) -> String {
        match self {
            Rule::None => "no rule (control)".into(),
            Rule::Idle(k) => format!("idle {k:?}"),
            Rule::Age(k) => format!("age {k:?}"),
        }
    }

    fn opts(self, base: &str, after: Duration) -> FwdOpts {
        let mut o = FwdOpts::to(addr_of(base));
        match self {
            Rule::None => {}
            Rule::Idle(k) => o.idle_kill = Some((after, k)),
            Rule::Age(k) => o.age_kill = Some((after, k)),
        }
        o
    }
}

/// A connection that sits idle for `after + 5 s` between two requests.
async fn between_requests(base: String, rule: Rule, keep_alive: bool, after: Duration) -> Value {
    let fwd = Fwd::start(rule.opts(&base, after)).await;
    let mut tr = transport();
    // Keep the pooled connection for as long as the test sleeps, so the idle connection is what
    // is reused (the binding's default drops it after 2 s, before any proxy could).
    tr.pool_idle = Duration::from_secs(600);
    if keep_alive {
        tr.h2_keep_alive = Some(ping_every(after));
    }
    let h = Http::with_transport(&fwd.url_for(&base), tr);
    let path = stream_path("idle");
    h.create(&path, JSON, None, false).await.unwrap();
    h.head(&path).await.unwrap();
    tokio::time::sleep(after + Duration::from_secs(5)).await;
    let (s0, c0) = (h.snapshot(), fwd.counts());
    let t0 = Instant::now();
    let r = h.head(&path).await;
    let took = t0.elapsed();
    let (s1, c1) = (h.snapshot(), fwd.counts());
    let ok = r.is_ok();
    json!({
        "phase": "idle between requests",
        "rule": rule.name(),
        "h2_keep_alive_pings": keep_alive,
        "request_after_idle_ok": ok,
        "error": r.err().map(|e| e.to_string()),
        "seconds": took.as_secs_f64(),
        "attempts": s1.requests - s0.requests,
        "transport_errors": s1.transport_errors - s0.transport_errors,
        "new_connections": c1.accepted - c0.accepted,
        "connections_the_proxy_ended": c1.killed,
    })
}

struct Flight {
    json: Value,
    woke_ms: Option<f64>,
    errors: u64,
    killed: u64,
}

/// A reader parked on a quiet stream; at `append_at` a record is appended (straight to the origin,
/// not through the proxy); `watch` is how long the reader has to deliver it.
async fn in_flight(
    base: String,
    live: Live,
    rule: Rule,
    keep_alive: bool,
    after: Duration,
    append_at: Duration,
    watch: Duration,
) -> Flight {
    let fwd = Fwd::start(rule.opts(&base, after)).await;
    let mut tr = transport();
    if keep_alive {
        tr.h2_keep_alive = Some(ping_every(after));
    }
    let h = Http::with_transport(&fwd.url_for(&base), tr);
    let direct = http(&base);
    let path = stream_path("flight");
    direct.create(&path, JSON, None, false).await.unwrap();
    let off = direct.head(&path).await.unwrap().next;
    let (tx, mut rx) = mpsc::unbounded_channel::<Ev>();
    let task = follow(h.clone(), path.clone(), off, live, 0, tx);
    let t0 = Instant::now();
    let mut log: Vec<(f64, String)> = Vec::new();
    let note = |log: &mut Vec<(f64, String)>, ev: &Ev| {
        let what = match &ev.kind {
            EvKind::Connected => "connected".to_owned(),
            EvKind::Data(n) => format!("{n} message(s) delivered"),
            EvKind::Reconnect(w) => format!("reconnect ({w})"),
            EvKind::Failed(w) => format!("FAILED ({w})"),
        };
        log.push((ev.at.saturating_duration_since(t0).as_secs_f64(), what));
    };
    while let Some(left) = append_at.checked_sub(t0.elapsed()) {
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Some(ev)) => note(&mut log, &ev),
            _ => break,
        }
    }
    let t_append = Instant::now();
    let appended = direct
        .append(&path, JSON, b"{\"after_the_proxy_rule\":true}".to_vec(), None, false)
        .await
        .is_ok();
    let mut woke_ms = None;
    let deadline = t_append + watch;
    while woke_ms.is_none() {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else { break };
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Some(ev)) => {
                note(&mut log, &ev);
                if matches!(ev.kind, EvKind::Data(_)) {
                    woke_ms = Some(ev.at.saturating_duration_since(t_append).as_secs_f64() * 1000.0);
                }
            }
            _ => break,
        }
    }
    task.abort();
    let (s, c) = (h.snapshot(), fwd.counts());
    Flight {
        json: json!({
            "phase": "in flight",
            "reader": format!("{live:?}"),
            "rule": rule.name(),
            "h2_keep_alive_pings": keep_alive,
            "appended_at_s": append_at.as_secs(),
            "append_ok": appended,
            "delivered_after_ms": woke_ms,
            "transport_errors": s.transport_errors,
            "connections_opened": c.accepted,
            "connections_the_proxy_ended": c.killed,
            "events": log.iter().map(|(t, w)| format!("{t:.1}s {w}")).collect::<Vec<_>>(),
        }),
        woke_ms,
        errors: s.transport_errors,
        killed: c.killed,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "measurement"]
async fn a_proxy_that_closes_idle_connections_after_60_s() {
    let base = require_tls!();
    let after = Duration::from_secs(secs_env("DCP_IDLE_SECS", 60));
    let poll_s = secs_env("DCP_STREAMS_LONG_POLL_S", 30);
    let kind = std::env::var("DCP_STREAMS_KIND").unwrap_or_default();
    // How long the server stays silent on a parked reader.
    let lp_silence = Duration::from_secs(poll_s);
    let sse_silence = Duration::from_secs(if kind == "go" {
        secs_env("DCP_STREAMS_SSE_RECONNECT_S", 60)
    } else {
        poll_s
    });
    let h2 = wanted_alpn() == "h2";
    let append_at = after + after / 4;
    let watch = Duration::from_secs(80);

    // (rule, HTTP/2 keep-alive pings). Pings only exist in HTTP/2.
    let mut cases: Vec<(Rule, bool)> = vec![
        (Rule::None, false),
        (Rule::Idle(Kill::Fin), false),
        (Rule::Idle(Kill::Rst), false),
        (Rule::Idle(Kill::Blackhole), false),
        (Rule::Age(Kill::Blackhole), false),
    ];
    if h2 {
        cases.push((Rule::Idle(Kill::Fin), true));
        cases.push((Rule::Age(Kill::Blackhole), true));
    }

    let mut idle_jobs = Vec::new();
    for &(rule, ka) in cases.iter().filter(|(r, _)| !matches!(r, Rule::Age(_))) {
        idle_jobs.push((rule, ka, tokio::spawn(between_requests(base.clone(), rule, ka, after))));
    }
    let mut flights = Vec::new();
    for live in [Live::LongPoll, Live::Sse] {
        for &(rule, ka) in &cases {
            flights.push((
                live,
                rule,
                ka,
                tokio::spawn(in_flight(base.clone(), live, rule, ka, after, append_at, watch)),
            ));
        }
    }

    let mut report = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for (rule, ka, j) in idle_jobs {
        let v = j.await.unwrap();
        let ok = v["request_after_idle_ok"] == true;
        let errs = v["transport_errors"].as_u64().unwrap_or(99);
        let newc = v["new_connections"].as_u64().unwrap_or(99);
        match (rule, ka) {
            (Rule::None, _) if !ok || errs != 0 || newc != 0 => {
                problems.push(format!("control (no rule) was not clean: {v}"))
            }
            // FIN and RST are noticed at once by both HTTP versions: a new connection, no error.
            (Rule::Idle(Kill::Fin | Kill::Rst), false) if !ok || newc != 1 => {
                problems.push(format!("after {}: expected one new connection and an answer: {v}", rule.name()))
            }
            // Pings every 15 s keep the idle rule from ever firing.
            (Rule::Idle(_), true) if !ok || newc != 0 || v["connections_the_proxy_ended"] != 0 => {
                problems.push(format!("keep-alive pings did not keep the connection alive: {v}"))
            }
            _ => {}
        }
        report.push(v);
    }
    for (live, rule, ka, j) in flights {
        let f = j.await.unwrap();
        let silence = if live == Live::Sse { sse_silence } else { lp_silence };
        let hit = silence > after;
        match (rule, ka) {
            (Rule::None, _) => {
                if f.woke_ms.is_none_or(|m| m > 2000.0) || f.errors != 0 || f.killed != 0 {
                    problems.push(format!("{live:?} control: {}", f.json));
                }
            }
            (Rule::Idle(k @ (Kill::Fin | Kill::Rst)), false) => {
                // Never lose a record: whatever the proxy did, the reader resumes from its offset.
                if f.woke_ms.is_none_or(|m| m > 5000.0) {
                    problems.push(format!("{live:?} {k:?}: the record was not delivered promptly: {}", f.json));
                }
                if live == Live::LongPoll && hit && (f.killed == 0 || f.errors == 0) {
                    problems.push(format!("{live:?} {k:?}: the server was silent {silence:?} > {after:?}, so the proxy should have ended the poll: {}", f.json));
                }
                if live == Live::LongPoll && !hit && (f.killed != 0 || f.errors != 0) {
                    problems.push(format!("{live:?} {k:?}: the server's poll cycle {silence:?} is shorter than the proxy's {after:?} yet the connection was ended: {}", f.json));
                }
            }
            // Pings on an HTTP/2 connection keep an idle rule from firing at all.
            (Rule::Idle(_), true) => {
                if f.woke_ms.is_none_or(|m| m > 2000.0) || f.killed != 0 || f.errors != 0 {
                    problems.push(format!("{live:?}: pings did not defeat the idle rule: {}", f.json));
                }
            }
            // A connection that dies silently while pings are flowing is noticed by the pings.
            (Rule::Age(_), true) => {
                if f.woke_ms.is_none_or(|m| m > 40_000.0) {
                    problems.push(format!("{live:?}: a silently dead connection with keep-alive pings was not replaced within 40 s: {}", f.json));
                }
            }
            // Silence without pings is reported, not judged.
            (Rule::Idle(Kill::Blackhole), false) | (Rule::Age(_), false) => {}
        }
        report.push(f.json);
    }
    emit(
        "idle_proxy",
        json!({"ctx": {"kind": kind, "http": std::env::var("DCP_STREAMS_HTTP").unwrap_or_default()},
               "proxy_after_s": after.as_secs(), "server_long_poll_s": poll_s,
               "server_sse_silence_s": sse_silence.as_secs(), "results": report}),
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
