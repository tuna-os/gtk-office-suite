//! The cross-binding checks. Each takes a fresh [`Rig`] (one Synapse room, a
//! `MatrixStore` per principal, a `WopiHost` over those same stores) and
//! returns `Err(Failure)` when the two bindings disagree or break a rule.
//!
//! They return instead of panicking so `tests/negative_control.rs` can wrap a
//! store in one of `dcp-conformance`'s `Defective` stores and assert that the
//! check *fails*: a check that cannot fail proves nothing.
//!
//! Spec references: lease rules are section 4.3, version rules 4.2, the WOPI
//! mapping 8.2. `docs/rfc/0004-spike-d-results.md` has the wire facts used
//! here (the 1010 conflict body, the lock-mismatch shape, `X-WOPI-ItemVersion`).

use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use dcp_conformance::{
    assert_same_content, assert_same_view, ensure, new_file, Failure, BASE_FILE,
};
use dcp_core::lease::{self, Acquire, Claim, Pause, Release, Settle, MATRIX_SETTLE_MS};
use dcp_core::schema::{self, Kind};
use dcp_core::{graph, Author, DocumentStore, LeaseState, StoreError, Version};
use dcp_wopi::timefmt::iso8601_ms;
use serde_json::Value;

use crate::raw::Resp;
use crate::rig::{Rig, Who};

type R = Result<(), Failure>;

pub type CheckFuture<'a> = Pin<Box<dyn Future<Output = R> + Send + 'a>>;

pub struct Check {
    pub name: &'static str,
    pub run: for<'a> fn(&'a Rig) -> CheckFuture<'a>,
}

macro_rules! checks {
    ($($name:ident),+ $(,)?) => {
        /// Every cross-binding check, in this file's order.
        pub const ALL: &[Check] = &[
            $(Check { name: stringify!($name), run: |r| Box::pin($name(r)) }),+
        ];
    };
}

checks!(
    wopi_publish_is_read_by_a_matrix_user,
    native_publish_opens_through_wopi_getfile,
    native_publish_between_getfile_and_putfile_is_a_409,
    forced_wopi_save_forks_and_matrix_shows_the_conflict,
    native_offline_publish_shows_as_a_conflict_to_the_matrix_reader,
    viewer_token_cannot_write,
    wopi_lock_blocks_native_acquire_and_the_reverse,
    native_lease_blocks_wopi_save,
    owner_break_invalidates_the_wopi_lock,
    native_lease_expiry_frees_the_wopi_lock,
    wopi_lock_refresh_is_a_new_record_on_matrix,
    wopi_lock_on_an_empty_document_omits_base_version,
    removed_members_wopi_token_gets_no_new_content_and_no_write,
    forged_version_is_dropped_by_matrix_readers,
    forged_version_is_dropped_by_wopi_readers,
);

pub fn by_name(name: &str) -> &'static Check {
    ALL.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no check named {name}"))
}

// ---------------------------------------------------------------- helpers

fn status(what: &str, r: &Resp, want: u16) -> R {
    ensure!(r.status == want, "{what}: expected HTTP {want}, got {} ({:?})", r.status, r.text());
    Ok(())
}

fn field<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Wait until the WOPI host, through its own backing store, reports `id` as
/// the head. What the host sees is what a `PutFile` will be judged against.
async fn wait_host_head(rig: &Rig, who: Who, id: &str) -> Result<Value, Failure> {
    let raw = rig.raw(who);
    rig.until(&format!("the WOPI host's CheckFileInfo for {who:?} to show Version {id}"), || async {
        let r = raw.check_file_info().await?;
        status("CheckFileInfo", &r, 200)?;
        let j = r.json()?;
        if field(&j, "Version") == id {
            return Ok(Ok(j));
        }
        Ok(Err(format!("Version {:?}", field(&j, "Version"))))
    })
    .await
}

/// Lease rule 3's settle interval, in real time (the binding is a real Synapse).
struct RealPause;

#[async_trait::async_trait]
impl Pause for RealPause {
    async fn pause(&self, ms: u64) {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
    }
}

static REAL_PAUSE: RealPause = RealPause;

fn settle() -> Settle<'static> {
    Settle::new(&REAL_PAUSE, MATRIX_SETTLE_MS)
}

fn claim(rig: &Rig, who: Who, token: &str, duration_ms: u64) -> Claim {
    Claim::new(rig.principal(who), format!("native-{who:?}"), token, &rig.first, duration_ms)
}

fn heads_of_info(info: &Value) -> Vec<String> {
    let mut h: Vec<String> = info
        .get("DcpHeads")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    h.sort();
    h
}

// ------------------------------------------------- versions, both directions

