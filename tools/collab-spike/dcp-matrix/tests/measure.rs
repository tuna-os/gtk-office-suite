//! Spike A measurements against a real Synapse. Each test records JSON lines
//! (see `common::record`) that `docs/rfc/0004-spike-a-results.md` quotes.
//!
//! Run:  cargo test -p dcp-matrix --test measure -- --ignored --test-threads=1 --nocapture
//!
//! They are `#[ignore]`d only so a plain `cargo test` stays quick; with
//! `--ignored` they fail loudly without `DCP_SYNAPSE_URL`. A measurement that
//! cannot get its answer fails rather than recording a made-up one.

mod common;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use base64::Engine;
use common::proxy::Proxy;
use common::*;
use dcp_core::*;
use dcp_matrix::matrix_sdk::config::SyncSettings;
use dcp_matrix::matrix_sdk::ruma::events::AnyMessageLikeEventContent;
use dcp_matrix::matrix_sdk::ruma::{serde::Raw, RoomId};
use dcp_matrix::state;
use dcp_matrix::synapse::Registered;
use dcp_matrix::{MatrixConfig, MatrixStore};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn update(seq: u64, payload_len: usize) -> Update {
    let raw = noise(payload_len, seq);
    Update {
        dcp: 1,
        algorithm: "loro/1".into(),
        doc_model: "tables/1".into(),
        base_version: "$0123456789012345678901234567890123456789012".into(),
        seq,
        payload: Payload::Base64 { data: base64::engine::general_purpose::STANDARD.encode(&raw), size: payload_len as u64 },
    }
}

/// A held lease at `renewal` (spec 4.3 rule 8: 0 over a record that is not
/// held, the replaced record's renewal + 1 over a held one).
fn held(who: &MatrixStore, session: &str, base: &str, renewal: u64) -> Lease {
    Lease { dcp: 1, state: LeaseState::Held, holder: Some(Holder { principal: who.principal(), session: session.into() }), token: Some(session.into()), base_version: Some(base.into()), renewal: Some(renewal), duration_ms: Some(60_000) }
}

fn released() -> Lease {
    Lease { dcp: 1, state: LeaseState::Released, holder: None, token: None, base_version: None, renewal: None, duration_ms: None }
}

/// Rule 8 for a lease written over `cur`.
fn renewal_over(cur: Option<&Lease>) -> u64 {
    match cur {
        Some(l) if l.state == LeaseState::Held => l.renewal.map_or(0, |r| r + 1),
        _ => 0,
    }
}

fn is_default(syn: &Synapse) -> bool {
    syn.profile == "default"
}

/// A second device for the same account, through the password login.
async fn second_device(syn: &Synapse, reg: &Registered, tweak: impl FnOnce(&mut MatrixConfig)) -> MatrixStore {
    let r: Value = dcp_matrix::matrix_sdk::reqwest::Client::new()
        .post(format!("{}/_matrix/client/v3/login", syn.url))
        .json(&json!({"type": "m.login.password", "identifier": {"type": "m.id.user", "user": reg.user_id}, "password": reg.password, "initial_device_display_name": "second device"}))
        .send()
        .await
        .expect("login")
        .json()
        .await
        .expect("login json");
    let reg2 = Registered {
        user_id: r["user_id"].as_str().expect("user_id").to_owned(),
        device_id: r["device_id"].as_str().expect("device_id").to_owned(),
        access_token: r["access_token"].as_str().expect("access_token").to_owned(),
        password: reg.password.clone(),
    };
    assert_ne!(reg2.device_id, reg.device_id);
    syn.connect_as(&reg2, tweak).await
}

/// `(container engine, container name)` when the Synapse was started by the
/// scripts, to read its on-disk size.
fn container() -> Option<(String, String)> {
    Some((std::env::var("DCP_CONTAINER_ENGINE").ok()?, std::env::var("DCP_SYNAPSE_CONTAINER").ok()?))
}

fn container_sh(script: &str) -> Option<String> {
    let (engine, name) = container()?;
    let out = std::process::Command::new(engine).args(["exec", &name, "sh", "-c", script]).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

// ---------------------------------------------------------------------------
// 0. What is under test.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn m00_environment() {
    let Some(syn) = require_synapse!() else { return };
    let server = dcp_matrix::synapse::server_header(&syn.url).await.unwrap();
    let alice = syn.user("env").await;
    record(&syn, "environment", json!({
        "server": server,
        "image_id": std::env::var("DCP_SYNAPSE_IMAGE_ID").ok(),
        "matrix_sdk": "0.19.1",
        "alice_init_warnings": alice.init_warnings(),
        "synapse_rc_message_effective": if is_default(&syn) { "Synapse default (0.2/s, burst 10)" } else { "relaxed profile" },
    }));
}

