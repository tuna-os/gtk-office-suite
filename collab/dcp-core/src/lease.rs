//! Lease rules, spec 4.3.
//!
//! The lease is advisory. The pure predicates (`is_active`, `can_acquire`,
//! `renewal_due`) take the time as an argument; the async helpers (`acquire`
//! and friends) drive a [`DocumentStore`] and implement lease rules 3 and 8:
//! write over what was read (the `renewal` counter), READ BACK, wait the
//! settle interval, READ BACK again, and claim the lease only if both records
//! name this session and carry the `renewal` it wrote.

use async_trait::async_trait;

use crate::{DocId, DocumentStore, Holder, Lease, LeaseState, Role, Stamped, StoreError};

/// Lease rule 1: tolerated disagreement between the local clock and the
/// binding's.
pub const SKEW_MS: u64 = 30_000;

/// Lease rule 3: the settle interval. Spec 8.1 gives the Matrix value: at
/// least 500 ms (after 500 ms every one of 40 forced races read one record).
pub const MATRIX_SETTLE_MS: u64 = 500;

/// Spec 8.2 rule 4: a WOPI lease lasts 30 minutes, always.
pub const WOPI_DURATION_MS: u64 = 1_800_000;

/// Lets time pass between the two read-backs of lease rule 3. The core has no
/// clock of its own: a real binding sleeps, an in-memory store moves its
/// injected clock.
#[async_trait]
pub trait Pause: Send + Sync {
    async fn pause(&self, ms: u64);
}

/// The settle interval of lease rule 3 and how to wait it out.
#[derive(Clone, Copy)]
pub struct Settle<'a> {
    pub pause: &'a dyn Pause,
    pub interval_ms: u64,
}

impl<'a> Settle<'a> {
    pub fn new(pause: &'a dyn Pause, interval_ms: u64) -> Self {
        Settle { pause, interval_ms }
    }

    async fn wait(&self) {
        self.pause.pause(self.interval_ms).await;
    }
}

/// What a session asks for: the lease record it would write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub holder: Holder,
    /// Opaque; the WOPI lock string when a WOPI client is in use.
    pub token: String,
    /// The version the holder started from; `None` only when the document has
    /// no version yet (spec 4.3: omitted, never a placeholder).
    pub base_version: Option<String>,
    pub duration_ms: u64,
}

impl Claim {
    pub fn new(
        principal: impl Into<String>,
        session: impl Into<String>,
        token: impl Into<String>,
        base_version: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Claim {
            holder: Holder {
                principal: principal.into(),
                session: session.into(),
            },
            token: token.into(),
            base_version: Some(base_version.into()),
            duration_ms,
        }
    }

    /// A claim on a document that has no version yet.
    pub fn without_base_version(mut self) -> Self {
        self.base_version = None;
        self
    }

    /// The record this claim writes over `current` (what the session just
    /// read). Lease rule 8: `renewal` is 0 over a record that is not held, and
    /// the replaced record's `renewal` plus 1 over a held one (a renewal, a
    /// takeover after expiry, a break), so no write repeats a record.
    pub fn lease_over(&self, current: Option<&Stamped<Lease>>) -> Lease {
        Lease {
            dcp: 1,
            state: LeaseState::Held,
            holder: Some(self.holder.clone()),
            token: Some(self.token.clone()),
            base_version: self.base_version.clone(),
            renewal: Some(next_renewal(current.map(|c| &c.body))),
            duration_ms: Some(self.duration_ms),
        }
    }
}

/// Lease rule 8: the `renewal` a lease written over `current` must carry.
pub fn next_renewal(current: Option<&Lease>) -> u64 {
    match current {
        Some(l) if l.state == LeaseState::Held => l.renewal.unwrap_or(0) + 1,
        _ => 0,
    }
}

/// The record that releases a lease (spec examples/lease-released.json).
pub fn released() -> Lease {
    Lease {
        dcp: 1,
        state: LeaseState::Released,
        holder: None,
        token: None,
        base_version: None,
        renewal: None,
        duration_ms: None,
    }
}