/// Publish through WOPI (`WopiClientStore -> WopiHost -> MatrixStore`) and read
/// it through a second Matrix user: the same version, the WOPI writer's parent
/// and identity, and the same view through both bindings.
pub async fn wopi_publish_is_read_by_a_matrix_user(rig: &Rig) -> R {
    let doc = &rig.doc;
    let first = rig.first.clone();
    let writer = rig.sim(Who::Editor);
    let bytes: &[u8] = b"written through WOPI, read through Matrix";
    let published = writer.publish_version(doc, vec![first.clone()], new_file(bytes), None).await?;
    ensure!(
        published.id.starts_with('$'),
        "the WOPI save must be a Matrix event, got version id {:?}",
        published.id
    );

    let reader = rig.native(Who::Viewer);
    ensure!(
        reader.principal() != rig.principal(Who::Editor),
        "the Matrix reader must be a different principal from the WOPI writer"
    );
    let view = rig.wait_head(Who::Viewer, &published.id).await?;
    ensure!(view.heads[0].bytes == bytes, "the Matrix reader got different bytes than the WOPI writer saved");
    ensure!(view.rejected.is_empty(), "the Matrix reader rejected versions: {:?}", view.rejected);
    let seen = &view.heads[0].version;
    ensure!(
        seen.body.parents == [first.clone()],
        "the Matrix reader sees parents {:?}, the WOPI writer saved a child of {first}",
        seen.body.parents
    );
    ensure!(
        seen.sender == rig.principal(Who::Editor) && seen.body.created_by.principal == rig.principal(Who::Editor),
        "the version is by {} / {}, expected the WOPI token's principal {}",
        seen.sender,
        seen.body.created_by.principal,
        rig.principal(Who::Editor)
    );
    ensure!(
        seen.ts_ms == published.ts_ms,
        "the binding timestamp differs: WOPI says {}, Matrix {}",
        published.ts_ms,
        seen.ts_ms
    );
    ensure!(
        seen.body.message.as_deref().is_some_and(|m| m.starts_with("WOPI PutFile")),
        "the host's message did not reach the Matrix reader: {:?}",
        seen.body.message
    );

    // Both bindings, one document: same heads, bytes, rejections, same content.
    let wopi_reader = rig.sim(Who::Viewer);
    assert_same_view(&**reader, &wopi_reader, doc).await?;
    assert_same_view(&**rig.native(Who::Owner), &**reader, doc).await?;
    assert_same_content(&**reader, doc, &wopi_reader, doc).await?;
    let ids = rig.version_ids(&**reader).await?;
    let mut want = vec![first, published.id.clone()];
    want.sort();
    ensure!(ids == want, "the Matrix reader lists versions {ids:?}, expected {want:?}");

    // The WOPI session took a lease and gave it back.
    let l = reader.lease(doc).await?;
    ensure!(
        matches!(&l, Some(l) if l.body.state == LeaseState::Released),
        "after the WOPI save the lease must read released on Matrix, got {l:?}"
    );
    Ok(())
}

/// Publish natively and open it through WOPI: GetFile serves those bytes with
/// the Matrix event ID as the item version, CheckFileInfo describes it.
pub async fn native_publish_opens_through_wopi_getfile(rig: &Rig) -> R {
    let doc = &rig.doc;
    let bytes: &[u8] = b"published natively, opened through WOPI";
    let native = rig
        .native(Who::Owner)
        .publish_version(doc, vec![rig.first.clone()], new_file(bytes), Some("native message".into()))
        .await?;

    for who in [Who::Editor, Who::Viewer] {
        let info = wait_host_head(rig, who, &native.id).await?;
        ensure!(
            field(&info, "LastModifiedTime") == iso8601_ms(native.ts_ms),
            "{who:?}: LastModifiedTime {:?} is not the binding timestamp {}",
            field(&info, "LastModifiedTime"),
            iso8601_ms(native.ts_ms)
        );
        ensure!(
            info["UserCanWrite"] == Value::Bool(who == Who::Editor),
            "{who:?}: UserCanWrite is {} ",
            info["UserCanWrite"]
        );
        ensure!(
            info["Size"] == Value::from(bytes.len() as u64),
            "{who:?}: Size is {}, the file is {} bytes",
            info["Size"],
            bytes.len()
        );
        let dv = &info["DcpVersion"];
        ensure!(
            dv["created_by"]["principal"] == rig.principal(Who::Owner).as_str() && dv["message"] == "native message",
            "{who:?}: the version metadata did not survive the WOPI path: {dv}"
        );
        ensure!(field(&info, "DcpVersionSender") == rig.principal(Who::Owner), "{who:?}: sender lost");

        let got = rig.raw(who).get_file(None).await?;
        status("GetFile", &got, 200)?;
        ensure!(got.body == bytes, "{who:?}: GetFile returned different bytes than the native publish");
        ensure!(
            got.item_version.as_deref() == Some(native.id.as_str()),
            "{who:?}: X-WOPI-ItemVersion {:?}, expected the Matrix event ID {}",
            got.item_version,
            native.id
        );
    }
    assert_same_view(&**rig.native(Who::Owner), &rig.sim(Who::Viewer), doc).await?;
    Ok(())
}

/// A native publish lands between a WOPI client's GetFile and its PutFile (the
/// lock is held: lease rule 6 lets the native writer publish anyway). The
/// PutFile must be the 1010 conflict, 409 with no `X-WOPI-Lock`, and publish
/// nothing. Re-opening and saving then works, as a child of the native version.
pub async fn native_publish_between_getfile_and_putfile_is_a_409(rig: &Rig) -> R {
    let doc = &rig.doc;
    let ed = rig.raw(Who::Editor);
    let info = ed.check_file_info().await?;
    status("CheckFileInfo", &info, 200)?;
    let info = info.json()?;
    ensure!(field(&info, "Version") == rig.first, "the editor's session did not open the first version");
    let modified = field(&info, "LastModifiedTime").to_owned();
    let got = ed.get_file(None).await?;
    status("GetFile", &got, 200)?;
    ensure!(got.body == BASE_FILE && got.item_version.as_deref() == Some(rig.first.as_str()), "GetFile of the first version");
    let lock = "interop-lock-between";
    status("LOCK", &ed.lock(lock).await?, 200)?;

    let native = rig
        .native(Who::Owner)
        .publish_version(doc, vec![rig.first.clone()], new_file(b"native publish in between"), None)
        .await?;
    // Rule 6: a lease is advisory. It is still the WOPI session's.
    let l = rig.native(Who::Owner).lease(doc).await?;
    ensure!(
        matches!(&l, Some(l) if l.body.state == LeaseState::Held && l.body.token.as_deref() == Some(lock)),
        "the native publish must leave the WOPI lease alone, got {l:?}"
    );
    wait_host_head(rig, Who::Editor, &native.id).await?;

    let put = ed.put_file(Some(lock), Some(&modified), b"stale save from the WOPI session").await?;
    status("stale PutFile", &put, 409)?;
    ensure!(
        put.lock.is_none(),
        "a changed-document 409 must not carry X-WOPI-Lock (Collabora reads that as a lock mismatch), got {:?}",
        put.lock
    );
    ensure!(
        put.json()?["COOLStatusCode"] == 1010,
        "the 409 body must say COOLStatusCode 1010, got {:?}",
        put.text()
    );

    // Nothing was published, whoever asks.
    let mut want = vec![rig.first.clone(), native.id.clone()];
    want.sort();
    for who in Who::ALL {
        let ids = rig.version_ids(&**rig.native(who)).await?;
        ensure!(ids == want, "{who:?} lists {ids:?} after the refused PutFile, expected {want:?}");
    }
    rig.wait_head(Who::Viewer, &native.id).await?;

    // The session still holds its lock and can recover: re-open, save.
    let held = ed.get_lock().await?;
    ensure!(held.lock.as_deref() == Some(lock), "the refused PutFile must not drop the lock, GET_LOCK says {:?}", held.lock);
    let reopened = ed.check_file_info().await?.json()?;
    ensure!(field(&reopened, "Version") == native.id, "re-opened at {}", field(&reopened, "Version"));
    let again = ed.get_file(None).await?;
    ensure!(again.body == b"native publish in between", "re-opened GetFile returned the wrong bytes");
    let saved = ed
        .put_file(Some(lock), Some(field(&reopened, "LastModifiedTime")), b"saved after re-opening")
        .await?;
    status("PutFile after re-opening", &saved, 200)?;
    let id = saved.item_version.clone().ok_or_else(|| Failure::new("PutFile response has no X-WOPI-ItemVersion"))?;
    let view = rig.wait_head(Who::Viewer, &id).await?;
    ensure!(
        view.heads[0].version.body.parents == [native.id.clone()] && view.heads[0].bytes == b"saved after re-opening",
        "the recovered save must be a child of the native version: {:?}",
        view.heads[0].version.body.parents
    );
    status("UNLOCK", &ed.unlock(lock).await?, 200)?;
    Ok(())
}

