//! Lease rule 3 (write, read back, wait the settle interval, read back again)
//! under stress on a real Synapse, relaxed or default rate limits.
//!
//! Spike A forced 40 races with ONE read-back and saw both sessions believe
//! they held the lease in 7 of 40 (the final state always named one holder).
//! Draft 1 made it two read-backs and a 500 ms settle interval
//! (`lease::confirm`, `MATRIX_SETTLE_MS`) and ran one race per journey. This
//! runs the real rule `DCP_RACE_ROUNDS` times (default 200) and reports the
//! distribution:
//!
//! * how often the FIRST read-back is wrong (both sessions would have believed
//!   it, under the draft-0 single read-back rule);
//! * what the rule itself concluded (`lease::confirm`: both reads must name the
//!   session and the `renewal` it wrote), and whether the final state, read 1.5 s
//!   after both finished, agrees with the one session the rule let acquire;
//! * the gap between the two writes' `origin_server_ts`, the quantity the
//!   settle interval has to exceed.
//!
//! A second test (`two_read_rule_tolerance`) forces the competing write to
//! arrive a chosen time after the first (both sessions read the lease free
//! first), to find where the 500 ms interval stops being enough.
//!
//! The race, per round: a third principal (the owner) releases the lease, two
//! editors with no client-side limiter start together (even rounds) or the
//! second starts 0 to 119 ms late (odd rounds, as in spike A), each running
//! `lease::available`, `lease::write`, one observed read-back, then
//! `lease::confirm`. On the default profile the homeserver's own limiter paces
//! the writes, which is the point of running it there.

use std::time::{Duration, Instant};

use dcp_conformance::Failure;
use dcp_core::lease::{self, Acquire, Claim, Pause, Settle, MATRIX_SETTLE_MS};
use dcp_core::{DocId, DocumentStore, Role, StoreError};
use dcp_interop::common::{self, file, meta, Synapse};
use dcp_interop::profile;
use dcp_interop::rig::synapse;
use dcp_matrix::MatrixStore;
use serde_json::{json, Value};

struct RealPause;

