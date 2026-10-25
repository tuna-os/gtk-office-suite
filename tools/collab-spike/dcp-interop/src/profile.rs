//! Running the interop suites against either Synapse profile.
//!
//! `scripts/synapse-up.sh relaxed` lifts every `rc_*` limit; `default` adds
//! none, so Synapse's own apply (`rc_message`: 0.2 events/s per user, burst
//! 10, shared by every event the user sends in any room, state events
//! included). The Matrix binding already queues per room and honours
//! `retry_after_ms` (spike A), so on `default` nothing is refused, it is
//! delayed. What changes for a test is how long an effect takes to arrive, so:
//!
//! * the wait a check allows for an effect ([`settle_for`]) and the deadline
//!   for a whole check or journey ([`deadline_for`]) are explicit per profile,
//!   and generous on `default`;
//! * every run records how long it took and how much of that the client spent
//!   waiting for the limiter ([`Timing`]), so "slow on default" is a measured
//!   statement, never a guess; the label is derived from the timing
//!   ([`SLOW_ON_DEFAULT_SECS`]), not written by hand;
//! * an assertion is never weakened for `default`. A check or journey that
//!   really fails there is listed in [`DEFAULT_PROFILE_FAILURES`] with the
//!   phrase its failure must contain, and the suite then asserts that exact
//!   failure: it cannot start passing or fail for another reason unnoticed.
//!   The table is empty while nothing fails;
//! * **`default-drained`** (`DCP_DRAIN_BURST=1` on the default profile): a
//!   fresh user has Synapse's burst of 10 events, and one journey sends 3 to 8
//!   events in all (measured), so on plain `default` nothing is ever delayed.
//!   The drained profile spends each principal's burst once the room is set
//!   up and read, right before the check or journey starts
//!   ([`drain_message_budget`]: send into a scratch room until the first
//!   `M_LIMIT_EXCEEDED`), which is a user who has been editing for a while: from
//!   then on every event costs about 5 s. That is where the limits bite.

use std::future::Future;
use std::time::{Duration, Instant};

use dcp_conformance::{
    journey_names, run_one, Failure, JourneyResult, Outcome, Report, StoreFactory, JOURNEYS,
};
use dcp_matrix::{MatrixStore, StatsSnapshot};
use serde_json::json;

use crate::common::{self, Synapse};
use crate::factories::InteropFactory;
use crate::rig::{Rig, RigOptions};

/// What a check waits for an effect to reach another handle, relaxed.
pub const SETTLE_RELAXED: Duration = Duration::from_secs(30);
/// The same on Synapse's default limits: one event costs up to ~5 s once the
/// 10-event burst is spent (spike A: `retry_after_ms` about 4.5 s), and a check
/// that waits for an effect that itself needs several events must outlast them.
pub const SETTLE_DEFAULT: Duration = Duration::from_secs(300);
/// The longest one check (rig setup included) or one journey may take.
pub const DEADLINE_RELAXED: Duration = Duration::from_secs(15 * 60);
pub const DEADLINE_DEFAULT: Duration = Duration::from_secs(45 * 60);
/// A run longer than this on `default` is labelled `slow-on-default` in the
/// output and in the measurements file. Informational: nothing asserts on it.
pub const SLOW_ON_DEFAULT_SECS: f64 = 60.0;

pub fn is_default(syn: &Synapse) -> bool {
    syn.profile == "default"
}

/// `DCP_DRAIN_BURST=1`: spend every principal's message budget before the
/// document is created. Only meaningful on the default profile: setting it
/// on any other panics, so a relaxed run cannot claim to be a drained one.
pub fn drained(syn: &Synapse) -> bool {
    let on = std::env::var("DCP_DRAIN_BURST").as_deref() == Ok("1");
    assert!(!on || is_default(syn), "DCP_DRAIN_BURST=1 needs the default Synapse profile, got {:?}", syn.profile);
    on
}

/// `relaxed`, `default` or `default-drained`: what a measurement is labelled.
pub fn label(syn: &Synapse) -> String {
    if drained(syn) {
        "default-drained".into()
    } else {
        syn.profile.clone()
    }
}