/// A WOPI save without Collabora's timestamp precondition (a forced save)
/// after a native publish forks: two heads. The Matrix reader shows the
/// conflict, WOPI opens the document read-only, and a native merge makes it
/// writable again. (Spec lease rule 6: accepted as a version, a branch.)
pub async fn forced_wopi_save_forks_and_matrix_shows_the_conflict(rig: &Rig) -> R {
    let doc = &rig.doc;
    let ed = rig.raw(Who::Editor);
    status("GetFile", &ed.get_file(None).await?, 200)?;
    let lock = "interop-lock-forced";
    status("LOCK", &ed.lock(lock).await?, 200)?;
    let native = rig
        .native(Who::Owner)
        .publish_version(doc, vec![rig.first.clone()], new_file(b"native edit A"), None)
        .await?;
    wait_host_head(rig, Who::Editor, &native.id).await?;

    let forced = ed.put_file(Some(lock), None, b"forced WOPI edit B").await?;
    status("forced PutFile", &forced, 200)?;
    let wopi_id = forced.item_version.clone().ok_or_else(|| Failure::new("PutFile response has no X-WOPI-ItemVersion"))?;

    for who in Who::ALL {
        let v = rig.wait_heads(who, 2).await?;
        ensure!(v.is_conflict(), "{who:?}: two heads must be reported as a conflict");
        let mut ids: Vec<String> = v.head_ids().into_iter().map(str::to_owned).collect();
        ids.sort();
        let mut want = vec![native.id.clone(), wopi_id.clone()];
        want.sort();
        ensure!(ids == want, "{who:?} lists heads {ids:?}, expected {want:?}");
        ensure!(
            v.heads.iter().all(|h| h.version.body.parents == [rig.first.clone()]),
            "{who:?}: both heads must be children of the first version"
        );
        let mut contents: Vec<Vec<u8>> = v.heads.iter().map(|h| h.bytes.clone()).collect();
        contents.sort();
        ensure!(
            contents == [b"forced WOPI edit B".to_vec(), b"native edit A".to_vec()],
            "{who:?} shows the wrong content for the heads"
        );
    }
    assert_same_view(&**rig.native(Who::Viewer), &rig.sim(Who::Viewer), doc).await?;

    // WOPI cannot show a conflict: the host opens the document read-only and
    // lists the heads in an extension.
    let info = ed.check_file_info().await?.json()?;
    ensure!(info["UserCanWrite"] == false && info["ReadOnly"] == true, "a conflicted document must open read-only over WOPI: {info}");
    let mut want = vec![native.id.clone(), wopi_id.clone()];
    want.sort();
    ensure!(heads_of_info(&info) == want, "DcpHeads is {:?}, expected {want:?}", heads_of_info(&info));
    let got = ed.get_file(None).await?;
    ensure!(got.heads.as_deref() == Some("2"), "GetFile X-DCP-Heads is {:?}", got.heads);

    // A native merge of both heads resolves it, and WOPI follows.
    let view = rig.view(Who::Owner).await?;
    let heads: Vec<_> = view.heads.iter().map(|h| &h.version).collect();
    let merged = rig
        .native(Who::Owner)
        .publish_version(doc, graph::merge_parents(&heads), new_file(b"merged natively"), None)
        .await?;
    let info = wait_host_head(rig, Who::Editor, &merged.id).await?;
    ensure!(info["UserCanWrite"] == true, "after the merge the editor must be able to write again: {info}");
    ensure!(heads_of_info(&info) == [merged.id.clone()], "DcpHeads after the merge: {:?}", heads_of_info(&info));
    assert_same_view(&**rig.native(Who::Viewer), &rig.sim(Who::Viewer), doc).await?;
    Ok(())
}