/// Binding time at which a held lease stops being active.
pub fn expires_at(lease: &Stamped<Lease>) -> Option<u64> {
    match (lease.body.state, lease.body.duration_ms) {
        (LeaseState::Held, Some(d)) => Some(lease.ts_ms.saturating_add(d)),
        // A held lease without a duration violates the schema; it protects nothing.
        _ => None,
    }
}

/// Lease rule 2, in the binding's time: active while `state` is `held` and
/// binding timestamp plus `duration_ms` is later than the current binding time.
pub fn is_active(lease: &Stamped<Lease>, binding_now_ms: u64) -> bool {
    expires_at(lease).is_some_and(|end| end > binding_now_ms)
}

/// Lease rule 1 when only the LOCAL clock is at hand: the lease timestamp is
/// the binding's, so the comparison must allow [`SKEW_MS`]. The allowance
/// errs toward "still active", so a session never takes a lease the holder
/// may still believe it has. Prefer [`is_active`] with the binding's time.
pub fn is_active_local(lease: &Stamped<Lease>, local_now_ms: u64) -> bool {
    expires_at(lease).is_some_and(|end| end.saturating_add(SKEW_MS) > local_now_ms)
}

/// Does the record name this principal and session as holder?
pub fn names(lease: &Lease, who: &Holder) -> bool {
    lease.state == LeaseState::Held && lease.holder.as_ref() == Some(who)
}

/// Lease rule 3: a session MAY acquire only if no lease is active, or if it
/// is the holder.
pub fn can_acquire(current: Option<&Stamped<Lease>>, who: &Holder, binding_now_ms: u64) -> bool {
    match current {
        None => true,
        Some(l) => !is_active(l, binding_now_ms) || names(&l.body, who),
    }
}

/// Lease rule 4: the binding time by which the holder must have renewed: half
/// of `duration_ms` after the record was stamped.
pub fn renew_by(lease: &Stamped<Lease>) -> Option<u64> {
    match (lease.body.state, lease.body.duration_ms) {
        (LeaseState::Held, Some(d)) => Some(lease.ts_ms.saturating_add(d / 2)),
        _ => None,
    }
}

/// True once half of the duration has elapsed (renewal is due at the half
/// mark, not after it).
pub fn renewal_due(lease: &Stamped<Lease>, binding_now_ms: u64) -> bool {
    renew_by(lease).is_some_and(|at| binding_now_ms >= at)
}

/// What [`acquire`] and its parts conclude.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acquire {
    /// Both read-backs name this session and the `renewal` it wrote: it holds
    /// the lease.
    Acquired(Stamped<Lease>),
    /// Another session's lease was active, so nothing was written.
    Held(Stamped<Lease>),
    /// The write happened (or was refused as a conflict) but a read-back names
    /// someone else, or nothing. Another writer won the race.
    Lost(Option<Stamped<Lease>>),
}

impl Acquire {
    pub fn is_acquired(&self) -> bool {
        matches!(self, Acquire::Acquired(_))
    }
}

/// Step 1: is the lease available to `holder` right now?
pub async fn available(
    store: &dyn DocumentStore,
    doc: &DocId,
    holder: &Holder,
) -> Result<Result<(), Stamped<Lease>>, StoreError> {
    let now = store.now_ms(doc).await?;
    match store.lease(doc).await? {
        Some(current) if !can_acquire(Some(&current), holder, now) => Ok(Err(current)),
        _ => Ok(Ok(())),
    }
}

/// What [`write`] tried, and what the store said.
#[derive(Debug)]
pub struct Attempt {
    /// The record that was submitted (with its `renewal`, rule 8).
    pub lease: Lease,
    /// The store's answer. A refusal as [`StoreError::Conflict`] (a binding
    /// with compare-and-set) is not a failure of the procedure: the read-backs
    /// will say who holds the lease.
    pub result: Result<Stamped<Lease>, StoreError>,
}