/// Send into a scratch room until Synapse answers `M_LIMIT_EXCEEDED`: the
/// user's `rc_message` bucket is then empty, and every later event of this
/// user, in any room, waits for a refill (0.2/s: about 5 s). Returns how many
/// events it took (the burst, 10, plus what refilled meanwhile). The client's
/// own retry is off (`MatrixStore::connect`), so the 429 comes back as an error.
pub async fn drain_message_budget(store: &MatrixStore) -> Result<u32, Failure> {
    use dcp_matrix::matrix_sdk::ruma::api::client::room::create_room;
    let client = store.client();
    // Room creation is rate limited too: retry it until it goes through.
    let mut room = None;
    for _ in 0..60 {
        match client.create_room(create_room::v3::Request::new()).await {
            Ok(r) => {
                room = Some(r);
                break;
            }
            Err(e) if is_rate_limited(&e.to_string()) || is_rate_limited(&format!("{e:?}")) => {
                tokio::time::sleep(Duration::from_secs(5)).await
            }
            Err(e) => return Err(Failure::new(format!("draining the message budget: cannot create a scratch room: {e}"))),
        }
    }
    let room = room.ok_or_else(|| Failure::new("draining the message budget: the scratch room was always rate limited"))?;
    for sent in 1..=200u32 {
        match room.send_raw("m.room.message", json!({"msgtype": "m.text", "body": "spending the burst"})).await {
            Ok(_) => {}
            Err(e) if is_rate_limited(&e.to_string()) || is_rate_limited(&format!("{e:?}")) => return Ok(sent - 1),
            Err(e) => return Err(Failure::new(format!("draining the message budget: {e}"))),
        }
    }
    Err(Failure::new("draining the message budget: 200 events and still no M_LIMIT_EXCEEDED: is this the default profile?"))
}

/// `MatrixStore::send_unchecked` (a send around the store's role check, with no
/// queue) repeated while the homeserver answers with its rate limit. A 429 is
/// not a verdict on the event: on the default profile Synapse answers it
/// before it looks at the sender's power level, so a forged event sent by a
/// user whose budget is spent first gets `M_LIMIT_EXCEEDED`, and only the
/// retry gets the 403 (or the acceptance) the forgery checks are about.
pub async fn send_unchecked_patiently(
    store: &MatrixStore,
    doc: &dcp_core::DocId,
    event_type: &str,
    content: serde_json::Value,
) -> Result<String, dcp_core::StoreError> {
    for _ in 0..60 {
        match store.send_unchecked(doc, event_type, content.clone()).await {
            Err(dcp_core::StoreError::RateLimited { retry_after_ms }) => {
                tokio::time::sleep(Duration::from_millis(retry_after_ms.unwrap_or(5_000).clamp(500, 30_000))).await
            }
            other => return other,
        }
    }
    Err(dcp_core::StoreError::RateLimited { retry_after_ms: None })
}

/// [`send_unchecked_patiently`] for a state event (`MatrixStore::send_state_unchecked`).
pub async fn send_state_unchecked_patiently(
    store: &MatrixStore,
    doc: &dcp_core::DocId,
    event_type: &str,
    state_key: &str,
    content: serde_json::Value,
) -> Result<String, dcp_core::StoreError> {
    for _ in 0..60 {
        match store.send_state_unchecked(doc, event_type, state_key, content.clone()).await {
            Err(dcp_core::StoreError::RateLimited { retry_after_ms }) => {
                tokio::time::sleep(Duration::from_millis(retry_after_ms.unwrap_or(5_000).clamp(500, 30_000))).await
            }
            other => return other,
        }
    }
    Err(dcp_core::StoreError::RateLimited { retry_after_ms: None })
}

fn is_rate_limited(text: &str) -> bool {
    text.contains("M_LIMIT_EXCEEDED") || text.contains("LimitExceeded") || text.contains("Too Many Requests") || text.contains("429")
}

pub fn settle_for(syn: &Synapse) -> Duration {
    if is_default(syn) {
        SETTLE_DEFAULT
    } else {
        SETTLE_RELAXED
    }
}

pub fn deadline_for(syn: &Synapse) -> Duration {
    if is_default(syn) {
        DEADLINE_DEFAULT
    } else {
        DEADLINE_RELAXED
    }
}

/// `(label, name, phrase)`: on the profile `label` (`default` or
/// `default-drained`) `name` (a check, `<check>@draft0`, or `<stack>/<journey>`)
/// is known to fail, and the failure must contain `phrase`. Filled in only
/// from a measured run, with the run cited in `docs/rfc/0004-spike-f-results.md`.
pub const DEFAULT_PROFILE_FAILURES: &[(&str, &str, &str)] = &[];

pub fn expected_failure(syn: &Synapse, name: &str) -> Option<&'static str> {
    let label = label(syn);
    DEFAULT_PROFILE_FAILURES.iter().find(|(l, n, _)| *l == label && *n == name).map(|(_, _, p)| *p)
}

/// Add the counters of several stores.
pub fn sum_stats(stores: &[&MatrixStore]) -> StatsSnapshot {
    let mut t = StatsSnapshot::default();
    for s in stores {
        let x = s.stats();
        t.sent += x.sent;
        t.rate_limited += x.rate_limited;
        t.network_retries += x.network_retries;
        t.waited_ms += x.waited_ms;
        t.ignored_invalid += x.ignored_invalid;
    }
    t
}