// ---------------------------------------------------------------------------
// 1. Publish-to-visible latency for a second user.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn m01_publish_to_visible_latency() {
    let Some(syn) = require_synapse!() else { return };
    let alice = syn.user_with("alice", |c| c.events_per_sec = 0.0).await;
    let reg_b = syn.register("bob").await;
    let bob = syn.connect_as(&reg_b, |c| c.background_sync = false).await;
    let doc = alice.create_document(&meta("Latency"), Some(file(b"v1"))).await.unwrap();
    alice.grant(&doc, &bob.principal(), Role::Viewer).await.unwrap();
    assert_eq!(bob.versions(&doc).await.unwrap().len(), 1, "bob joined and reads v1");

    // Bob's observer: one /sync loop; an event is "visible" when it is in a sync
    // response decrypted, or, if it arrived before its key, when a later fetch
    // decrypts it.
    let (tx, mut rx) = mpsc::unbounded_channel::<(String, Instant, bool)>();
    let client = bob.client().clone();
    let rid = RoomId::parse(&doc).unwrap();
    let obs = tokio::spawn(async move {
        let mut pending: Vec<dcp_matrix::matrix_sdk::ruma::OwnedEventId> = Vec::new();
        loop {
            let Ok(resp) = client.sync_once(SyncSettings::new().timeout(Duration::from_secs(30))).await else {
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            };
            let now = Instant::now();
            if let Some(j) = resp.rooms.joined.get(&rid) {
                for ev in &j.timeline.events {
                    let Some(id) = ev.event_id() else { continue };
                    if ev.kind.is_utd() {
                        pending.push(id.to_owned());
                    } else if let Ok(v) = serde_json::from_str::<Value>(ev.raw().json().get()) {
                        if v["type"].as_str().is_some_and(|t| t.starts_with("org.tunaos.dcp.")) {
                            let _ = tx.send((id.to_string(), now, false));
                        }
                    }
                }
            }
            if !pending.is_empty() {
                if let Some(room) = client.get_room(&rid) {
                    let mut still = Vec::new();
                    for id in pending.drain(..) {
                        match room.event(&id, None).await {
                            Ok(ev) if !ev.kind.is_utd() => {
                                let _ = tx.send((id.to_string(), Instant::now(), true));
                            }
                            _ => still.push(id),
                        }
                    }
                    pending = still;
                }
            }
        }
    });

    let base = Instant::now();
    let rel = |i: Instant| ms(i.duration_since(base));
    let mut seen: HashMap<String, (Instant, bool)> = HashMap::new();
    async fn wait_visible(rx: &mut mpsc::UnboundedReceiver<(String, Instant, bool)>, seen: &mut HashMap<String, (Instant, bool)>, id: &str) -> (Instant, bool) {
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            while let Ok((i, t, u)) = rx.try_recv() {
                seen.entry(i).or_insert((t, u));
            }
            if let Some(v) = seen.get(id) {
                return *v;
            }
            assert!(Instant::now() < end, "event {id} never became visible to bob within 60 s");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    // Raw event latency: queue + encrypt + PUT, no preflight and no timestamp read-back,
    // spaced out so neither the client limiter nor Synapse's limit shapes it.
    let n_raw = if is_default(&syn) { 4 } else { 30 };
    let mut from_start = Vec::new();
    let mut from_ack = Vec::new();
    let mut key_late = 0;
    for i in 0..(n_raw + 2) {
        sleep_ms(300).await;
        let t0 = Instant::now();
        let id = alice.send_queued(&doc, state::UPDATE, serde_json::to_value(update(i as u64, 400)).unwrap()).await.unwrap();
        let t_ack = Instant::now();
        let (vis, late) = wait_visible(&mut rx, &mut seen, &id).await;
        if i >= 2 {
            from_start.push(ms(vis.duration_since(t0)));
            from_ack.push(rel(vis) - rel(t_ack));
            key_late += late as u32;
        }
    }
    // The full publish path: role check, queued PUT, timestamp read-back.
    let n_full = if is_default(&syn) { 2 } else { 15 };
    let mut full_start = Vec::new();
    for i in 0..n_full {
        sleep_ms(300).await;
        let t0 = Instant::now();
        let sent = alice.publish_update(&doc, update(1000 + i as u64, 400)).await.unwrap();
        let (vis, _) = wait_visible(&mut rx, &mut seen, &sent.id).await;
        full_start.push(ms(vis.duration_since(t0)));
    }
    // Versions add an upload, a verify-download and the version event.
    let n_versions = if is_default(&syn) { 1 } else { 10 };
    let mut v_start = Vec::new();
    let mut v_ack = Vec::new();
    for i in 0..n_versions {
        sleep_ms(300).await;
        let t0 = Instant::now();
        let sent = alice.publish_version(&doc, vec![], file(&noise(20_000, 100 + i as u64)), None).await.unwrap();
        let t_ack = Instant::now();
        let (vis, _) = wait_visible(&mut rx, &mut seen, &sent.id).await;
        v_start.push(ms(vis.duration_since(t0)));
        v_ack.push(rel(vis) - rel(t_ack));
    }
    obs.abort();
    record(&syn, "latency_publish_to_visible_ms", json!({
        "method": "second user's single /sync long-poll loop (timeout 30 s); visible = event decrypted in a sync response or by a fetch after its key arrived",
        "raw_update_400B_send_queued": { "from_call_start": summarize(from_start), "from_call_return": summarize(from_ack), "key_arrived_after_event": key_late },
        "publish_update_400B_full_path": { "from_call_start": summarize(full_start) },
        "versions_20KB_file": { "from_call_start": summarize(v_start), "from_call_return": summarize(v_ack) },
    }));
}

// ---------------------------------------------------------------------------
// 2. Event rate under the homeserver's rate limits.
// ---------------------------------------------------------------------------