/// Step 2: read the current record (so `renewal` follows rule 8) and write the
/// claim over it. Every error but a [`StoreError::Conflict`] propagates.
pub async fn write(
    store: &dyn DocumentStore,
    doc: &DocId,
    claim: &Claim,
) -> Result<Attempt, StoreError> {
    let current = store.lease(doc).await?;
    write_over(store, doc, claim, current.as_ref()).await
}

/// [`write`] over a record the session read EARLIER (`current`): two sessions
/// that both looked before either wrote do this, and on a binding without
/// compare-and-set both writes succeed with the same `renewal` (rule 7).
pub async fn write_over(
    store: &dyn DocumentStore,
    doc: &DocId,
    claim: &Claim,
    current: Option<&Stamped<Lease>>,
) -> Result<Attempt, StoreError> {
    let lease = claim.lease_over(current);
    match store.write_lease(doc, lease.clone()).await {
        Ok(written) => Ok(Attempt {
            lease,
            result: Ok(written),
        }),
        Err(e @ StoreError::Conflict(_)) => Ok(Attempt {
            lease,
            result: Err(e),
        }),
        Err(e) => Err(e),
    }
}

/// Does `record` say this session holds the lease it wrote?
fn confirms(record: &Stamped<Lease>, claim: &Claim, wrote: &Lease) -> bool {
    names(&record.body, &claim.holder) && record.body.renewal == wrote.renewal
}

/// Step 3, lease rule 3: read the lease back, wait the settle interval, read
/// it back again, and claim the lease only if BOTH records name this session
/// and carry the `renewal` it wrote. One read-back is not enough: two
/// sessions that wrote at once can each read their own record before the
/// later write lands. The loser learns of its loss at the second read.
pub async fn confirm(
    store: &dyn DocumentStore,
    doc: &DocId,
    claim: &Claim,
    attempt: &Attempt,
    settle: &Settle<'_>,
) -> Result<Acquire, StoreError> {
    match store.lease(doc).await? {
        Some(first) if confirms(&first, claim, &attempt.lease) => {
            settle.wait().await;
            match store.lease(doc).await? {
                Some(second) if confirms(&second, claim, &attempt.lease) => {
                    Ok(Acquire::Acquired(second))
                }
                other => Ok(Acquire::Lost(other)),
            }
        }
        other => Ok(Acquire::Lost(other)),
    }
}

/// Acquire (or renew, if already the holder) the lease:
/// [`available`], then [`write`], then [`confirm`].
pub async fn acquire(
    store: &dyn DocumentStore,
    doc: &DocId,
    claim: &Claim,
    settle: &Settle<'_>,
) -> Result<Acquire, StoreError> {
    if let Err(current) = available(store, doc, &claim.holder).await? {
        return Ok(Acquire::Held(current));
    }
    let attempt = write(store, doc, claim).await?;
    confirm(store, doc, claim, &attempt, settle).await
}

/// Lease rule 4 for the holder: renew once half the duration has elapsed (a
/// write under rule 8: `renewal` goes up by one). `None` when no renewal is
/// due yet. Pass the record from the last [`Acquire::Acquired`] / renewal.
pub async fn renew_if_due(
    store: &dyn DocumentStore,
    doc: &DocId,
    claim: &Claim,
    current: &Stamped<Lease>,
    settle: &Settle<'_>,
) -> Result<Option<Acquire>, StoreError> {
    let now = store.now_ms(doc).await?;
    if !renewal_due(current, now) {
        return Ok(None);
    }
    acquire(store, doc, claim, settle).await.map(Some)
}

