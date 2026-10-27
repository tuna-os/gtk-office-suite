//! Measurements for `docs/rfc/0004-spike-tls-results.md`: what TLS and HTTP/2 cost and how they
//! behave with many streams on one connection. `#[ignore]`d, run one at a time in a release build
//! (`--ignored --test-threads=1`). Each prints `MEASURE <name> <json>` (and appends it to
//! `DCP_MEASURE_OUT`); every result carries the server kind and HTTP version of the run.
//!
//! The append-to-second-client latency, the bare append and the lease latency are the spike B
//! tests in `measure.rs`, run unchanged against a TLS and a plain URL.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use dcp_streams::crypto;
use dcp_streams::http::{Transport, JSON, OCTETS};
use dcp_streams::Http;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::task::JoinSet;

use crate::common::*;
use crate::fwd::{Fwd, FwdOpts};
use crate::live::{follow, Ev, EvKind, Live};
use crate::measure::{emit, ms, summary};
use crate::rawtls::*;
use crate::require_tls;

fn ctx() -> Value {
    json!({
        "kind": std::env::var("DCP_STREAMS_KIND").unwrap_or_else(|_| "?".into()),
        "http": std::env::var("DCP_STREAMS_HTTP").unwrap_or_else(|_| "-".into()),
    })
}

fn out(name: &str, v: Value) {
    emit(name, json!({"ctx": ctx(), "result": v}));
}

fn stream_path(what: &str) -> String {
    format!("dcp/{what}-{}/s", hex::encode(crypto::random::<6>()))
}

fn addr_of(base: &str) -> SocketAddr {
    format!("127.0.0.1:{}", Target::parse(base).port).parse().unwrap()
}

