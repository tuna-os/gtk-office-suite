//! How many Matrix events does a real Collabora editing session generate
//! through `WopiHost<MatrixStore>`, and does that meet Synapse's limits?
//! (Spec section 13, question 10: how a WOPI session's autosaves, exit-saves
//! and lock refreshes fit a rate-limited binding.)
//!
//! A scripted session: Collabora opens a document served from a Matrix room, a
//! user types a short text every `DCP_CHURN_TYPE_EVERY_SECS` (5) seconds for
//! `DCP_CHURN_SECS` (180) seconds and nothing else (no explicit save), then
//! closes the tab; the test then waits for Collabora's `UNLOCK` (at most
//! `DCP_CHURN_TAIL_SECS`, 90). Everything is then counted from the room
//! itself: the owner pages the room's timeline (`GET /rooms/{id}/messages`, which
//! includes the lease state events) and the events stamped after the session
//! started are bucketed by minute and by type, next to what the WOPI host
//! logged (LOCK, PutFile with its autosave and exit-save flags, UNLOCK) and the
//! editor store's own counters (events sent, `M_LIMIT_EXCEEDED` answers, time
//! spent waiting for the limiter). The Collabora configuration under test is
//! whatever `scripts/collabora-up.sh` was started with (the workflow runs it
//! with Collabora's own defaults and with tightened timers); its effective
//! values are printed by the workflow, and `DCP_CHURN_LABEL` names the run.
//!
//! `DCP_CHURN_STRESS=1` is for a Collabora configured to save far more often
//! than its own defaults, to find where Synapse's limit starts to bite: the
//! session is then measured, not required to finish cleanly (what happened to
//! the lease, the PutFiles and the last typed text is printed as
//! `CHURN OUTCOME`), because an overloaded session failing is a result. Only
//! the consistency of the counting itself is still asserted.
//!
//! `DCP_DRAIN_BURST=1` (default Synapse) spends the editor's message budget
//! before the session starts (see `dcp_interop::profile`): the regime where
//! the homeserver's limit is already the binding one, to see what Collabora
//! does when the host's PutFile and LOCK answers are paced by it.
//!
//! Needs both services like `collabora_over_matrix.rs` (and `--test-threads=1`).
//! Meaningful on the default Synapse profile; it runs, and says which profile
//! it ran on, on either.

#[path = "../../dcp-wopi/tests/shared/collabora_driver.rs"]
mod driver;
#[path = "../../dcp-wopi/tests/shared/odf.rs"]
mod odf;

use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dcp_interop::common::{self, Synapse};
use dcp_interop::rig::synapse;
use dcp_interop::{Rig, RigOptions, Who};
use dcp_matrix::MatrixStore;
use driver::*;
use serde_json::{json, Value};