/// A native publish based on an old head (an offline edit coming back) forks
/// the document; the Matrix reader shows both heads, and so does WOPI.
pub async fn native_offline_publish_shows_as_a_conflict_to_the_matrix_reader(rig: &Rig) -> R {
    let doc = &rig.doc;
    let online = rig
        .sim(Who::Editor)
        .publish_version(doc, vec![rig.first.clone()], new_file(b"edit made online through WOPI"), None)
        .await?;
    rig.wait_head(Who::Owner, &online.id).await?;
    // The owner was offline since the first version and now comes back.
    let offline = rig
        .native(Who::Owner)
        .publish_version(doc, vec![rig.first.clone()], new_file(b"edit made offline"), None)
        .await?;

    let reader = rig.native(Who::Viewer);
    let v = rig.wait_heads(Who::Viewer, 2).await?;
    ensure!(v.is_conflict(), "two heads must be reported as a conflict, never resolved silently");
    let mut ids: Vec<String> = v.head_ids().into_iter().map(str::to_owned).collect();
    ids.sort();
    let mut want = vec![online.id.clone(), offline.id.clone()];
    want.sort();
    ensure!(ids == want, "the Matrix reader lists heads {ids:?}, expected {want:?}: one was hidden");
    ensure!(
        v.heads.windows(2).all(|w| w[0].version.ts_ms >= w[1].version.ts_ms),
        "heads must be newest first by binding timestamp"
    );
    let mut contents: Vec<Vec<u8>> = v.heads.iter().map(|h| h.bytes.clone()).collect();
    contents.sort();
    ensure!(
        contents == [b"edit made offline".to_vec(), b"edit made online through WOPI".to_vec()],
        "wrong content for the heads"
    );
    assert_same_view(&**rig.native(Who::Owner), &**reader, doc).await?;
    assert_same_view(&**reader, &rig.sim(Who::Viewer), doc).await?;

    let info = wait_host_head(rig, Who::Editor, &v.heads[0].version.id).await?;
    ensure!(info["UserCanWrite"] == false, "the WOPI editor must get a read-only session on a conflicted document: {info}");
    ensure!(heads_of_info(&info) == want, "DcpHeads is {:?}, expected {want:?}", heads_of_info(&info));
    Ok(())
}

// ----------------------------------------------------------------- roles

/// A viewer's WOPI token cannot lock or save, nor can the viewer natively; an
/// editor's token on the same document can. Nothing the viewer tried leaves a
/// version or a lease.
pub async fn viewer_token_cannot_write(rig: &Rig) -> R {
    let doc = &rig.doc;
    let vw = rig.raw(Who::Viewer);
    let info = vw.check_file_info().await?;
    status("CheckFileInfo", &info, 200)?;
    let info = info.json()?;
    ensure!(
        info["UserCanWrite"] == false && info["ReadOnly"] == true,
        "a viewer's session must be read-only: {info}"
    );
    let modified = field(&info, "LastModifiedTime").to_owned();
    let got = vw.get_file(None).await?;
    status("viewer GetFile", &got, 200)?;
    ensure!(got.body == BASE_FILE, "a viewer can read the document");

    status("viewer LOCK", &vw.lock("viewer-lock").await?, 403)?;
    status("viewer PutFile without a lock", &vw.put_file(None, None, b"viewer was here").await?, 403)?;
    status(
        "viewer PutFile with a lock and the timestamp",
        &vw.put_file(Some("viewer-lock"), Some(&modified), b"viewer was here").await?,
        403,
    )?;
    status("viewer UNLOCK", &vw.unlock("viewer-lock").await?, 403)?;
    let through_wopi = rig
        .sim(Who::Viewer)
        .publish_version(doc, vec![rig.first.clone()], new_file(b"viewer was here"), None)
        .await;
    ensure!(
        matches!(through_wopi, Err(StoreError::Forbidden(_))),
        "a viewer saving through WopiClientStore must be Forbidden, got {through_wopi:?}"
    );
    let natively = rig
        .native(Who::Viewer)
        .publish_version(doc, vec![rig.first.clone()], new_file(b"viewer was here"), None)
        .await;
    ensure!(
        matches!(natively, Err(StoreError::Forbidden(_))),
        "a viewer publishing natively must be Forbidden, got {natively:?}"
    );

    // Control: the same document, an editor's token.
    let saved = rig
        .sim(Who::Editor)
        .publish_version(doc, vec![rig.first.clone()], new_file(b"editor was here"), None)
        .await?;
    let v = rig.wait_head(Who::Owner, &saved.id).await?;
    ensure!(v.heads[0].bytes == b"editor was here", "the editor's save");
    let mut want = vec![rig.first.clone(), saved.id.clone()];
    want.sort();
    let ids = rig.version_ids(&**rig.native(Who::Owner)).await?;
    ensure!(ids == want, "nothing the viewer tried may leave a version: {ids:?}, expected {want:?}");
    let l = rig.native(Who::Owner).lease(doc).await?;
    ensure!(
        matches!(&l, Some(l) if l.body.state == LeaseState::Released),
        "the viewer's refused LOCK left a lease record: {l:?}"
    );
    Ok(())
}

// ----------------------------------------------------------------- leases