#[async_trait::async_trait]
impl Pause for RealPause {
    async fn pause(&self, ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

static PAUSE: RealPause = RealPause;

/// What one session saw in one round.
#[derive(Debug, Clone)]
struct Seen {
    session: String,
    /// It saw the other's active lease and wrote nothing.
    refused_at_check: bool,
    /// `origin_server_ts` of its write.
    write_ts: Option<u64>,
    /// The single read-back named this session and the renewal it wrote.
    first_read_ok: bool,
    /// `lease::confirm` (two reads around the settle interval) acquired.
    acquired: bool,
    /// How long the whole acquisition took, ms.
    took_ms: u128,
}

async fn session(
    store: &MatrixStore,
    doc: &DocId,
    claim: &Claim,
    start_after_ms: u64,
) -> Result<Seen, StoreError> {
    if start_after_ms > 0 {
        tokio::time::sleep(Duration::from_millis(start_after_ms)).await;
    }
    let t = Instant::now();
    let mut seen = Seen {
        session: claim.holder.session.clone(),
        refused_at_check: false,
        write_ts: None,
        first_read_ok: false,
        acquired: false,
        took_ms: 0,
    };
    if lease::available(store, doc, &claim.holder).await?.is_err() {
        seen.refused_at_check = true;
        seen.took_ms = t.elapsed().as_millis();
        return Ok(seen);
    }
    let attempt = lease::write(store, doc, claim).await?;
    seen.write_ts = attempt.result.as_ref().ok().map(|s| s.ts_ms);
    // The observation draft 0 would have acted on: one read-back.
    seen.first_read_ok = store
        .lease(doc)
        .await?
        .is_some_and(|r| lease::names(&r.body, &claim.holder) && r.body.renewal == attempt.lease.renewal);
    // The rule itself.
    let settle = Settle::new(&PAUSE, MATRIX_SETTLE_MS);
    seen.acquired = matches!(lease::confirm(store, doc, claim, &attempt, &settle).await?, Acquire::Acquired(_));
    seen.took_ms = t.elapsed().as_millis();
    Ok(seen)
}

struct Pair {
    owner: MatrixStore,
    alice: MatrixStore,
    bob: MatrixStore,
    doc: DocId,
    base: String,
}

async fn pair(syn: &Synapse) -> Result<Pair, Failure> {
    // No client-side limiter: the homeserver's is the only pacing, and on the
    // relaxed profile nothing paces the writes at all.
    let owner = syn.user_with("lrowner", |c| c.events_per_sec = 0.0).await;
    let alice = syn.user_with("lralice", |c| c.events_per_sec = 0.0).await;
    let bob = syn.user_with("lrbob", |c| c.events_per_sec = 0.0).await;
    let doc = owner.create_document(&meta("Lease race"), Some(file(b"v1"))).await?;
    owner.grant(&doc, &alice.principal(), Role::Editor).await?;
    owner.grant(&doc, &bob.principal(), Role::Editor).await?;
    alice.role(&doc).await?;
    bob.role(&doc).await?;
    let base = owner.versions(&doc).await?.remove(0).id;
    Ok(Pair { owner, alice, bob, doc, base })
}

fn claim(who: &MatrixStore, session: &str, base: &str) -> Claim {
    Claim::new(who.principal(), session, format!("token-{session}"), base, 1_800_000)
}

fn rounds_from_env(var: &str, default: usize) -> usize {
    std::env::var(var).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_two_read_rule_under_stress() {
    let Some(syn) = synapse() else { return };
    let rounds = rounds_from_env("DCP_RACE_ROUNDS", 200);
    let name = "lease_race";
    let result = profile::within_deadline(&syn, name, run_stress(&syn, rounds)).await;
    profile::judge(&syn, name, result);
}

async fn run_stress(syn: &Synapse, rounds: usize) -> Result<(), Failure> {
    let p = pair(syn).await?;
    let started = Instant::now();
    let before = profile::sum_stats(&[&p.owner, &p.alice, &p.bob]);
    let mut tally = [0u32; 12];
    const FIRST_BOTH: usize = 0;
    const FIRST_ONE: usize = 1;
    const FIRST_NONE: usize = 2;
    const RULE_BOTH: usize = 3;
    const RULE_ONE: usize = 4;
    const RULE_NONE: usize = 5;
    const FINAL_AGREES: usize = 6;
    const FINAL_DISAGREES: usize = 7;
    const REFUSED_AT_CHECK: usize = 8;
    const FIRST_WRONG_RULE_RESOLVED: usize = 9;
    const FIRST_WRONG_RULE_NOT_RESOLVED: usize = 10;
    const BOTH_WROTE: usize = 11;
    let mut gaps: Vec<f64> = Vec::new();
    let mut rows: Vec<Value> = Vec::new();
    let mut violations: Vec<String> = Vec::new();

    for round in 0..rounds {
        p.owner.write_lease(&p.doc, lease::released()).await?;
        if !profile::is_default(syn) {
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        let offset = if round % 2 == 0 { 0 } else { (round as u64 * 37) % 120 };
        let (ca, cb) = (claim(&p.alice, &format!("a{round}"), &p.base), claim(&p.bob, &format!("b{round}"), &p.base));
        let t = Instant::now();
        let (ra, rb) = tokio::join!(
            session(&p.alice, &p.doc, &ca, 0),
            session(&p.bob, &p.doc, &cb, offset)
        );
        let (a, b) = (ra?, rb?);
        let round_ms = t.elapsed().as_millis();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let last = p.owner.lease(&p.doc).await?.ok_or_else(|| Failure::new("no lease record after the round"))?;
        let final_session = last.body.holder.as_ref().map(|h| h.session.clone()).unwrap_or_default();

        let wrote = [&a, &b].iter().filter(|s| !s.refused_at_check).count();
        if wrote == 2 {
            tally[BOTH_WROTE] += 1;
        }
        tally[REFUSED_AT_CHECK] += [&a, &b].iter().filter(|s| s.refused_at_check).count() as u32;
        let first = [&a, &b].iter().filter(|s| s.first_read_ok).count();
        tally[[FIRST_NONE, FIRST_ONE, FIRST_BOTH][first]] += 1;
        let rule = [&a, &b].iter().filter(|s| s.acquired).count();
        tally[[RULE_NONE, RULE_ONE, RULE_BOTH][rule]] += 1;
        let agrees = rule == 1 && [&a, &b].iter().any(|s| s.acquired && s.session == final_session);
        if rule == 1 {
            tally[if agrees { FINAL_AGREES } else { FINAL_DISAGREES }] += 1;
        }
        if first == 2 {
            tally[if rule == 1 && agrees { FIRST_WRONG_RULE_RESOLVED } else { FIRST_WRONG_RULE_NOT_RESOLVED }] += 1;
        }
        let gap = match (a.write_ts, b.write_ts) {
            (Some(x), Some(y)) => {
                let g = x as i64 - y as i64;
                gaps.push(g.unsigned_abs() as f64);
                Some(g)
            }
            _ => None,
        };
        if rule == 2 {
            violations.push(format!("round {round}: BOTH sessions acquired (write ts gap a-b {gap:?} ms, final holder {final_session})"));
        }
        if rule == 1 && !agrees {
            violations.push(format!("round {round}: one session acquired but the final state names {final_session} (write ts gap a-b {gap:?} ms)"));
        }
        rows.push(json!({
            "round": round, "bob_start_offset_ms": offset, "round_ms": round_ms,
            "alice": {"refused_at_check": a.refused_at_check, "first_read_ok": a.first_read_ok, "acquired": a.acquired, "took_ms": a.took_ms},
            "bob": {"refused_at_check": b.refused_at_check, "first_read_ok": b.first_read_ok, "acquired": b.acquired, "took_ms": b.took_ms},
            "write_ts_gap_alice_minus_bob_ms": gap, "final_holder": final_session,
        }));
    }
    let spent = profile::stats_since(profile::sum_stats(&[&p.owner, &p.alice, &p.bob]), before);
    let summary = json!({
        "rounds": rounds,
        "total_secs": started.elapsed().as_secs_f64(),
        "sent_events": spent.sent, "rate_limited_429": spent.rate_limited, "waited_secs": spent.waited_ms as f64 / 1000.0,
        "both_wrote_in_rounds": tally[BOTH_WROTE],
        "one_session_refused_at_the_availability_check": tally[REFUSED_AT_CHECK],
        "first_read_back": {"both_believe": tally[FIRST_BOTH], "exactly_one": tally[FIRST_ONE], "neither": tally[FIRST_NONE]},
        "two_read_rule": {"both_acquired": tally[RULE_BOTH], "exactly_one_acquired": tally[RULE_ONE], "none_acquired": tally[RULE_NONE]},
        "final_state_after_1_5s": {"names_the_acquirer": tally[FINAL_AGREES], "names_someone_else": tally[FINAL_DISAGREES]},
        "first_read_back_wrong": {"resolved_by_the_second_read": tally[FIRST_WRONG_RULE_RESOLVED], "not_resolved": tally[FIRST_WRONG_RULE_NOT_RESOLVED]},
        "write_ts_gap_abs_ms": common::summarize(gaps),
    });
    println!("LEASE-RACE SUMMARY {summary}");
    common::record(syn, "lease_race_summary", summary);
    common::record(syn, "lease_race_rows", json!({ "rows": rows }));
    if !violations.is_empty() {
        return Err(Failure::new(format!(
            "the two-read rule failed in {} of {rounds} rounds: {}",
            violations.len(),
            violations.join("; ")
        )));
    }
    Ok(())
}

/// Where the settle interval stops being enough. Both sessions read the lease
/// (free) first; alice writes at once, bob writes `gap` ms later over the same
/// stale record, so the later write lands `gap` ms after the first. If it
/// lands before alice's second read-back (at about 500 ms plus her read
/// latencies) alice must lose; after it, alice has already concluded she holds
/// the lease and bob, who read his own write twice, concludes the same: both
/// acquired. Reported, not asserted against a threshold; what is asserted is
/// that two holders never occur when the server's own
/// timestamps put the two writes 300 ms or less apart.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_read_rule_tolerance() {
    let Some(syn) = synapse() else { return };
    let default = profile::is_default(&syn);
    let gaps: &[u64] = if default { &[100, 450, 700] } else { &[100, 300, 450, 550, 700, 1000] };
    let per_gap: usize = if default { 3 } else { 6 };
    let result = profile::within_deadline(&syn, "two_read_rule_tolerance", run_tolerance(&syn, gaps, per_gap)).await;
    profile::judge(&syn, "two_read_rule_tolerance", result);
}

async fn run_tolerance(syn: &Synapse, gaps: &[u64], per_gap: usize) -> Result<(), Failure> {
    let p = pair(syn).await?;
    let mut rows: Vec<Value> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for &gap in gaps {
        let (mut both, mut one, mut none) = (0, 0, 0);
        let mut measured: Vec<f64> = Vec::new();
        for n in 0..per_gap {
            p.owner.write_lease(&p.doc, lease::released()).await?;
            if !profile::is_default(syn) {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            let tag = format!("{gap}-{n}");
            let (ca, cb) = (claim(&p.alice, &format!("a{tag}"), &p.base), claim(&p.bob, &format!("b{tag}"), &p.base));
            // Both look first, both see it free.
            let stale = p.owner.lease(&p.doc).await?;
            let (ra, rb) = tokio::join!(
                tolerance_session(&p.alice, &p.doc, &ca, stale.as_ref(), 0),
                tolerance_session(&p.bob, &p.doc, &cb, stale.as_ref(), gap)
            );
            let ((a_acq, a_ts), (b_acq, b_ts)) = (ra?, rb?);
            if let (Some(x), Some(y)) = (a_ts, b_ts) {
                measured.push(y as f64 - x as f64);
            }
            match (a_acq, b_acq) {
                (true, true) => {
                    both += 1;
                    // Judged on the gap the server stamped, not the one asked for:
                    // on the default profile the limiter moves the writes.
                    if let (Some(x), Some(y)) = (a_ts, b_ts) {
                        if (y as i64 - x as i64).abs() <= 300 {
                            failures.push(format!(
                                "requested gap {gap} ms: both sessions acquired although the writes were only {} ms apart",
                                y as i64 - x as i64
                            ));
                        }
                    }
                }
                (false, false) => none += 1,
                _ => one += 1,
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
        println!("TOLERANCE gap {gap} ms: both acquired {both}, exactly one {one}, none {none} of {per_gap}; measured write gap bob-alice ms {measured:?}");
        rows.push(json!({"requested_gap_ms": gap, "rounds": per_gap, "both_acquired": both, "exactly_one": one, "none": none, "measured_write_gap_bob_minus_alice_ms": measured}));
    }
    common::record(syn, "lease_tolerance", json!({ "settle_ms": MATRIX_SETTLE_MS, "rows": rows }));
    if failures.is_empty() {
        Ok(())
    } else {
        Err(Failure::new(failures.join("; ")))
    }
}

/// Write over the stale record the caller read, `delay_ms` late; then the rule's two reads.
async fn tolerance_session(
    store: &MatrixStore,
    doc: &DocId,
    claim: &Claim,
    stale: Option<&dcp_core::Stamped<dcp_core::Lease>>,
    delay_ms: u64,
) -> Result<(bool, Option<u64>), StoreError> {
    if delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }
    let attempt = lease::write_over(store, doc, claim, stale).await?;
    let ts = attempt.result.as_ref().ok().map(|s| s.ts_ms);
    let settle = Settle::new(&PAUSE, MATRIX_SETTLE_MS);
    let acquired = matches!(lease::confirm(store, doc, claim, &attempt, &settle).await?, Acquire::Acquired(_));
    Ok((acquired, ts))
}
