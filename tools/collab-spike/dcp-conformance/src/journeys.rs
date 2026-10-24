//! Spec section 12 tests 1 to 9 as journeys against [`DocumentStore`].
//!
//! Each journey is a `pub async fn` taking the [`StoreFactory`] under test,
//! building its own fresh fixture, and returning `Err(Failure)` when the
//! binding breaks the rule. Journeys assert exact outcomes (heads, bytes,
//! which session holds the lease, the state after applying updates), because
//! a journey that cannot fail proves nothing: `tests/mutation.rs` runs each
//! against a deliberately broken store.
//!
//! Tests 10 (a WOPI client round trip) and 11 (schemas) are not store
//! journeys: the first belongs to the WOPI crate, the second to
//! `dcp_core::schema`.

use std::fmt::Debug;
use std::sync::Arc;

use dcp_core::lease::{self, Acquire, Claim, Pause, Release, Settle, MATRIX_SETTLE_MS};
use dcp_core::updates::{self, Receiver, Skip, StreamWriter, Target};
use dcp_core::view::{read_view, Head, Reason, View};
use dcp_core::{
    graph, DocumentStore, Lease, LeaseState, Role, Stamped, StoreError, Update, Version,
};

use crate::compare::assert_same_view;
use crate::fixture::{new_file, Fixture, StoreFactory, Time, ALGORITHM, BASE_FILE, DOC_MODEL};
use crate::wrappers::{CorruptingStore, Corruption, Target as CorruptTarget};
use crate::{ensure, Failure};

type Handle = Arc<dyn DocumentStore>;

/// Lets the settle interval of lease rule 3 pass on the fixture's own clock:
/// real time for a real binding, the injected clock for an in-memory one.
struct TimePause(Arc<dyn Time>);

#[async_trait::async_trait]
impl Pause for TimePause {
    async fn pause(&self, ms: u64) {
        self.0.pass(ms).await;
    }
}

// ---------------------------------------------------------------- helpers

async fn view_when(
    fx: &Fixture,
    who: &Handle,
    what: &str,
    pred: impl Fn(&View) -> bool,
) -> Result<View, Failure> {
    fx.settle(what, || async {
        let v = read_view(&**who, &fx.doc).await?;
        Ok(pred(&v).then_some(v))
    })
    .await
}

async fn wait_for_head(fx: &Fixture, who: &Handle, id: &str) -> Result<View, Failure> {
    let what = format!("{} sees head {id}", who.principal());
    view_when(fx, who, &what, |v| v.head_ids() == [id]).await
}

async fn publish_child(
    fx: &Fixture,
    who: &Handle,
    parents: &[&str],
    bytes: &[u8],
) -> Result<Stamped<Version>, Failure> {
    Ok(who
        .publish_version(
            &fx.doc,
            parents.iter().map(|p| p.to_string()).collect(),
            new_file(bytes),
            None,
        )
        .await?)
}

async fn wait_lease(
    fx: &Fixture,
    who: &Handle,
    what: &str,
    pred: impl Fn(Option<&Stamped<Lease>>) -> bool,
) -> Result<Option<Stamped<Lease>>, Failure> {
    fx.settle(what, || async {
        let l = who.lease(&fx.doc).await?;
        Ok(pred(l.as_ref()).then_some(l))
    })
    .await
}

fn forbidden<T: Debug>(what: &str, r: Result<T, StoreError>) -> Result<(), Failure> {
    match r {
        Err(StoreError::Forbidden(_)) => Ok(()),
        other => Err(Failure::new(format!(
            "{what} must be refused with Forbidden by the store, got {other:?}"
        ))),
    }
}

fn update(
    algorithm: &str,
    doc_model: &str,
    base: &str,
    seq: u64,
    bytes: &[u8],
) -> Result<Update, Failure> {
    Ok(Update {
        dcp: 1,
        algorithm: algorithm.into(),
        doc_model: doc_model.into(),
        base_version: base.into(),
        seq,
        payload: updates::inline_payload(bytes).map_err(|e| Failure::new(e.to_string()))?,
    })
}

/// The toy algorithm: the state is the loaded file followed by every payload
/// applied, in order.
struct AppendDoc {
    state: Vec<u8>,
    supported: bool,
}

impl AppendDoc {
    fn from_file(bytes: &[u8]) -> Self {
        AppendDoc {
            state: bytes.to_vec(),
            supported: true,
        }
    }

    fn knows_nothing(bytes: &[u8]) -> Self {
        AppendDoc {
            state: bytes.to_vec(),
            supported: false,
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.state).into_owned()
    }
}

