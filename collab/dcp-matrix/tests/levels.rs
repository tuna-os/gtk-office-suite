//! Spike Q15: what it costs to learn a sender's power level AT an event (spec
//! 8.1, question 15), and what the alternatives to `/context` are, on a real
//! Synapse. Needs `DCP_SYNAPSE_URL`; fails without it unless
//! `DCP_SKIP_SYNAPSE=1`. Results are printed as `MEASURE {json}` lines and
//! appended to `$DCP_MEASURE_OUT` (`common::record`).
//!
//! * `state_at_event_requests_are_not_rate_limited`: 600 `/context` requests,
//!   sequential and concurrent, count `M_LIMIT_EXCEEDED`; and on the default
//!   profile a control that shows the comparison can see tokens being spent
//!   (sends spend them, `/context` requests do not).
//! * `listing_30_versions`: what each rule adds to a read of 30 versions.
//! * `the_alternatives_to_context_probed`: `/state?at=`, `/members?at=` and the
//!   SDK's call against `/context`, after a demotion.

mod common;

use std::time::{Duration, Instant};

use common::*;
use dcp_core::*;
use dcp_matrix::matrix_sdk::reqwest;
use dcp_matrix::matrix_sdk::ruma::api::client::room::create_room;
use dcp_matrix::matrix_sdk::Room;
use dcp_matrix::{MatrixStore, SenderRule};
use serde_json::{json, Value};

fn limited(text: &str) -> bool {
    text.contains("M_LIMIT_EXCEEDED") || text.contains("LimitExceeded") || text.contains("Too Many Requests") || text.contains("429")
}

/// A room outside any document, for spending a user's message budget.
async fn scratch_room(store: &MatrixStore) -> Room {
    for _ in 0..60 {
        match store.client().create_room(create_room::v3::Request::new()).await {
            Ok(r) => return r,
            Err(e) if limited(&format!("{e:?}")) => tokio::time::sleep(Duration::from_secs(5)).await,
            Err(e) => panic!("scratch room: {e}"),
        }
    }
    panic!("the scratch room was always rate limited");
}