/// A lock held through WOPI is an active lease to a native session (it cannot
/// acquire; the record names the WOPI holder, the lock string and WOPI's fixed
/// 30 minutes). Once released a native session acquires, and then WOPI's LOCK
/// gets the 409 with the native token. Released natively, WOPI locks again.
pub async fn wopi_lock_blocks_native_acquire_and_the_reverse(rig: &Rig) -> R {
    let doc = &rig.doc;
    let ed = rig.raw(Who::Editor);
    status("GetFile", &ed.get_file(None).await?, 200)?;
    status("LOCK", &ed.lock("wopi-lock-A").await?, 200)?;

    let owner = rig.native(Who::Owner);
    let mine = claim(rig, Who::Owner, "native-token-owner", 60_000);
    match lease::acquire(&**owner, doc, &mine, &settle()).await? {
        Acquire::Held(cur) => {
            ensure!(cur.body.state == LeaseState::Held, "the lease the owner was refused for is not held: {cur:?}");
            ensure!(cur.body.token.as_deref() == Some("wopi-lock-A"), "the held lease carries token {:?}", cur.body.token);
            ensure!(
                cur.body.holder.as_ref().map(|h| h.principal.as_str()) == Some(rig.principal(Who::Editor).as_str()),
                "the held lease names holder {:?}, expected the WOPI token's principal",
                cur.body.holder
            );
            ensure!(cur.body.duration_ms == Some(30 * 60 * 1000), "WOPI locks last 30 minutes, got {:?}", cur.body.duration_ms);
            ensure!(cur.body.renewal == Some(0), "a WOPI LOCK over no record must write renewal 0 (rule 8), got {:?}", cur.body.renewal);
            schema::validate_typed(Kind::Lease, &cur.body).map_err(|e| Failure::new(format!("the WOPI host's lease does not validate: {e}")))?;
            ensure!(cur.body.base_version.as_deref() == Some(rig.first.as_str()), "base_version {:?}", cur.body.base_version);
        }
        other => return Err(Failure::new(format!("a native session acquired a lease the WOPI client holds: {other:?}"))),
    }
    // The same principal in another session is also refused: the holder is a session.
    let editor_native = claim(rig, Who::Editor, "native-token-editor", 60_000);
    let a = lease::acquire(&**rig.native(Who::Editor), doc, &editor_native, &settle()).await?;
    ensure!(matches!(a, Acquire::Held(_)), "the WOPI holder's own principal in another session acquired: {a:?}");
    // The Matrix reader sees the same record.
    let seen = rig.native(Who::Viewer).lease(doc).await?;
    ensure!(
        matches!(&seen, Some(l) if l.body.token.as_deref() == Some("wopi-lock-A") && l.body.state == LeaseState::Held),
        "the Matrix reader sees {seen:?}"
    );

    // WOPI lets go; the native session takes over.
    status("UNLOCK", &ed.unlock("wopi-lock-A").await?, 200)?;
    let got = lease::acquire(&**owner, doc, &mine, &settle()).await?;
    let Acquire::Acquired(rec) = got else {
        return Err(Failure::new(format!("after UNLOCK a native session must acquire: {got:?}")));
    };
    ensure!(rec.body.renewal == Some(0), "over the WOPI host's released record the count starts again at 0, got {:?}", rec.body.renewal);

    // ... and now WOPI is the one refused, with the native token as the current lock.
    let refused = ed.lock("wopi-lock-B").await?;
    status("LOCK against a native lease", &refused, 409)?;
    ensure!(
        refused.lock.as_deref() == Some("native-token-owner"),
        "the 409 must carry the current lock string (WOPI), got {:?}",
        refused.lock
    );
    let current = ed.get_lock().await?;
    ensure!(current.lock.as_deref() == Some("native-token-owner"), "GET_LOCK shows {:?}", current.lock);

    // Released natively, WOPI can lock again.
    let r = lease::release(&**owner, doc, &mine.holder).await?;
    ensure!(r == Release::Released, "native release: {r:?}");
    status("LOCK after the native release", &ed.lock("wopi-lock-B").await?, 200)?;
    status("UNLOCK", &ed.unlock("wopi-lock-B").await?, 200)?;
    Ok(())
}

/// A native lease held by someone else stops a WOPI save (409 with the native
/// token, nothing published), but not a native writer: lease rule 6 makes the
/// lease advisory, and only WOPI is stopped by it.
pub async fn native_lease_blocks_wopi_save(rig: &Rig) -> R {
    let doc = &rig.doc;
    let owner = rig.native(Who::Owner);
    let mine = claim(rig, Who::Owner, "native-token-hold", 60_000);
    let got = lease::acquire(&**owner, doc, &mine, &settle()).await?;
    ensure!(got.is_acquired(), "the owner must acquire a free lease: {got:?}");

    let ed = rig.raw(Who::Editor);
    let info = ed.check_file_info().await?.json()?;
    let modified = field(&info, "LastModifiedTime").to_owned();
    status("GetFile", &ed.get_file(None).await?, 200)?;
    for (what, resp) in [
        ("LOCK", ed.lock("wopi-wants-in").await?),
        ("PutFile with its own lock string", ed.put_file(Some("wopi-wants-in"), Some(&modified), b"blocked").await?),
        ("PutFile with no lock", ed.put_file(None, Some(&modified), b"blocked").await?),
    ] {
        status(what, &resp, 409)?;
        ensure!(
            resp.lock.as_deref() == Some("native-token-hold"),
            "{what}: the 409 must carry the current lock string, got {:?}",
            resp.lock
        );
    }
    let through_wopi = rig
        .sim(Who::Editor)
        .publish_version(doc, vec![rig.first.clone()], new_file(b"blocked"), None)
        .await;
    ensure!(
        matches!(through_wopi, Err(StoreError::Conflict(_))),
        "a WopiClientStore save under a foreign lease must be a Conflict, got {through_wopi:?}"
    );
    let ids = rig.version_ids(&**owner).await?;
    ensure!(ids == [rig.first.clone()], "a blocked WOPI save published {ids:?}");

    // Advisory (rule 6): a native editor is not stopped, and the lease stays.
    let native = rig
        .native(Who::Editor)
        .publish_version(doc, vec![rig.first.clone()], new_file(b"native editor ignores the lease"), None)
        .await?;
    let l = owner.lease(doc).await?;
    ensure!(
        matches!(&l, Some(l) if l.body.token.as_deref() == Some("native-token-hold")),
        "the lease changed: {l:?}"
    );
    rig.wait_head(Who::Viewer, &native.id).await?;
    Ok(())
}

/// Lease rule 5: an owner breaks a WOPI holder's lock natively. The WOPI
/// session finds out as WOPI says it must: GET_LOCK shows the new lock, its
/// PutFile is a 409 carrying it, and it cannot unlock the breaker's lease.
pub async fn owner_break_invalidates_the_wopi_lock(rig: &Rig) -> R {
    let doc = &rig.doc;
    let ed = rig.raw(Who::Editor);
    let info = ed.check_file_info().await?.json()?;
    let modified = field(&info, "LastModifiedTime").to_owned();
    status("GetFile", &ed.get_file(None).await?, 200)?;
    status("LOCK", &ed.lock("wopi-lock-C").await?, 200)?;

    let breaker = claim(rig, Who::Owner, "native-token-break", 60_000);
    match lease::break_lease(&**rig.native(Who::Owner), doc, &breaker, &settle()).await? {
        Acquire::Acquired(rec) => {
            ensure!(rec.body.token.as_deref() == Some("native-token-break"), "the break wrote {rec:?}");
            ensure!(
                rec.body.renewal == Some(1),
                "a break over the WOPI host's held record (renewal 0) must write renewal 1 (rule 8), got {:?}",
                rec.body.renewal
            );
        }
        other => return Err(Failure::new(format!("the owner could not break the lease: {other:?}"))),
    }
    let now = ed.get_lock().await?;
    ensure!(now.lock.as_deref() == Some("native-token-break"), "GET_LOCK after the break shows {:?}", now.lock);
    let put = ed.put_file(Some("wopi-lock-C"), Some(&modified), b"saved with a broken lock").await?;
    status("PutFile with the broken lock", &put, 409)?;
    ensure!(put.lock.as_deref() == Some("native-token-break"), "the 409 must carry the breaker's lock, got {:?}", put.lock);
    let un = ed.unlock("wopi-lock-C").await?;
    status("UNLOCK with the broken lock", &un, 409)?;
    ensure!(un.lock.as_deref() == Some("native-token-break"), "UNLOCK 409 carries {:?}", un.lock);
    let ids = rig.version_ids(&**rig.native(Who::Owner)).await?;
    ensure!(ids == [rig.first.clone()], "a save with a broken lock published {ids:?}");
    // The breaker's lease is untouched by the failed UNLOCK.
    let l = rig.native(Who::Viewer).lease(doc).await?;
    ensure!(
        matches!(&l, Some(l) if l.body.token.as_deref() == Some("native-token-break") && l.body.state == LeaseState::Held),
        "the breaker's lease was disturbed: {l:?}"
    );
    Ok(())
}