/// One HEAD on a connection that is already handshaken, over whatever ALPN chose. Reading the
/// answer is also what lets rustls process the server's session ticket.
async fn exercise(c: Conn, t: &Target) {
    if c.alpn().as_deref() == Some("h2") {
        let (send, driver) = h2::client::handshake(c.stream).await.unwrap();
        tokio::spawn(async move {
            let _ = driver.await;
        });
        let mut send = send.ready().await.unwrap();
        let req = http::Request::head(format!("https://{}{}dcp-handshake", t.authority(), t.prefix))
            .body(())
            .unwrap();
        let (resp, _) = send.send_request(req, true).unwrap();
        let _ = resp.await.unwrap();
    } else {
        let mut s = c.stream;
        s.write_all(
            format!(
                "HEAD {}dcp-handshake HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                t.prefix,
                t.authority()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut sink = Vec::new();
        let _ = s.read_to_end(&mut sink).await;
    }
}

// ------------------------------------------------------------------ cold start

async fn client_cost(base: &str, n: usize) -> Value {
    let path = stream_path("cold");
    http(base).create(&path, JSON, None, false).await.unwrap();
    let (mut build, mut first, mut second) = (vec![], vec![], vec![]);
    for _ in 0..n {
        let t0 = Instant::now();
        let h = http(base);
        build.push(ms(t0.elapsed()));
        let t1 = Instant::now();
        h.head(&path).await.unwrap();
        first.push(ms(t1.elapsed()));
        let t2 = Instant::now();
        h.head(&path).await.unwrap();
        second.push(ms(t2.elapsed()));
    }
    json!({
        "client_build_ms (reqwest + rustls config + the CA)": summary(build),
        "first_request_ms (DNS, TCP, TLS, request)": summary(first),
        "second_request_ms (warm connection)": summary(second),
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn handshake_and_cold_start_cost() {
    let base = require_tls!();
    let t = Target::parse(&base);
    let addr = addr_of(&base);
    let alpn = wanted_alpn();
    let ca = ca_pem();

    // Full handshakes: a new configuration (so no session to resume) every time.
    let (mut tcp, mut full) = (vec![], vec![]);
    for _ in 0..50 {
        let c = connect(addr, &t.host, client_config(&ca, &[alpn])).await.unwrap();
        assert_eq!(c.kind(), "Some(Full)");
        tcp.push(ms(c.tcp));
        full.push(ms(c.tls));
    }

    // Resumed handshakes: one configuration, and every connection reads a response so that the
    // server's NewSessionTicket is processed.
    let cfg = client_config(&ca, &[alpn]);
    let first = connect(addr, &t.host, cfg.clone()).await.unwrap();
    assert_eq!(first.kind(), "Some(Full)");
    exercise(first, &t).await;
    let (mut resumed, mut kinds) = (vec![], vec![]);
    for _ in 0..50 {
        let c = connect(addr, &t.host, cfg.clone()).await.unwrap();
        kinds.push(c.kind());
        resumed.push(ms(c.tls));
        exercise(c, &t).await;
    }
    let n_resumed = kinds.iter().filter(|k| *k == "Some(Resumed)").count();
    assert!(
        n_resumed >= 25,
        "only {n_resumed} of 50 handshakes resumed: the 'resumed' numbers would be full handshakes"
    );

    // Bytes on the wire for one handshake (the server's session ticket included), counted by a
    // forwarder: a full one, then one that resumes with a ticket the first one left in `cfg`.
    let f1 = Fwd::start(FwdOpts::to(addr)).await;
    let c = connect(f1.addr, &t.host, client_config(&ca, &[alpn])).await.unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let full_bytes = f1.counts();
    drop(c);
    let f2 = Fwd::start(FwdOpts::to(addr)).await;
    let c = connect(f2.addr, &t.host, cfg.clone()).await.unwrap();
    let was = c.kind();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let resumed_bytes = f2.counts();
    drop(c);

    let mut r = json!({
        "tcp_connect_ms": summary(tcp),
        "tls_full_handshake_ms": summary(full),
        "tls_resumed_handshake_ms": summary(resumed),
        "resumed_of_50": n_resumed,
        "handshake_bytes_full (client->server, server->client)": [full_bytes.up, full_bytes.down],
        "handshake_bytes_second_connection": {"kind": was, "up": resumed_bytes.up, "down": resumed_bytes.down},
        "alpn": alpn,
    });
    r["client_tls"] = client_cost(&base, 30).await;
    if let Ok(plain) = std::env::var("DCP_STREAMS_PLAIN_URL") {
        r["client_plain_same_front_end"] = client_cost(&plain, 30).await;
    }
    if let Ok(direct) = std::env::var("DCP_STREAMS_DIRECT_URL") {
        r["client_plain_origin_direct"] = client_cost(&direct, 30).await;
    }
    out("handshake_and_cold_start", r);
}

// ------------------------------------------------------------------ bytes per request

async fn bytes_per_request(base: &str, n: usize) -> Value {
    let fwd = Fwd::start(FwdOpts::to(addr_of(base))).await;
    let via = fwd.url_for(base);
    let h = http(&via);
    let path = stream_path("ovh");
    h.create(&path, JSON, None, false).await.unwrap();
    let body = |i: usize| format!("{{\"i\":\"{i:05}\",\"pad\":\"{}\"}}", "x".repeat(600)).into_bytes();
    let body_len = body(0).len();
    for i in 0..20 {
        h.append(&path, JSON, body(i), None, false).await.unwrap();
    }
    let a0 = fwd.counts();
    for i in 0..n {
        h.append(&path, JSON, body(i), None, false).await.unwrap();
    }
    let a1 = fwd.counts();
    for _ in 0..n {
        h.head(&path).await.unwrap();
    }
    let a2 = fwd.counts();
    let per = |x: u64| x as f64 / n as f64;
    json!({
        "url": base,
        "connections": a2.accepted,
        "body_bytes": body_len,
        "append": {
            "client_to_server_per_request": per(a1.up - a0.up),
            "request_overhead_beyond_body": per(a1.up - a0.up) - body_len as f64,
            "server_to_client_per_response": per(a1.down - a0.down),
        },
        "head": {
            "client_to_server_per_request": per(a2.up - a1.up),
            "server_to_client_per_response": per(a2.down - a1.down),
        },
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn request_overhead_per_record() {
    let base = require_tls!();
    let mut all = vec![bytes_per_request(&base, 200).await];
    if let Ok(plain) = std::env::var("DCP_STREAMS_PLAIN_URL") {
        all.push(bytes_per_request(&plain, 200).await);
    }
    if let Ok(direct) = std::env::var("DCP_STREAMS_DIRECT_URL") {
        all.push(bytes_per_request(&direct, 200).await);
    }
    out("request_overhead", Value::Array(all));
}

// ------------------------------------------------------------------ HTTP/2 settings

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn the_servers_http2_settings_and_ping_time() {
    let base = require_tls!();
    if wanted_alpn() != "h2" {
        eprintln!("SKIPPED {} (this run is HTTP/1.1: there are no HTTP/2 settings to read)", module_path!());
        return;
    }
    let t = Target::parse(&base);
    let c = connect_target(&t, client_config(&ca_pem(), &["h2"])).await.unwrap();
    let (send, mut driver) = h2::client::handshake(c.stream).await.unwrap();
    let mut pings = driver.ping_pong().expect("ping_pong");
    tokio::spawn(async move {
        let _ = driver.await;
    });
    let mut send = send.ready().await.unwrap();
    let req = http::Request::head(format!("https://{}{}dcp-settings", t.authority(), t.prefix))
        .body(())
        .unwrap();
    let (resp, _) = send.send_request(req, true).unwrap();
    let _ = resp.await.unwrap();
    let max_streams = send.current_max_send_streams();
    let mut rtt = Vec::new();
    for _ in 0..100 {
        let t0 = Instant::now();
        pings.ping(h2::Ping::opaque()).await.unwrap();
        rtt.push(ms(t0.elapsed()));
    }
    out(
        "http2_settings",
        json!({"max_concurrent_streams_the_server_allows": max_streams, "ping_ms (to the TLS terminator)": summary(rtt)}),
    );
    assert!(
        max_streams >= 100,
        "the server allows only {max_streams} concurrent streams: 50 live readers plus appends do not fit with room to spare"
    );
}

// ------------------------------------------------------------------ many streams, one client

struct Mux {
    label: &'static str,
    n: usize,
    live: Live,
    /// One-way delay the forwarder adds.
    delay: Duration,
    /// About one chunk in `.0` is held `.1` (see `fwd.rs`).
    stall: Option<(u32, Duration)>,
    bulk_mb: usize,
}

async fn wake_one(
    h: &Http,
    path: &str,
    i: usize,
    rx: &mut UnboundedReceiver<Ev>,
    k: usize,
) -> Result<f64, String> {
    let t0 = Instant::now();
    h.append(
        path,
        JSON,
        format!("{{\"k\":{k},\"pad\":\"{}\"}}", "y".repeat(200)).into_bytes(),
        None,
        false,
    )
    .await
    .map_err(|e| e.to_string())?;
    loop {
        match tokio::time::timeout(Duration::from_secs(20), rx.recv()).await {
            Ok(Some(Ev { idx, at, kind: EvKind::Data(_) })) if idx == i => {
                return Ok(ms(at.saturating_duration_since(t0)))
            }
            Ok(Some(Ev { kind: EvKind::Failed(m), .. })) => return Err(m),
            Ok(Some(_)) => continue,
            _ => return Err("no delivery within 20 s".into()),
        }
    }
}

async fn wake_burst(
    h: &Http,
    paths: &[String],
    rx: &mut UnboundedReceiver<Ev>,
    k: usize,
) -> Result<Vec<f64>, String> {
    let n = paths.len();
    let t0 = Instant::now();
    let mut set = JoinSet::new();
    for p in paths {
        let (h, p) = (h.clone(), p.clone());
        set.spawn(async move {
            h.append(&p, JSON, format!("{{\"k\":{k},\"pad\":\"{}\"}}", "z".repeat(200)).into_bytes(), None, false)
                .await
        });
    }
    let mut lat = vec![f64::NAN; n];
    let mut got = 0;
    while got < n {
        match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
            Ok(Some(Ev { idx, at, kind: EvKind::Data(_) })) => {
                if lat[idx].is_nan() {
                    lat[idx] = ms(at.saturating_duration_since(t0));
                    got += 1;
                }
            }
            Ok(Some(Ev { kind: EvKind::Failed(m), .. })) => return Err(m),
            Ok(Some(_)) => {}
            _ => return Err(format!("only {got} of {n} deliveries within 30 s")),
        }
    }
    while let Some(r) = set.join_next().await {
        r.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    }
    Ok(lat)
}

fn drain(rx: &mut UnboundedReceiver<Ev>) {
    while rx.try_recv().is_ok() {}
}

async fn mux(base: &str, o: &Mux) -> Value {
    let fwd = Fwd::start(FwdOpts {
        delay: o.delay,
        stall: o.stall,
        ..FwdOpts::to(addr_of(base))
    })
    .await;
    let via = fwd.url_for(base);
    let h = http(&via);
    let direct = http(base);
    let tag = hex::encode(crypto::random::<4>());
    let mut paths = Vec::new();
    let mut offs = Vec::new();
    for i in 0..o.n {
        let p = format!("dcp/mux-{tag}/s{i}");
        direct.create(&p, JSON, None, false).await.unwrap();
        offs.push(direct.head(&p).await.unwrap().next);
        paths.push(p);
    }
    let bulk = format!("dcp/mux-{tag}/bulk");
    if o.bulk_mb > 0 {
        direct
            .create(&bulk, OCTETS, Some(vec![0x5a; o.bulk_mb << 20]), true)
            .await
            .unwrap();
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    let tasks: Vec<_> = (0..o.n)
        .map(|i| follow(h.fork(), paths[i].clone(), offs[i].clone(), o.live, i, tx.clone()))
        .collect();
    let t_setup = Instant::now();
    let mut connected = 0;
    while connected < o.n {
        match tokio::time::timeout(Duration::from_secs(60), rx.recv()).await {
            Ok(Some(Ev { kind: EvKind::Connected, .. })) => connected += 1,
            Ok(Some(Ev { kind: EvKind::Failed(m), idx, .. })) => panic!("reader {idx} failed: {m}"),
            Ok(Some(_)) => {}
            _ => panic!("only {connected} of {} readers connected within 60 s", o.n),
        }
    }
    tokio::time::sleep(Duration::from_millis(800)).await;
    let setup_ms = ms(t_setup.elapsed());
    let parked = fwd.counts();

    let mut result = json!({"label": o.label, "live": format!("{:?}", o.live), "streams": o.n,
        "one_way_delay_ms": o.delay.as_millis() as u64, "stall": o.stall.map(|(n, d)| format!("1 in {n} chunks for {d:?}")),
        "connections_with_all_readers_parked": parked.accepted, "setup_ms": setup_ms});

    // Paced: one message to one stream at a time; time until that stream's reader has it.
    let mut paced = Vec::new();
    let mut failures = Vec::new();
    for k in 0..2 {
        for i in 0..o.n {
            match wake_one(&h, &paths[i], i, &mut rx, k).await {
                Ok(v) => paced.push(v),
                Err(e) => failures.push(e),
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    drain(&mut rx);
    result["paced_ms"] = summary(paced);

    // Burst: one message to every stream at once, over the one pool.
    let mut burst = Vec::new();
    for k in 10..13 {
        match wake_burst(&h, &paths, &mut rx, k).await {
            Ok(v) => burst.extend(v),
            Err(e) => failures.push(e),
        }
        drain(&mut rx);
    }
    result["burst_of_all_streams_ms"] = summary(burst);

    // While a bulk download shares the client: paced wake-ups during it, and its own time.
    if o.bulk_mb > 0 {
        let hb = h.clone();
        let bp = bulk.clone();
        let dl = tokio::spawn(async move {
            let t0 = Instant::now();
            let r = hb.read_bytes(&bp).await;
            (t0.elapsed(), r.map(|v| v.len()))
        });
        let mut during = Vec::new();
        let mut i = 0;
        while !dl.is_finished() && during.len() < 2000 {
            match wake_one(&h, &paths[i % o.n], i % o.n, &mut rx, 100 + i).await {
                Ok(v) => during.push(v),
                Err(e) => failures.push(e),
            }
            i += 1;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let (took, len) = dl.await.unwrap();
        let len = len.unwrap_or(0);
        result["during_bulk_download_ms"] = summary(during);
        result["bulk_download"] = json!({"mib": o.bulk_mb, "bytes_received": len, "seconds": took.as_secs_f64(),
            "mib_per_s": (len as f64 / 1048576.0) / took.as_secs_f64()});
        drain(&mut rx);
    }
    for t in tasks {
        t.abort();
    }
    let end = fwd.counts();
    result["connections_at_end"] = json!(end.accepted);
    result["wire_bytes"] = json!({"up": end.up, "down": end.down});
    result["failures"] = json!(failures);
    assert!(failures.is_empty(), "{failures:?}");
    result
}

async fn mux_test(m: Mux) {
    let base = require_tls!();
    let r = mux(&base, &m).await;
    out(&format!("mux_{}", m.label), r);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn fifty_long_polls_on_one_client() {
    mux_test(Mux { label: "long_poll", n: 50, live: Live::LongPoll, delay: Duration::ZERO, stall: None, bulk_mb: 8 }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn fifty_sse_streams_on_one_client() {
    mux_test(Mux { label: "sse", n: 50, live: Live::Sse, delay: Duration::ZERO, stall: None, bulk_mb: 8 }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn fifty_long_polls_at_40_ms_round_trip() {
    mux_test(Mux { label: "long_poll_rtt40", n: 50, live: Live::LongPoll, delay: Duration::from_millis(20), stall: None, bulk_mb: 8 }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn fifty_long_polls_when_the_connection_stalls() {
    // Emulated retransmission stalls: about one chunk in 200 is held 250 ms, with the
    // connection's later bytes behind it.
    mux_test(Mux { label: "long_poll_stalls", n: 50, live: Live::LongPoll, delay: Duration::from_millis(5), stall: Some((200, Duration::from_millis(250))), bulk_mb: 0 }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn fifty_sse_streams_when_the_connection_stalls() {
    mux_test(Mux { label: "sse_stalls", n: 50, live: Live::Sse, delay: Duration::from_millis(5), stall: Some((200, Duration::from_millis(250))), bulk_mb: 0 }).await;
}

// ------------------------------------------------------------------ bulk over a long path

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn a_large_download_over_a_long_path() {
    let base = require_tls!();
    let direct = http(&base);
    let path = format!("dcp/bulk-{}/f", hex::encode(crypto::random::<6>()));
    let mib = 8usize;
    direct.create(&path, OCTETS, Some(vec![0x33; mib << 20]), true).await.unwrap();
    let windows: Vec<bool> = if wanted_alpn() == "h2" { vec![false, true] } else { vec![false] };
    let mut rows = Vec::new();
    for rtt in [0u64, 40, 100] {
        for &adaptive in &windows {
            let fwd = Fwd::start(FwdOpts {
                delay: Duration::from_millis(rtt / 2),
                ..FwdOpts::to(addr_of(&base))
            })
            .await;
            let via = fwd.url_for(&base);
            let h = Http::with_transport(
                &via,
                Transport {
                    ca_pem: Some(ca_pem()),
                    version: wanted_version(),
                    require_tls: true,
                    h2_adaptive_window: adaptive,
                    ..Transport::default()
                },
            );
            let t0 = Instant::now();
            let got = h.read_bytes(&path).await.unwrap().len();
            let secs = t0.elapsed().as_secs_f64();
            assert_eq!(got, mib << 20);
            rows.push(json!({"rtt_ms": rtt, "h2_adaptive_window": adaptive, "seconds": secs, "mib_per_s": mib as f64 / secs}));
        }
    }
    out("bulk_download_8_mib", Value::Array(rows));
}

// ------------------------------------------------------------------ the server closes an idle connection

async fn gap_row(label: &str, base: &str, gap: u64) -> Value {
    let fwd = Fwd::start(FwdOpts::to(addr_of(base))).await;
    let mut tr = if base.starts_with("https://") { transport() } else { Transport::default() };
    // Keep the pooled connection for the whole gap: it is the server that ends it.
    tr.pool_idle = Duration::from_secs(600);
    let h = Http::with_transport(&fwd.url_for(base), tr);
    let path = stream_path("gap");
    h.create(&path, JSON, None, false).await.unwrap();
    h.head(&path).await.unwrap();
    tokio::time::sleep(Duration::from_secs(gap)).await;
    let (s0, c0) = (h.snapshot(), fwd.counts());
    let t0 = Instant::now();
    let r = h.head(&path).await;
    let took = ms(t0.elapsed());
    let (s1, c1) = (h.snapshot(), fwd.counts());
    json!({
        "server": label,
        "idle_gap_s": gap,
        "ok": r.is_ok(),
        "second_request_ms": took,
        "attempts": s1.requests - s0.requests,
        "transport_errors": s1.transport_errors - s0.transport_errors,
        "new_connections": c1.accepted - c0.accepted,
    })
}

/// The server (or its front end) closes a keep-alive connection that has been idle for N
/// seconds (`DCP_STREAMS_SERVER_IDLE_S`, the idle timeout this front end was started with;
/// the Node origin itself closes at 5 s). What the next request costs, and whether it fails.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn reconnecting_after_the_server_closes_an_idle_connection() {
    let base = require_tls!();
    let mut targets = vec![("tls front end".to_owned(), base.clone())];
    if let Ok(d) = std::env::var("DCP_STREAMS_DIRECT_URL") {
        targets.push(("node origin, plain, direct (keep-alive timeout 5 s)".to_owned(), d));
    }
    let mut rows = Vec::new();
    for (label, b) in &targets {
        for gap in [1u64, 3, 7, 12] {
            rows.push(gap_row(label, b, gap).await);
        }
    }
    let idle = std::env::var("DCP_STREAMS_SERVER_IDLE_S").unwrap_or_else(|_| "unset".into());
    out("reconnect_after_server_idle_close", json!({"front_end_idle_timeout_s": idle, "rows": rows}));
}