/// Send into `room` until the first 429 (or `max` sends); the event IDs of the sends that went through.
async fn send_until_limited(room: &Room, max: usize) -> Vec<String> {
    let mut ids = Vec::new();
    for _ in 0..max {
        match room.send_raw("m.room.message", json!({"msgtype": "m.text", "body": "spending a token"})).await {
            Ok(r) => ids.push(r.response.event_id.to_string()),
            Err(e) if limited(&format!("{e:?}")) => return ids,
            Err(e) => panic!("send: {e}"),
        }
    }
    ids
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn state_at_event_requests_are_not_rate_limited() {
    let Some(syn) = require_synapse!() else { return };
    let owner = syn.user("ctx").await;
    let doc = owner.create_document(&meta("Context limits"), Some(file(b"v1"))).await.unwrap();
    let first = owner.versions(&doc).await.unwrap()[0].id.clone();

    const SEQUENTIAL: usize = 300;
    let before = owner.level_stats();
    let t = Instant::now();
    for _ in 0..SEQUENTIAL {
        let state = owner.state_at_event(&doc, &first, true).await.unwrap();
        assert!(state.iter().any(|e| e["type"] == "m.room.power_levels"), "the answer must carry the power levels: {state:?}");
    }
    let seq_secs = t.elapsed().as_secs_f64();
    let seq = owner.level_stats().since(before);

    const TASKS: usize = 8;
    const EACH: usize = 40;
    let before = owner.level_stats();
    let t = Instant::now();
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..TASKS {
        let (o, d, f) = (owner.clone(), doc.clone(), first.clone());
        set.spawn(async move {
            for _ in 0..EACH {
                o.state_at_event(&d, &f, true).await.unwrap();
            }
        });
    }
    while let Some(r) = set.join_next().await {
        r.unwrap();
    }
    let conc_secs = t.elapsed().as_secs_f64();
    let conc = owner.level_stats().since(before);

    record(
        &syn,
        "q15_context_rate_limit",
        json!({
            "sequential": { "requests": seq.requests, "429s": seq.rate_limited, "secs": seq_secs, "per_second": SEQUENTIAL as f64 / seq_secs, "request_ms_total": seq.request_ms },
            "concurrent": { "tasks": TASKS, "requests": conc.requests, "429s": conc.rate_limited, "secs": conc_secs, "per_second": (TASKS * EACH) as f64 / conc_secs, "request_ms_total": conc.request_ms },
        }),
    );
    assert_eq!(seq.requests as usize, SEQUENTIAL);
    assert_eq!(conc.requests as usize, TASKS * EACH);
    assert_eq!(seq.rate_limited, 0, "sequential /context requests hit M_LIMIT_EXCEEDED on the {} profile", syn.profile);
    assert_eq!(conc.rate_limited, 0, "concurrent /context requests hit M_LIMIT_EXCEEDED on the {} profile", syn.profile);

    if syn.profile != "default" {
        println!("NOTE the token-spending control needs the default profile; this is {:?}: nothing is limited here, so only the /context counts above were measured", syn.profile);
        return;
    }

    // Does a /context request spend a token of the user's message budget?
    // How many sends does a fresh user get after X, until the first 429:
    //   A: nothing between          B: 300 /context requests between
    //   C: 5 more sends between (the control: spending tokens must show)
    let a = syn.user("budgetA").await;
    let ra = scratch_room(&a).await;
    let total_a = send_until_limited(&ra, 200).await.len();

    let b = syn.user("budgetB").await;
    let rb = scratch_room(&b).await;
    let first_b = send_until_limited_n(&rb, 3).await;
    let rb_id = rb.room_id().to_string();
    let before = b.level_stats();
    for _ in 0..300 {
        b.state_at_event(&rb_id, &first_b[0], true).await.unwrap();
    }
    let b_contexts = b.level_stats().since(before);
    let rest_b = send_until_limited(&rb, 200).await.len();

    let c = syn.user("budgetC").await;
    let rc = scratch_room(&c).await;
    let _ = send_until_limited_n(&rc, 3).await;
    let _ = send_until_limited_n(&rc, 5).await;
    let rest_c = send_until_limited(&rc, 200).await.len();

    record(
        &syn,
        "q15_context_spends_no_message_budget",
        json!({
            "sends_until_429_fresh_user": total_a,
            "after_3_sends_and_300_context_requests": { "contexts": b_contexts.requests, "429s_on_contexts": b_contexts.rate_limited, "further_sends_until_429": rest_b },
            "control_after_3_plus_5_sends": { "further_sends_until_429": rest_c },
        }),
    );
    assert!(total_a >= 5, "a fresh user's burst is about 10 events; got {total_a}");
    assert_eq!(b_contexts.rate_limited, 0);
    // 3 were sent before the contexts: if 300 requests spent tokens, nothing would be left.
    assert!(
        rest_b + 3 + 1 >= total_a,
        "300 /context requests spent message budget: a fresh user sent {total_a} before the first 429, this one {rest_b} after 3 sends and the contexts"
    );
    // The control: 5 more sends must leave about 5 fewer.
    assert!(
        rest_c + 4 <= rest_b,
        "the control cannot see spent tokens: {rest_c} sends left after 8 sends against {rest_b} after 3 sends and 300 /context requests"
    );
}

async fn send_until_limited_n(room: &Room, n: usize) -> Vec<String> {
    let mut ids = Vec::new();
    for _ in 0..n {
        let r = room
            .send_raw("m.room.message", json!({"msgtype": "m.text", "body": "spending a token"}))
            .await
            .expect("send within the burst");
        ids.push(r.response.event_id.to_string());
    }
    ids
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listing_30_versions() {
    let Some(syn) = require_synapse!() else { return };
    const N: usize = 30;
    let owner = syn.user("owner").await;
    let editor = syn.user("editor").await;
    let viewer = syn.user("viewer").await;
    let other = syn.user("other").await;
    let doc = owner.create_document(&meta("Thirty versions"), Some(file(b"v0"))).await.unwrap();
    owner.grant(&doc, &editor.principal(), Role::Editor).await.unwrap();
    owner.grant(&doc, &viewer.principal(), Role::Viewer).await.unwrap();
    // Joined before the versions are sent, so that none of them is history to this device.
    assert_eq!(viewer.role(&doc).await.unwrap(), Role::Viewer);
    assert_eq!(editor.role(&doc).await.unwrap(), Role::Editor);
    let mut parent = owner.versions(&doc).await.unwrap()[0].id.clone();

    let mut power_level_changes = 0;
    let t = Instant::now();
    for i in 1..=N {
        // Four changes of the table between the versions, so the readers walk several tables.
        if [7, 14, 21, 28].contains(&i) {
            let role = if power_level_changes % 2 == 0 { Role::Editor } else { Role::Viewer };
            owner.grant(&doc, &other.principal(), role).await.unwrap();
            power_level_changes += 1;
        }
        let v = editor.publish_version(&doc, vec![parent.clone()], file(format!("version {i}").as_bytes()), None).await.unwrap();
        parent = v.id;
    }
    println!("published {N} versions and {power_level_changes} power-level changes in {:.1}s", t.elapsed().as_secs_f64());

    // The viewer has every version decrypted before anything is timed.
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let r = viewer.read_report_with(&doc, SenderRule::Trust).await.unwrap();
        if r.versions.len() == N + 1 && r.undecryptable == 0 {
            break;
        }
        assert!(Instant::now() < deadline, "the viewer never read all {} versions: {} listed, {} undecryptable", N + 1, r.versions.len(), r.undecryptable);
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    const REPS: usize = 5;
    let mut baseline_total = 0.0;
    let cases: [(&str, SenderRule, usize); 9] = [
        ("trust-sender", SenderRule::Trust, 1),
        ("current-table", SenderRule::CurrentTable, 1),
        ("timeline-walk", SenderRule::TimelineWalk, 1),
        ("prev-content-walk", SenderRule::PrevContentWalk, 1),
        ("hybrid", SenderRule::Hybrid, 1),
        ("context", SenderRule::Context, 1),
        ("sdk-context", SenderRule::SdkContext, 1),
        ("context, 4 in flight", SenderRule::Context, 4),
        ("context, 8 in flight", SenderRule::Context, 8),
    ];
    for (label, rule, in_flight) in cases {
        viewer.set_level_concurrency(in_flight);
        let (mut totals, mut checks, mut reqs, mut request_ms, mut bytes) = (vec![], vec![], vec![], vec![], vec![]);
        for _ in 0..REPS {
            viewer.clear_level_cache();
            let t = Instant::now();
            let r = viewer.read_report_with(&doc, rule).await.unwrap();
            totals.push(t.elapsed().as_secs_f64() * 1000.0);
            checks.push(r.sender_check_ms as f64);
            reqs.push(r.level_stats.requests as f64);
            request_ms.push(r.level_stats.request_ms as f64);
            bytes.push(r.level_stats.bytes as f64);
            assert_eq!(r.versions.len(), N + 1, "{label}: every version is accepted (the editor was an editor throughout)");
            assert_eq!(r.rejected_unauthorised + r.rejected_unresolved, 0, "{label}: nothing is dropped for its sender");
            assert_eq!(r.level_stats.rate_limited, 0, "{label}: M_LIMIT_EXCEEDED while listing {} versions on the {} profile", N + 1, syn.profile);
            match rule {
                SenderRule::Context | SenderRule::SdkContext => {
                    assert_eq!(r.level_stats.requests as usize, N + 1, "{label}: one request per version, cold");
                    assert_eq!(r.level_stats.cache_hits, 0);
                }
                _ => assert_eq!(r.level_stats.requests, 0, "{label}: no state-at-event request"),
            }
        }
        let total = median(totals.clone());
        if rule == SenderRule::Trust {
            baseline_total = total;
        }
        let extra = (total - baseline_total).max(0.0);
        let data = json!({
            "rule": label, "in_flight": in_flight, "versions": N + 1, "power_level_changes_in_history": power_level_changes + 1,
            "reps": REPS,
            "read_ms_median": total, "read_ms_all": totals,
            "sender_check_ms_median": median(checks), "extra_over_trust_ms": extra,
            "extra_per_version_ms": extra / (N + 1) as f64,
            "requests": median(reqs), "request_ms_sum_median": median(request_ms), "bytes_median": median(bytes),
        });
        record(&syn, "q15_listing_30_versions", data);
    }

    // Warm: the same read again without clearing the cache costs no request.
    for (label, rule) in [("context", SenderRule::Context), ("sdk-context", SenderRule::SdkContext)] {
        viewer.set_level_concurrency(1);
        viewer.clear_level_cache();
        let cold = viewer.read_report_with(&doc, rule).await.unwrap();
        let t = Instant::now();
        let warm = viewer.read_report_with(&doc, rule).await.unwrap();
        record(
            &syn,
            "q15_listing_30_versions_warm",
            json!({
                "rule": label, "cold_requests": cold.level_stats.requests, "warm_requests": warm.level_stats.requests,
                "warm_cache_hits": warm.level_stats.cache_hits, "warm_read_ms": t.elapsed().as_secs_f64() * 1000.0,
                "cached_levels": viewer.cached_levels(),
            }),
        );
        assert_eq!(warm.level_stats.requests, 0, "{label}: a warm read makes no request");
        assert_eq!(warm.level_stats.cache_hits as usize, N + 1);
    }
}

async fn get_json(http: &reqwest::Client, token: &str, url: reqwest::Url) -> (u16, Value, usize) {
    let r = http.get(url).bearer_auth(token).send().await.expect("GET");
    let status = r.status().as_u16();
    let body = r.bytes().await.expect("body");
    let n = body.len();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null), n)
}