/// A native lease is active until `ts + duration`, by the binding's clock; a
/// WOPI LOCK is refused until then and succeeds after it (lease rule 2).
pub async fn native_lease_expiry_frees_the_wopi_lock(rig: &Rig) -> R {
    let doc = &rig.doc;
    let mine = claim(rig, Who::Owner, "native-token-short", 6_000);
    let got = lease::acquire(&**rig.native(Who::Owner), doc, &mine, &settle()).await?;
    ensure!(got.is_acquired(), "the owner must acquire a free lease: {got:?}");
    let started = Instant::now();

    let ed = rig.raw(Who::Editor);
    status("LOCK against an active native lease", &ed.lock("wopi-after-expiry").await?, 409)?;
    let mut attempts = 0u32;
    rig.until("the WOPI LOCK to succeed once the native lease has expired", || {
        attempts += 1;
        let ed = ed.clone();
        async move {
            let r = ed.lock("wopi-after-expiry").await?;
            match r.status {
                200 => Ok(Ok(())),
                409 => Ok(Err(format!("LOCK still 409, current lock {:?}", r.lock))),
                other => Err(Failure::new(format!("LOCK answered HTTP {other}: {:?}", r.text()))),
            }
        }
    })
    .await?;
    println!("native lease of 6000 ms: WOPI LOCK succeeded after {} ms, {attempts} attempts", started.elapsed().as_millis());

    let l = rig.native(Who::Viewer).lease(doc).await?;
    ensure!(
        matches!(&l, Some(l) if l.body.token.as_deref() == Some("wopi-after-expiry") && l.body.state == LeaseState::Held),
        "after expiry the lease must be the WOPI one, got {l:?}"
    );
    ensure!(
        l.as_ref().is_some_and(|l| l.body.renewal == Some(1)),
        "a WOPI LOCK over an expired HELD native record continues the count (rule 8): expected renewal 1, got {l:?}"
    );
    let a = lease::acquire(&**rig.native(Who::Owner), doc, &mine, &settle()).await?;
    ensure!(matches!(a, Acquire::Held(_)), "the native session acquired over the new WOPI lock: {a:?}");
    status("UNLOCK", &ed.unlock("wopi-after-expiry").await?, 200)?;
    Ok(())
}

// --------------------------------------------------------------- removal

/// The owner removes the editor (natively). The editor's WOPI token is still in
/// the host's registry, so any refusal comes from the Matrix store behind it.
///
/// What the code does (observed against Synapse 1.163.2 and asserted): the host
/// keeps the token, but the Matrix store behind it refuses every request, reads
/// included. `MatrixStore::room_for` tries to re-join the room it was kicked
/// from, Synapse answers `403 M_FORBIDDEN` ("You are not invited to this room"),
/// and the host maps `StoreError::Forbidden` to HTTP 403. So the token does not
/// merely get no new content: it gets nothing, not even the versions it could
/// read before. The refusal is the Matrix binding's and the room's, not the
/// host's: the WOPI host has no notion of membership and never learns of the
/// removal. Anything still in the member's own device is outside this check
/// (spec section 11).
pub async fn removed_members_wopi_token_gets_no_new_content_and_no_write(rig: &Rig) -> R {
    let doc = &rig.doc;
    let ed = rig.raw(Who::Editor);
    status("before removal: CheckFileInfo", &ed.check_file_info().await?, 200)?;
    status("before removal: GetFile", &ed.get_file(None).await?, 200)?;

    rig.native(Who::Owner).revoke(doc, &rig.principal(Who::Editor)).await?;
    let secret: &[u8] = b"published after the editor was removed";
    let after = rig
        .native(Who::Owner)
        .publish_version(doc, vec![rig.first.clone()], new_file(secret), None)
        .await?;
    // A member who stayed gets it through WOPI: the failure below is the removal's.
    let info = wait_host_head(rig, Who::Viewer, &after.id).await?;
    ensure!(info["Version"] == after.id.as_str(), "a remaining member must see the new version through WOPI");

    let probes: Vec<(&str, Resp)> = vec![
        ("CheckFileInfo", ed.check_file_info().await?),
        ("GetFile", ed.get_file(None).await?),
        ("GetFile of the new version", ed.get_file(Some(&after.id)).await?),
        ("LOCK", ed.lock("removed-members-lock").await?),
        ("PutFile with no lock", ed.put_file(None, None, b"written by a removed member").await?),
        ("PutFile with a lock", ed.put_file(Some("removed-members-lock"), None, b"written by a removed member").await?),
    ];
    for (what, r) in &probes {
        println!("removed member's token, {what}: HTTP {} {:?}", r.status, r.text());
        ensure!(
            r.status == 403 && r.text().contains("M_FORBIDDEN"),
            "{what}: a removed member's token must be refused by the Matrix store with 403 M_FORBIDDEN \
             (401 would mean the host forgot the token), got HTTP {} {:?}",
            r.status,
            r.text()
        );
        ensure!(
            !r.body.windows(secret.len()).any(|w| w == secret),
            "{what}: the removed member received content published after the removal"
        );
        ensure!(r.item_version.as_deref() != Some(after.id.as_str()), "{what}: served the version published after the removal");
    }
    // The members who stayed are unaffected.
    let got = rig.raw(Who::Viewer).get_file(None).await?;
    ensure!(got.body == secret && got.item_version.as_deref() == Some(after.id.as_str()), "a remaining member lost access");

    let through_wopi = rig
        .sim(Who::Editor)
        .publish_version(doc, vec![after.id.clone()], new_file(b"x"), None)
        .await;
    ensure!(
        matches!(through_wopi, Err(StoreError::Forbidden(_))),
        "a removed member saving through WopiClientStore must be Forbidden: {through_wopi:?}"
    );
    let mut want = vec![rig.first.clone(), after.id.clone()];
    want.sort();
    let ids = rig.version_ids(&**rig.native(Who::Owner)).await?;
    ensure!(ids == want, "the removed member's attempts left versions: {ids:?}, expected {want:?}");
    let l = rig.native(Who::Owner).lease(doc).await?;
    ensure!(l.is_none(), "the removed member's LOCK left a lease record: {l:?}");
    Ok(())
}

