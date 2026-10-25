//! Measurements for `docs/rfc/0004-spike-b-results.md`. Every test here is
//! `#[ignore]`d (they take minutes) and is run by the CI job with
//! `--ignored --test-threads=1`. Each prints `MEASURE <name> <json>` and, when
//! `DCP_MEASURE_OUT` is set, appends the same line to that file.

use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dcp_conformance::{new_file, standard_meta, ALGORITHM, BASE_FILE, DOC_MODEL};
use dcp_core::updates::StreamWriter;
use dcp_core::{DocumentStore, Role};
use dcp_streams::http::{HttpError, JSON, OCTETS, START};
use dcp_streams::{crypto, Identity, StreamsConfig, StreamsStore};
use serde_json::json;

use crate::common::*;
use crate::properties::{raw, Rig};
use crate::require_streams;

pub fn emit(name: &str, v: serde_json::Value) {
    let line = format!("MEASURE {name} {v}");
    println!("{line}");
    if let Ok(path) = std::env::var("DCP_MEASURE_OUT") {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

pub fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i]
}

pub fn summary(mut v: Vec<f64>) -> serde_json::Value {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    json!({
        "n": v.len(),
        "min": pct(&v, 0.0), "p50": pct(&v, 0.5), "p95": pct(&v, 0.95), "max": pct(&v, 1.0),
        "mean": v.iter().sum::<f64>() / v.len().max(1) as f64,
    })
}