pub fn stats_since(now: StatsSnapshot, then: StatsSnapshot) -> StatsSnapshot {
    StatsSnapshot {
        sent: now.sent - then.sent,
        rate_limited: now.rate_limited - then.rate_limited,
        network_retries: now.network_retries - then.network_retries,
        waited_ms: now.waited_ms - then.waited_ms,
        ignored_invalid: now.ignored_invalid - then.ignored_invalid,
    }
}

/// One timed run of a check or a journey.
#[derive(Debug, Clone)]
pub struct Timing {
    pub kind: &'static str,
    pub name: String,
    pub stack: String,
    pub profile: String,
    /// Building the rig (room, invites, first version); 0 for a journey, which
    /// builds its own fixture inside the run.
    pub setup_secs: f64,
    /// The check or the journey.
    pub secs: f64,
    pub outcome: &'static str,
    pub detail: String,
    pub sent: u64,
    pub rate_limited: u64,
    pub waited_secs: f64,
}

impl Timing {
    pub fn slow_on_default(&self) -> bool {
        self.profile.starts_with("default") && self.setup_secs + self.secs > SLOW_ON_DEFAULT_SECS
    }

    /// Print one line and append the record to `$DCP_MEASURE_OUT` (the helper
    /// spike A's measurements use).
    pub fn record(&self, syn: &Synapse) {
        println!(
            "TIMING {:<7} {:<10} {:<66} {:<15} setup {:>7.1}s run {:>7.1}s  sent {:>3}  429s {:>3}  waited {:>7.1}s  {}{}",
            self.kind,
            self.stack.split_whitespace().next().unwrap_or(""),
            self.name,
            self.profile,
            self.setup_secs,
            self.secs,
            self.sent,
            self.rate_limited,
            self.waited_secs,
            self.outcome,
            if self.slow_on_default() { "  slow-on-default" } else { "" },
        );
        common::record(
            syn,
            "interop_timing",
            json!({
                "kind": self.kind, "name": self.name, "stack": self.stack, "label": self.profile,
                "setup_secs": self.setup_secs, "secs": self.secs, "outcome": self.outcome,
                "detail": self.detail, "sent": self.sent, "rate_limited": self.rate_limited,
                "waited_secs": self.waited_secs, "slow_on_default": self.slow_on_default(),
            }),
        );
    }
}

fn outcome_label(r: &Result<(), Failure>) -> (&'static str, String) {
    match r {
        Ok(()) => ("passed", String::new()),
        Err(f) => ("failed", f.0.clone()),
    }
}

/// Build a rig with `opts` on `syn`, run `check` on it, record the timing, all
/// under the profile's deadline. The returned error is the check's (or the
/// rig's, or the deadline's).
pub async fn run_check(
    syn: &Synapse,
    name: &str,
    opts: RigOptions,
    check: for<'a> fn(&'a Rig) -> crate::checks::CheckFuture<'a>,
) -> Result<(), Failure> {
    let deadline = deadline_for(syn);
    let started = Instant::now();
    let body = async {
        let rig = Rig::new(syn, opts).await.map_err(|f| Failure::new(format!("building the rig: {f}")))?;
        let setup = started.elapsed().as_secs_f64();
        let before = rig.stats();
        let t = Instant::now();
        let result = check(&rig).await;
        Ok::<_, Failure>((setup, t.elapsed().as_secs_f64(), crate::profile::stats_since(rig.stats(), before), result))
    };
    let (setup_secs, secs, spent, result) = match tokio::time::timeout(deadline, body).await {
        Ok(Ok(x)) => x,
        Ok(Err(f)) => return finish(syn, name, "check", started.elapsed().as_secs_f64(), 0.0, StatsSnapshot::default(), Err(f)),
        Err(_) => {
            let f = Failure::new(format!("no result within the {deadline:?} deadline of the {} profile", syn.profile));
            return finish(syn, name, "check", deadline.as_secs_f64(), 0.0, StatsSnapshot::default(), Err(f));
        }
    };
    finish(syn, name, "check", setup_secs, secs, spent, result)
}

fn finish(
    syn: &Synapse,
    name: &str,
    kind: &'static str,
    setup_secs: f64,
    secs: f64,
    spent: StatsSnapshot,
    result: Result<(), Failure>,
) -> Result<(), Failure> {
    let (outcome, detail) = outcome_label(&result);
    Timing {
        kind,
        name: name.to_owned(),
        stack: "wopi+matrix".into(),
        profile: label(syn),
        setup_secs,
        secs,
        outcome,
        detail,
        sent: spent.sent,
        rate_limited: spent.rate_limited,
        waited_secs: spent.waited_ms as f64 / 1000.0,
    }
    .record(syn);
    result
}