// ------------------------------------------------- the renewal counter (4.3)

/// Collabora refreshes a lock by sending `LOCK` again with the same string.
/// Over Matrix that is a state event whose content would be IDENTICAL to the
/// host's own current one, which Synapse silently discards (spike A). The
/// host writes `renewal` + 1 (lease rule 8), so each refresh is a new event
/// with a later timestamp; over a released record the count starts again at 0.
pub async fn wopi_lock_refresh_is_a_new_record_on_matrix(rig: &Rig) -> R {
    let doc = &rig.doc;
    let ed = rig.raw(Who::Editor);
    status("GetFile", &ed.get_file(None).await?, 200)?;
    let reader = rig.native(Who::Viewer);
    let lock = "interop-refresh-lock";

    status("LOCK", &ed.lock(lock).await?, 200)?;
    let mut previous = reader.lease(doc).await?.ok_or_else(|| Failure::new("no lease record after LOCK"))?;
    ensure!(previous.body.renewal == Some(0), "first LOCK: renewal {:?}", previous.body.renewal);
    for expected in [1u64, 2, 3] {
        // The same lock string again: a refresh.
        status("refresh (LOCK, same string)", &ed.lock(lock).await?, 200)?;
        let now = reader.lease(doc).await?.ok_or_else(|| Failure::new("no lease record after the refresh"))?;
        ensure!(
            now.body.renewal == Some(expected),
            "refresh {expected}: the record carries renewal {:?}, expected {expected} (rule 8)",
            now.body.renewal
        );
        ensure!(
            now.id != previous.id && now.ts_ms > previous.ts_ms,
            "refresh {expected} did not produce a new event with a later timestamp: {} @ {} after {} @ {} (Synapse drops an identical state event)",
            now.id,
            now.ts_ms,
            previous.id,
            previous.ts_ms
        );
        ensure!(
            now.body.token.as_deref() == Some(lock) && now.body.holder == previous.body.holder && now.body.base_version == previous.body.base_version,
            "a refresh must keep the holder, token and base version: {:?} after {:?}",
            now.body,
            previous.body
        );
        previous = now;
    }

    // Released, then locked again: over a record that is not held it is 0.
    status("UNLOCK", &ed.unlock(lock).await?, 200)?;
    status("LOCK after UNLOCK", &ed.lock(lock).await?, 200)?;
    let again = reader.lease(doc).await?.ok_or_else(|| Failure::new("no lease record"))?;
    ensure!(again.body.renewal == Some(0), "over a released record the count starts again at 0, got {:?}", again.body.renewal);
    status("UNLOCK", &ed.unlock(lock).await?, 200)?;
    Ok(())
}

/// A lease on a document with no version yet omits `base_version` (never a
/// placeholder), still validates against the schema, and the WOPI session can
/// then save the document's first version (no parents).
pub async fn wopi_lock_on_an_empty_document_omits_base_version(rig: &Rig) -> R {
    let (doc, ed) = rig.empty_document().await?;
    let owner = rig.native(Who::Owner);
    ensure!(owner.versions(&doc).await?.is_empty(), "the document must start with no version");

    let got = ed.get_file(None).await?;
    status("GetFile of an empty document", &got, 200)?;
    ensure!(got.body.is_empty(), "an empty document has no content (version rule 7)");
    let lock = "interop-empty-lock";
    status("LOCK", &ed.lock(lock).await?, 200)?;
    let l = owner.lease(&doc).await?.ok_or_else(|| Failure::new("no lease record after LOCK"))?;
    ensure!(l.body.state == LeaseState::Held && l.body.token.as_deref() == Some(lock), "{:?}", l.body);
    ensure!(l.body.base_version.is_none(), "base_version must be omitted on an empty document, got {:?}", l.body.base_version);
    ensure!(l.body.renewal == Some(0) && l.body.duration_ms == Some(1_800_000), "{:?}", l.body);
    schema::validate_typed(Kind::Lease, &l.body).map_err(|e| Failure::new(format!("the lease does not validate: {e}")))?;
    let wire = serde_json::to_value(&l.body).map_err(|e| Failure::new(e.to_string()))?;
    ensure!(wire.get("base_version").is_none(), "the record on the wire has a base_version: {wire}");

    // The first version: no parents.
    let put = ed.put_file(Some(lock), None, b"the first version, saved through WOPI").await?;
    status("PutFile of the first version", &put, 200)?;
    let id = put.item_version.clone().ok_or_else(|| Failure::new("PutFile response has no X-WOPI-ItemVersion"))?;
    let versions = owner.versions(&doc).await?;
    ensure!(
        versions.len() == 1 && versions[0].id == id && versions[0].body.parents.is_empty(),
        "the first version must have no parents: {versions:?}"
    );
    status("UNLOCK", &ed.unlock(lock).await?, 200)?;
    Ok(())
}

// ------------------------------------- forged versions from a viewer (8.1)