impl Target for AppendDoc {
    fn supports(&self, algorithm: &str, doc_model: &str) -> bool {
        self.supported && algorithm == ALGORITHM && doc_model == DOC_MODEL
    }
    fn dry_run(&self, _: &str, _: &str, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn apply(&mut self, _: &str, _: &str, payload: &[u8]) {
        self.state.extend_from_slice(payload);
    }
}

fn plus(base: &[u8], more: &str) -> String {
    format!("{}{more}", String::from_utf8_lossy(base))
}

// ------------------------------------------------------------ journey 1

/// Test 1: two instances converge on the same head.
pub async fn convergence_on_one_head(f: &dyn StoreFactory) -> Result<(), Failure> {
    let fx = f.fresh().await?;
    let first = fx.first_version.clone();
    for (name, who) in [
        ("owner", &fx.owner),
        ("editor", &fx.editor),
        ("viewer", &fx.viewer),
    ] {
        let v = wait_for_head(&fx, who, &first).await?;
        ensure!(
            v.heads[0].bytes == BASE_FILE,
            "{name} reads different bytes for the first version"
        );
    }

    let v2 = publish_child(&fx, &fx.owner, &[&first], b"edit by the owner").await?;
    for who in [&fx.editor, &fx.viewer] {
        let v = wait_for_head(&fx, who, &v2.id).await?;
        ensure!(
            v.heads[0].bytes == b"edit by the owner",
            "{} reads the wrong bytes of v2",
            who.principal()
        );
    }

    let v3 = publish_child(&fx, &fx.editor, &[&v2.id], b"edit by the editor").await?;
    for who in [&fx.owner, &fx.viewer] {
        wait_for_head(&fx, who, &v3.id).await?;
    }

    assert_same_view(&*fx.owner, &*fx.editor, &fx.doc).await?;
    assert_same_view(&*fx.owner, &*fx.viewer, &fx.doc).await?;
    let v = read_view(&*fx.owner, &fx.doc).await?;
    ensure!(
        v.heads.len() == 1,
        "expected one head, got {:?}",
        v.head_ids()
    );
    ensure!(
        v.rejected.is_empty(),
        "versions were rejected: {:?}",
        v.rejected
    );
    ensure!(
        v.heads[0].bytes == b"edit by the editor",
        "the converged head has the wrong content"
    );
    if fx.caps.history {
        let all = fx.owner.versions(&fx.doc).await?;
        ensure!(
            all.len() == 3,
            "version rule 4: a save adds a child, nothing is overwritten; found {} versions",
            all.len()
        );
    }
    Ok(())
}

// ------------------------------------------------------------ journey 2

/// Test 2: a second head is shown, not hidden.
pub async fn second_head_shown_not_hidden(f: &dyn StoreFactory) -> Result<(), Failure> {
    let fx = f.fresh().await?;
    let first = fx.first_version.clone();

    // Two offline edits from the same version.
    let a = publish_child(&fx, &fx.owner, &[&first], b"offline edit A").await?;
    let b = publish_child(&fx, &fx.editor, &[&first], b"offline edit B").await?;

    for (name, who) in [
        ("owner", &fx.owner),
        ("editor", &fx.editor),
        ("viewer", &fx.viewer),
    ] {
        let v = view_when(&fx, who, &format!("{name} sees both heads"), |v| {
            v.heads.len() == 2
        })
        .await?;
        ensure!(
            v.is_conflict(),
            "{name}: two heads must be reported as a conflict"
        );
        let mut ids = v.head_ids();
        ids.sort();
        let mut want = vec![a.id.as_str(), b.id.as_str()];
        want.sort();
        ensure!(
            ids == want,
            "{name} lists heads {ids:?}, expected both {want:?}: one was hidden"
        );
        let mut contents: Vec<Vec<u8>> = v.heads.iter().map(|h| h.bytes.clone()).collect();
        contents.sort();
        ensure!(
            contents == [b"offline edit A".to_vec(), b"offline edit B".to_vec()],
            "{name} shows the wrong content for the heads"
        );
        ensure!(
            v.heads
                .windows(2)
                .all(|w| w[0].version.ts_ms >= w[1].version.ts_ms),
            "{name}: heads are not newest first by binding timestamp"
        );
    }
    assert_same_view(&*fx.owner, &*fx.editor, &fx.doc).await?;

    // Nothing merged them behind the user's back.
    let after = read_view(&*fx.viewer, &fx.doc).await?;
    ensure!(
        after.heads.len() == 2,
        "the conflict went away without a merge"
    );

    // The user merges: a version listing every head as a parent.
    let heads: Vec<&Stamped<Version>> = after.heads.iter().map(|h| &h.version).collect();
    let parents = graph::merge_parents(&heads);
    let parents_ref: Vec<&str> = parents.iter().map(String::as_str).collect();
    let merged = publish_child(&fx, &fx.owner, &parents_ref, b"merged by the user").await?;
    for who in [&fx.owner, &fx.editor, &fx.viewer] {
        let v = wait_for_head(&fx, who, &merged.id).await?;
        ensure!(
            v.heads[0].bytes == b"merged by the user",
            "wrong merged content"
        );
    }
    if fx.caps.history {
        let all = fx.owner.versions(&fx.doc).await?;
        ensure!(
            all.len() == 4,
            "merging must keep every earlier version; found {}",
            all.len()
        );
    }
    Ok(())
}

// ------------------------------------------------------------ journey 3

/// Test 3 (reader half): a file whose bytes differ from the version's `size`
/// and `sha256` is discarded and reported by version, whichever way it was
/// damaged.
pub async fn corrupt_file_rejected(f: &dyn StoreFactory) -> Result<(), Failure> {
    use dcp_core::integrity::IntegrityError;

    let fx = f.fresh().await?;
    let first = fx.first_version.clone();
    let v2 = publish_child(&fx, &fx.owner, &[&first], b"the second version's content").await?;
    let honest = wait_for_head(&fx, &fx.editor, &v2.id).await?;
    ensure!(
        honest.rejected.is_empty(),
        "an undamaged file was rejected: {:?}",
        honest.rejected
    );

    for corruption in Corruption::ALL {
        let damaged = CorruptingStore::new(
            fx.editor.clone(),
            corruption,
            CorruptTarget::Version(v2.id.clone()),
        );
        let v = read_view(&damaged, &fx.doc).await?;
        let rejection = v.rejected.iter().find(|r| r.id == v2.id);
        let Some(rejection) = rejection else {
            return Err(Failure::new(format!(
                "{corruption:?}: the damaged version {} was accepted",
                v2.id
            )));
        };
        let Reason::Integrity(cause) = &rejection.reason else {
            return Err(Failure::new(format!(
                "{corruption:?}: rejected for the wrong reason: {:?}",
                rejection.reason
            )));
        };
        let size_changed = !matches!(corruption, Corruption::FlipBit);
        ensure!(
            matches!(cause, IntegrityError::SizeMismatch { .. }) == size_changed,
            "{corruption:?}: expected {} but got {cause:?}",
            if size_changed {
                "a size mismatch"
            } else {
                "a hash mismatch"
            }
        );
        ensure!(
            v.head_ids() == [first.as_str()],
            "{corruption:?}: with v2 discarded the parent is the head, got {:?}",
            v.head_ids()
        );
    }

    // When the only version is damaged there is no content, and it is reported.
    let everything = CorruptingStore::new(
        fx.editor.clone(),
        Corruption::Truncate,
        CorruptTarget::Every,
    );
    let v = read_view(&everything, &fx.doc).await?;
    ensure!(
        v.heads.is_empty(),
        "a damaged file was shown as a head: {:?}",
        v.head_ids()
    );
    ensure!(
        v.rejected.len() >= 2,
        "every damaged version must be reported, got {:?}",
        v.rejected
    );
    Ok(())
}

/// Test 3 (writer half): a truncated upload is never published. Needs the
/// fixture's [`crate::Faults`].
pub async fn truncated_upload_never_published(f: &dyn StoreFactory) -> Result<(), Failure> {
    let fx = f.fresh().await?;
    let first = fx.first_version.clone();
    let Some(faults) = fx.faults.clone() else {
        return Err(Failure::new(
            "capabilities.upload_faults is set but the fixture has no Faults",
        ));
    };
    faults.truncate_next_upload();
    let res = fx
        .editor
        .publish_version(
            &fx.doc,
            vec![first.clone()],
            new_file(b"a file that is cut in half on upload"),
            None,
        )
        .await;
    ensure!(
        res.is_err(),
        "a version whose upload was truncated was published: {res:?}"
    );

    // The next, healthy save works, and it is the only thing that appeared.
    let good = publish_child(&fx, &fx.editor, &[&first], b"a healthy save").await?;
    let v = wait_for_head(&fx, &fx.owner, &good.id).await?;
    ensure!(
        v.rejected.is_empty(),
        "something unreadable was published: {:?}",
        v.rejected
    );
    if fx.caps.history {
        let mut ids: Vec<String> = fx
            .owner
            .versions(&fx.doc)
            .await?
            .into_iter()
            .map(|v| v.id)
            .collect();
        ids.sort();
        let mut want = vec![first, good.id];
        want.sort();
        ensure!(
            ids == want,
            "after the failed upload the document has versions {ids:?}, expected {want:?}"
        );
    }
    Ok(())
}

// ------------------------------------------------------------ journey 4

/// Test 4: a second session cannot take an active lease, and the read-back
/// resolves a simultaneous write.
pub async fn lease_exclusion_and_readback(f: &dyn StoreFactory) -> Result<(), Failure> {
    let fx = f.fresh().await?;
    let first = fx.first_version.clone();
    let (ed, ow) = (&fx.editor, &fx.owner);
    let ce = Claim::new(
        ed.principal(),
        "session-editor",
        "token-editor",
        &first,
        60_000,
    );
    let co = Claim::new(
        ow.principal(),
        "session-owner",
        "token-owner",
        &first,
        60_000,
    );

    let pause = TimePause(fx.time.clone());
    let st = Settle::new(&pause, MATRIX_SETTLE_MS);

    // Both sessions look, both see a free lease, both write, then both read back.
    ensure!(
        lease::available(&**ed, &fx.doc, &ce.holder).await?.is_ok(),
        "no lease was ever written, yet it is not available"
    );
    ensure!(
        lease::available(&**ow, &fx.doc, &co.holder).await?.is_ok(),
        "no lease was ever written, yet it is not available"
    );
    // Both looked before either wrote: both build their record over "no lease".
    let seen_by_ed = ed.lease(&fx.doc).await?;
    let seen_by_ow = ow.lease(&fx.doc).await?;
    let we = lease::write_over(&**ed, &fx.doc, &ce, seen_by_ed.as_ref()).await?;
    let wo = lease::write_over(&**ow, &fx.doc, &co, seen_by_ow.as_ref()).await?;
    // Rule 8: written over nothing, so renewal 0, for both.
    ensure!(
        we.lease.renewal == Some(0) && wo.lease.renewal == Some(0),
        "a lease written over no record must carry renewal 0: editor {:?}, owner {:?}",
        we.lease.renewal,
        wo.lease.renewal
    );
    if fx.caps.racy_lease {
        ensure!(
            we.result.is_ok() && wo.result.is_ok(),
            "the binding says it has no compare-and-set but refused a write: editor {:?}, owner {:?}",
            we.result,
            wo.result
        );
    }
    // Give both handles time to see the same latest record. If they never do,
    // the read-backs below decide: that is the rule being tested.
    fx.settle_soft(|| async {
        let a = ed.lease(&fx.doc).await?.map(|l| l.id);
        let b = ow.lease(&fx.doc).await?.map(|l| l.id);
        Ok((a.is_some() && a == b).then_some(()))
    })
    .await?;
    let re = lease::confirm(&**ed, &fx.doc, &ce, &we, &st).await?;
    let ro = lease::confirm(&**ow, &fx.doc, &co, &wo, &st).await?;
    ensure!(
        re.is_acquired() != ro.is_acquired(),
        "exactly one session may hold the lease after a simultaneous write; editor: {re:?}, owner: {ro:?}"
    );
    let (winner, wc, loser, lc) = if re.is_acquired() {
        (ed, &ce, ow, &co)
    } else {
        (ow, &co, ed, &ce)
    };

    // The loser cannot take an active lease.
    let again = lease::acquire(&**loser, &fx.doc, lc, &st).await?;
    ensure!(
        matches!(again, Acquire::Held(_)),
        "a session took (or fought for) an active lease: {again:?}"
    );
    ensure!(
        lease::holds(&**winner, &fx.doc, &wc.holder).await?,
        "the winner lost the lease to a refused attempt"
    );

    // Rule 6: a save without the lease is still a version.
    let saved = publish_child(&fx, loser, &[&first], b"saved without holding the lease").await?;
    wait_for_head(&fx, winner, &saved.id).await?;

    // The winner lets go; only then may the other take it.
    ensure!(
        lease::release(&**winner, &fx.doc, &wc.holder).await? == Release::Released,
        "the holder could not release"
    );
    ensure!(
        lease::release(&**loser, &fx.doc, &lc.holder).await? == Release::NotHolder,
        "a session that does not hold the lease released it"
    );

    // Rule 5: an owner may break a lease; an editor may not; the former holder sees it.
    let editor_again = Claim::new(
        ed.principal(),
        "session-editor-2",
        "token-editor-2",
        &saved.id,
        60_000,
    );
    wait_lease(&fx, ed, "the release is visible", |l| {
        l.is_some_and(|l| l.body.state == LeaseState::Released)
    })
    .await?;
    let got = lease::acquire(&**ed, &fx.doc, &editor_again, &st).await?;
    let Acquire::Acquired(got) = got else {
        return Err(Failure::new(format!(
            "the lease was released but could not be acquired: {got:?}"
        )));
    };
    ensure!(
        got.body.renewal == Some(0),
        "a lease written over a released record must carry renewal 0 (rule 8), got {:?}",
        got.body.renewal
    );
    let breaker = Claim::new(
        ow.principal(),
        "session-owner-break",
        "token-break",
        &saved.id,
        60_000,
    );
    wait_lease(&fx, ow, "the owner sees the editor's lease", |l| {
        l.is_some_and(|l| lease::names(&l.body, &editor_again.holder))
    })
    .await?;
    let by_editor = lease::break_lease(
        &**ed,
        &fx.doc,
        &Claim::new(ed.principal(), "session-editor-3", "t", &saved.id, 60_000),
        &st,
    )
    .await;
    forbidden("an editor breaking a lease", by_editor)?;
    let broke = lease::break_lease(&**ow, &fx.doc, &breaker, &st).await?;
    let Acquire::Acquired(rec) = broke else {
        return Err(Failure::new(format!(
            "an owner could not break a lease: {broke:?}"
        )));
    };
    ensure!(
        rec.body.holder.as_ref() == Some(&breaker.holder),
        "a broken lease must be recorded as held by the breaker, got {:?}",
        rec.body.holder
    );
    ensure!(
        rec.body.renewal == Some(1),
        "a break is a write over a held record: renewal must be the replaced 0 plus 1 (rule 8), got {:?}",
        rec.body.renewal
    );
    fx.settle("the former holder sees its lease was broken", || async {
        Ok((!lease::holds(&**ed, &fx.doc, &editor_again.holder).await?).then_some(()))
    })
    .await?;
    let blocked = lease::acquire(&**ed, &fx.doc, &editor_again, &st).await?;
    ensure!(
        matches!(blocked, Acquire::Held(_)),
        "the former holder retook a lease the owner broke: {blocked:?}"
    );
    Ok(())
}

// ------------------------------------------------------------ journey 5

/// Test 5: lease expiry releases the document, and renewal at half the
/// duration keeps it.
pub async fn lease_expiry_releases(f: &dyn StoreFactory) -> Result<(), Failure> {
    // Long enough that the settle interval inside every acquire (rule 3)
    // does not eat the lease it just took.
    const D: u64 = 4_000;
    let fx = f.fresh().await?;
    let first = fx.first_version.clone();
    let (ed, ow) = (&fx.editor, &fx.owner);
    let ce = Claim::new(ed.principal(), "session-editor", "token-editor", &first, D);
    let co = Claim::new(ow.principal(), "session-owner", "token-owner", &first, D);
    let pause = TimePause(fx.time.clone());
    let st = Settle::new(&pause, MATRIX_SETTLE_MS);

    let Acquire::Acquired(held) = lease::acquire(&**ed, &fx.doc, &ce, &st).await? else {
        return Err(Failure::new(
            "a fresh document's lease could not be acquired",
        ));
    };
    ensure!(
        held.body.renewal == Some(0),
        "a fresh lease must carry renewal 0 (rule 8), got {:?}",
        held.body.renewal
    );
    wait_lease(&fx, ow, "the owner sees the editor's lease", |l| {
        l.is_some_and(|l| l.id == held.id)
    })
    .await?;
    let blocked = lease::acquire(&**ow, &fx.doc, &co, &st).await?;
    ensure!(
        matches!(blocked, Acquire::Held(_)),
        "an active lease was taken: {blocked:?}"
    );

    // Kill the holder: it stops renewing. After the duration anyone may acquire.
    fx.time.pass(D + 500).await;
    let Acquire::Acquired(taken) = lease::acquire(&**ow, &fx.doc, &co, &st).await? else {
        return Err(Failure::new("the lease did not expire after its duration"));
    };
    ensure!(
        taken.body.renewal == Some(1),
        "a takeover over an expired held record continues the count (rule 8): expected 1, got {:?}",
        taken.body.renewal
    );
    wait_lease(&fx, ed, "the editor sees the new lease", |l| {
        l.is_some_and(|l| l.id == taken.id)
    })
    .await?;
    ensure!(
        !lease::holds(&**ed, &fx.doc, &ce.holder).await?,
        "the dead holder still thinks it holds the lease"
    );
    let retry = lease::acquire(&**ed, &fx.doc, &ce, &st).await?;
    ensure!(
        matches!(retry, Acquire::Held(_)),
        "the expired holder retook the lease the new holder has: {retry:?}"
    );

    // Renewal: due at half the duration, and it really extends the lease.
    fx.time.pass(D / 2).await;
    let renewed = lease::renew_if_due(&**ow, &fx.doc, &co, &taken, &st).await?;
    let Some(Acquire::Acquired(renewed)) = renewed else {
        return Err(Failure::new(format!(
            "no renewal at half the duration: {renewed:?}"
        )));
    };
    ensure!(
        renewed.ts_ms > taken.ts_ms,
        "renewal did not produce a newer lease record"
    );
    ensure!(
        renewed.body.renewal == Some(2),
        "a renewal carries the previous renewal plus 1 (rule 8): expected 2, got {:?}",
        renewed.body.renewal
    );
    fx.time.pass(D * 3 / 4).await; // past the ORIGINAL expiry, inside the renewed one
    wait_lease(&fx, ed, "the editor sees the renewal", |l| {
        l.is_some_and(|l| l.id == renewed.id)
    })
    .await?;
    let still = lease::acquire(&**ed, &fx.doc, &ce, &st).await?;
    ensure!(
        matches!(still, Acquire::Held(_)),
        "renewal did not keep the lease alive: {still:?}"
    );

    // Normal close releases at once.
    ensure!(
        lease::release(&**ow, &fx.doc, &co.holder).await? == Release::Released,
        "release failed"
    );
    wait_lease(&fx, ed, "the editor sees the release", |l| {
        l.is_some_and(|l| l.body.state == LeaseState::Released)
    })
    .await?;
    let free = lease::acquire(&**ed, &fx.doc, &ce, &st).await?;
    ensure!(
        free.is_acquired(),
        "a released lease could not be acquired: {free:?}"
    );
    Ok(())
}

// ------------------------------------------------------------ journey 6

/// Test 6: roles are enforced at the store, not only in the caller.
pub async fn roles_enforced_at_store(f: &dyn StoreFactory) -> Result<(), Failure> {
    let fx = f.fresh().await?;
    let first = fx.first_version.clone();
    let caps = fx.caps;

    ensure!(
        fx.owner.role(&fx.doc).await? == Role::Owner,
        "the owner's role is not owner"
    );
    ensure!(
        fx.editor.role(&fx.doc).await? == Role::Editor,
        "the editor's role is not editor"
    );
    ensure!(
        fx.viewer.role(&fx.doc).await? == Role::Viewer,
        "the viewer's role is not viewer"
    );

    // A viewer reads.
    fx.viewer.meta(&fx.doc).await?;
    wait_for_head(&fx, &fx.viewer, &first).await?;

    // A viewer cannot write anything.
    forbidden(
        "a viewer publishing a version",
        fx.viewer
            .publish_version(
                &fx.doc,
                vec![first.clone()],
                new_file(b"viewer was here"),
                None,
            )
            .await,
    )?;
    if caps.updates {
        forbidden(
            "a viewer publishing an update",
            fx.viewer
                .publish_update(&fx.doc, update(ALGORITHM, DOC_MODEL, &first, 0, b"v")?)
                .await,
        )?;
    }
    if caps.arbitrary_lease_writes {
        let claim = Claim::new(
            fx.viewer.principal(),
            "session-viewer",
            "token-viewer",
            &first,
            60_000,
        );
        forbidden(
            "a viewer writing a lease",
            fx.viewer.write_lease(&fx.doc, claim.lease_over(None)).await,
        )?;
    }
    if caps.grant_revoke {
        forbidden(
            "a viewer granting itself a role",
            fx.viewer
                .grant(&fx.doc, &fx.viewer.principal(), Role::Owner)
                .await,
        )?;
        forbidden(
            "a viewer removing the editor",
            fx.viewer.revoke(&fx.doc, &fx.editor.principal()).await,
        )?;
    }

    // An editor writes, but does not administer.
    let by_editor = publish_child(&fx, &fx.editor, &[&first], b"editor was here").await?;
    if caps.grant_revoke {
        forbidden(
            "an editor granting a role",
            fx.editor
                .grant(&fx.doc, &fx.viewer.principal(), Role::Editor)
                .await,
        )?;
        forbidden(
            "an editor removing the viewer",
            fx.editor.revoke(&fx.doc, &fx.viewer.principal()).await,
        )?;
    }

    // Nothing the viewer tried left a trace.
    wait_for_head(&fx, &fx.owner, &by_editor.id).await?;
    if caps.history {
        let all = fx.owner.versions(&fx.doc).await?;
        ensure!(
            all.len() == 2,
            "only the editor's version should exist besides the first, found {}",
            all.len()
        );
    }
    if caps.arbitrary_lease_writes {
        let l = fx.owner.lease(&fx.doc).await?;
        ensure!(
            l.is_none(),
            "the viewer's refused lease write left a record: {l:?}"
        );
    }
    if caps.updates {
        let u = fx.owner.updates(&fx.doc).await?;
        ensure!(
            u.is_empty(),
            "the viewer's refused update left a record: {u:?}"
        );
    }

    // The owner changes a role, and the store follows it in both directions.
    if caps.grant_revoke {
        fx.owner
            .grant(&fx.doc, &fx.viewer.principal(), Role::Editor)
            .await?;
        let promoted = fx
            .settle("the promoted viewer can publish", || async {
                match fx
                    .viewer
                    .publish_version(
                        &fx.doc,
                        vec![by_editor.id.clone()],
                        new_file(b"now an editor"),
                        None,
                    )
                    .await
                {
                    Ok(v) => Ok(Some(v)),
                    Err(StoreError::Forbidden(_)) => Ok(None),
                    Err(e) => Err(e.into()),
                }
            })
            .await?;
        wait_for_head(&fx, &fx.owner, &promoted.id).await?;
        fx.owner
            .grant(&fx.doc, &fx.viewer.principal(), Role::Viewer)
            .await?;
        fx.settle("the demotion reaches the viewer", || async {
            Ok((fx.viewer.role(&fx.doc).await? == Role::Viewer).then_some(()))
        })
        .await?;
        forbidden(
            "a demoted viewer publishing",
            fx.viewer
                .publish_version(
                    &fx.doc,
                    vec![promoted.id.clone()],
                    new_file(b"still an editor?"),
                    None,
                )
                .await,
        )?;
    }
    Ok(())
}

// ------------------------------------------------------------ journey 6b

/// Test 6, second half: a viewer cannot publish a version that a reader
/// accepts, even by going around the store. The viewer's client sends the
/// event straight to the server ([`crate::Forger`]), once under its own name
/// and once with `created_by` naming the owner. Either the server refuses
/// (the gate: on Matrix `events["m.room.encrypted"]` is 50) or, when it does
/// not gate, every reader drops it (spec 8.1: the sender is below the editor
/// level, or `created_by.principal` is not the sender). A legitimate save sent
/// afterwards is the only head everywhere: the forged events, sent before it,
/// would have been seen by then.
pub async fn forged_version_from_a_viewer_is_not_accepted(
    f: &dyn StoreFactory,
) -> Result<(), Failure> {
    let fx = f.fresh().await?;
    let Some(forger) = fx.forger.clone() else {
        return Err(Failure::new(
            "capabilities.forgery is set but the fixture has no Forger",
        ));
    };
    let first = fx.first_version.clone();
    let base = fx
        .owner
        .versions(&fx.doc)
        .await?
        .into_iter()
        .find(|v| v.id == first)
        .ok_or_else(|| Failure::new("the first version is not listed"))?;

    let mut accepted_by_server = Vec::new();
    for (label, claimed) in [
        ("under its own name", fx.viewer.principal()),
        ("claiming to be the owner", fx.owner.principal()),
    ] {
        // Otherwise valid: the file is the first version's, so only the
        // authorisation rules can reject it.
        let forged = Version {
            dcp: 1,
            parents: vec![first.clone()],
            file: base.body.file.clone(),
            created_by: dcp_core::Author {
                principal: claimed,
                device: fx.viewer.device(),
            },
            message: Some(format!("forged by a viewer, {label}")),
            covers: None,
        };
        if let Some(id) = forger.forge_version(&fx.doc, forged).await? {
            accepted_by_server.push(id);
        }
    }

    let marker = publish_child(
        &fx,
        &fx.owner,
        &[&first],
        b"a legitimate save after the forgeries",
    )
    .await?;
    for (name, who) in [
        ("owner", &fx.owner),
        ("editor", &fx.editor),
        ("viewer", &fx.viewer),
    ] {
        let v = view_when(&fx, who, &format!("{name} sees the legitimate save"), |v| {
            v.heads.iter().any(|h| h.version.id == marker.id)
        })
        .await?;
        ensure!(
            v.head_ids() == [marker.id.as_str()],
            "{name}: a viewer's forged version was accepted by a reader (server accepted {accepted_by_server:?}); heads are {:?}",
            v.head_ids()
        );
        ensure!(
            v.rejected.is_empty(),
            "{name}: the forgery was not dropped by the reader's rules but rejected later: {:?}",
            v.rejected
        );
        let listed = who.versions(&fx.doc).await?;
        let leaked: Vec<&str> = listed
            .iter()
            .filter(|l| accepted_by_server.contains(&l.id))
            .map(|l| l.id.as_str())
            .collect();
        ensure!(
            leaked.is_empty(),
            "{name}'s store still lists the viewer's forged versions {leaked:?}"
        );
    }
    Ok(())
}

// ------------------------------------------------------------ journey 7

/// Test 7: a removed member does not read content published afterwards.
/// (What they already received stays readable, spec 11; not asserted.)
pub async fn revocation_hides_new_content(f: &dyn StoreFactory) -> Result<(), Failure> {
    let fx = f.fresh().await?;
    let first = fx.first_version.clone();
    wait_for_head(&fx, &fx.viewer, &first).await?;

    fx.owner.revoke(&fx.doc, &fx.viewer.principal()).await?;
    let secret: &[u8] = b"published after the viewer was removed";
    let hidden = publish_child(&fx, &fx.editor, &[&first], secret).await?;
    wait_for_head(&fx, &fx.owner, &hidden.id).await?;

    match fx.viewer.versions(&fx.doc).await {
        Ok(listed) => ensure!(
            listed.iter().all(|v| v.id != hidden.id),
            "the removed member still lists the version published after removal"
        ),
        Err(_) => {}
    }
    match fx.viewer.fetch_file(&fx.doc, &hidden).await {
        Ok(bytes) => ensure!(
            bytes != secret && dcp_core::integrity::verify(&hidden.body.file, &bytes).is_err(),
            "the removed member read the content published after removal"
        ),
        Err(_) => {}
    }
    forbidden(
        "a removed member publishing",
        fx.viewer
            .publish_version(&fx.doc, vec![hidden.id.clone()], new_file(b"x"), None)
            .await,
    )?;
    // Everyone who stayed still sees it.
    wait_for_head(&fx, &fx.editor, &hidden.id).await?;
    Ok(())
}

// ------------------------------------------------------------ journey 8

/// Test 8: an update of an unknown algorithm (or document model) is ignored,
/// shows nothing partial, and the document still opens from versions.
pub async fn unknown_algorithm_falls_back_to_versions(f: &dyn StoreFactory) -> Result<(), Failure> {
    let fx = f.fresh().await?;
    let first = fx.first_version.clone();

    // One stream: unknown algorithm, supported, unknown document model, supported.
    let sent = [
        update("unknown/9", DOC_MODEL, &first, 0, b"ZZZ")?,
        update(ALGORITHM, DOC_MODEL, &first, 1, b"ok")?,
        update(ALGORITHM, "tables/1", &first, 2, b"XX")?,
        update(ALGORITHM, DOC_MODEL, &first, 3, b"!")?,
    ];
    for u in sent.iter().cloned() {
        fx.editor.publish_update(&fx.doc, u).await?;
    }
    let log = fx
        .settle("the viewer receives all four updates", || async {
            let l = fx.viewer.updates(&fx.doc).await?;
            Ok((l.len() == 4).then_some(l))
        })
        .await?;

    let mut rx = Receiver::new(Some(first.clone()));
    let mut doc = AppendDoc::from_file(BASE_FILE);
    let report = rx.ingest(&log, &mut doc);
    ensure!(
        doc.text() == plus(BASE_FILE, "ok!"),
        "state after applying is {:?}: the unsupported updates must be ignored and the others applied",
        doc.text()
    );
    let unsupported = report
        .skipped
        .iter()
        .filter(|(_, s)| *s == Skip::Unsupported)
        .count();
    ensure!(
        unsupported == 2,
        "expected the 2 unsupported updates to be reported as skipped, got {report:?}"
    );
    ensure!(
        report.gaps.is_empty(),
        "an ignored update was mistaken for a lost one: {report:?}"
    );
    ensure!(
        report.dropped.is_empty(),
        "nothing should have been dropped: {report:?}"
    );

    // A program that supports no algorithm shows no partial result at all.
    let mut none = AppendDoc::knows_nothing(BASE_FILE);
    Receiver::new(Some(first.clone())).ingest(&log, &mut none);
    ensure!(
        none.state == BASE_FILE,
        "a program that supports nothing changed its state: {:?}",
        none.text()
    );

    // And the document opens from versions.
    let v = read_view(&*fx.viewer, &fx.doc).await?;
    ensure!(
        v.head_ids() == [first.as_str()] && v.heads[0].bytes == BASE_FILE,
        "the document does not open from its versions"
    );
    Ok(())
}

// ------------------------------------------------------------ journey 9

/// Test 9: a gap in an update stream stops that stream, and the receiver
/// recovers by reloading from the latest version.
pub async fn update_stream_gap_recovery(f: &dyn StoreFactory) -> Result<(), Failure> {
    let fx = f.fresh().await?;
    let first = fx.first_version.clone();

    // The editor makes four updates but one (seq 2) is lost on the way.
    let mut writer = StreamWriter::new(ALGORITHM, DOC_MODEL);
    let mk = |w: &mut StreamWriter, base: &str, p: &str| {
        w.next(base, p.as_bytes())
            .map_err(|e| Failure::new(e.to_string()))
    };
    let (u0, u1, _lost, u3) = (
        mk(&mut writer, &first, "a")?,
        mk(&mut writer, &first, "b")?,
        mk(&mut writer, &first, "c")?,
        mk(&mut writer, &first, "d")?,
    );
    for u in [u0, u1, u3] {
        fx.editor.publish_update(&fx.doc, u).await?;
    }
    let log = fx
        .settle(
            "the viewer receives the three updates that were sent",
            || async {
                let l = fx.viewer.updates(&fx.doc).await?;
                Ok((l.len() == 3).then_some(l))
            },
        )
        .await?;

    let mut rx = Receiver::new(Some(first.clone()));
    let mut doc = AppendDoc::from_file(BASE_FILE);
    let report = rx.ingest(&log, &mut doc);
    ensure!(
        doc.text() == plus(BASE_FILE, "ab"),
        "updates must apply in seq order and stop at the gap: state is {:?}, expected {:?}",
        doc.text(),
        plus(BASE_FILE, "ab")
    );
    ensure!(
        report.gaps.len() == 1 && report.gaps[0].expected == 2 && report.gaps[0].found == 3,
        "the gap (expected seq 2, found 3) was not reported: {report:?}"
    );
    ensure!(rx.is_stopped(), "the stream with the gap must stop");

    // Recovery: the editor saves its true state; the viewer reloads from the latest version.
    let truth = plus(BASE_FILE, "abcd");
    let v2 = publish_child(&fx, &fx.editor, &[&first], truth.as_bytes()).await?;
    let view = wait_for_head(&fx, &fx.viewer, &v2.id).await?;
    ensure!(
        doc.text() != truth,
        "the gap did not matter: the lossy state already equals the truth"
    );
    let head: &Head = &view.heads[0];
    rx.reload(&head.version);
    doc = AppendDoc::from_file(&head.bytes);
    ensure!(
        doc.text() == truth,
        "reloading from the latest version did not recover the state: {:?}",
        doc.text()
    );
    let report = rx.ingest(&log, &mut doc);
    ensure!(
        report.applied.is_empty() && report.gaps.is_empty() && !rx.is_stopped(),
        "after the reload the old updates (based on the old version) must not apply or stop anything: {report:?}"
    );

    // The stream carries on after recovery.
    let u4 = mk(&mut writer, &v2.id, "e")?;
    let sent = fx.editor.publish_update(&fx.doc, u4).await?;
    let log = fx
        .settle("the viewer receives the new update", || async {
            let l = fx.viewer.updates(&fx.doc).await?;
            Ok((l.len() == 4).then_some(l))
        })
        .await?;
    let report = rx.ingest(&log, &mut doc);
    ensure!(
        report.applied == [sent.id.clone()],
        "the update after recovery was not applied: {report:?}"
    );
    ensure!(
        doc.text() == plus(truth.as_bytes(), "e"),
        "state after recovery plus one update is {:?}",
        doc.text()
    );
    Ok(())
}
