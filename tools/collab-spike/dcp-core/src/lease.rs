//! Lease rules, spec 4.3.
//!
//! The lease is advisory. The pure predicates (`is_active`, `can_acquire`,
//! `renewal_due`) take the time as an argument; the async helpers (`acquire`
//! and friends) drive a [`DocumentStore`] and implement lease rule 3: write,
//! then READ BACK, and claim the lease only if the record names this session.

use crate::{DocId, DocumentStore, Holder, Lease, LeaseState, Role, Stamped, StoreError};

/// Lease rule 1: tolerated disagreement between the local clock and the
/// binding's.
pub const SKEW_MS: u64 = 30_000;

/// What a session asks for: the lease record it would write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub holder: Holder,
    /// Opaque; the WOPI lock string when a WOPI client is in use.
    pub token: String,
    /// The version the holder started from.
    pub base_version: String,
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
            base_version: base_version.into(),
            duration_ms,
        }
    }

    pub fn lease(&self) -> Lease {
        Lease {
            dcp: 0,
            state: LeaseState::Held,
            holder: Some(self.holder.clone()),
            token: Some(self.token.clone()),
            base_version: Some(self.base_version.clone()),
            duration_ms: Some(self.duration_ms),
        }
    }
}

/// The record that releases a lease (spec examples/lease-released.json).
pub fn released() -> Lease {
    Lease {
        dcp: 0,
        state: LeaseState::Released,
        holder: None,
        token: None,
        base_version: None,
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
    /// The read-back names this session: it holds the lease.
    Acquired(Stamped<Lease>),
    /// Another session's lease was active, so nothing was written.
    Held(Stamped<Lease>),
    /// The write happened (or was refused as a conflict) but the read-back
    /// names someone else, or nothing. Another writer won the race.
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

/// Step 2: write the claim. A refusal as [`StoreError::Conflict`] (a binding
/// with compare-and-set) is returned as `Ok(Err(..))`: it is not a failure,
/// the read-back will say who holds the lease. Every other error propagates.
pub async fn write(
    store: &dyn DocumentStore,
    doc: &DocId,
    claim: &Claim,
) -> Result<Result<Stamped<Lease>, StoreError>, StoreError> {
    match store.write_lease(doc, claim.lease()).await {
        Ok(written) => Ok(Ok(written)),
        Err(e @ StoreError::Conflict(_)) => Ok(Err(e)),
        Err(e) => Err(e),
    }
}

/// Step 3, lease rule 3: read the lease back and claim it only if the record
/// names this session. This is what resolves two sessions that wrote at once.
pub async fn confirm(
    store: &dyn DocumentStore,
    doc: &DocId,
    claim: &Claim,
) -> Result<Acquire, StoreError> {
    match store.lease(doc).await? {
        Some(record) if names(&record.body, &claim.holder) => Ok(Acquire::Acquired(record)),
        other => Ok(Acquire::Lost(other)),
    }
}

/// Acquire (or renew, if already the holder) the lease:
/// [`available`], then [`write`], then [`confirm`].
pub async fn acquire(
    store: &dyn DocumentStore,
    doc: &DocId,
    claim: &Claim,
) -> Result<Acquire, StoreError> {
    if let Err(current) = available(store, doc, &claim.holder).await? {
        return Ok(Acquire::Held(current));
    }
    let _ = write(store, doc, claim).await?;
    confirm(store, doc, claim).await
}

/// Lease rule 4 for the holder: renew once half the duration has elapsed.
/// `None` when no renewal is due yet. Pass the record from the last
/// [`Acquire::Acquired`] / renewal.
pub async fn renew_if_due(
    store: &dyn DocumentStore,
    doc: &DocId,
    claim: &Claim,
    current: &Stamped<Lease>,
) -> Result<Option<Acquire>, StoreError> {
    let now = store.now_ms(doc).await?;
    if !renewal_due(current, now) {
        return Ok(None);
    }
    acquire(store, doc, claim).await.map(Some)
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
) -> Result<Acquire, StoreError> {
    if store.role(doc).await? != Role::Owner {
        return Err(StoreError::Forbidden(
            "only an owner may break a lease".into(),
        ));
    }
    let _ = write(store, doc, claim).await?;
    confirm(store, doc, claim).await
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

    #[test]
    fn active_until_timestamp_plus_duration_exclusive() {
        let l = stamped(1_000, claim(&me(), 60_000).lease());
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
        let mut no_duration = claim(&me(), 5_000).lease();
        no_duration.duration_ms = None;
        assert!(!is_active(&stamped(0, no_duration), 0));
    }

    #[test]
    fn local_clock_comparison_allows_30_seconds_of_skew() {
        let l = stamped(0, claim(&me(), 60_000).lease());
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
        let held_by_other = stamped(0, claim(&other(), 60_000).lease());
        assert!(can_acquire(None, &me(), 0));
        assert!(!can_acquire(Some(&held_by_other), &me(), 59_999));
        assert!(can_acquire(Some(&held_by_other), &me(), 60_000), "expired");
        assert!(can_acquire(Some(&stamped(0, released())), &me(), 1));
        let mine = stamped(0, claim(&me(), 60_000).lease());
        assert!(can_acquire(Some(&mine), &me(), 10), "the holder may renew");
    }

    #[test]
    fn same_principal_other_session_is_not_the_holder() {
        let l = stamped(0, claim(&me(), 60_000).lease());
        let sibling = Holder {
            principal: me().principal,
            session: "s9".into(),
        };
        assert!(!can_acquire(Some(&l), &sibling, 10));
    }

    #[test]
    fn renewal_is_due_at_half_the_duration() {
        let l = stamped(1_000, claim(&me(), 60_000).lease());
        assert_eq!(renew_by(&l), Some(31_000));
        assert!(!renewal_due(&l, 30_999));
        assert!(renewal_due(&l, 31_000));
        assert!(renewal_due(&l, 50_000));
        assert!(
            !renewal_due(&stamped(0, released()), 99),
            "nothing to renew"
        );
    }

    // ---- against a store -------------------------------------------------

    struct World {
        clock: Arc<ManualClock>,
        a: Arc<dyn DocumentStore>,
        b: Arc<dyn DocumentStore>,
        owner: Arc<dyn DocumentStore>,
        viewer: Arc<dyn DocumentStore>,
        doc: DocId,
    }

    async fn world(cfg: MemoryConfig) -> World {
        let clock = Arc::new(ManualClock::new(1_000_000));
        let backend = MemoryBackend::with_config(clock.clone(), cfg);
        let owner = backend.store("@owner:mem", "OWNERDEV");
        let a = backend.store("@a:x", "ADEV");
        let b = backend.store("@b:x", "BDEV");
        let viewer = backend.store("@v:x", "VDEV");
        let meta = Meta {
            dcp: 0,
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
        let r = acquire(&*w.a, &w.doc, &claim(&me(), 60_000)).await.unwrap();
        assert!(r.is_acquired());
        let r = acquire(&*w.b, &w.doc, &claim(&other(), 60_000))
            .await
            .unwrap();
        assert!(matches!(r, Acquire::Held(l) if l.body.holder == Some(me())));
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
        let (ca, cb) = (claim(&me(), 60_000), claim(&other(), 60_000));
        assert!(available(&*w.a, &w.doc, &ca.holder).await.unwrap().is_ok());
        assert!(available(&*w.b, &w.doc, &cb.holder).await.unwrap().is_ok());
        assert!(write(&*w.a, &w.doc, &ca).await.unwrap().is_ok());
        assert!(
            write(&*w.b, &w.doc, &cb).await.unwrap().is_ok(),
            "the racy store accepted both writes"
        );
        let ra = confirm(&*w.a, &w.doc, &ca).await.unwrap();
        let rb = confirm(&*w.b, &w.doc, &cb).await.unwrap();
        assert!(
            matches!(&ra, Acquire::Lost(Some(l)) if l.body.holder == Some(other())),
            "{ra:?}"
        );
        assert!(rb.is_acquired(), "{rb:?}");
    }

    #[tokio::test]
    async fn compare_and_set_store_refuses_the_second_writer_and_read_back_agrees() {
        let w = world(MemoryConfig::default()).await;
        let (ca, cb) = (claim(&me(), 60_000), claim(&other(), 60_000));
        assert!(write(&*w.a, &w.doc, &ca).await.unwrap().is_ok());
        assert!(matches!(
            write(&*w.b, &w.doc, &cb).await.unwrap(),
            Err(StoreError::Conflict(_))
        ));
        assert!(confirm(&*w.a, &w.doc, &ca).await.unwrap().is_acquired());
        assert!(matches!(
            confirm(&*w.b, &w.doc, &cb).await.unwrap(),
            Acquire::Lost(_)
        ));
    }

    #[tokio::test]
    async fn renewal_extends_and_expiry_frees_the_lease() {
        let w = world(MemoryConfig::default()).await;
        let c = claim(&me(), 60_000);
        let Acquire::Acquired(first) = acquire(&*w.a, &w.doc, &c).await.unwrap() else {
            panic!()
        };
        assert!(
            renew_if_due(&*w.a, &w.doc, &c, &first)
                .await
                .unwrap()
                .is_none(),
            "not due at once"
        );
        w.clock.advance(30_000);
        let Some(Acquire::Acquired(second)) =
            renew_if_due(&*w.a, &w.doc, &c, &first).await.unwrap()
        else {
            panic!("due at half the duration")
        };
        assert!(second.ts_ms > first.ts_ms);
        w.clock.advance(59_000);
        assert!(
            acquire(&*w.b, &w.doc, &claim(&other(), 60_000))
                .await
                .unwrap()
                == Acquire::Held(second.clone())
        );
        w.clock.advance(2_000);
        assert!(acquire(&*w.b, &w.doc, &claim(&other(), 60_000))
            .await
            .unwrap()
            .is_acquired());
        assert!(
            !holds(&*w.a, &w.doc, &c.holder).await.unwrap(),
            "the former holder sees it is gone"
        );
    }

    #[tokio::test]
    async fn only_the_named_session_releases() {
        let w = world(MemoryConfig::default()).await;
        let ca = claim(&me(), 60_000);
        acquire(&*w.a, &w.doc, &ca).await.unwrap();
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
        assert!(acquire(&*w.b, &w.doc, &claim(&other(), 60_000))
            .await
            .unwrap()
            .is_acquired());
    }

    #[tokio::test]
    async fn owner_breaks_a_lease_and_the_holder_sees_it() {
        let w = world(MemoryConfig::default()).await;
        let ca = claim(&me(), 60_000);
        acquire(&*w.a, &w.doc, &ca).await.unwrap();
        let breaker = claim(
            &Holder {
                principal: "@owner:mem".into(),
                session: "so".into(),
            },
            60_000,
        );
        let r = break_lease(&*w.owner, &w.doc, &breaker).await.unwrap();
        let Acquire::Acquired(rec) = r else {
            panic!("{r:?}")
        };
        assert_eq!(rec.body.state, LeaseState::Held);
        assert_eq!(
            rec.body.holder,
            Some(breaker.holder.clone()),
            "recorded as held by the breaker"
        );
        assert!(
            !holds(&*w.a, &w.doc, &ca.holder).await.unwrap(),
            "visible to the former holder"
        );
    }

    #[tokio::test]
    async fn only_an_owner_may_break() {
        let w = world(MemoryConfig::default()).await;
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
        )
        .await
        .unwrap();
        let r = break_lease(&*w.b, &w.doc, &claim(&other(), 60_000)).await;
        assert!(matches!(r, Err(StoreError::Forbidden(_))));
    }

    #[tokio::test]
    async fn a_viewer_cannot_take_a_lease_and_nothing_is_claimed() {
        let w = world(MemoryConfig::default()).await;
        let v = Holder {
            principal: "@v:x".into(),
            session: "sv".into(),
        };
        let r = acquire(&*w.viewer, &w.doc, &claim(&v, 60_000)).await;
        assert!(matches!(r, Err(StoreError::Forbidden(_))), "{r:?}");
    }

    #[tokio::test]
    async fn lease_lost_while_editing_still_saves_as_a_version_and_shows_the_conflict() {
        // Failure table row 2: continue locally; before saving, try to
        // reacquire; if another session holds the lease, save as a version
        // anyway (rule 6) and show the resulting conflict.
        let w = world(MemoryConfig::default()).await;
        let base = w.a.versions(&w.doc).await.unwrap()[0].id.clone();
        let mine = Claim::new("@a:x", "s1", "t1", &base, 60_000);
        assert!(acquire(&*w.a, &w.doc, &mine).await.unwrap().is_acquired());
        w.clock.advance(61_000); // the lease expires while A keeps editing offline
        assert!(acquire(&*w.b, &w.doc, &claim(&other(), 60_000))
            .await
            .unwrap()
            .is_acquired());

        let retry = acquire(&*w.a, &w.doc, &mine).await.unwrap();
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