/// What a hostile viewer's client sends: a version event straight to the
/// homeserver, around `MatrixStore`'s own role check, once under its own name
/// and once with `created_by` naming the owner. The file is the first
/// version's, so only the authorisation rules can reject it. Returns the ids
/// of the events the server accepted.
///
/// In a room with the spec's power-level table (`m.room.encrypted` gated at
/// 50) the server must refuse both; in a room with the draft-0 table it
/// accepts both, and the readers are all that is left. Which one this room is
/// is read from its power levels, and the expectation follows it: a rig that
/// does not behave as its table says fails here.
async fn forge_as_viewer(rig: &Rig) -> Result<Vec<String>, Failure> {
    let doc = &rig.doc;
    let viewer = &rig.handles.viewer;
    let gated = rig.handles.owner.snapshot(doc).await?.power.server_gates_encrypted_timeline();
    let base = rig
        .native(Who::Owner)
        .versions(doc)
        .await?
        .into_iter()
        .find(|v| v.id == rig.first)
        .ok_or_else(|| Failure::new("the first version is not listed"))?;
    let mut accepted = Vec::new();
    for (label, claimed) in [("under its own name", rig.principal(Who::Viewer)), ("claiming to be the owner", rig.principal(Who::Owner))] {
        let forged = Version {
            dcp: 1,
            parents: vec![rig.first.clone()],
            file: base.body.file.clone(),
            created_by: Author { principal: claimed, device: viewer.device() },
            message: Some(format!("forged by a viewer, {label}")),
            covers: None,
        };
        let content = serde_json::to_value(&forged).map_err(|e| Failure::new(e.to_string()))?;
        let sent = viewer.send_unchecked(doc, "org.tunaos.dcp.version", content).await;
        match (gated, sent) {
            (true, Err(StoreError::Forbidden(_))) => {}
            (true, other) => {
                return Err(Failure::new(format!(
                    "the room's power levels gate m.room.encrypted at the editor level, so the server must refuse a viewer's version ({label}); got {other:?}"
                )))
            }
            (false, Ok(id)) => accepted.push(id),
            (false, other) => {
                return Err(Failure::new(format!(
                    "the room has no m.room.encrypted gate (draft-0 table), so the server must accept the viewer's event ({label}) for this check to mean anything; got {other:?}"
                )))
            }
        }
    }
    Ok(accepted)
}

/// Publish a legitimate save after the forgeries: it is the only head
/// anywhere, and the forgeries (sent before it) cannot be later than anything
/// a reader has seen once it sees this.
async fn legitimate_save_after_forgeries(rig: &Rig) -> Result<dcp_core::Stamped<Version>, Failure> {
    Ok(rig
        .native(Who::Owner)
        .publish_version(&rig.doc, vec![rig.first.clone()], new_file(b"a legitimate save after the forgeries"), None)
        .await?)
}

/// A viewer cannot publish a version a Matrix reader accepts (spec 6, 8.1):
/// refused by the server when `m.room.encrypted` is gated, otherwise dropped
/// by every Matrix reader (sender below the editor level; `created_by` not the
/// sender). Every principal's native view shows the legitimate save alone.
pub async fn forged_version_is_dropped_by_matrix_readers(rig: &Rig) -> R {
    let forged = forge_as_viewer(rig).await?;
    let marker = legitimate_save_after_forgeries(rig).await?;
    for who in Who::ALL {
        let v = rig
            .until(&format!("{who:?} to see the legitimate save"), || async {
                let v = rig.view(who).await?;
                if v.heads.iter().any(|h| h.version.id == marker.id) {
                    return Ok(Ok(v));
                }
                Ok(Err(format!("heads {:?}", v.head_ids())))
            })
            .await?;
        ensure!(
            v.head_ids() == [marker.id.as_str()],
            "{who:?}: a viewer's forged version was accepted by a Matrix reader (the server accepted {forged:?}); heads are {:?}",
            v.head_ids()
        );
        let listed = rig.version_ids(&**rig.native(who)).await?;
        ensure!(
            listed.iter().all(|id| !forged.contains(id)),
            "{who:?}'s store still lists the forged versions: {listed:?}"
        );
    }
    Ok(())
}

/// The same through WOPI: the host inherits the Matrix reader's defence. Every
/// WOPI token's CheckFileInfo has the legitimate save as its single head
/// (`DcpHeads`), the editor can still write (no phantom conflict), GetFile
/// serves the legitimate bytes, and the version listing the simulator reads
/// has no forgery. A viewer's own WOPI token cannot add one either.
pub async fn forged_version_is_dropped_by_wopi_readers(rig: &Rig) -> R {
    let forged = forge_as_viewer(rig).await?;
    let marker = legitimate_save_after_forgeries(rig).await?;
    for who in Who::ALL {
        let info = wait_host_head(rig, who, &marker.id).await?;
        ensure!(
            heads_of_info(&info) == [marker.id.clone()],
            "{who:?}: through WOPI the heads are {:?}, expected only the legitimate save (the server accepted {forged:?})",
            heads_of_info(&info)
        );
        ensure!(
            info["UserCanWrite"] == Value::Bool(who != Who::Viewer),
            "{who:?}: UserCanWrite is {} (a forged head would make the document read-only)",
            info["UserCanWrite"]
        );
        let got = rig.raw(who).get_file(None).await?;
        status("GetFile", &got, 200)?;
        ensure!(
            got.body == b"a legitimate save after the forgeries" && got.item_version.as_deref() == Some(marker.id.as_str()),
            "{who:?}: GetFile served something else than the legitimate save"
        );
        let listed = rig.version_ids(&rig.sim(who)).await?;
        ensure!(
            listed.iter().all(|id| !forged.contains(id)),
            "{who:?}: the WOPI version listing contains the forged versions: {listed:?}"
        );
    }
    // The viewer's WOPI token is no way around it.
    let vw = rig.raw(Who::Viewer);
    status("viewer PutFile", &vw.put_file(None, None, b"forged through WOPI").await?, 403)?;
    status("viewer LOCK", &vw.lock("forgery-lock").await?, 403)?;
    Ok(())
}