/// Panic unless `result` is what the profile's table says: a pass, or, for a
/// name in [`DEFAULT_PROFILE_FAILURES`] on `default`, a failure containing the
/// recorded phrase.
pub fn judge(syn: &Synapse, name: &str, result: Result<(), Failure>) {
    match (expected_failure(syn, name), result) {
        (None, Ok(())) => {}
        (None, Err(f)) => panic!("{name} failed on the {} profile: {f}", label(syn)),
        (Some(phrase), Err(f)) => {
            println!("{name}: failed on the {} profile as recorded: {f}", label(syn));
            assert!(f.0.contains(phrase), "{name}: failed for another reason than the recorded one ({phrase:?}): {f}");
        }
        (Some(phrase), Ok(())) => {
            panic!("{name} PASSED on the {} profile but is recorded as failing ({phrase:?}); update DEFAULT_PROFILE_FAILURES and the results document", label(syn))
        }
    }
}

/// `dcp_conformance::run_all`, one journey at a time under the profile's
/// deadline, each timed (with the rate-limit waits of the stores the factory
/// built for it). The report is the same as `run_all`'s, except that a journey
/// that exceeds the deadline is reported as failed.
pub async fn run_all_timed(factory: &InteropFactory) -> Report {
    let syn = factory.syn.clone();
    let deadline = deadline_for(&syn);
    let mut results: Vec<JourneyResult> = Vec::new();
    for name in journey_names() {
        let j = JOURNEYS.iter().find(|j| j.name == name).expect("journey");
        let before = factory.stats_total();
        let t = Instant::now();
        let r = match tokio::time::timeout(deadline, run_one(factory, name)).await {
            Ok(r) => r,
            Err(_) => JourneyResult {
                name: j.name,
                spec_test: j.spec_test,
                outcome: Outcome::Failed(Failure::new(format!(
                    "no result within the {deadline:?} deadline of the {} profile",
                    syn.profile
                ))),
            },
        };
        let secs = t.elapsed().as_secs_f64();
        let spent = stats_since(factory.stats_total(), before);
        let (verdict, detail) = match &r.outcome {
            Outcome::Passed => ("passed", String::new()),
            Outcome::Failed(f) => ("failed", f.0.clone()),
            Outcome::Skipped { missing } => ("skipped", missing.join(",")),
        };
        Timing {
            kind: "journey",
            name: r.name.to_owned(),
            stack: factory.name(),
            profile: label(&syn),
            setup_secs: 0.0,
            secs,
            outcome: verdict,
            detail,
            sent: spent.sent,
            rate_limited: spent.rate_limited,
            waited_secs: spent.waited_ms as f64 / 1000.0,
        }
        .record(&syn);
        results.push(r);
    }
    Report { binding: factory.name(), results }
}

/// Judge a whole conformance report for `stack` (a label that keys
/// [`DEFAULT_PROFILE_FAILURES`] as `"<stack>/<journey>"`): every failure must be
/// one the profile's table records, with its phrase, and every recorded
/// failure must still be failing. Skips are not judged here
/// (`Report::assert_skipped_exactly` does that).
pub fn judge_report(syn: &Synapse, stack: &str, report: &Report) {
    let mut problems = Vec::new();
    for r in &report.results {
        let key = format!("{stack}/{}", r.name);
        match (&r.outcome, expected_failure(syn, &key)) {
            (Outcome::Skipped { .. }, _) | (Outcome::Passed, None) => {}
            (Outcome::Failed(f), None) => problems.push(format!("{key} failed on the {} profile: {f}", label(syn))),
            (Outcome::Failed(f), Some(phrase)) if !f.0.contains(phrase) => {
                problems.push(format!("{key} failed for another reason than the recorded one ({phrase:?}): {f}"))
            }
            (Outcome::Failed(_), Some(_)) => {}
            (Outcome::Passed, Some(phrase)) => {
                problems.push(format!("{key} PASSED on the {} profile but is recorded as failing ({phrase:?})", label(syn)))
            }
        }
    }
    assert!(problems.is_empty(), "{}\n{report}", problems.join("\n"));
}

/// How long a negative control waits for an effect a defective store never
/// shows: it fails by a stated mismatch after this, so it is a few seconds on
/// a relaxed Synapse and longer where the homeserver paces every event.
pub fn control_settle(syn: &Synapse) -> Duration {
    if is_default(syn) {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(8)
    }
}

/// Run a future to completion, panicking when it exceeds the profile's deadline.
pub async fn within_deadline<T>(syn: &Synapse, what: &str, f: impl Future<Output = T>) -> T {
    let d = deadline_for(syn);
    match tokio::time::timeout(d, f).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: no result within the {d:?} deadline of the {} profile", syn.profile),
    }
}