pub fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn max_mb() -> usize {
    std::env::var("DCP_MEASURE_MAX_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16)
}

/// The byte position inside the stream a `Stream-Next-Offset` names. The
/// reference server's offsets are `<read-seq>_<byte-position>`; the protocol
/// says offsets are opaque, so this is a measurement aid and nothing else.
fn byte_pos(offset: &str) -> u64 {
    offset
        .rsplit('_')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

// ------------------------------------------------------------------ latency

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn latency_append_to_a_second_client() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    let n = rig.epoch().await;
    let upd = rig.path(n, "upd").await;
    let doc = rig.doc.clone();

    // Client B waits on the update stream with a long poll, as a live editor would.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(usize, Instant, Instant)>();
    let viewer = rig.viewer.clone();
    let hb = viewer.http().fork();
    let start_off = hb.head(&upd).await.unwrap().next;
    let upd2 = upd.clone();
    let b = tokio::spawn(async move {
        let mut at = start_off;
        let mut seen = 0usize;
        loop {
            let r = match hb.long_poll(&upd2, &at, true).await {
                Ok(r) => r,
                Err(_) => return,
            };
            let t_raw = Instant::now();
            at = r.next;
            if r.items.is_empty() {
                continue;
            }
            // Fully received: now what a client would do: read and verify through the store.
            let got = viewer.updates(&doc).await.map(|u| u.len()).unwrap_or(0);
            let t_store = Instant::now();
            for _ in 0..r.items.len() {
                seen += 1;
                let _ = tx.send((seen, t_raw, t_store));
            }
            let _ = got;
        }
    });

    let mut w = StreamWriter::new(ALGORITHM, DOC_MODEL);
    let mut post = Vec::new();
    let mut raw_lat = Vec::new();
    let mut store_lat = Vec::new();
    // warm up connections and the clock
    for i in 0..30 {
        rig.editor
            .publish_update(
                &rig.doc,
                w.next(&rig.first, format!("w{i}").as_bytes()).unwrap(),
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    while rx.try_recv().is_ok() {}
    let mut sent = Vec::new();
    for i in 0..200 {
        let t0 = Instant::now();
        rig.editor
            .publish_update(
                &rig.doc,
                w.next(&rig.first, format!("m{i:04}-{}", "x".repeat(90)).as_bytes())
                    .unwrap(),
            )
            .await
            .unwrap();
        post.push(ms(t0.elapsed()));
        sent.push(t0);
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
    let mut got = 0;
    while let Ok(Some((k, t_raw, t_store))) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv()).await
    {
        if let Some(t0) = sent.get(k.wrapping_sub(31)) {
            // `seen` counts from the start of the measured batch because the warm-up was drained
            raw_lat.push(ms(t_raw.duration_since(*t0)));
            store_lat.push(ms(t_store.duration_since(*t0)));
        }
        got += 1;
        if got >= 200 {
            break;
        }
    }
    b.abort();
    emit(
        "latency",
        json!({
            "payload_bytes": 100,
            "append_call_ms (publish_update returns: refresh ctl + encrypt + sign + POST)": summary(post),
            "to_long_poll_wakeup_ms (A starts the append -> B's long poll returns)": summary(raw_lat),
            "to_verified_in_B_ms (A starts -> B's store.updates() has decrypted and verified it)": summary(store_lat),
        }),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn latency_of_the_server_alone_for_a_bare_append() {
    let base = require_streams!();
    let http = crate::common::http(&base);
    let path = format!("dcp/bare-{}/s", hex::encode(crypto::random::<6>()));
    http.create(&path, JSON, None, false).await.unwrap();
    let mut v = Vec::new();
    for i in 0..300 {
        let body = format!("{{\"i\":{i},\"pad\":\"{}\"}}", "x".repeat(600)).into_bytes();
        let t0 = Instant::now();
        http.append(&path, JSON, body, None, false).await.unwrap();
        v.push(ms(t0.elapsed()));
    }
    emit(
        "bare_append_ms",
        json!({"payload_bytes": 620, "summary": summary(v)}),
    );
}

// ------------------------------------------------------------------ catch-up

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn catch_up_time_for_n_updates_with_and_without_a_snapshot() {
    let base = require_streams!();
    for n in [100usize, 1000, 5000] {
        let rig = Rig::new(&base).await;
        let mut w = StreamWriter::new(ALGORITHM, DOC_MODEL);
        let t0 = Instant::now();
        let mut truth = BASE_FILE.to_vec();
        for i in 0..n {
            let p = format!("{i:05}-{}", "x".repeat(94)); // 100 bytes
            truth.extend_from_slice(p.as_bytes());
            rig.editor
                .publish_update(&rig.doc, w.next(&rig.first, p.as_bytes()).unwrap())
                .await
                .unwrap();
        }
        let write_s = t0.elapsed().as_secs_f64();
        let cover = rig
            .editor
            .publish_covering_version(
                &rig.doc,
                vec![rig.first.clone()],
                new_file(&truth),
                None,
                ALGORITHM,
            )
            .await
            .unwrap();
        for i in 0..10 {
            rig.editor
                .publish_update(
                    &rig.doc,
                    w.next(&cover.id, format!("t{i}").as_bytes()).unwrap(),
                )
                .await
                .unwrap();
        }
        // New members, one per reader, so that every reader starts cold.
        let mut full_ms = Vec::new();
        let mut snap_ms = Vec::new();
        let (mut full_cost, mut snap_cost) = (None, None);
        for round in 0..5 {
            for snapshots in [false, true] {
                let mut cfg = StreamsConfig::new("COLD");
                cfg.use_snapshots = snapshots;
                let s = Arc::new(StreamsStore::new(
                    crate::common::http(&base),
                    Identity::generate(),
                    cfg,
                ));
                s.calibrate().await.unwrap();
                rig.owner
                    .grant(&rig.doc, &s.principal(), Role::Viewer)
                    .await
                    .unwrap();
                let before = s.http().snapshot();
                let t = Instant::now();
                let ups = s.updates(&rig.doc).await.unwrap();
                let took = ms(t.elapsed());
                let after = s.http().snapshot();
                let cost = json!({"requests": after.requests - before.requests, "bytes": after.bytes_received - before.bytes_received, "updates_read": ups.len()});
                if snapshots {
                    snap_ms.push(took);
                    snap_cost = Some(cost);
                } else {
                    full_ms.push(took);
                    full_cost = Some(cost);
                }
                let _ = round;
            }
        }
        emit(
            "catch_up_updates",
            json!({
                "n_updates": n, "update_payload_bytes": 100,
                "writer_seconds_for_all_updates": write_s,
                "without_snapshot_ms": summary(full_ms), "without_snapshot_cost": full_cost,
                "with_snapshot_ms": summary(snap_ms), "with_snapshot_cost": snap_cost,
            }),
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn catch_up_time_for_n_versions() {
    let base = require_streams!();
    for n in [50usize, 200, 500] {
        let rig = Rig::new(&base).await;
        let mut parent = rig.first.clone();
        let t0 = Instant::now();
        for i in 0..n {
            parent = rig
                .editor
                .publish_version(
                    &rig.doc,
                    vec![parent],
                    new_file(format!("version {i}").as_bytes()),
                    None,
                )
                .await
                .unwrap()
                .id;
        }
        let write_s = t0.elapsed().as_secs_f64();
        let mut v = Vec::new();
        let mut cost = None;
        for _ in 0..5 {
            let s = Arc::new(StreamsStore::new(
                crate::common::http(&base),
                Identity::generate(),
                StreamsConfig::new("COLD"),
            ));
            s.calibrate().await.unwrap();
            rig.owner
                .grant(&rig.doc, &s.principal(), Role::Viewer)
                .await
                .unwrap();
            let before = s.http().snapshot();
            let t = Instant::now();
            let vs = s.versions(&rig.doc).await.unwrap();
            v.push(ms(t.elapsed()));
            assert_eq!(vs.len(), n + 1);
            let after = s.http().snapshot();
            cost = Some(
                json!({"requests": after.requests - before.requests, "bytes": after.bytes_received - before.bytes_received}),
            );
        }
        // plus opening the head: one file fetch and decrypt
        let s = Arc::new(StreamsStore::new(
            crate::common::http(&base),
            Identity::generate(),
            StreamsConfig::new("COLD"),
        ));
        s.calibrate().await.unwrap();
        rig.owner
            .grant(&rig.doc, &s.principal(), Role::Viewer)
            .await
            .unwrap();
        let t = Instant::now();
        let view = dcp_core::view::read_view(&*s, &rig.doc).await.unwrap();
        let open_ms = ms(t.elapsed());
        assert_eq!(view.heads.len(), 1);
        emit(
            "catch_up_versions",
            json!({"n_versions": n, "writer_seconds": write_s, "list_versions_ms": summary(v), "cost": cost, "open_head_ms_including_list_and_verify": open_ms}),
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn catch_up_across_many_epochs() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    let mut parent = rig.first.clone();
    for i in 0..20 {
        parent = rig
            .editor
            .publish_version(
                &rig.doc,
                vec![parent],
                new_file(format!("v{i}").as_bytes()),
                None,
            )
            .await
            .unwrap()
            .id;
    }
    let mut rows = Vec::new();
    let mut have = 3usize;
    for target in [10usize, 25, 50] {
        while have < target {
            rig.owner
                .grant(&rig.doc, &store_at(&base, "m").principal(), Role::Viewer)
                .await
                .unwrap();
            have += 1;
        }
        let mut v = Vec::new();
        let mut cost = None;
        for _ in 0..5 {
            let s = Arc::new(StreamsStore::new(
                crate::common::http(&base),
                Identity::generate(),
                StreamsConfig::new("COLD"),
            ));
            s.calibrate().await.unwrap();
            rig.owner
                .grant(&rig.doc, &s.principal(), Role::Viewer)
                .await
                .unwrap();
            have += 1;
            let before = s.http().snapshot();
            let t = Instant::now();
            let vs = s.versions(&rig.doc).await.unwrap();
            let us = s.updates(&rig.doc).await.unwrap();
            v.push(ms(t.elapsed()));
            assert_eq!((vs.len(), us.len()), (21, 0));
            let after = s.http().snapshot();
            cost = Some(
                json!({"requests": after.requests - before.requests, "bytes": after.bytes_received - before.bytes_received}),
            );
        }
        rows.push(json!({"members_so_far": have, "epochs": have, "cold_reader_versions_plus_updates_ms": summary(v), "cost": cost}));
    }
    emit(
        "catch_up_across_many_epochs (20 versions, no updates)",
        json!({"rows": rows}),
    );
}

// ------------------------------------------------------------------ overhead

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn record_overhead_in_bytes() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    let n = rig.epoch().await;
    let upd = rig.path(n, "upd").await;
    let mut rows = Vec::new();
    let mut w = StreamWriter::new(ALGORITHM, DOC_MODEL);
    for payload in [0usize, 1, 100, 1000, 10_000, 34_000] {
        let before = rig.owner.http().head(&upd).await.unwrap().next;
        let bytes = vec![b'p'; payload];
        let u = w.next(&rig.first, &bytes).unwrap();
        let schema_json = serde_json::to_vec(&u).unwrap().len();
        rig.editor.publish_update(&rig.doc, u).await.unwrap();
        let after = rig.owner.http().head(&upd).await.unwrap().next;
        let msgs = raw(&rig.owner, &upd).await;
        let last = msgs.last().unwrap();
        let record_json = serde_json::to_vec(last).unwrap().len();
        rows.push(json!({
            "payload_bytes": payload,
            "dcp_update_object_json_bytes": schema_json,
            "record_json_bytes_on_the_wire": record_json,
            "server_stored_bytes (Stream-Next-Offset delta)": byte_pos(&after) - byte_pos(&before),
            "overhead_bytes_over_payload": record_json as i64 - payload as i64,
            "record_bytes_per_payload_byte": if payload > 0 { record_json as f64 / payload as f64 } else { 0.0 },
        }));
    }
    // What is in the fixed part: the record header, per field.
    let sample: dcp_streams::wire::Record =
        serde_json::from_value(raw(&rig.owner, &upd).await[0].clone()).unwrap();
    let header = json!({
        "v+k+e (JSON keys and values)": serde_json::to_vec(&json!({"v":1,"k":"upd","e":2})).unwrap().len(),
        "a (author principal string)": sample.a.len(),
        "n (nonce, base64url of 24 bytes)": sample.n.len(),
        "s (Ed25519 signature, base64url of 64 bytes)": sample.s.len(),
        "c_tag (AEAD tag, bytes)": crypto::TAG_LEN,
    });

    // A version and its file.
    let v = rig
        .editor
        .publish_version(
            &rig.doc,
            vec![rig.first.clone()],
            new_file(&vec![b'f'; 10_000]),
            None,
        )
        .await
        .unwrap();
    let log = rig.path(n, "log").await;
    let last = raw(&rig.owner, &log).await.pop().unwrap();
    let file_id = match &v.body.file.locator {
        dcp_core::Locator::DurableStream { href } => href.rsplit('/').next().unwrap().to_owned(),
        _ => unreachable!(),
    };
    let blob = rig
        .owner
        .http()
        .read_bytes(&format!("dcp/{}/f/{file_id}", rig.doc))
        .await
        .unwrap();
    // Epoch records.
    let mut epoch_rows = Vec::new();
    let members_target = [1usize, 5, 20, 50];
    let mut have = 3usize; // owner, editor, viewer
    let mut t_rot = Vec::new();
    for target in members_target {
        while have < target {
            let t = Instant::now();
            rig.owner
                .grant(&rig.doc, &store_at(&base, "m").principal(), Role::Viewer)
                .await
                .unwrap();
            t_rot.push((have + 1, ms(t.elapsed())));
            have += 1;
        }
        if target < 3 {
            continue;
        }
        let ctl = format!("dcp/{}/ctl", rig.doc);
        let all = raw(&rig.owner, &ctl).await;
        let last_epoch = all.last().unwrap();
        epoch_rows.push(json!({"members": have, "epoch_record_json_bytes": serde_json::to_vec(last_epoch).unwrap().len()}));
    }
    emit(
        "record_overhead",
        json!({
            "update_rows": rows, "record_header_fixed_parts": header,
            "version_record_json_bytes (10 kB file, one parent)": serde_json::to_vec(&last).unwrap().len(),
            "file_blob": {"plaintext_bytes": 10_000, "stored_bytes": blob.len(), "overhead_bytes": blob.len() - 10_000},
            "epoch_record_by_roster_size": epoch_rows,
            "rotation_ms_by_roster_size (grant of the Nth member: create streams + ctl append + seals)": t_rot,
        }),
    );
}

// ------------------------------------------------------------------ limits

async fn alive(base: &str) -> bool {
    crate::common::http(base)
        .head("dcp/nothing-here")
        .await
        .map(|_| true)
        .unwrap_or_else(|e| matches!(e, HttpError::NotFound))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn the_largest_record_and_file_the_server_accepts() {
    let base = require_streams!();
    let http = crate::common::http(&base);
    let cap = max_mb();
    let mut rows = Vec::new();
    // A JSON message of this many bytes (a string field), then a closed octet stream of the same size.
    for mb in [1usize, 4, 16, 32, 64, 128, 256, 512] {
        if mb > cap {
            break;
        }
        let size = mb * 1024 * 1024;
        let id = hex::encode(crypto::random::<6>());
        let jpath = format!("dcp/limit-{id}/j");
        http.create(&jpath, JSON, None, false).await.unwrap();
        let mut body = Vec::with_capacity(size + 16);
        body.extend_from_slice(b"{\"p\":\"");
        body.resize(size, b'a');
        body.extend_from_slice(b"\"}");
        let t = Instant::now();
        let json_res = http.append(&jpath, JSON, body, None, false).await;
        let json_ms = ms(t.elapsed());
        let json_read = match &json_res {
            Ok(_) => {
                let t = Instant::now();
                let r = http.read_all(&jpath, START).await;
                Some((
                    r.map(|x| x.0.len()).map_err(|e| e.to_string()),
                    ms(t.elapsed()),
                ))
            }
            Err(_) => None,
        };
        let opath = format!("dcp/limit-{id}/o");
        let t = Instant::now();
        let oct_res = http
            .create(&opath, OCTETS, Some(vec![7u8; size]), true)
            .await;
        let oct_ms = ms(t.elapsed());
        let oct_read = match &oct_res {
            Ok(_) => {
                let t = Instant::now();
                let r = http.read_bytes(&opath).await;
                Some((
                    r.map(|x| x.len()).map_err(|e| e.to_string()),
                    ms(t.elapsed()),
                ))
            }
            Err(_) => None,
        };
        rows.push(json!({
            "mib": mb,
            "json_append": format!("{:?}", json_res.as_ref().map(|_| "ok").map_err(|e| e.to_string())), "json_append_ms": json_ms,
            "json_read_back": format!("{json_read:?}"),
            "octet_create_closed": format!("{:?}", oct_res.as_ref().map(|_| "ok").map_err(|e| e.to_string())), "octet_ms": oct_ms,
            "octet_read_back": format!("{oct_read:?}"),
            "server_still_up": alive(&base).await,
        }));
        let _ = http.delete(&jpath).await;
        let _ = http.delete(&opath).await;
        if json_res.is_err() && oct_res.is_err() {
            break;
        }
    }
    emit("limits", json!({"rows": rows, "cap_mib_tried": cap}));

    // Through the binding: the largest file a client uploads (encrypt, upload, read back, decrypt).
    let rig = Rig::new(&base).await;
    let mut file_rows = Vec::new();
    for mb in [1usize, 8, 32] {
        if mb > cap {
            break;
        }
        let bytes = vec![0x5au8; mb * 1024 * 1024];
        let t = Instant::now();
        let r = rig
            .editor
            .publish_version(&rig.doc, vec![rig.first.clone()], new_file(&bytes), None)
            .await;
        file_rows.push(json!({"mib": mb, "result": format!("{:?}", r.as_ref().map(|_| "published").map_err(|e| e.to_string())), "publish_ms (encrypt+upload+read back+decrypt+compare)": ms(t.elapsed())}));
    }
    emit("limits_through_the_binding", json!({"rows": file_rows}));
}

// ------------------------------------------------------------------ time

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn the_clock_before_and_after_calibration() {
    let base = require_streams!();
    let mut cold = Vec::new();
    let mut warm = Vec::new();
    let mut calibrate_ms = Vec::new();
    for i in 0..40 {
        let s = store_at(&base, "CLOCK");
        // One request, as a viewer that has never written would have.
        let _ = s.http().head("dcp/nothing-here").await;
        let est = s.http().clock.estimate().unwrap() as i64;
        cold.push((est - dcp_streams::clock::local_wall_ms() as i64) as f64);
        let t = Instant::now();
        s.calibrate().await.unwrap();
        calibrate_ms.push(ms(t.elapsed()));
        tokio::time::sleep(Duration::from_millis(37 + (i % 7) * 71)).await;
        let est = s.now_ms(&"x".to_owned()).await.unwrap() as i64;
        warm.push((est - dcp_streams::clock::local_wall_ms() as i64) as f64);
    }
    emit(
        "clock_error_ms (estimate minus the host's clock; server and test share a host, so this is the estimate's error)",
        json!({"after_one_response": summary(cold), "after_calibration": summary(warm), "calibration_duration_ms": summary(calibrate_ms)}),
    );
}

// ------------------------------------------------------------------ cost of the lease protocol

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn lease_acquire_and_renew_latency() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    let mut acquire = Vec::new();
    let mut renew = Vec::new();
    let pause = Pause;
    let st = dcp_core::lease::Settle::new(&pause, 0);
    for round in 0..20 {
        let claim = dcp_core::lease::Claim::new(
            rig.editor.principal(),
            &format!("s{round}"),
            "t",
            &rig.first,
            60_000,
        );
        let t = Instant::now();
        let got = dcp_core::lease::acquire(&*rig.editor, &rig.doc, &claim, &st)
            .await
            .unwrap();
        acquire.push(ms(t.elapsed()));
        assert!(got.is_acquired(), "{got:?}");
        let t = Instant::now();
        let again = dcp_core::lease::acquire(&*rig.editor, &rig.doc, &claim, &st)
            .await
            .unwrap();
        renew.push(ms(t.elapsed()));
        assert!(again.is_acquired());
        dcp_core::lease::release(&*rig.editor, &rig.doc, &claim.holder)
            .await
            .unwrap();
    }
    emit(
        "lease_ms (settle interval 0: the protocol cost without the 500 ms wait)",
        json!({"acquire": summary(acquire), "renew": summary(renew)}),
    );
}

struct Pause;
#[async_trait::async_trait]
impl dcp_core::lease::Pause for Pause {
    async fn pause(&self, _ms: u64) {}
}

#[allow(dead_code)]
fn _standard() -> dcp_core::Meta {
    standard_meta()
}