fn url(syn: &Synapse, segments: &[&str], query: &[(&str, &str)]) -> reqwest::Url {
    let mut u = reqwest::Url::parse(&syn.url).unwrap();
    u.path_segments_mut().unwrap().extend(["_matrix", "client", "v3"]).extend(segments);
    for (k, v) in query {
        u.query_pairs_mut().append_pair(k, v);
    }
    u
}

fn editor_level(state: &[Value], editor: &str) -> Option<i64> {
    state.iter().find(|e| e["type"] == "m.room.power_levels").and_then(|e| e["content"]["users"][editor].as_i64())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_alternatives_to_context_probed() {
    let Some(syn) = require_synapse!() else { return };
    let owner = syn.user("owner").await;
    let editor = syn.user("editor").await;
    let doc = owner.create_document(&meta("Probes"), Some(file(b"v1"))).await.unwrap();
    owner.grant(&doc, &editor.principal(), Role::Editor).await.unwrap();
    let v2 = editor.publish_version(&doc, vec![owner.versions(&doc).await.unwrap()[0].id.clone()], file(b"v2"), None).await.unwrap();
    owner.grant(&doc, &editor.principal(), Role::Viewer).await.unwrap();
    let editor_id = editor.principal();
    assert_eq!(owner.snapshot(&doc).await.unwrap().power.role_of(&editor_id), Role::Viewer, "the room now shows the editor as a viewer");

    // 1. /context at v2: the table AT v2, the editor still an editor.
    let before = owner.level_stats();
    let filtered = owner.state_at_event(&doc, &v2.id, true).await.unwrap();
    let filtered_cost = owner.level_stats().since(before);
    let before = owner.level_stats();
    let unfiltered = owner.state_at_event(&doc, &v2.id, false).await.unwrap();
    let unfiltered_cost = owner.level_stats().since(before);
    let at_v2 = editor_level(&filtered, &editor_id);
    assert_eq!(at_v2, Some(50), "/context?limit=0 must show the editor at level 50 at v2 (the probe can tell the past from the present)");
    assert_eq!(editor_level(&unfiltered, &editor_id), Some(50));
    let types: std::collections::BTreeSet<String> = filtered.iter().filter_map(|e| e["type"].as_str().map(str::to_owned)).collect();

    // 2. the SDK's own call
    let rid = dcp_matrix::matrix_sdk::ruma::RoomId::parse(&doc).unwrap();
    let room = owner.client().get_room(&rid).unwrap();
    let eid = dcp_matrix::matrix_sdk::ruma::EventId::parse(&v2.id).unwrap();
    let sdk = room.event_with_context(&eid, false, 0u32.into(), None).await.unwrap();
    let sdk_state: Vec<Value> = sdk.state.iter().filter_map(|r| serde_json::from_str(r.json().get()).ok()).collect();
    let sdk_level = editor_level(&sdk_state, &editor_id);

    // 3. /state and /state?at=<token> and /members?at=<token>
    let http = reqwest::Client::new();
    let token = owner.client().access_token().unwrap();
    let t = Instant::now();
    let (st, current, current_bytes) = get_json(&http, &token, url(&syn, &["rooms", &doc, "state"], &[])).await;
    let state_ms = t.elapsed().as_secs_f64() * 1000.0;
    let current_level = editor_level(current.as_array().map(Vec::as_slice).unwrap_or(&[]), &editor_id);
    assert_eq!(st, 200);
    assert_eq!(current_level, None, "/state is the CURRENT table: the editor is no longer listed");

    let (ctx_status, ctx, _) = get_json(&http, &token, url(&syn, &["rooms", &doc, "context", &v2.id], &[("limit", "1")])).await;
    assert_eq!(ctx_status, 200);
    let tokens: Vec<String> = ["start", "end"].iter().filter_map(|k| ctx[*k].as_str().map(str::to_owned)).collect();
    let mut at_probes = Vec::new();
    for tok in &tokens {
        let (s, body, _) = get_json(&http, &token, url(&syn, &["rooms", &doc, "state"], &[("at", tok)])).await;
        let level = editor_level(body.as_array().map(Vec::as_slice).unwrap_or(&[]), &editor_id);
        at_probes.push(json!({ "endpoint": "/state?at=<token from /context>", "status": s, "editor_level_in_answer": level,
            "verdict": if level == Some(50) { "honours at" } else { "ignores at: the current table" } }));
        let (s, body, _) = get_json(&http, &token, url(&syn, &["rooms", &doc, "members"], &[("at", tok)])).await;
        let kinds: std::collections::BTreeSet<String> =
            body["chunk"].as_array().map(|a| a.iter().filter_map(|e| e["type"].as_str().map(str::to_owned)).collect()).unwrap_or_default();
        at_probes.push(json!({ "endpoint": "/members?at=<token from /context>", "status": s, "event_types_in_answer": kinds }));
    }

    record(
        &syn,
        "q15_alternatives_probed",
        json!({
            "context_filtered": { "events": filtered.len(), "types": types, "bytes": filtered_cost.bytes, "request_ms": filtered_cost.request_ms, "editor_level_at_v2": at_v2 },
            "context_unfiltered": { "events": unfiltered.len(), "bytes": unfiltered_cost.bytes, "request_ms": unfiltered_cost.request_ms },
            "sdk_event_with_context": { "events": sdk_state.len(), "editor_level_at_v2": sdk_level },
            "state_current": { "bytes": current_bytes, "ms": state_ms, "editor_level": current_level },
            "tokens_from_context": tokens, "at_probes": at_probes,
        }),
    );
    assert_eq!(sdk_level, Some(50), "the SDK's event_with_context returns the state at the event");
}

/// The walks judge a cached `/messages` pass with no client: the raw events the
/// server returned, kept as a string (a cache on disk would hold the same),
/// judged by `levels::judge_transcript`, which has no network to ask. Its
/// verdicts equal the online ones. In a draft-0 room, so that the ex-editor's
/// forged event exists: the offline verdicts say no to it and yes to the
/// editor's earlier version. The controls: a cache without the power-levels
/// events judges nothing, and the cache does not hold what `/context` would
/// have returned for an event that is not in it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_walks_judge_a_cached_transcript_offline() {
    let Some(syn) = require_synapse!() else { return };
    let owner = syn.user_with("owner", |c| c.enforce_encrypted_pl = false).await;
    let editor = syn.user("editor").await;
    let doc = owner.create_document(&meta("Offline"), Some(file(b"v1"))).await.unwrap();
    owner.grant(&doc, &editor.principal(), Role::Editor).await.unwrap();
    let first = owner.versions(&doc).await.unwrap()[0].id.clone();
    let v2 = editor.publish_version(&doc, vec![first.clone()], file(b"v2"), None).await.unwrap();
    owner.grant(&doc, &editor.principal(), Role::Viewer).await.unwrap();
    let forged = Version {
        dcp: 1,
        parents: vec![v2.id.clone()],
        file: v2.body.file.clone(),
        created_by: Author { principal: editor.principal(), device: editor.device() },
        message: Some("forged by the ex-editor".into()),
        covers: None,
    };
    let f = editor
        .send_unchecked(&doc, dcp_matrix::state::VERSION, serde_json::to_value(&forged).unwrap())
        .await
        .expect("a draft-0 room accepts the ex-editor's event");
    let v3 = owner.publish_version(&doc, vec![v2.id.clone()], file(b"v3"), None).await.unwrap();

    // Online: what the store says under the forward walk.
    let ids = [first.clone(), v2.id.clone(), f.clone(), v3.id.clone()];
    let mut online = std::collections::BTreeMap::new();
    for _ in 0..30 {
        let r = owner.read_report_with(&doc, SenderRule::TimelineWalk).await.unwrap();
        online = r.checks.iter().map(|c| (c.id.clone(), c.level)).collect();
        if ids.iter().all(|i| online.contains_key(i)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(ids.iter().all(|i| online.contains_key(i)), "the owner never judged all four events: {online:?}");

    // The cache: the raw pass, newest first, through a string.
    let http = reqwest::Client::new();
    let token = owner.client().access_token().unwrap();
    let (status, body, _) = get_json(&http, &token, url(&syn, &["rooms", &doc, "messages"], &[("dir", "b"), ("limit", "200")])).await;
    assert_eq!(status, 200);
    let cache = serde_json::to_string(&body["chunk"]).unwrap();
    drop(body);

    // Offline: no store, no client, no request.
    let events: Vec<Value> = serde_json::from_str(&cache).unwrap();
    assert!(events.len() > 4, "the cache holds the whole pass: {} events", events.len());
    for rule in [SenderRule::TimelineWalk, SenderRule::PrevContentWalk] {
        let offline = dcp_matrix::levels::judge_transcript(rule, &events);
        for id in &ids {
            assert_eq!(offline.get(id).copied().flatten(), online[id], "{rule:?}: the offline level of {id} differs from the online one");
        }
        let permitted = |id: &String| offline[id].map(|l| l.permitted());
        assert_eq!(permitted(&first), Some(true), "{rule:?}");
        assert_eq!(permitted(&v2.id), Some(true), "{rule:?}: the editor's version from before the demotion");
        assert_eq!(permitted(&f), Some(false), "{rule:?}: the ex-editor's forged event");
        assert_eq!(permitted(&v3.id), Some(true), "{rule:?}");
    }

    // Controls: nothing judged without the power-levels events, and nothing guessed.
    let stripped: Vec<Value> = events.iter().filter(|e| e["type"] != "m.room.power_levels").cloned().collect();
    let none = dcp_matrix::levels::judge_transcript(SenderRule::TimelineWalk, &stripped);
    assert!(ids.iter().all(|i| none[i].is_none()), "a cache without power-levels events must judge nothing: {none:?}");
    let unknown = dcp_matrix::levels::judge_transcript(SenderRule::TimelineWalk, &events);
    assert!(!unknown.contains_key("$not-in-the-cache"), "an event the cache does not hold is not judged");
    record(
        &syn,
        "q15_offline_transcript",
        json!({ "events_in_cache": events.len(), "cache_bytes": cache.len(), "judged": ids.len(), "forged_dropped": true }),
    );
}

/// The hybrid walks the room's own power-levels events only when the room does
/// not federate (`m.federate` false, what the binding creates by default). In a
/// room other homeservers may join, the state at an event is the result of state
/// resolution and the hybrid asks the server for every event; the verdicts are
/// the same ones (this room has one homeserver, so the walk is right here too:
/// the test shows which source the hybrid chose, not that the walk is wrong).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_room_that_federates_is_judged_by_the_server_and_not_by_the_walk() {
    let Some(syn) = require_synapse!() else { return };
    let owner = syn
        .user_with("owner", |c| {
            c.enforce_encrypted_pl = false;
            c.federate = true;
        })
        .await;
    let editor = syn.user("editor").await;
    let doc = owner.create_document(&meta("Federating"), Some(file(b"v1"))).await.unwrap();
    let http = reqwest::Client::new();
    let token = owner.client().access_token().unwrap();
    let (status, create, _) = get_json(&http, &token, url(&syn, &["rooms", &doc, "state", "m.room.create"], &[])).await;
    assert_eq!(status, 200);
    assert_eq!(create["m.federate"], json!(true), "the room was created to federate: {create}");

    owner.grant(&doc, &editor.principal(), Role::Editor).await.unwrap();
    let first = owner.versions(&doc).await.unwrap()[0].id.clone();
    let v2 = editor.publish_version(&doc, vec![first.clone()], file(b"v2"), None).await.unwrap();
    owner.grant(&doc, &editor.principal(), Role::Viewer).await.unwrap();
    let forged = Version {
        dcp: 1,
        parents: vec![v2.id.clone()],
        file: v2.body.file.clone(),
        created_by: Author { principal: editor.principal(), device: editor.device() },
        message: Some("forged by the ex-editor".into()),
        covers: None,
    };
    let f = editor.send_unchecked(&doc, dcp_matrix::state::VERSION, serde_json::to_value(&forged).unwrap()).await.unwrap();

    let ids = [first, v2.id.clone(), f.clone()];
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let r = owner.read_report_with(&doc, SenderRule::TimelineWalk).await.unwrap();
        if ids.iter().all(|i| r.checks.iter().any(|c| &c.id == i)) {
            break;
        }
        assert!(Instant::now() < deadline, "the owner never judged all three events");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    owner.clear_level_cache();
    let hybrid = owner.read_report_with(&doc, SenderRule::Hybrid).await.unwrap();
    let walk = owner.read_report_with(&doc, SenderRule::TimelineWalk).await.unwrap();
    assert_eq!(hybrid.level_stats.requests as usize, ids.len(), "the hybrid asked the server for each event of a federating room");
    assert_eq!(walk.level_stats.requests, 0);
    let by_id = |r: &dcp_matrix::ReadReport| -> std::collections::BTreeMap<String, Option<dcp_matrix::Level>> {
        r.checks.iter().filter(|c| ids.contains(&c.id)).map(|c| (c.id.clone(), c.level)).collect()
    };
    assert_eq!(by_id(&hybrid), by_id(&walk), "the server and the walk agree on every level");
    assert!(hybrid.unauthorised_ids.contains(&f) && !hybrid.unauthorised_ids.contains(&v2.id), "the ex-editor's forged event is dropped, the earlier version kept");
}
