# RFC-0004 Spike D: WOPI host adapter results

Date: 2026-10-24 · Tracks: [RFC-0004](0004-interoperable-document-collaboration.md), spec [section 8.2](../DOCUMENT-COLLABORATION-SPEC.md) · Code: [`tools/collab-spike/dcp-wopi/`](../../tools/collab-spike/dcp-wopi/)

> **Draft 1 follow-up (2026-10-24).** The measurements below are of the draft-0 host and are
> not redone. The host changed with draft 1: `base_version` is omitted on an empty document
> (no `"none"` sentinel), a lock refresh writes `renewal` + 1, the duration is always
> 1 800 000 ms, a `PutFile` from a token that read no version is refused, and "the host over
> the Matrix binding" (listed under Not verified) was run, including a real Collabora. See
> [the interop results](0004-spike-interop-results.md#draft-1-update).

**These results come from one Collabora Online build (26.04.5.2, the
`collabora/code:latest` image of 2026-10-23) on GitHub-hosted runners, driven
by our own tests with a generated document. Every observation is of that one
build and one run. They are not a compatibility statement about Collabora or
about WOPI clients in general.** Anything not run is listed under
[Not verified](#not-verified).

## Recommendation

**Go, with conditions, for L1 (versions and lease) on session-hosted
editors through a WOPI host adapter.** Not for L2 or L3.

The deciding result is the real round trip. Collabora Online opened a document
served by `WopiHost` by `WOPISrc`, a user typed, the document was saved, and
the backing store then held a new DCP version whose only parent was the version
Collabora had opened and whose `content.xml` contained the typed text. The
save sent the lock string of the lease. A stale save was refused as a conflict
and published nothing, and a viewer got a read-only session that never locked
or saved. The same host and simulator also pass the part of the shared
conformance journeys that WOPI can express (4 of 10 passed, 6 skipped with a
stated reason, 0 failed).

The conditions, each of which the spec does not yet say (see
[Spec corrections](#spec-corrections)):

1. **The host must remember, per access token, which version the session read**
   (`GetFile`). WOPI gives a `PutFile` no other way to name its base, and the
   spec's "the version the editor opened" has no wire carrier.
2. **A conflict is detected by Collabora's timestamp precondition, not by the
   lock.** The lease only stops a second WOPI client; a native DCP writer is
   not stopped by it (spec lease rule 6), so the timestamp check is what
   protects Collabora's user from overwriting that writer's version.
3. **WOPI cannot express a DCP conflict** (two heads) or a merge. The host must
   choose how to show one; this spike opens a multi-head document read-only.
4. **A fixed 30-minute lock** is part of WOPI. DCP's range (1 s to 1 h) is
   wider than WOPI can use.
5. **L2 updates, L3 presence, role changes and lease breaking have no WOPI
   carrier.** A document edited by Collabora is L1 only.

What this does not show: how the adapter behaves over the Matrix binding. Every
number here is over an in-memory store. Collabora's save frequency against the
Matrix limits is Spike A's question, and is open.

## Method

`tools/collab-spike/` is a standalone Cargo workspace (see
[RFC-0001's spike](0001-spike-results.md#method) for the pattern). `dcp-wopi`
contains:

- `WopiHost` (axum 0.8): serves WOPI over any `Arc<dyn DocumentStore>` through
  an access-token registry `token -> (doc, store handle, principal)`.
  `CheckFileInfo`, `GetFile`, `PutFile` and `POST /wopi/files/{id}` with
  `X-WOPI-Override` of `LOCK`, `GET_LOCK`, `REFRESH_LOCK`, `UNLOCK` and
  `UNLOCK_AND_RELOCK` (a `LOCK` carrying `X-WOPI-OldLock`). Every other
  override answers 501. Every request is logged (token redacted) for this
  document.
- `WopiClientStore`: a WOPI client simulator that is itself a `DocumentStore`,
  speaking only WOPI HTTP to a `WopiHost`.
- `tests/collabora_e2e.rs`: drives a real Collabora over its COOL WebSocket
  protocol (`/cool/<encoded WOPISrc>/ws`, then `load url=`, `textinput`,
  `save`, `closedocument`), as described in `wsd/protocol.txt` of the Collabora
  Online source.
- `scripts/collabora-up.sh`: starts `collabora/code` with `aliasgroup1`, TLS
  off and `--network host`.
- `.github/workflows/collab-spike.yml`: the `wopi-sim` job on every push (it was
  `collab-spike-wopi.yml`'s `simulator` job on `spike/collab-wopi` when these runs
  were made); the `collabora` job on `workflow_dispatch`, nightly, on a PR touching
  `dcp-wopi/**`, or when the head commit message contains `[collabora]` (a workflow
  can only be dispatched once it is on the default branch).

**Sources for Collabora's behaviour.** The Collabora SDK pages are behind an
anti-bot wall and were not readable. Behaviour was taken from Microsoft's public
WOPI REST pages (`Lock`, `GetLock`, `RefreshLock`, `Unlock`, `UnlockAndRelock`,
`PutFile`, `GetFile`, the concepts page) and from the Collabora Online source
(`wsd/WopiStorage.cpp`, `wsd/Storage.cpp`, `wsd/DocumentBroker.cpp`,
`wsd/CheckFileInfo.cpp`, `test/UnitWOPI*.cpp`, `wsd/protocol.txt`) as mirrored
at `CollaboraOnline/online.mirror@4e474be`. The mirror's `main` is newer than
the 26.04.5.2 build, so each claim below says whether the run confirmed it.

**The run.** Commit `3d213b0`, workflow run
[40068842245](https://github.com/tuna-os/gtk-office-suite/actions/runs/40068842245)
(job `collabora`, success). The Collabora job's command:

```
docker pull collabora/code:latest   # sha256:c7415b963e32eb0986a562583f154d36d3d7a461e1ddfb660a95e87094328acb
./scripts/collabora-up.sh           # --network host --cap-add MKNOD, aliasgroup1=http://127.0.0.1:18080,
                                    # --o:ssl.enable=false --o:ssl.termination=false
                                    # --o:storage.wopi.locking.refresh=10
DCP_WOPI_HOST_PORT=18080 DCP_WOPI_LOG=1 \
  cargo test -p dcp-wopi --test collabora_e2e -- --nocapture --test-threads=1
```

The same job was re-run on commit `c564cad` (which adds only the
`DCP_COLLABORA_WAIT_SECS` switch and this document): run
[40070090805](https://github.com/tuna-os/gtk-office-suite/actions/runs/40070090805),
both jobs green, the same four Collabora tests passed (a new lock string,
`cool-lock7C380847`, so a distinct execution).

`storage.wopi.locking.refresh` is set to 10 s so that a lock refresh is
observable in a short test; Collabora's default is 900 s (`Storage.hpp`).

Collabora's own documents are generated: a minimal ODT built in the test
(`tests/support/mod.rs`) whose one paragraph says `base`.

## Results

### Spec test 10: a WOPI client round-trips

Passed on the real Collabora:
`collabora_edits_and_saves_a_new_dcp_version` (run above). Asserted, not just
printed:

- `perm: edit` was granted, and `statechanged: .uno:ModifiedStatus=true`
  followed `textinput id=0 text=DcpWopiSpike42`.
- After `save dontTerminateEdit=0 dontSaveIfUnmodified=0` the store had 2
  versions. The new one has `parents == [<the version Collabora loaded>]` and
  `created_by.principal == "@alice:test"`.
- The saved file is an ODF package whose `content.xml` contains
  `DcpWopiSpike42` (read back from the store and parsed with the `zip` crate).
- The lease while the document was open was `held`, `token` equal to Collabora's
  lock string, `base_version` equal to the version it loaded, `duration_ms`
  1 800 000.
- On closing the WebSocket Collabora sent `UNLOCK` with the same string, the
  host answered 200, and the lease read `released`.

### What Collabora actually sent

The host's request log for one session (test 1, token redacted; times from the
run's log):

| # | Request | Headers Collabora set | Status |
|---|---|---|---|
| 0 | `GET /wopi/files/{id}?access_token=…` (CheckFileInfo) | `User-Agent: COOLWSD HTTP Agent 26.04.5.2`, `X-COOL-WOPI-ServerId` | 200 |
| 1 | `GET /wopi/files/{id}/contents?access_token=…` (GetFile) | same | 200 |
| 2 | `POST /wopi/files/{id}` | `X-WOPI-Override: LOCK`, `X-WOPI-Lock: cool-lockAC647357`, `Content-Length: 0` | 200 |
| 3 | `POST /wopi/files/{id}/contents` (PutFile) | `X-WOPI-Override: PUT`, `X-WOPI-Lock: cool-lockAC647357`, `X-COOL-WOPI-IsModifiedByUser: true`, `X-COOL-WOPI-IsAutosave: false`, `X-COOL-WOPI-IsExitSave: false`, `X-COOL-WOPI-Timestamp: 2026-10-24T06:20:03.369Z`, `Content-Length: 8234` | 200 |
| 4 | `POST /wopi/files/{id}` on WebSocket close | `X-WOPI-Override: UNLOCK`, `X-WOPI-Lock: cool-lockAC647357` | 200 |

What the log establishes, each of which is a measurement and not a reading of
the source:

- **Order: CheckFileInfo, GetFile, LOCK, PutFile, UNLOCK.** The lock is taken
  after the download. (A comment in `DocumentBroker::lockIfEditing` says
  "before downloading"; this build did not.) The simulator uses this order.
- **The lock string is Collabora's** (`cool-lock` plus 8 hex digits, per
  `LockContext::initSupportsLocks`), the same for the whole session, and the
  host stored it as the lease `token` unchanged.
- **The timestamp header is `X-COOL-WOPI-Timestamp`** (the source also sends
  `X-LOOL-WOPI-Timestamp` to a server it believes is legacy; the host honours
  both). Its value was the exact `LastModifiedTime` string from CheckFileInfo,
  and after a save the string from the `PutFile` response. **Verified.**
- **`X-WOPI-Editors` was not sent.** The token travels in the query string (the
  log does not record an `Authorization` header).
- **Refresh is `LOCK` again with the same string**, not `REFRESH_LOCK`. With the
  period set to 10 s the host saw a second `LOCK` with the identical string and
  status 200 about 10 s after the first, and the lease's binding timestamp
  moved. Over the refresh test Collabora sent **no** `GET_LOCK`, `REFRESH_LOCK` or
  `UNLOCK_AND_RELOCK` (the test asserts this; it is a single session). **Verified.**
- **Save latency in this run:** `PutFile` arrived about 0.18 s after the `save`
  command was sent (kit already warm; times from two log lines on one machine).
  One run, no repeat.

### Conflict, 409 and the timestamp header

`collabora_save_after_a_native_publish_is_refused_as_a_conflict` (passed):

1. Collabora opens version v1 and takes the lock.
2. A **native** DCP writer publishes a child of v1. The store accepts it (the
   lease is advisory, spec lease rule 6).
3. Collabora saves, sending the stale `X-COOL-WOPI-Timestamp`.
4. The host answers **`409` with a JSON body `{"COOLStatusCode":1010,"Message":"document changed in storage"}`
   and no `X-WOPI-Lock` header**, and publishes nothing (2 versions in the store).
5. Collabora reported `commandresult: save … "Conflict: Document changed in
   storage"` and `error: cmd=storage kind=documentconflict` to the client, then
   sent `UNLOCK` (the host answered 200).

The absence of `X-WOPI-Lock` on that 409 is deliberate and matters: the source
of `handleUploadToStorageResponse` reads a 409 that carries `X-WOPI-Lock` and no
`COOLStatusCode` as a **lock mismatch** ("Locked by another editor"), not a
changed document. Marked **verified** for the 1010 form; the lock-mismatch form
was checked against the host only, not against Collabora (see Not verified).

### Lock semantics: does the lease map cleanly?

Yes for acquire, refresh and release, with one fixed number. Tested on the wire
by `tests/host_semantics.rs` against the Microsoft pages:

| WOPI | Host behaviour | DCP lease |
|---|---|---|
| `LOCK`, unlocked | 200 | `write_lease` held, `token` = the string, then read back (lease rule 3) |
| `LOCK`, same string | 200, treated as refresh | same record re-stamped: binding timestamp moves, holder and base kept |
| `LOCK`, other string | 409, `X-WOPI-Lock: <current>`, no change | lease untouched |
| `GET_LOCK` | 200, `X-WOPI-Lock: <current>` or the empty string | `lease()` |
| `REFRESH_LOCK` | as `LOCK` same string; mismatch or unlocked: 409 with current or empty | re-stamp |
| `UNLOCK` | match: 200; else 409 with current or empty | `released` |
| `UNLOCK_AND_RELOCK` (`LOCK` + `X-WOPI-OldLock`) | match: swap the string, 200; else 409 | one record replaces the other |
| lock empty or missing | 400 | n/a |
| lock longer than 256 | 400 | spec 4.3 token limit |
| viewer or commenter | 403 | roles, spec 5 |
| expired | an expired lease is an unlocked file (`GET_LOCK` returns the empty string) | lease rule 2 by the binding clock |

Sixteen concurrent `LOCK`s with different strings (two tokens) gave exactly one 200.

### Conformance journeys over WOPI

`WopiClientStore -> HTTP -> WopiHost -> dcp_core::memory`, `tests/conformance_over_wopi.rs`:

```
  PASS    convergence_on_one_head (spec test 1)
  SKIPPED second_head_shown_not_hidden (spec test 2): binding lacks arbitrary_parents
  PASS    corrupt_file_rejected (spec test 3)
  PASS    truncated_upload_never_published (spec test 3)
  SKIPPED lease_exclusion_and_readback (spec test 4): binding lacks arbitrary_lease_writes
  SKIPPED lease_expiry_releases (spec test 5): binding lacks arbitrary_lease_writes
  PASS    roles_enforced_at_store (spec test 6)
  SKIPPED revocation_hides_new_content (spec test 7): binding lacks grant_revoke, revocation
  SKIPPED unknown_algorithm_falls_back_to_versions (spec test 8): binding lacks updates
  SKIPPED update_stream_gap_recovery (spec test 9): binding lacks updates
  4 passed, 0 failed, 6 skipped
```

The skipped list is pinned by `assert_skipped_exactly`, so a skip cannot appear
or vanish unnoticed. A mutant (a backing store that hides its newest version)
makes `convergence_on_one_head` fail through the WOPI path, so the path does not
hide a store's faults. Why each skip:

| Skipped | Reason |
|---|---|
| test 2, second head | A merge needs two parents; a WOPI save has one. The fork half works: see below |
| tests 4 and 5 | The journeys write leases with a holder, session, base version and duration. WOPI carries the lock string only. Lock exclusion and expiry **are** tested on the wire, in `host_semantics.rs` and `client_store.rs` |
| test 7 | No grant or revoke in WOPI |
| tests 8 and 9 | No update stream in WOPI |

Fork from an older version works through the extension below:
`saving_a_child_of_an_older_version_forks` (a native writer publishes a child of
v1; the simulator then saves a child of v1; two heads) and
`collabora_timestamp_conflict_is_reported_as_1010`.

`history` is declared **true** in these capabilities, and that needs an
explanation: plain WOPI has no way to list versions or fetch an old one. The
host offers two vendor extensions (`DcpVersions` in CheckFileInfo only when
`?dcp_history=1` is asked for, and `GetFile?dcp_version=<id>`), so the
simulator, which is a DCP store, can read the graph. **A stock WOPI client,
Collabora included, never uses them and cannot.**

### Test counts

On CI (`simulator` job, run
[40068842245](https://github.com/tuna-os/gtk-office-suite/actions/runs/40068842245), success):
2 unit tests, 15 in `host_semantics`, 9 in `client_store`, 3 in
`conformance_over_wopi`: 29 passed, 0 failed. On the `collabora` job: 4 passed
(`collabora_edits_and_saves_a_new_dcp_version`,
`collabora_opens_a_viewer_read_only_and_never_locks_or_saves`,
`collabora_refreshes_the_lease_with_the_same_lock_string`,
`collabora_save_after_a_native_publish_is_refused_as_a_conflict`), 0 failed.

Two earlier Collabora runs failed and were fixed; they are recorded here
because the failures are findings. Run 40063006409 was cancelled by a newer
push before it started. Run
[40064043987](https://github.com/tuna-os/gtk-office-suite/actions/runs/40064043987)
failed at the WebSocket upgrade, and run
[40066475503](https://github.com/tuna-os/gtk-office-suite/actions/runs/40066475503)
failed in the second test:

- Collabora 26.04 **rejects a WebSocket upgrade without an `Origin` header**:
  the client got `403 Forbidden` and Collabora logged "Rejecting WebSocket
  upgrade due to disallowed origin". The browser sends its own origin; a
  non-browser driver must send Collabora's own URL (the upgrade is then `101`).
- `closedocument` sent while the WebSocket stays open did not make Collabora
  unlock: no `UNLOCK` arrived in the 60 s we waited. The `UNLOCK` was sent when
  the connection finally closed, which was after the test had shut the host down,
  so it failed (`Un-successful WOPI::Unlock with HTTP status 0`). A real
  `Close` frame, as a browser tab sends, produced `UNLOCK` within a second.
  **Unlock happens when the last editing session disconnects.** Collabora's
  document broker is keyed by the `WOPISrc` without the token
  (`docKey http%3A%2F%2F127.0.0.1%3A18080%2Fwopi%2Ffiles%2Fdoc1` in its log): a
  second session on the same file id, opened while the first broker was
  unloading, was refused with `error: cmd=load kind=docunloading`.

`collabora_e2e` fails loudly with no Collabora (checked in the `simulator`
job: `Collabora Online is not reachable at …`) and skips only when
`DCP_SKIP_COLLABORA=1`, printing `SKIPPED collabora_e2e: …`.

## Roles

`CheckFileInfo` carries the role through fields Collabora reads:

| DCP role | `UserCanWrite` | `UserCanOnlyComment` | Collabora (observed for a viewer) |
|---|---|---|---|
| owner, editor | true | false | `perm: edit`, `filemode readOnly:false` |
| commenter | false | true | not run |
| viewer | false | false | `perm: readonly`, `filemode readOnly:true`; no `LOCK`, no `PutFile`, no lease; typing and `save` changed nothing (store: 1 version) |

Owner and editor are the same in standard WOPI fields. The extension `DcpRole`
carries the difference to the simulator only.

## What was lost in the mapping

| DCP | Through WOPI |
|---|---|
| Version graph | CheckFileInfo describes one file. `Version` is an opaque string of the head. History needs the extension above |
| More than one parent (merge) | A save has one parent. Inexpressible |
| Two heads shown, not hidden | Cannot be shown by a WOPI client. This host opens such a document read-only (`UserCanWrite` false, `DcpHeads` lists them) |
| `message` on a version | No field. The host writes `WOPI PutFile (autosave=…, exit_save=…)` |
| Authorship of co-editors | `X-WOPI-Editors` was not sent. `created_by` is the principal of the token that sent the save |
| Lease `holder`, `base_version`, `duration_ms` | The client sends a lock string. The host fills the rest from the token and its own settings |
| Lease break by an owner (rule 5) | No WOPI operation. The owner's `LOCK` with another string is a 409 like anyone's |
| Grant, revoke | None |
| L2 updates, L3 presence | None. Collabora has its own co-editing and presence inside one server, invisible to DCP |
| Binding clock | None in WOPI; the extension `DcpNowMs` carries it to the simulator |

## Spec corrections

For spec 8.2 (and the lease section it touches). Status says how each was
established.

1. **PutFile parent (8.2 table).** "The version the editor opened" has no wire
   carrier. A host MUST record, per access token, the version last returned by
   `GetFile` (or written by `PutFile`) and use it as the parent. A `PutFile`
   with no lock from a session that has not read the head SHOULD be refused.
   *Established by the design; the round trip confirms the parent was right.*
2. **Timestamp conflict detection (8.2 last paragraph).** "To be confirmed in
   Spike D": header `X-COOL-WOPI-Timestamp` (legacy `X-LOOL-WOPI-Timestamp`),
   value the exact `LastModifiedTime` string last given (CheckFileInfo, then the
   `PutFile` response), omitted on a forced save. Conflict response: `409` with a
   JSON body `{"COOLStatusCode":1010}` and **no** `X-WOPI-Lock` header.
   *Verified on 26.04.5.2.* A forced save (`savetostorage force=1`) is sent
   without the header; it was not run against Collabora.
3. **Call order (8.2).** The spec lists operations without order. Measured:
   CheckFileInfo, GetFile, LOCK, PutFile, UNLOCK. The lease's `base_version`
   must come from the session's read, not from the head at the time of `LOCK`.
   *Verified.*
4. **Which lock operations Collabora uses (8.2 lists five).** Only `LOCK`
   (acquire and refresh, same string), `UNLOCK` and `PutFile`. `GET_LOCK`,
   `REFRESH_LOCK` and `UNLOCK_AND_RELOCK` were never sent; a host still has to
   answer them for other clients. *Verified for one session of 26.04.5.2.*
5. **Lease duration (4.3).** WOPI fixes the lifetime at 30 minutes and a refresh
   resets it to 30 minutes. A WOPI host sets `duration_ms` to 1 800 000. Collabora
   refreshes every 900 s (`storage.wopi.locking.refresh`), which is exactly half
   the duration, so lease rule 4's "before half" should read "at or before half".
   *Refresh period from the source default; the 10 s variant was run.*
6. **Token length (4.3).** The spec limit is 256 characters; WOPI allows 1024 and
   a host says so with `SupportsExtendedLockLength`. A DCP host MUST report it
   `false` and answer a longer lock with 400. Collabora's strings are 17
   characters. *Host test.*
7. **Locks are not owned (4.3 rules 3 and 5).** A WOPI lock is authorised by the
   string alone; a different token can refresh or release it (Microsoft's
   concepts page). The DCP `holder` records who took it, and an owner's "break"
   has no WOPI call. Breaking must be done at the store; the WOPI client then
   finds out from the 409 on its next `PutFile`. *Host test.*
8. **Lease on a document with no version (4.3).** `base_version` is required
   when held, and an empty document has none. This spike wrote `"none"`. The
   schema should allow omission when the document has no version.
9. **A save after the lease expired (10, 4.3 rule 6).** The spec says to save as
   a version anyway. WOPI forbids it: a `PutFile` carrying a lock string on an
   unlocked file is a 409 with an empty `X-WOPI-Lock`, and the host here follows
   that. A WOPI session therefore cannot "save anyway" after expiry; it must
   re-lock first. Either 8.2 grants a host an exception that is not WOPI, or the
   row in section 10 does not hold for WOPI. *Host test; Collabora's reaction
   not run.*
10. **More than one head (6, 8.2).** 8.2 is silent. A WOPI client cannot show a
    conflict. Recommended: serve the document read-only and list the heads in an
    extension, as this host does.
11. **Role mapping (5, 8.2).** The `commenter` role maps to `UserCanOnlyComment`.
    Owner and editor cannot be told apart in standard fields. *Viewer
    observed; commenter not run.*
12. **File name (4.1 and CheckFileInfo).** Collabora rejects a `BaseFileName`
    containing `/`, and picks the application from the extension. `title` has
    no extension requirement, so the host derives one from the MIME type.
13. **Access tokens for conformance test 10.** The row should say how the token
    is minted (the host's registry here) and that Collabora's document broker is
    keyed by the WOPISrc, so a file id must not be reused while a session on it
    is closing.

## Not verified

- **Two Collabora sessions on one document**, and what `created_by` and the
  lock look like then. One session only.
- **Autosave and exit-save `PutFile`** (`IsAutosave` or `IsExitSave` true) and
  how many versions they make. Not exercised. Version churn against Matrix's
  3 events/s is the question and is open.
- **Collabora's forced save after a conflict** (`savetostorage force=1`), and
  the fork it would make through this host. The fork is tested with the
  simulator, not with Collabora.
- **A lock mismatch against Collabora** (a 409 with `X-WOPI-Lock` on `LOCK` or
  `PutFile`): only the host side was checked. The source says the session goes
  read-only on a failed `LOCK`; not observed.
- **Lease expiry while Collabora is editing**, and `UNLOCK` retry after a
  transient failure (source only).
- **HTTPS, WOPI proof keys (`X-WOPI-Proof`).** The host validates the access
  token only. Do not deploy this host as it is.
- **The host over the Matrix binding.** Every run is over `dcp_core::memory`
  or the small in-test store in `tests/support/mod.rs` (it has a corruption knob
  the memory store lacks, so the focused tests were not moved to
  `MemoryStore`).
- **Other WOPI clients** (OnlyOffice, Microsoft 365), other Collabora builds,
  large files, concurrent saves.
- **The Collabora SDK documentation**: unreadable (anti-bot). Nothing here is
  cited to it.
- **`commenter` through Collabora.**
- The simulator reproduces the measured call order; it is not Collabora and
  proves only that the host answers the way the Microsoft pages say.

## Reproducing

```
# simulator tests (no Collabora)
cd tools/collab-spike
cargo test -p dcp-wopi --lib --test host_semantics --test client_store --test conformance_over_wopi

# real Collabora (docker or podman)
DCP_WOPI_HOST_PORT=18080 ./scripts/collabora-up.sh
DCP_WOPI_HOST_PORT=18080 DCP_WOPI_LOG=1 cargo test -p dcp-wopi --test collabora_e2e -- --nocapture --test-threads=1
./scripts/collabora-up.sh down

# explicit skip (prints SKIPPED collabora_e2e)
DCP_SKIP_COLLABORA=1 cargo test -p dcp-wopi --test collabora_e2e -- --nocapture
```
