//! Spec test 5 (`lease_expiry_releases`) is written with a 4 second lease and the
//! 500 ms settle interval of lease rule 3. Between the renewed record's own
//! timestamp and the check that it is still alive the journey spends the settle
//! interval, two read-backs and two more reads. That fits a store that answers in
//! tens of milliseconds and not one that needs more than about 100 ms per
//! operation, which is Nextcloud over a federation. This is the same journey with
//! the duration a parameter, run at 4 s and at 20 s on both topologies, to show
//! whether the failure is the binding's lease logic or the journey's clock.

mod common;

use std::sync::Arc;

use common::*;
use dcp_conformance::{ensure, Failure, Fixture, StoreFactory};
use dcp_core::lease::{self, Acquire, Claim, Pause, Release, Settle, MATRIX_SETTLE_MS};
use dcp_core::LeaseState;

struct RealPause;

#[async_trait::async_trait]
impl Pause for RealPause {
    async fn pause(&self, ms: u64) {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
    }
}

async fn wait_lease(
    fx: &Fixture,
    who: &Arc<dyn dcp_core::DocumentStore>,
    what: &str,
    pred: impl Fn(Option<&dcp_core::Stamped<dcp_core::Lease>>) -> bool,
) -> Result<(), Failure> {
    fx.settle(what, || async {
        let l = who.lease(&fx.doc).await?;
        Ok(pred(l.as_ref()).then_some(()))
    })
    .await
}

/// `dcp_conformance::journeys::lease_expiry_releases` with `d` for its 4 000 ms.
async fn lease_expiry_releases(f: &dyn StoreFactory, d: u64) -> Result<(), Failure> {
    let fx = f.fresh().await?;
    let first = fx.first_version.clone();
    let (ed, ow) = (&fx.editor, &fx.owner);
    let ce = Claim::new(ed.principal(), "session-editor", "token-editor", &first, d);
    let co = Claim::new(ow.principal(), "session-owner", "token-owner", &first, d);
    let pause = RealPause;
    let st = Settle::new(&pause, MATRIX_SETTLE_MS);

    let Acquire::Acquired(held) = lease::acquire(&**ed, &fx.doc, &ce, &st).await? else {
        return Err(Failure::new("a fresh document's lease could not be acquired"));
    };
    ensure!(held.body.renewal == Some(0), "renewal {:?}", held.body.renewal);
    wait_lease(&fx, ow, "the owner sees the editor's lease", |l| l.is_some_and(|l| l.id == held.id)).await?;
    let blocked = lease::acquire(&**ow, &fx.doc, &co, &st).await?;
    ensure!(matches!(blocked, Acquire::Held(_)), "an active lease was taken: {blocked:?}");

    // Kill the holder: it stops renewing. After the duration anyone may acquire.
    fx.time.pass(d + 500).await;
    let Acquire::Acquired(taken) = lease::acquire(&**ow, &fx.doc, &co, &st).await? else {
        return Err(Failure::new("the lease did not expire after its duration"));
    };
    ensure!(taken.body.renewal == Some(1), "takeover renewal {:?}", taken.body.renewal);
    wait_lease(&fx, ed, "the editor sees the new lease", |l| l.is_some_and(|l| l.id == taken.id)).await?;
    ensure!(!lease::holds(&**ed, &fx.doc, &ce.holder).await?, "the dead holder still thinks it holds the lease");
    let retry = lease::acquire(&**ed, &fx.doc, &ce, &st).await?;
    ensure!(matches!(retry, Acquire::Held(_)), "the expired holder retook the lease: {retry:?}");

    // Renewal at half the duration extends the lease.
    fx.time.pass(d / 2).await;
    let renewed = lease::renew_if_due(&**ow, &fx.doc, &co, &taken, &st).await?;
    let Some(Acquire::Acquired(renewed)) = renewed else {
        return Err(Failure::new(format!("no renewal at half the duration: {renewed:?}")));
    };
    ensure!(renewed.ts_ms > taken.ts_ms, "renewal did not produce a newer record");
    ensure!(renewed.body.renewal == Some(2), "renewal counter {:?}", renewed.body.renewal);
    fx.time.pass(d * 3 / 4).await; // past the ORIGINAL expiry, inside the renewed one
    wait_lease(&fx, ed, "the editor sees the renewal", |l| l.is_some_and(|l| l.id == renewed.id)).await?;
    let still = lease::acquire(&**ed, &fx.doc, &ce, &st).await?;
    ensure!(matches!(still, Acquire::Held(_)), "renewal did not keep the lease alive: {still:?}");

    ensure!(lease::release(&**ow, &fx.doc, &co.holder).await? == Release::Released, "release failed");
    wait_lease(&fx, ed, "the editor sees the release", |l| l.is_some_and(|l| l.body.state == LeaseState::Released)).await?;
    let free = lease::acquire(&**ed, &fx.doc, &ce, &st).await?;
    ensure!(free.is_acquired(), "a released lease could not be acquired: {free:?}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lease_expiry_at_a_duration_that_fits_the_binding() {
    if require("lease_expiry_at_a_duration_that_fits_the_binding", &[url_a(), url_b()]).await.is_none() {
        return;
    }
    let mut results = Vec::new();
    for (label, f) in [
        ("one instance", NcFactory::single(false).await),
        ("federation", NcFactory::federated().await),
    ] {
        for d in [4_000u64, 20_000] {
            let r = lease_expiry_releases(&f, d).await;
            results.push(serde_json::json!({ "topology": label, "duration_ms": d, "passed": r.is_ok(), "failure": r.err().map(|e| e.to_string().chars().take(160).collect::<String>()) }));
        }
    }
    measure("lease_expiry_by_duration", serde_json::json!(results));
    for r in &results {
        if r["duration_ms"] == 20_000 {
            assert_eq!(r["passed"], true, "a 20 s lease must work on every topology: {r}");
        }
    }
}