async fn rate_run(syn: &Synapse, label: &str, eps: f64, burst: u32, n: usize) -> Value {
    let store = syn.user_with("rate", |c| {
        c.events_per_sec = eps;
        c.burst = burst;
        c.send_deadline = Duration::from_secs(900);
    })
    .await;
    let doc = store.create_document(&meta(label), Some(file(b"v1"))).await.unwrap();
    // Warm up key sharing and the first-use paths with one event, then reset counters by differencing.
    store.publish_update(&doc, update(0, 100)).await.unwrap();
    let before = store.stats();
    let t0 = Instant::now();
    let mut done_at = Vec::with_capacity(n);
    let mut first_429_at: Option<usize> = None;
    for i in 0..n {
        store.publish_update(&doc, update(1 + i as u64, 100)).await.unwrap_or_else(|e| panic!("{label}: event {i} failed: {e:?}"));
        done_at.push(t0.elapsed().as_secs_f64());
        if first_429_at.is_none() && store.stats().rate_limited > before.rate_limited {
            first_429_at = Some(i);
        }
    }
    let total = t0.elapsed().as_secs_f64();
    let after = store.stats();
    let mut per_sec: Vec<u32> = vec![0; total.ceil() as usize + 1];
    for t in &done_at {
        per_sec[*t as usize] += 1;
    }
    per_sec.truncate(40);
    let mut gaps: Vec<f64> = done_at.windows(2).map(|w| (w[1] - w[0]) * 1000.0).collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    json!({
        "label": label, "client_limiter_eps": eps, "client_burst": burst, "events": n,
        "total_s": total, "achieved_events_per_s": n as f64 / total,
        "http_429_seen": after.rate_limited - before.rate_limited,
        "events_completed_before_first_429": first_429_at.unwrap_or(n),
        "completed_in_first_second": done_at.iter().filter(|t| **t < 1.0).count(),
        "waited_ms_total": after.waited_ms - before.waited_ms,
        "events_completed_per_second_first_40s": per_sec,
        "inter_event_gap_ms": { "p50": percentile(&gaps, 0.5), "p95": percentile(&gaps, 0.95), "max": gaps.last() },
        "note": "each event is publish_update: role check (GET state) + queued PUT + timestamp read-back (GET event); only the PUT is rate limited",
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn m02_event_rate() {
    let Some(syn) = require_synapse!() else { return };
    let mut runs = Vec::new();
    if is_default(&syn) {
        // Synapse default rc_message: 0.2/s, burst 10, per user.
        runs.push(rate_run(&syn, "unlimited client, default server", 0.0, 1, 30).await);
        runs.push(rate_run(&syn, "spec 3/s burst 3, default server", 3.0, 3, 30).await);
        runs.push(rate_run(&syn, "client matched to server: 0.2/s burst 10", 0.2, 10, 30).await);
    } else {
        runs.push(rate_run(&syn, "unlimited client, relaxed server", 0.0, 1, 300).await);
        runs.push(rate_run(&syn, "spec 3/s burst 3, relaxed server", 3.0, 3, 60).await);
        runs.push(rate_run(&syn, "10/s burst 10, relaxed server", 10.0, 10, 100).await);
    }
    record(&syn, "event_rate", json!({ "runs": runs }));
}

// ---------------------------------------------------------------------------
// 3. Inline update payload sizes through E2EE and the 64 KiB event limit.
// ---------------------------------------------------------------------------

async fn try_payload(syn: &Synapse, alice: &MatrixStore, bob: &MatrixStore, doc: &str, len: usize, seq: u64) -> Value {
    let u = update(seq, len);
    let plaintext_json_bytes = serde_json::to_string(&u).unwrap().len();
    match alice.publish_update(&doc.to_owned(), u.clone()).await {
        Ok(sent) => {
            let wire = server_event(syn, alice, doc, &sent.id).await.map(|v| serde_json::to_string(&v).unwrap().len()).ok();
            let read = bob.updates(&doc.to_owned()).await.unwrap();
            let ok = read.iter().any(|r| r.id == sent.id && r.body == u);
            json!({ "payload_bytes": len, "ok": ok, "accepted_by_server": true, "plaintext_content_json_bytes": plaintext_json_bytes, "event_bytes_as_stored": wire })
        }
        Err(e) => json!({ "payload_bytes": len, "ok": false, "accepted_by_server": false, "plaintext_content_json_bytes": plaintext_json_bytes, "error": format!("{e:?}") }),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn m03_inline_payload_limit() {
    let Some(syn) = require_synapse!() else { return };
    let alice = syn.user_with("alice", |c| {
        c.max_inline_payload = usize::MAX;
        c.events_per_sec = 0.0;
    })
    .await;
    let bob = syn.user("bob").await;
    let doc = alice.create_document(&meta("Payload"), Some(file(b"v1"))).await.unwrap();
    alice.grant(&doc, &bob.principal(), Role::Viewer).await.unwrap();
    bob.versions(&doc).await.unwrap();

    let mut results = Vec::new();
    let mut seq = 0;
    for len in [24_000usize, 30_000, 34_000, 40_000] {
        results.push(try_payload(&syn, &alice, &bob, &doc, len, seq).await);
        seq += 1;
    }
    let mut largest_ok: Option<usize> = results.iter().filter(|r| r["ok"] == true).filter_map(|r| r["payload_bytes"].as_u64()).max().map(|v| v as usize);
    // Bisect between the largest success and the smallest failure, relaxed profile
    // only (it costs a dozen events; Synapse's default burst is 10 per user).
    let mut search = Vec::new();
    if !is_default(&syn) {
        let smallest_fail = results.iter().filter(|r| r["ok"] == false).filter_map(|r| r["payload_bytes"].as_u64()).min().map(|v| v as usize);
        if let (Some(mut lo), Some(mut hi)) = (largest_ok, smallest_fail) {
            while hi - lo > 64 {
                let mid = (lo + hi) / 2;
                let r = try_payload(&syn, &alice, &bob, &doc, mid, seq).await;
                seq += 1;
                if r["ok"] == true {
                    lo = mid;
                } else {
                    hi = mid;
                }
                search.push(r);
            }
            largest_ok = Some(lo);
        }
    }
    record(&syn, "inline_payload_limit", json!({
        "spec_limit_bytes": 34000,
        "tried": results, "bisect": search,
        "largest_payload_bytes_that_worked": largest_ok,
        "event_limit_bytes": 65536,
    }));
}

// ---------------------------------------------------------------------------
// 4. Encrypted media round trip.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn m04_encrypted_media_round_trip() {
    let Some(syn) = require_synapse!() else { return };
    let alice = syn.user_with("alice", |c| c.events_per_sec = 0.0).await;
    let bob = syn.user("bob").await;
    let doc = alice.create_document(&meta("Media"), None).await.unwrap();
    alice.grant(&doc, &bob.principal(), Role::Viewer).await.unwrap();
    let mut rows = Vec::new();
    let sizes: Vec<usize> = if is_default(&syn) { vec![1 << 10, 64 << 10, 1 << 20] } else { vec![1 << 10, 64 << 10, 1 << 20, 8 << 20, 32 << 20] };
    for (i, len) in sizes.into_iter().enumerate() {
        let bytes = noise(len, 900 + i as u64);
        let t0 = Instant::now();
        let v = alice.publish_version(&doc, vec![], file(&bytes), None).await.unwrap();
        let publish_ms = ms(t0.elapsed());
        let t1 = Instant::now();
        let seen = bob.versions(&doc).await.unwrap().into_iter().find(|s| s.id == v.id).expect("bob lists the version");
        let list_ms = ms(t1.elapsed());
        let t2 = Instant::now();
        let got = bob.fetch_file(&doc, &seen).await.unwrap();
        let fetch_ms = ms(t2.elapsed());
        assert_eq!(got, bytes);
        let event_bytes = server_event(&syn, &alice, &doc, &v.id).await.map(|e| serde_json::to_string(&e).unwrap().len()).ok();
        // Ciphertext size on the server: the mxc URI is in the (encrypted) file object.
        let Locator::MatrixEncryptedMedia { file: f } = &v.body.file.locator else { panic!("locator") };
        let mxc = f["url"].as_str().unwrap().to_owned();
        let (server, id) = mxc.trim_start_matches("mxc://").split_once('/').unwrap();
        let blob = dcp_matrix::matrix_sdk::reqwest::Client::new()
            .get(format!("{}/_matrix/client/v1/media/download/{server}/{id}", syn.url))
            .bearer_auth(alice.client().access_token().unwrap())
            .send()
            .await
            .unwrap();
        let status = blob.status().as_u16();
        let stored = blob.bytes().await.map(|b| b.len()).ok();
        rows.push(json!({
            "plaintext_bytes": len, "publish_ms_including_upload_and_verify_download": publish_ms,
            "second_user_list_ms": list_ms, "second_user_fetch_and_decrypt_ms": fetch_ms,
            "stored_ciphertext_bytes": stored, "media_download_http_status": status,
            "version_event_bytes_as_stored": event_bytes,
        }));
    }
    record(&syn, "encrypted_media_round_trip", json!({ "rows": rows, "synapse_max_upload_size": "50M (homeserver.yaml)" }));
}

// ---------------------------------------------------------------------------
// 5. Room growth and cost of reading history.
// ---------------------------------------------------------------------------

async fn initial_sync_bytes(syn: &Synapse, token: &str, timeline_limit: u32) -> (usize, f64) {
    let filter = json!({"room": {"timeline": {"limit": timeline_limit}}}).to_string();
    let mut url = dcp_matrix::matrix_sdk::reqwest::Url::parse(&format!("{}/_matrix/client/v3/sync", syn.url)).unwrap();
    url.query_pairs_mut().append_pair("filter", &filter).append_pair("timeout", "0");
    let t = Instant::now();
    let r = dcp_matrix::matrix_sdk::reqwest::Client::new().get(url).bearer_auth(token).send().await.unwrap();
    let b = r.bytes().await.unwrap();
    (b.len(), ms(t.elapsed()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn m05_room_growth() {
    let Some(syn) = require_synapse!() else { return };
    if is_default(&syn) {
        eprintln!("m05 runs on the relaxed profile only (1000 events at 0.2/s would take over an hour)");
        return;
    }
    let alice = syn.user_with("alice", |c| c.events_per_sec = 0.0).await;
    let reg_b = syn.register("bob").await;
    let bob = syn.connect_as(&reg_b, |_| {}).await;
    let doc = alice.create_document(&meta("Growth"), Some(file(b"v1"))).await.unwrap();
    alice.grant(&doc, &bob.principal(), Role::Editor).await.unwrap();
    bob.versions(&doc).await.unwrap();

    let checkpoints = [10usize, 100, 300, 1000];
    let mut sent = 0usize;
    let mut rows = Vec::new();
    for cp in checkpoints {
        while sent < cp {
            alice.publish_update(&doc, update(sent as u64, 1000)).await.unwrap();
            sent += 1;
        }
        let t = Instant::now();
        let rep = bob.read_report(&doc).await.unwrap();
        let read_ms = ms(t.elapsed());
        // A new device of an existing member, as a client reinstalling or adding a device would be.
        let dev2 = second_device(&syn, &reg_b, |c| c.background_sync = false).await;
        let (sync_bytes_10, sync_ms_10) = initial_sync_bytes(&syn, dev2.client().access_token().unwrap().as_str(), 10).await;
        let (sync_bytes_100, sync_ms_100) = initial_sync_bytes(&syn, dev2.client().access_token().unwrap().as_str(), 100).await;
        let t = Instant::now();
        let rep2 = dev2.read_report(&doc).await;
        let dev2_ms = ms(t.elapsed());
        let (dev2_updates, dev2_utd, dev2_err) = match &rep2 {
            Ok(r) => (r.updates.len(), r.undecryptable, None),
            Err(e) => (0, 0, Some(format!("{e:?}"))),
        };
        rows.push(json!({
            "updates_in_room": sent,
            "bob_full_history_read_ms": read_ms, "bob_updates_returned": rep.updates.len(), "bob_versions_returned": rep.versions.len(), "bob_undecryptable": rep.undecryptable,
            "history_pages_of_100": (sent + 1).div_ceil(100),
            "new_device_initial_sync_bytes_timeline_limit_10": sync_bytes_10, "new_device_initial_sync_ms_limit_10": sync_ms_10,
            "new_device_initial_sync_bytes_timeline_limit_100": sync_bytes_100, "new_device_initial_sync_ms_limit_100": sync_ms_100,
            "new_device_read_ms": dev2_ms, "new_device_updates_readable": dev2_updates, "new_device_undecryptable": dev2_utd, "new_device_read_error": dev2_err,
            "synapse_data_du_kib": container_sh("du -sk /data | cut -f1"),
            "synapse_db_bytes": container_sh("stat -c %s /data/homeserver.db"),
        }));
    }
    record(&syn, "room_growth", json!({ "update_payload_bytes": 1000, "rows": rows }));
}

// ---------------------------------------------------------------------------
// 6. Late joiner: history_visibility "shared" and encrypted history.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn m06_late_joiner_reads_history() {
    let Some(syn) = require_synapse!() else { return };
    let alice = syn.user("alice").await;
    let carol = syn.user_with("carol", |c| c.utd_wait = Duration::from_secs(20)).await;
    let doc = alice.create_document(&meta("History"), Some(file(&noise(2000, 61)))).await.unwrap();
    let v1 = alice.versions(&doc).await.unwrap().remove(0);
    let v2 = alice.publish_version(&doc, vec![v1.id.clone()], file(&noise(2000, 62)), None).await.unwrap();
    alice.publish_update(&doc, update(0, 500)).await.unwrap();
    let snap = alice.snapshot(&doc).await.unwrap();
    // Invite *after* the content exists.
    let t = Instant::now();
    alice.grant(&doc, &carol.principal(), Role::Viewer).await.unwrap();
    let grant_ms = ms(t.elapsed());
    let t = Instant::now();
    let rep = carol.read_report(&doc).await;
    let read_ms = ms(t.elapsed());
    let (ids, utd, files_ok) = match &rep {
        Ok(r) => {
            let mut ok = 0;
            for v in &r.versions {
                if let Ok(b) = carol.fetch_file(&doc, v).await {
                    ok += (hex::encode(Sha256::digest(&b)) == v.body.file.sha256) as usize;
                }
            }
            (r.versions.iter().map(|v| v.id.clone()).collect::<Vec<_>>(), r.undecryptable, ok)
        }
        Err(_) => (vec![], 0, 0),
    };
    record(&syn, "late_joiner_history", json!({
        "history_visibility": snap.history_visibility,
        "alice_init_warnings": alice.init_warnings(),
        "versions_published_before_invite": 2, "updates_published_before_invite": 1,
        "grant_ms": grant_ms, "carol_read_ms_including_20s_key_wait": read_ms,
        "carol_read_error": rep.as_ref().err().map(|e| format!("{e:?}")),
        "carol_versions_visible": ids.len(), "carol_v1_visible": ids.contains(&v1.id), "carol_v2_visible": ids.contains(&v2.id),
        "carol_files_that_verify": files_ok,
        "carol_undecryptable_events": utd,
        "carol_updates_visible": rep.as_ref().map(|r| r.updates.len()).unwrap_or(0),
    }));
}

// ---------------------------------------------------------------------------
// 7. The lease race.
// ---------------------------------------------------------------------------

/// Lease rule 3 as written: acquire only if none is active (or ours), write,
/// read back, and believe only if the record names our session.
async fn acquire(s: &MatrixStore, doc: &DocId, session: &str, base: &str) -> (bool, bool, Option<u64>) {
    let cur = s.lease(doc).await.unwrap();
    let now = s.now_ms(doc).await.unwrap();
    if let Some(c) = &cur {
        if c.body.state == LeaseState::Held && c.ts_ms + c.body.duration_ms.unwrap_or(0) > now && c.body.holder.as_ref().map(|h| h.session.as_str()) != Some(session) {
            return (false, false, None);
        }
    }
    let written = s.write_lease(doc, held(s, session, base, renewal_over(cur.as_ref().map(|c| &c.body)))).await.unwrap();
    let back = s.lease(doc).await.unwrap().unwrap();
    (back.body.holder.as_ref().map(|h| h.session.as_str()) == Some(session), true, Some(written.ts_ms))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn m07_lease_race() {
    let Some(syn) = require_synapse!() else { return };
    let alice = syn.user_with("alice", |c| c.events_per_sec = 0.0).await;
    let bob = syn.user_with("bob", |c| c.events_per_sec = 0.0).await;
    let doc = alice.create_document(&meta("Race"), Some(file(b"v1"))).await.unwrap();
    alice.grant(&doc, &bob.principal(), Role::Editor).await.unwrap();
    let base = alice.versions(&doc).await.unwrap().remove(0).id;
    alice.write_lease(&doc, held(&alice, "warm-a", &base, 0)).await.unwrap();
    bob.write_lease(&doc, held(&bob, "warm-b", &base, 1)).await.unwrap();

    let rounds = if is_default(&syn) { 4 } else { 40 };
    let mut tally = json!({"both_believe_at_first_read_back": 0, "exactly_one_believes": 0, "none_believes": 0, "one_refused_at_check": 0, "settled_agree": 0, "settled_holder_is_a_believer": 0});
    let mut rows = Vec::new();
    for round in 0..rounds {
        alice.write_lease(&doc, released()).await.unwrap();
        sleep_ms(if is_default(&syn) { 6000 } else { 1100 }).await;
        // Half the rounds start both at once; half stagger bob by 0-120 ms.
        let offset = if round % 2 == 0 { 0 } else { (round as u64 * 37) % 120 };
        let (sa, sb) = (format!("a{round}"), format!("b{round}"));
        let (ra, rb) = tokio::join!(acquire(&alice, &doc, &sa, &base), async {
            sleep_ms(offset).await;
            acquire(&bob, &doc, &sb, &base).await
        });
        sleep_ms(500).await;
        let (fa, fb) = (alice.lease(&doc).await.unwrap().unwrap(), bob.lease(&doc).await.unwrap().unwrap());
        let winner = fa.body.holder.as_ref().unwrap().session.clone();
        let believers = [ra.0, rb.0].iter().filter(|b| **b).count();
        let mut bump = |k: &str| tally[k] = json!(tally[k].as_u64().unwrap() + 1);
        match believers {
            2 => bump("both_believe_at_first_read_back"),
            1 => bump("exactly_one_believes"),
            _ => bump("none_believes"),
        }
        if !ra.1 || !rb.1 {
            bump("one_refused_at_check");
        }
        if fa.id == fb.id {
            bump("settled_agree");
        }
        if (ra.0 && winner == sa) || (rb.0 && winner == sb) {
            bump("settled_holder_is_a_believer");
        }
        let ts_gap = match (ra.2, rb.2) {
            (Some(a), Some(b)) => Some(a as i64 - b as i64),
            _ => None,
        };
        rows.push(json!({"round": round, "bob_offset_ms": offset, "alice_believes": ra.0, "bob_believes": rb.0, "alice_wrote": ra.1, "bob_wrote": rb.1, "write_ts_gap_alice_minus_bob_ms": ts_gap, "settled_winner": winner}));
    }
    record(&syn, "lease_race", json!({ "rounds": rounds, "tally": tally, "rows": rows }));
}

// ---------------------------------------------------------------------------
// 8. Member removal: does the removed device read the next version?
// ---------------------------------------------------------------------------

/// One trial: owner, a to-be-removed viewer, an editor and (for the control)
/// the owner/editor who stays. Returns what the removed device could do.
async fn removal_trial(syn: &Synapse, variant: &str, trial: usize) -> Value {
    let alice = syn.user_with("alice", |c| c.events_per_sec = 0.0).await;
    let bob = syn.user("bob").await;
    let guard_off = variant.ends_with("guard_off");
    let ed = syn.user_with("ed", |c| {
        c.events_per_sec = 0.0;
        c.guard_stale_members = !guard_off;
    })
    .await;
    let doc = alice.create_document(&meta("Removal"), Some(file(&noise(1500, 71)))).await.unwrap();
    alice.grant(&doc, &bob.principal(), Role::Viewer).await.unwrap();
    alice.grant(&doc, &ed.principal(), Role::Editor).await.unwrap();
    let v1 = alice.versions(&doc).await.unwrap().remove(0);
    bob.versions(&doc).await.unwrap();
    ed.versions(&doc).await.unwrap();

    let t = Instant::now();
    let upd = serde_json::to_value(update(7, 300)).unwrap();
    let (new_id, publisher_is_owner, revoke_ms): (String, bool, f64) = match variant {
        "owner_revoke_then_owner_publishes" => {
            alice.revoke(&doc, &bob.principal()).await.unwrap();
            let r = ms(t.elapsed());
            (alice.publish_update(&doc, update(7, 300)).await.unwrap().id, true, r)
        }
        "owner_kick_then_owner_publishes_with_preflight" => {
            let room = alice.client().get_room(&RoomId::parse(&doc).unwrap()).unwrap();
            room.kick_user(&dcp_matrix::matrix_sdk::ruma::UserId::parse(bob.principal()).unwrap(), None).await.unwrap();
            let r = ms(t.elapsed());
            (alice.publish_update(&doc, update(7, 300)).await.unwrap().id, true, r)
        }
        "owner_kick_then_owner_send_queued_no_preflight" => {
            let room = alice.client().get_room(&RoomId::parse(&doc).unwrap()).unwrap();
            room.kick_user(&dcp_matrix::matrix_sdk::ruma::UserId::parse(bob.principal()).unwrap(), None).await.unwrap();
            let r = ms(t.elapsed());
            (alice.send_queued(&doc, state::UPDATE, upd).await.unwrap(), true, r)
        }
        "owner_revoke_then_editor_send_queued_no_preflight" => {
            alice.revoke(&doc, &bob.principal()).await.unwrap();
            let r = ms(t.elapsed());
            (ed.send_queued(&doc, state::UPDATE, upd).await.unwrap(), false, r)
        }
        "owner_kick_no_wait_then_editor_send_queued_no_preflight" => {
            let room = alice.client().get_room(&RoomId::parse(&doc).unwrap()).unwrap();
            room.kick_user(&dcp_matrix::matrix_sdk::ruma::UserId::parse(bob.principal()).unwrap(), None).await.unwrap();
            let r = ms(t.elapsed());
            (ed.send_queued(&doc, state::UPDATE, upd).await.unwrap(), false, r)
        }
        "owner_revoke_then_editor_publishes_guard_off" | "owner_revoke_then_editor_publishes_guard_on" => {
            alice.revoke(&doc, &bob.principal()).await.unwrap();
            let r = ms(t.elapsed());
            (ed.publish_update(&doc, update(7, 300)).await.unwrap().id, false, r)
        }
        other => panic!("unknown variant {other}"),
    };
    let enc = server_event(syn, &alice, &doc, &new_id).await.unwrap();
    let s_old = session_id_of(&alice, &doc, &v1.id).await;
    let s_new = session_id_of(&alice, &doc, &new_id).await;
    json!({
        "variant": variant, "trial": trial, "remove_call_ms": revoke_ms,
        "removed_device_decrypts_event_sent_after_removal": can_decrypt(&bob, &doc, &enc).await,
        "control_remaining_member_decrypts": can_decrypt(if publisher_is_owner { &ed } else { &alice }, &doc, &enc).await,
        "megolm_session_rotated": s_old != s_new,
        "removed_device_decrypts_event_sent_before": can_decrypt(&bob, &doc, &server_event(syn, &alice, &doc, &v1.id).await.unwrap()).await,
        "http_status_removed_user_fetching_new_event": server_event(syn, &bob, &doc, &new_id).await.err(),
        "removed_store_versions_call": match bob.versions(&doc).await { Ok(v) => format!("Ok({} versions)", v.len()), Err(e) => format!("Err({e:?})").chars().take(90).collect() },
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn m08_member_removal() {
    let Some(syn) = require_synapse!() else { return };
    let trials = if is_default(&syn) { 2 } else { 10 };
    let variants = [
        "owner_revoke_then_owner_publishes",
        "owner_kick_then_owner_publishes_with_preflight",
        "owner_kick_then_owner_send_queued_no_preflight",
        "owner_revoke_then_editor_send_queued_no_preflight",
        "owner_kick_no_wait_then_editor_send_queued_no_preflight",
        "owner_revoke_then_editor_publishes_guard_off",
        "owner_revoke_then_editor_publishes_guard_on",
    ];
    let mut rows = Vec::new();
    let mut summary = Vec::new();
    for v in variants {
        let mut leaks = 0;
        for t in 0..trials {
            let r = removal_trial(&syn, v, t).await;
            if r["removed_device_decrypts_event_sent_after_removal"] == true {
                leaks += 1;
            }
            rows.push(r);
        }
        summary.push(json!({"variant": v, "trials": trials, "removed_device_could_decrypt": leaks}));
    }
    record(&syn, "member_removal", json!({ "summary": summary, "rows": rows }));
}

// ---------------------------------------------------------------------------
// 9. Offline and back: this binding's queue, and matrix-sdk's own send queue.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn m09_offline_then_online() {
    let Some(syn) = require_synapse!() else { return };
    let host = syn.url.trim_start_matches("http://").trim_end_matches('/').to_owned();
    let proxy = Proxy::start(&host).await;
    let alice = syn.user_with("alice", |c| c.events_per_sec = 0.0).await;
    let reg_p = syn.register("phone").await;
    let phone = syn.connect_as(&reg_p, |c| {
        c.homeserver = proxy.url.clone();
        c.events_per_sec = 0.0;
        c.backoff_min = Duration::from_millis(300);
        c.backoff_max = Duration::from_secs(3);
    })
    .await;
    let doc = alice.create_document(&meta("Offline"), Some(file(b"v1"))).await.unwrap();
    alice.grant(&doc, &phone.principal(), Role::Editor).await.unwrap();
    phone.versions(&doc).await.unwrap();

    // --- A. this binding's queue ---
    // A publish call during the outage fails fast (its preflight read cannot reach the server).
    proxy.cut();
    let t = Instant::now();
    let preflight_err = phone.publish_update(&doc, update(900, 100)).await.err().map(|e| format!("{e:?}"));
    let preflight_ms = ms(t.elapsed());
    // Events already in the queue when the network drops are retried, in order, exactly once.
    let mut handles = Vec::new();
    for i in 0..3u64 {
        let phone = phone.clone();
        let doc = doc.clone();
        let content = serde_json::to_value(update(i, 200)).unwrap();
        handles.push(tokio::spawn(async move { phone.send_queued(&doc, state::UPDATE, content).await }));
        sleep_ms(50).await;
    }
    sleep_ms(6000).await;
    let still_pending = handles.iter().filter(|h| !h.is_finished()).count();
    let retries_during_outage = phone.stats().network_retries;
    let t = Instant::now();
    proxy.restore();
    let mut ids = Vec::new();
    for h in handles {
        ids.push(tokio::time::timeout(Duration::from_secs(60), h).await.expect("queued event sent after restore").unwrap().expect("send ok"));
    }
    let drain_ms = ms(t.elapsed());
    let seen = alice.updates(&doc).await.unwrap();
    let mut seqs: Vec<(u64, u64)> = seen.iter().filter(|u| u.sender == phone.principal()).map(|u| (u.ts_ms, u.body.seq)).collect();
    seqs.sort();
    let order: Vec<u64> = seqs.iter().map(|s| s.1).collect();
    let mine = &seen.iter().filter(|u| u.sender == phone.principal()).count();
    let a = json!({
        "publish_during_outage_error": preflight_err, "publish_during_outage_returned_after_ms": preflight_ms,
        "queued_events_still_pending_after_6s_outage": still_pending,
        "network_retries_counted_during_outage": retries_during_outage,
        "drain_after_restore_ms": drain_ms,
        "events_from_phone_visible_to_alice": mine, "arrival_order_by_origin_server_ts": order,
        "distinct_event_ids": ids.iter().collect::<std::collections::HashSet<_>>().len(),
    });

    // --- B. matrix-sdk's send queue ---
    let room = phone.client().get_room(&RoomId::parse(&doc).unwrap()).unwrap();
    let sq = room.send_queue();
    let (_, mut updates) = sq.subscribe().await.unwrap();
    let mut errors = phone.client().send_queue().subscribe_errors();
    proxy.cut();
    for i in 0..3u64 {
        let content = serde_json::to_value(update(100 + i, 200)).unwrap();
        let raw: Raw<AnyMessageLikeEventContent> = Raw::from_json(serde_json::value::to_raw_value(&content).unwrap());
        sq.send_raw(raw, state::UPDATE.to_owned()).await.expect("enqueue locally");
    }
    let mut log: Vec<String> = Vec::new();
    let t0 = Instant::now();
    let mut sent = Vec::new();
    let mut first_error_ms = None;
    let mut recoverable = None;
    let outage_end = Instant::now() + Duration::from_secs(8);
    while Instant::now() < outage_end {
        tokio::select! {
            u = updates.recv() => {
                if let Ok(u) = u {
                    let name = format!("{u:?}");
                    log.push(format!("+{:.0}ms {}", ms(t0.elapsed()), name.split(['(', '{', ' ']).next().unwrap_or("?")));
                }
            }
            e = errors.recv() => {
                if let Ok(e) = e { first_error_ms.get_or_insert(ms(t0.elapsed())); recoverable = Some(e.is_recoverable); }
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
    let enabled_after_outage_room = sq.is_enabled();
    let enabled_after_outage_global = phone.client().send_queue().is_enabled();
    proxy.restore();
    let t1 = Instant::now();
    // Without touching the queue: does it recover by itself?
    let wait_end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < wait_end && sent.len() < 3 {
        if let Ok(Ok(u)) = tokio::time::timeout(Duration::from_millis(500), updates.recv()).await {
            if let dcp_matrix::matrix_sdk::send_queue::RoomSendQueueUpdate::SentEvent { event_id, .. } = &u {
                sent.push((event_id.to_string(), ms(t1.elapsed())));
            }
            log.push(format!("+{:.0}ms(after restore) {}", ms(t0.elapsed()), format!("{u:?}").split(['(', '{', ' ']).next().unwrap_or("?")));
        }
    }
    let recovered_unaided = sent.len();
    // Then enable it by hand.
    phone.client().send_queue().set_enabled(true).await;
    sq.set_enabled(true);
    let t2 = Instant::now();
    let wait_end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < wait_end && sent.len() < 3 {
        if let Ok(Ok(u)) = tokio::time::timeout(Duration::from_millis(500), updates.recv()).await {
            if let dcp_matrix::matrix_sdk::send_queue::RoomSendQueueUpdate::SentEvent { event_id, .. } = &u {
                sent.push((event_id.to_string(), ms(t2.elapsed())));
            }
            log.push(format!("+{:.0}ms(after set_enabled) {}", ms(t0.elapsed()), format!("{u:?}").split(['(', '{', ' ']).next().unwrap_or("?")));
        }
    }
    let b = json!({
        "first_send_error_after_ms": first_error_ms, "error_is_recoverable": recoverable,
        "room_queue_enabled_after_outage": enabled_after_outage_room, "global_queue_enabled_after_outage": enabled_after_outage_global,
        "sent_without_any_action_after_restore": recovered_unaided,
        "sent_total_after_set_enabled": sent.len(), "sent_at_ms_since_step": sent.iter().map(|s| s.1).collect::<Vec<_>>(),
        "update_log": log,
    });
    record(&syn, "offline_then_online", json!({ "dcp_matrix_queue": a, "matrix_sdk_send_queue": b }));
}

// ---------------------------------------------------------------------------
// 10. Clock: Date header and calibrated clock against origin_server_ts.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn m10_clock_sources() {
    let Some(syn) = require_synapse!() else { return };
    let owner = syn.user_with("alice", |c| c.events_per_sec = 0.0).await; // Calibrated (default)
    let doc = owner.create_document(&meta("Clock"), Some(file(b"v1"))).await.unwrap();
    let mk = |kind: dcp_matrix::ClockSource, prefix: &'static str| {
        let syn = syn.clone();
        async move { syn.user_with(prefix, move |c| { c.clock = kind; c.events_per_sec = 0.0; }).await }
    };
    let date = mk(dcp_matrix::ClockSource::ServerDate, "date").await;
    let probe = mk(dcp_matrix::ClockSource::ProbeEvent, "probe").await;
    for s in [&date, &probe] {
        owner.grant(&doc, &s.principal(), Role::Editor).await.unwrap();
    }
    let n = if is_default(&syn) { 5 } else { 20 };
    let (mut date_err, mut cal_err) = (Vec::new(), Vec::new());
    let (mut date_cost, mut probe_cost, mut cal_cost) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..n {
        let t = Instant::now();
        let p = probe.now_ms(&doc).await.unwrap();
        probe_cost.push(ms(t.elapsed()));
        let t = Instant::now();
        let d = date.now_ms(&doc).await.unwrap();
        date_cost.push(ms(t.elapsed()));
        let t = Instant::now();
        let c = owner.now_ms(&doc).await.unwrap();
        cal_cost.push(ms(t.elapsed()));
        date_err.push(d as f64 - p as f64);
        cal_err.push(c as f64 - p as f64);
        sleep_ms(137).await;
    }
    // Staleness: the owner's calibration is from its last write; sample again after 20 s idle.
    sleep_ms(20_000).await;
    let p = probe.now_ms(&doc).await.unwrap();
    let c = owner.now_ms(&doc).await.unwrap();
    let local = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    record(&syn, "clock_sources", json!({
        "reference": "ProbeEvent: origin_server_ts of an event written for the purpose; the other two are read a few ms after it, so a small positive bias is expected",
        "date_header_minus_reference_ms": summarize(date_err),
        "calibrated_minus_reference_ms": summarize(cal_err),
        "calibrated_minus_reference_after_20s_idle_ms": c as f64 - p as f64,
        "date_header_call_ms": summarize(date_cost), "probe_event_call_ms": summarize(probe_cost), "calibrated_call_ms": summarize(cal_cost),
        "local_clock_minus_server_ms": local - p as i64,
    }));
}

// ---------------------------------------------------------------------------
// 11. Late-joiner matrix: when does history sharing on invite work?
// ---------------------------------------------------------------------------

/// What the late joiner saw after being granted `variant`.
async fn late_trial(syn: &Synapse, variant: &str) -> Value {
    let alice_unlimited = variant.contains("unlimited");
    let alice = syn.user_with("alice", |c| {
        if alice_unlimited {
            c.events_per_sec = 0.0;
        }
    })
    .await;
    let late = syn.user_with("late", |c| c.utd_wait = Duration::from_secs(10)).await;
    let doc = alice.create_document(&meta("Late"), Some(file(b"v1"))).await.unwrap();
    let role = if variant.starts_with("viewer") { Role::Viewer } else { Role::Editor };
    match variant {
        v if v.contains("pl_before_invite") => {
            let snap = alice.snapshot(&doc).await.unwrap();
            let mut pl = snap.power_levels_content.clone();
            pl["users"][late.principal()] = json!(50);
            alice.send_state_unchecked(&doc, "m.room.power_levels", "", pl).await.unwrap();
            alice.grant(&doc, &late.principal(), Role::Editor).await.unwrap();
        }
        v if v.contains("lease_after_invite") => {
            alice.grant(&doc, &late.principal(), role).await.unwrap();
            let base = alice.versions(&doc).await.unwrap().remove(0).id;
            alice.write_lease(&doc, held(&alice, "s", &base, 0)).await.unwrap();
        }
        v if v.contains("read_after_3s") => {
            alice.grant(&doc, &late.principal(), role).await.unwrap();
            sleep_ms(3000).await;
        }
        _ => alice.grant(&doc, &late.principal(), role).await.unwrap(),
    }
    let rep = late.read_report(&doc).await.unwrap();
    let rid = RoomId::parse(&doc).unwrap();
    let pending = late.client().get_pending_key_bundle_details_for_room(&rid).await;
    json!({
        "variant": variant,
        "v1_visible": rep.versions.len(), "undecryptable": rep.undecryptable,
        "bundle_still_pending_on_late_device": match pending { Ok(Some(_)) => "yes", Ok(None) => "no", Err(_) => "error" },
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn m11_late_joiner_matrix() {
    let Some(syn) = require_synapse!() else { return };
    // The SDK says why it did or did not share history, at info/warn level.
    let _ = tracing_subscriber::fmt()
        .with_env_filter("matrix_sdk::room::shared_room_history=debug,matrix_sdk::encryption::tasks=debug,matrix_sdk_crypto::store=info")
        .with_test_writer()
        .try_init();
    let trials = if is_default(&syn) { 1 } else { 3 };
    let variants = [
        "viewer_limited", "viewer_unlimited", "editor_limited", "editor_unlimited",
        "editor_unlimited_pl_before_invite", "viewer_unlimited_lease_after_invite",
        "editor_unlimited_read_after_3s",
    ];
    let mut rows = Vec::new();
    let mut summary = Vec::new();
    for v in variants {
        let mut ok = 0;
        for _ in 0..trials {
            let r = late_trial(&syn, v).await;
            ok += r["v1_visible"].as_u64().unwrap_or(0);
            rows.push(r);
        }
        summary.push(json!({"variant": v, "trials": trials, "v1_visible_in": ok}));
    }
    record(&syn, "late_joiner_matrix", json!({ "summary": summary, "rows": rows }));
}

// ---------------------------------------------------------------------------
// 12. Identical state content from one sender is not a new event.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn m12_state_event_dedup() {
    let Some(syn) = require_synapse!() else { return };
    let alice = syn.user_with("alice", |c| c.events_per_sec = 0.0).await;
    let bob = syn.user_with("bob", |c| c.events_per_sec = 0.0).await;
    let doc = alice.create_document(&meta("Dedup"), Some(file(b"v1"))).await.unwrap();
    alice.grant(&doc, &bob.principal(), Role::Editor).await.unwrap();
    let base = alice.versions(&doc).await.unwrap().remove(0).id;
    let l = held(&alice, "s", &base, 0);
    let content = serde_json::to_value(&l).unwrap();
    let a = alice.send_state_unchecked(&doc, state::LEASE, "", content.clone()).await.unwrap();
    sleep_ms(1100).await;
    let b = alice.send_state_unchecked(&doc, state::LEASE, "", content.clone()).await.unwrap();
    let c = bob.send_state_unchecked(&doc, state::LEASE, "", content.clone()).await.unwrap();
    let d = alice.send_state_unchecked(&doc, state::LEASE, "", content).await.unwrap();
    // The binding's renewal path: the record carries renewal + 1, so it is not
    // identical content and Synapse creates a new event.
    let w1 = alice.write_lease(&doc, Lease { renewal: Some(1), ..l.clone() }).await.unwrap();
    sleep_ms(1100).await;
    let w2 = alice.write_lease(&doc, Lease { renewal: Some(2), ..l.clone() }).await.unwrap();
    record(&syn, "state_event_dedup", json!({
        "same_sender_same_content_twice_returns_same_event_id": a == b,
        "other_sender_same_content_is_a_new_event": c != b,
        "same_sender_again_after_other_sender_is_a_new_event": d != c,
        "binding_renewal_new_event_and_later_ts": w1.id != w2.id && w2.ts_ms > w1.ts_ms,
        "binding_renewal_counters": [w1.body.renewal, w2.body.renewal],
    }));
}