/// Does the binding currently say this session holds an active lease?
/// (A holder checks this before it saves; a broken lease shows up here, which
/// is how it is "visible to the former holder", rule 5.)
pub async fn holds(
    store: &dyn DocumentStore,
    doc: &DocId,
    holder: &Holder,
) -> Result<bool, StoreError> {
    let now = store.now_ms(doc).await?;
    Ok(store
        .lease(doc)
        .await?
        .is_some_and(|l| names(&l.body, holder) && is_active(&l, now)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Release {
    Released,
    /// The record did not name this session, so nothing was written.
    NotHolder,
}

/// Lease rule 4: release when the document closes normally. Only the session
/// the record names releases it.
pub async fn release(
    store: &dyn DocumentStore,
    doc: &DocId,
    holder: &Holder,
) -> Result<Release, StoreError> {
    match store.lease(doc).await? {
        Some(l) if names(&l.body, holder) => {
            store.write_lease(doc, released()).await?;
            Ok(Release::Released)
        }
        _ => Ok(Release::NotHolder),
    }
}

/// Lease rule 5: a principal with the `owner` role MAY break a lease. The
/// result is a lease held by the breaker, and the read-back confirms it.
/// Anyone else is refused here and, in a store that enforces it, there too.
pub async fn break_lease(
    store: &dyn DocumentStore,
    doc: &DocId,
    claim: &Claim,
    settle: &Settle<'_>,
) -> Result<Acquire, StoreError> {
    if store.role(doc).await? != Role::Owner {
        return Err(StoreError::Forbidden(
            "only an owner may break a lease".into(),
        ));
    }
    let attempt = write(store, doc, claim).await?;
    confirm(store, doc, claim, &attempt, settle).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{ManualClock, MemoryBackend, MemoryConfig};
    use crate::{Meta, NewFile};
    use std::sync::Arc;

    fn stamped(ts: u64, lease: Lease) -> Stamped<Lease> {
        Stamped::new("$l", ts, "@a:x", lease)
    }

    fn me() -> Holder {
        Holder {
            principal: "@a:x".into(),
            session: "s1".into(),
        }
    }

    fn other() -> Holder {
        Holder {
            principal: "@b:x".into(),
            session: "s2".into(),
        }
    }

    fn claim(h: &Holder, duration_ms: u64) -> Claim {
        Claim::new(
            h.principal.clone(),
            h.session.clone(),
            format!("tok-{}", h.session),
            "$v1",
            duration_ms,
        )
    }

    /// A held record as a claim writes it over nothing (`renewal` 0).
    fn held(c: &Claim) -> Lease {
        c.lease_over(None)
    }

    #[test]
    fn active_until_timestamp_plus_duration_exclusive() {
        let l = stamped(1_000, held(&claim(&me(), 60_000)));
        assert!(is_active(&l, 1_000));
        assert!(is_active(&l, 60_999));
        assert!(
            !is_active(&l, 61_000),
            "ts + duration is not later than now: expired"
        );
        assert!(!is_active(&l, 90_000));
    }

    #[test]
    fn released_and_malformed_leases_are_never_active() {
        assert!(!is_active(&stamped(0, released()), 0));
        let mut no_duration = held(&claim(&me(), 5_000));
        no_duration.duration_ms = None;
        assert!(!is_active(&stamped(0, no_duration), 0));
    }

    #[test]
    fn local_clock_comparison_allows_30_seconds_of_skew() {
        let l = stamped(0, held(&claim(&me(), 60_000)));
        assert!(
            is_active_local(&l, 60_000),
            "past the binding expiry but inside the skew"
        );
        assert!(is_active_local(&l, 89_999));
        assert!(!is_active_local(&l, 90_000));
        assert_eq!(SKEW_MS, 30_000);
        assert!(
            !is_active(&l, 60_000),
            "while the binding's own clock has no skew allowance"
        );
    }

    #[test]
    fn acquire_allowed_when_none_inactive_or_holder_only() {
        let held_by_other = stamped(0, held(&claim(&other(), 60_000)));
        assert!(can_acquire(None, &me(), 0));
        assert!(!can_acquire(Some(&held_by_other), &me(), 59_999));
        assert!(can_acquire(Some(&held_by_other), &me(), 60_000), "expired");
        assert!(can_acquire(Some(&stamped(0, released())), &me(), 1));
        let mine = stamped(0, held(&claim(&me(), 60_000)));
        assert!(can_acquire(Some(&mine), &me(), 10), "the holder may renew");
    }

    #[test]
    fn same_principal_other_session_is_not_the_holder() {
        let l = stamped(0, held(&claim(&me(), 60_000)));
        let sibling = Holder {
            principal: me().principal,
            session: "s9".into(),
        };
        assert!(!can_acquire(Some(&l), &sibling, 10));
    }

    #[test]
    fn renewal_is_due_at_half_the_duration() {
        let l = stamped(1_000, held(&claim(&me(), 60_000)));
        assert_eq!(renew_by(&l), Some(31_000));
        assert!(!renewal_due(&l, 30_999));
        assert!(renewal_due(&l, 31_000));
        assert!(renewal_due(&l, 50_000));
        assert!(
            !renewal_due(&stamped(0, released()), 99),
            "nothing to renew"
        );
    }

    /// Lease rule 8: 0 over nothing or a released record, previous + 1 over a
    /// held one, whoever wrote it and whether or not it has expired.
    #[test]
    fn renewal_counter_follows_rule_8() {
        let c = claim(&me(), 60_000);
        assert_eq!(c.lease_over(None).renewal, Some(0));
        assert_eq!(
            c.lease_over(Some(&stamped(0, released()))).renewal,
            Some(0),
            "over a released record"
        );
        let first = stamped(0, c.lease_over(None));
        let renewed = c.lease_over(Some(&first));
        assert_eq!(renewed.renewal, Some(1));
        assert_ne!(renewed, first.body, "a renewal is a different record");
        let taken = claim(&other(), 60_000).lease_over(Some(&stamped(1, renewed)));
        assert_eq!(taken.renewal, Some(2), "a takeover continues the count");
        assert_eq!(released().renewal, None);
    }

    #[test]
    fn a_claim_on_an_empty_document_omits_the_base_version() {
        let c = claim(&me(), 60_000).without_base_version();
        let l = c.lease_over(None);
        assert_eq!(l.base_version, None);
        crate::schema::validate_typed(crate::schema::Kind::Lease, &l).unwrap();
        let v = serde_json::to_value(&l).unwrap();
        assert!(v.get("base_version").is_none(), "{v}");
    }

    // ---- against a store -------------------------------------------------

    struct ManualPause(Arc<ManualClock>);

    #[async_trait]
    impl Pause for ManualPause {
        async fn pause(&self, ms: u64) {
            self.0.advance(ms);
        }
    }

    struct World {
        clock: Arc<ManualClock>,
        pause: ManualPause,
        a: Arc<dyn DocumentStore>,
        b: Arc<dyn DocumentStore>,
        owner: Arc<dyn DocumentStore>,
        viewer: Arc<dyn DocumentStore>,
        doc: DocId,
    }

    impl World {
        fn settle(&self) -> Settle<'_> {
            Settle::new(&self.pause, MATRIX_SETTLE_MS)
        }
    }

    async fn world(cfg: MemoryConfig) -> World {
        let clock = Arc::new(ManualClock::new(1_000_000));
        let backend = MemoryBackend::with_config(clock.clone(), cfg);
        let owner = backend.store("@owner:mem", "OWNERDEV");
        let a = backend.store("@a:x", "ADEV");
        let b = backend.store("@b:x", "BDEV");
        let viewer = backend.store("@v:x", "VDEV");
        let meta = Meta {
            dcp: 1,
            title: "t".into(),
            mime: "text/plain".into(),
            levels: vec![crate::Level::L1],
            algorithms: None,
            editor_services: None,
        };
        let doc = owner
            .create_document(
                &meta,
                Some(NewFile {
                    mime: "text/plain".into(),
                    bytes: b"x".to_vec(),
                }),
            )
            .await
            .unwrap();
        owner.grant(&doc, "@a:x", Role::Editor).await.unwrap();
        owner.grant(&doc, "@b:x", Role::Editor).await.unwrap();
        owner.grant(&doc, "@v:x", Role::Viewer).await.unwrap();
        World {
            pause: ManualPause(clock.clone()),
            clock,
            a: Arc::new(a),
            b: Arc::new(b),
            owner: Arc::new(owner),
            viewer: Arc::new(viewer),
            doc,
        }
    }

    #[tokio::test]
    async fn acquire_then_other_session_is_held_off() {
        let w = world(MemoryConfig::default()).await;
        let st = w.settle();
        let r = acquire(&*w.a, &w.doc, &claim(&me(), 60_000), &st)
            .await
            .unwrap();
        assert!(r.is_acquired());
        let r = acquire(&*w.b, &w.doc, &claim(&other(), 60_000), &st)
            .await
            .unwrap();
        assert!(matches!(r, Acquire::Held(l) if l.body.holder == Some(me())));
    }

    #[tokio::test]
    async fn the_second_read_back_waits_the_settle_interval() {
        let w = world(MemoryConfig::default()).await;
        let before = w.a.now_ms(&w.doc).await.unwrap();
        let r = acquire(&*w.a, &w.doc, &claim(&me(), 60_000), &w.settle())
            .await
            .unwrap();
        assert!(r.is_acquired());
        let after = w.a.now_ms(&w.doc).await.unwrap();
        assert!(
            after - before >= MATRIX_SETTLE_MS,
            "acquire must wait the settle interval between its two read-backs"
        );
    }

    #[tokio::test]
    async fn read_back_resolves_two_writers_that_both_succeed() {
        // Racy store: no compare-and-set, both writes succeed. Both sessions
        // checked first, then wrote, then read back: the second write is the
        // one the record keeps, and only that session may claim the lease.
        let w = world(MemoryConfig {
            cas_leases: false,
            ..MemoryConfig::default()
        })
        .await;
        let st = w.settle();
        let (ca, cb) = (claim(&me(), 60_000), claim(&other(), 60_000));
        assert!(available(&*w.a, &w.doc, &ca.holder).await.unwrap().is_ok());
        assert!(available(&*w.b, &w.doc, &cb.holder).await.unwrap().is_ok());
        let wa = write(&*w.a, &w.doc, &ca).await.unwrap();
        assert!(wa.result.is_ok());
        let wb = write(&*w.b, &w.doc, &cb).await.unwrap();
        assert!(wb.result.is_ok(), "the racy store accepted both writes");
        let ra = confirm(&*w.a, &w.doc, &ca, &wa, &st).await.unwrap();
        let rb = confirm(&*w.b, &w.doc, &cb, &wb, &st).await.unwrap();
        assert!(
            matches!(&ra, Acquire::Lost(Some(l)) if l.body.holder == Some(other())),
            "{ra:?}"
        );
        assert!(rb.is_acquired(), "{rb:?}");
    }

    /// The reason for the second read: a session whose first read-back still
    /// names it must not claim the lease if a later write replaced the record
    /// during the settle interval.
    #[tokio::test]
    async fn a_write_landing_during_the_settle_interval_is_caught_by_the_second_read() {
        struct LateWriter {
            clock: Arc<ManualClock>,
            store: Arc<dyn DocumentStore>,
            doc: DocId,
            claim: Claim,
        }
        #[async_trait]
        impl Pause for LateWriter {
            async fn pause(&self, ms: u64) {
                self.clock.advance(ms);
                // The other session's write lands while A waits.
                let cur = self.store.lease(&self.doc).await.unwrap();
                self.store
                    .write_lease(&self.doc, self.claim.lease_over(cur.as_ref()))
                    .await
                    .unwrap();
            }
        }
        let w = world(MemoryConfig {
            cas_leases: false,
            ..MemoryConfig::default()
        })
        .await;
        let late = LateWriter {
            clock: w.clock.clone(),
            store: w.b.clone(),
            doc: w.doc.clone(),
            claim: claim(&other(), 60_000),
        };
        let ca = claim(&me(), 60_000);
        let wa = write(&*w.a, &w.doc, &ca).await.unwrap();
        let r = confirm(&*w.a, &w.doc, &ca, &wa, &Settle::new(&late, MATRIX_SETTLE_MS))
            .await
            .unwrap();
        assert!(
            matches!(&r, Acquire::Lost(Some(l)) if l.body.holder == Some(other())),
            "the first read named A, the second did not: {r:?}"
        );
    }

    #[tokio::test]
    async fn compare_and_set_store_refuses_the_second_writer_and_read_back_agrees() {
        let w = world(MemoryConfig::default()).await;
        let st = w.settle();
        let (ca, cb) = (claim(&me(), 60_000), claim(&other(), 60_000));
        let wa = write(&*w.a, &w.doc, &ca).await.unwrap();
        assert!(wa.result.is_ok());
        let wb = write(&*w.b, &w.doc, &cb).await.unwrap();
        assert!(matches!(wb.result, Err(StoreError::Conflict(_))));
        assert!(confirm(&*w.a, &w.doc, &ca, &wa, &st)
            .await
            .unwrap()
            .is_acquired());
        assert!(matches!(
            confirm(&*w.b, &w.doc, &cb, &wb, &st).await.unwrap(),
            Acquire::Lost(_)
        ));
    }

    #[tokio::test]
    async fn renewal_extends_and_expiry_frees_the_lease() {
        let w = world(MemoryConfig::default()).await;
        let st = w.settle();
        let c = claim(&me(), 60_000);
        let Acquire::Acquired(first) = acquire(&*w.a, &w.doc, &c, &st).await.unwrap() else {
            panic!()
        };
        assert_eq!(first.body.renewal, Some(0));
        assert!(
            renew_if_due(&*w.a, &w.doc, &c, &first, &st)
                .await
                .unwrap()
                .is_none(),
            "not due at once"
        );
        w.clock.advance(30_000);
        let Some(Acquire::Acquired(second)) = renew_if_due(&*w.a, &w.doc, &c, &first, &st)
            .await
            .unwrap()
        else {
            panic!("due at half the duration")
        };
        assert!(second.ts_ms > first.ts_ms);
        assert_eq!(
            second.body.renewal,
            Some(1),
            "a renewal increases the counter by one (rule 8)"
        );
        w.clock.advance(59_000);
        assert!(
            acquire(&*w.b, &w.doc, &claim(&other(), 60_000), &st)
                .await
                .unwrap()
                == Acquire::Held(second.clone())
        );
        w.clock.advance(2_000);
        let Acquire::Acquired(taken) = acquire(&*w.b, &w.doc, &claim(&other(), 60_000), &st)
            .await
            .unwrap()
        else {
            panic!("expired: free to take")
        };
        assert_eq!(
            taken.body.renewal,
            Some(2),
            "a takeover over an expired held record continues the count"
        );
        assert!(
            !holds(&*w.a, &w.doc, &c.holder).await.unwrap(),
            "the former holder sees it is gone"
        );
    }

    #[tokio::test]
    async fn only_the_named_session_releases() {
        let w = world(MemoryConfig::default()).await;
        let st = w.settle();
        let ca = claim(&me(), 60_000);
        acquire(&*w.a, &w.doc, &ca, &st).await.unwrap();
        assert_eq!(
            release(&*w.b, &w.doc, &other()).await.unwrap(),
            Release::NotHolder
        );
        assert!(holds(&*w.a, &w.doc, &ca.holder).await.unwrap());
        assert_eq!(
            release(&*w.a, &w.doc, &ca.holder).await.unwrap(),
            Release::Released
        );
        assert!(!holds(&*w.a, &w.doc, &ca.holder).await.unwrap());
        let Acquire::Acquired(next) = acquire(&*w.b, &w.doc, &claim(&other(), 60_000), &st)
            .await
            .unwrap()
        else {
            panic!("free after release")
        };
        assert_eq!(next.body.renewal, Some(0), "over a released record");
    }

    #[tokio::test]
    async fn owner_breaks_a_lease_and_the_holder_sees_it() {
        let w = world(MemoryConfig::default()).await;
        let st = w.settle();
        let ca = claim(&me(), 60_000);
        acquire(&*w.a, &w.doc, &ca, &st).await.unwrap();
        let breaker = claim(
            &Holder {
                principal: "@owner:mem".into(),
                session: "so".into(),
            },
            60_000,
        );
        let r = break_lease(&*w.owner, &w.doc, &breaker, &st).await.unwrap();
        let Acquire::Acquired(rec) = r else {
            panic!("{r:?}")
        };
        assert_eq!(rec.body.state, LeaseState::Held);
        assert_eq!(
            rec.body.holder,
            Some(breaker.holder.clone()),
            "recorded as held by the breaker"
        );
        assert_eq!(rec.body.renewal, Some(1), "a break is a write over a held record");
        assert!(
            !holds(&*w.a, &w.doc, &ca.holder).await.unwrap(),
            "visible to the former holder"
        );
    }

    #[tokio::test]
    async fn only_an_owner_may_break() {
        let w = world(MemoryConfig::default()).await;
        let st = w.settle();
        acquire(
            &*w.owner,
            &w.doc,
            &claim(
                &Holder {
                    principal: "@owner:mem".into(),
                    session: "so".into(),
                },
                60_000,
            ),
            &st,
        )
        .await
        .unwrap();
        let r = break_lease(&*w.b, &w.doc, &claim(&other(), 60_000), &st).await;
        assert!(matches!(r, Err(StoreError::Forbidden(_))));
    }

    #[tokio::test]
    async fn a_viewer_cannot_take_a_lease_and_nothing_is_claimed() {
        let w = world(MemoryConfig::default()).await;
        let v = Holder {
            principal: "@v:x".into(),
            session: "sv".into(),
        };
        let r = acquire(&*w.viewer, &w.doc, &claim(&v, 60_000), &w.settle()).await;
        assert!(matches!(r, Err(StoreError::Forbidden(_))), "{r:?}");
    }

    #[tokio::test]
    async fn lease_lost_while_editing_still_saves_as_a_version_and_shows_the_conflict() {
        // Failure table row 2: continue locally; before saving, try to
        // reacquire; if another session holds the lease, save as a version
        // anyway (rule 6) and show the resulting conflict.
        let w = world(MemoryConfig::default()).await;
        let st = w.settle();
        let base = w.a.versions(&w.doc).await.unwrap()[0].id.clone();
        let mine = Claim::new("@a:x", "s1", "t1", &base, 60_000);
        assert!(acquire(&*w.a, &w.doc, &mine, &st)
            .await
            .unwrap()
            .is_acquired());
        w.clock.advance(61_000); // the lease expires while A keeps editing offline
        assert!(acquire(&*w.b, &w.doc, &claim(&other(), 60_000), &st)
            .await
            .unwrap()
            .is_acquired());

        let retry = acquire(&*w.a, &w.doc, &mine, &st).await.unwrap();
        assert!(
            matches!(retry, Acquire::Held(_)),
            "another session holds it now"
        );
        let file = |s: &str| NewFile {
            mime: "text/plain".into(),
            bytes: s.as_bytes().to_vec(),
        };
        w.a.publish_version(&w.doc, vec![base.clone()], file("A's edits"), None)
            .await
            .unwrap();
        w.b.publish_version(&w.doc, vec![base], file("B's edits"), None)
            .await
            .unwrap();
        let view = crate::view::read_view(&*w.owner, &w.doc).await.unwrap();
        assert!(view.is_conflict(), "both saves are kept, as two heads");
    }
}