fn base_odt() -> Vec<u8> {
    odf::make_odt("base")
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

/// Every event of the room, newest first, as the homeserver stores it.
async fn room_events(syn: &Synapse, owner: &MatrixStore, room: &str) -> Vec<Value> {
    let token = owner.client().access_token().expect("the owner's access token");
    let http = dcp_matrix::matrix_sdk::reqwest::Client::new();
    let mut out: Vec<Value> = Vec::new();
    let mut from: Option<String> = None;
    loop {
        let mut url = format!("{}/_matrix/client/v3/rooms/{}/messages?dir=b&limit=500", syn.url.trim_end_matches('/'), enc(room));
        if let Some(f) = &from {
            url.push_str(&format!("&from={}", enc(f)));
        }
        let page: Value = http.get(&url).bearer_auth(&token).send().await.expect("GET messages").json().await.expect("messages json");
        let chunk = page["chunk"].as_array().cloned().unwrap_or_default();
        if chunk.is_empty() {
            break;
        }
        out.extend(chunk);
        match page["end"].as_str() {
            Some(e) => from = Some(e.to_owned()),
            None => break,
        }
    }
    out
}

/// A Synapse token bucket (burst `burst`, `per_sec` refill) fed with the
/// events' server timestamps: the first event that would have found it empty,
/// and how many would have. Run over events that already went through the
/// client's queue, so it understates what the client wanted to send.
fn bucket_misses(ts_ms: &[u64], burst: f64, per_sec: f64) -> (usize, Option<u64>) {
    let mut tokens = burst;
    let mut last = ts_ms.first().copied().unwrap_or(0);
    let (mut misses, mut first_miss) = (0, None);
    for &t in ts_ms {
        tokens = (tokens + (t.saturating_sub(last)) as f64 / 1000.0 * per_sec).min(burst);
        last = t;
        if tokens < 1.0 {
            misses += 1;
            first_miss.get_or_insert(t);
        } else {
            tokens -= 1.0;
        }
    }
    (misses, first_miss)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scripted_collabora_session_and_the_homeservers_event_limits() {
    if skip("collabora_churn") {
        return;
    }
    let Some(syn) = synapse() else { return };
    let e = env();
    require_collabora(&e).await;
    let secs = env_u64("DCP_CHURN_SECS", 180);
    let every = env_u64("DCP_CHURN_TYPE_EVERY_SECS", 5);
    let tail = env_u64("DCP_CHURN_TAIL_SECS", 90);
    let label = std::env::var("DCP_CHURN_LABEL").unwrap_or_else(|_| "unlabelled".into());
    let stress = std::env::var("DCP_CHURN_STRESS").as_deref() == Ok("1");
    let rig = Rig::new(&syn, RigOptions::honest().for_collabora(e.host_port, base_odt)).await.expect("rig");
    let owner = rig.native(Who::Owner);
    let editor_store = rig.handles.editor.clone();
    println!(
        "CHURN run '{label}': profile {}, {secs} s of typing every {every} s, tail up to {tail} s, room {}",
        dcp_interop::profile::label(&syn), rig.doc
    );

    let before = editor_store.stats();
    let t0 = now_ms();
    let started = Instant::now();
    let mut s = {
        let src = wopi_src_with_token(&e, &rig.doc, rig.token(Who::Editor));
        open_session(&e, &src).await
    };
    assert!(s.seen(|m| m == "perm: edit"), "Collabora did not grant `perm: edit`");
    let opened_after = started.elapsed();

    // Typing, nothing else: no explicit save, so whatever versions appear are
    // Collabora's own autosave, idle save and exit save.
    let mut last_token = String::new();
    let mut typed = 0u32;
    let typing_started = Instant::now();
    while typing_started.elapsed() < Duration::from_secs(secs) {
        last_token = format!("DcpChurn{typed:03}");
        type_text(&mut s, &format!("{last_token} ")).await;
        typed += 1;
        tokio::time::sleep(Duration::from_secs(every)).await;
    }
    let typing_ended_ms = now_ms();
    s.close().await;
    let closed = Instant::now();
    let mut unlock_after = None;
    while closed.elapsed() < Duration::from_secs(tail) {
        if rig.host.request_log().iter().any(|r| r.operation.as_deref() == Some("UNLOCK")) {
            unlock_after = Some(closed.elapsed());
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    // Collabora gives up on a WOPI call after about 3 s (its log says
    // "Timeout while requesting ... after 3003ms"). A call the host is still
    // working on when the connection drops is not logged as completed, but its
    // effect lands when the homeserver lets the event through. So, when no UNLOCK
    // was seen, wait (up to `DCP_CHURN_EXTRA_SECS`, 120) for the lease to read
    // released before counting; then let anything else the host queued land.
    if unlock_after.is_none() {
        let extra = Instant::now();
        while extra.elapsed() < Duration::from_secs(env_u64("DCP_CHURN_EXTRA_SECS", 120)) {
            if matches!(owner.lease(&rig.doc).await, Ok(Some(l)) if l.body.state == dcp_core::LeaseState::Released) {
                println!("CHURN the lease read released {:.1} s after the tail ended, with no completed UNLOCK in the host's log", extra.elapsed().as_secs_f64());
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    tokio::time::sleep(Duration::from_secs(5)).await;
    let t1 = now_ms();
    let log = rig.host.request_log();
    write_request_log(&log);

    // ---- the host's side
    let mut wopi: BTreeMap<String, u32> = BTreeMap::new();
    let (mut autosave_puts, mut exit_puts, mut plain_puts, mut refused_puts) = (0u32, 0u32, 0u32, 0u32);
    for r in &log {
        let op = r.operation.clone().unwrap_or_else(|| r.method.clone());
        let kind = if r.path.ends_with("/contents") { format!("{op} /contents") } else { op };
        *wopi.entry(format!("{kind} -> {}", r.status)).or_default() += 1;
        if r.operation.as_deref() == Some("PUT") {
            if r.status != 200 {
                refused_puts += 1;
            } else if r.header("x-cool-wopi-isexitsave") == Some("true") {
                exit_puts += 1;
            } else if r.header("x-cool-wopi-isautosave") == Some("true") {
                autosave_puts += 1;
            } else {
                plain_puts += 1;
            }
        }
    }
    let locks = log.iter().filter(|r| r.operation.as_deref() == Some("LOCK")).count();

    // ---- the room's side
    let events = room_events(&syn, owner_store(&rig), &rig.doc).await;
    let mut in_window: Vec<&Value> = events
        .iter()
        .filter(|ev| ev["origin_server_ts"].as_u64().is_some_and(|t| t >= t0 && t <= t1))
        .collect();
    in_window.sort_by_key(|ev| ev["origin_server_ts"].as_u64().unwrap_or(0));
    let minutes = ((t1 - t0) / 60_000 + 1) as usize;
    // [minute] -> (state events, timeline events)
    let mut per_minute = vec![(0u32, 0u32); minutes];
    let mut by_type: BTreeMap<String, u32> = BTreeMap::new();
    let mut by_sender: BTreeMap<String, u32> = BTreeMap::new();
    for ev in &in_window {
        let ts = ev["origin_server_ts"].as_u64().unwrap();
        let m = ((ts - t0) / 60_000) as usize;
        let state = ev.get("state_key").is_some();
        if state {
            per_minute[m].0 += 1;
        } else {
            per_minute[m].1 += 1;
        }
        *by_type.entry(ev["type"].as_str().unwrap_or("?").to_owned()).or_default() += 1;
        *by_sender.entry(ev["sender"].as_str().unwrap_or("?").to_owned()).or_default() += 1;
    }
    let editor_ts: Vec<u64> = in_window
        .iter()
        .filter(|ev| ev["sender"].as_str() == Some(rig.principal(Who::Editor).as_str()))
        .filter_map(|ev| ev["origin_server_ts"].as_u64())
        .collect();
    // Synapse's own default for rc_message: 0.2/s, burst 10 per user.
    let (misses, first_miss) = bucket_misses(&editor_ts, 10.0, 0.2);
    let spent = dcp_interop::profile::stats_since(editor_store.stats(), before);
    let window_secs = (t1 - t0) as f64 / 1000.0;
    let budget = 10.0 + 0.2 * window_secs;
    let versions = rig.native(Who::Owner).versions(&rig.doc).await.expect("versions").len();
    let lease = owner.lease(&rig.doc).await;

    println!("CHURN window: {window_secs:.0} s from the start of the Collabora load (opened after {:.1} s, typing ended at +{:.0} s, UNLOCK {} after the close); typed {typed} times", opened_after.as_secs_f64(), (typing_ended_ms - t0) as f64 / 1000.0, unlock_after.map_or("never seen".into(), |d| format!("{:.1} s", d.as_secs_f64())));
    println!("CHURN events per minute (state = lease records, timeline = encrypted versions/updates):");
    for (i, (st, tl)) in per_minute.iter().enumerate() {
        println!("CHURN   minute {i}: {:>3} state + {:>3} timeline = {:>3}", st, tl, st + tl);
    }
    // When each event landed, seconds after the start of the load: the cadence.
    let offsets: Vec<Value> = in_window
        .iter()
        .map(|ev| {
            let who = if ev["sender"].as_str() == Some(rig.principal(Who::Editor).as_str()) { "editor" } else { "other" };
            let kind = match ev["type"].as_str() {
                Some("org.tunaos.dcp.lease") => "lease",
                Some("m.room.encrypted") => "version",
                other => other.unwrap_or("?"),
            };
            json!([(ev["origin_server_ts"].as_u64().unwrap() - t0) as f64 / 1000.0, kind, who])
        })
        .collect();
    println!("CHURN event offsets (s, kind, sender): {}", Value::Array(offsets.clone()));
    println!("CHURN events in the room during the window: {} ({by_type:?}); by sender {by_sender:?}", in_window.len());
    println!("CHURN WOPI requests the host saw: {wopi:?}");
    println!("CHURN PutFile 200s: {autosave_puts} autosave, {exit_puts} exit-save, {plain_puts} neither; refused PutFiles: {refused_puts}; LOCKs: {locks}; versions in the room now: {versions} (1 before)");
    println!(
        "CHURN editor store counters over the window: {} events sent, {} M_LIMIT_EXCEEDED, {:.1} s waited for the limiter; Synapse's default budget for {window_secs:.0} s is 10 + 0.2 x {window_secs:.0} = {budget:.0} events",
        spent.sent, spent.rate_limited, spent.waited_ms as f64 / 1000.0
    );
    println!("CHURN a token bucket (burst 10, 0.2/s) fed with the editor's {} server timestamps would have been empty for {misses} of them (first at {:?})", editor_ts.len(), first_miss.map(|t| t.saturating_sub(t0)));
    common::record(
        &syn,
        "collabora_churn",
        json!({
            "label": label, "synapse_profile": dcp_interop::profile::label(&syn), "typing_secs": secs, "type_every_secs": every, "typed": typed, "window_secs": window_secs,
            "per_minute_state_timeline": per_minute, "events_by_type": by_type, "events_by_sender": by_sender,
            "wopi_requests": wopi, "put_autosave": autosave_puts, "put_exit_save": exit_puts, "put_other": plain_puts, "put_refused": refused_puts,
            "locks": locks, "versions_after": versions, "unlock_seen": unlock_after.is_some(),
            "editor_events_sent": spent.sent, "editor_429s": spent.rate_limited, "editor_waited_secs": spent.waited_ms as f64 / 1000.0,
            "editor_events_in_room": editor_ts.len(), "budget_events": budget,
            "bucket_misses_by_server_timestamps": misses, "event_offsets": offsets, "stress": stress,
        }),
    );
    let last_token_saved = rig
        .view(Who::Owner)
        .await
        .ok()
        .filter(|v| v.heads.len() == 1)
        .is_some_and(|v| odf::content_xml(&v.heads[0].bytes).contains(&last_token));
    let released = matches!(&lease, Ok(Some(l)) if l.body.state == dcp_core::LeaseState::Released);
    println!(
        "CHURN OUTCOME '{label}': UNLOCK seen {}, lease released {released}, refused PutFiles {refused_puts}, last typed text in the single newest head {last_token_saved}",
        unlock_after.is_some()
    );

    // What must hold whatever the numbers are: the room never holds MORE events
    // from the editor than its store counted as sent, and, unless this is a
    // stress run, exactly as many. (Synapse drops a state event whose content
    // equals the sender's current one. Likely cause of the one event missing in
    // the stress run with a spent budget [INFERENCE, consistent with Collabora's
    // log of two timed-out UNLOCKs]: Collabora gave up on an UNLOCK after 3 s and
    // sent it again while the first was still queued behind the limiter, so the
    // host wrote `released` twice and the second left no event.)
    let in_room = in_window.iter().filter(|ev| ev["sender"].as_str() == Some(rig.principal(Who::Editor).as_str())).count() as u64;
    println!("CHURN counting: {in_room} events from the editor in the room, {} counted as sent by its store", spent.sent);
    assert!(in_room <= spent.sent, "the room holds more events from the editor ({in_room}) than its store counted as sent ({})", spent.sent);
    if !stress {
        assert_eq!(in_room, spent.sent, "the room holds a different number of events from the editor than its store counted as sent");
    }
    if stress {
        return;
    }
    // And, unless this is a stress run, that the session really edited and
    // saved through Matrix and closed cleanly.
    assert!(unlock_after.is_some(), "Collabora did not send UNLOCK within {tail} s of the close");
    assert!(released, "UNLOCK must release the lease in the Matrix room: {lease:?}");
    assert_eq!(refused_puts, 0, "Collabora's PutFiles were refused: {wopi:?}");
    assert!(autosave_puts + exit_puts + plain_puts >= 1, "no PutFile reached the host in the whole session: {wopi:?}");
    assert!(last_token_saved, "the last typed text {last_token:?} is not in the single newest version's content.xml");
}

fn owner_store(rig: &Rig) -> &MatrixStore {
    &rig.handles.owner
}
