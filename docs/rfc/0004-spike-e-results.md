# RFC-0004 Spike E: OCM and WebDAV binding results

Date: 2026-10-24 · Tracks: [RFC-0004](0004-interoperable-document-collaboration.md), spec [section 8.3](../DOCUMENT-COLLABORATION-SPEC.md) · Code: [`tools/collab-spike/dcp-webdav/`](../../tools/collab-spike/dcp-webdav/), [`scripts/nextcloud-*.sh`](../../tools/collab-spike/scripts/), workflow [`collab-spike.yml`](../../.github/workflows/collab-spike.yml)

**These results come from two Nextcloud builds, 34.0.5 and 35.0.2 (the official `nextcloud:*-apache` images, pinned by digest below), with PostgreSQL 16, on GitHub-hosted 2-core runners, over plain HTTP on one docker network, driven by our own tests. Every observation is of those builds and those runs. They are not a compatibility statement about Nextcloud, ownCloud, or OCM servers in general: no other OCM implementation (CERNBox, OpenCloud, Seafile, the OCM-stub reference) was run, nothing ran over HTTPS, and the Open Cloud Mesh *sender* role was Nextcloud's alone.** Anything not run is listed under [Not verified](#not-verified).

CI run [**40139702356**](https://github.com/tuna-os/gtk-office-suite/actions/runs/40139702356), commit **cee909b9e7dc9bd2387683f09c4b544bf556a67f** (all four jobs green; the commit that adds this document changes nothing else), is the run every number below comes from unless another is named. Earlier runs on the same branch (`40136040072`, `40136928500`, `40137699998`, `40138993440`) are cited where a result varied between runs; the artifacts of every run are on the Actions page for a few weeks only, so the logs quoted here should be re-run, not re-read, if they have expired. Artifacts: `webdav-measurements-34.0.5`, `webdav-measurements-35.0.2` (test logs, `measure.jsonl`, both servers' logs) and `nextcloud-probe-34.0.5`, `nextcloud-probe-35.0.2` (the raw protocol probe: every request and response).

## Recommendation

**The spec's section 8.3 outline is rejected as written. A binding over Nextcloud WebDAV with OCS sharing, designed differently, is GO for L0 and L1 for `viewer` and `editor` roles, on one server and across an OCM federation, with the conditions below. L2 and L3 are not offered. `commenter` and, across a federation, `owner` cannot be expressed.**

The outline said: a version is a file state at an ETag; publishing is a conditional `PUT` and a `412` means a second head; a lease SHOULD be a WebDAV `LOCK`. Measured:

| Outline claim | Result |
|---|---|
| A version is a file state at an ETag | **Rejected.** A file replaced about 0.2 s after it was created kept its ETag in **16 of 20 trials with different-size content and 16 of 20 with same-size content on 34.0.5 (18 and 18 on 35.0.2)**; in every one of those 80 trials the ETag moved **exactly when the file's whole-second `getlastmodified` moved** and in none did one move without the other. A writer holding the old ETag was accepted by `If-Match` in exactly the trials where the ETag had not moved. (Earlier runs: 17 and 20 of 20, 18 and 16 of 20.) |
| Publishing is a conditional `PUT` (`If-Match`) | **Rejected for overwrites.** Two writers presenting the same ETag at once: **0 of 20 rounds had two winners, 1 had one, 19 had none** (`423` for both; both builds in the final run; over six batches 1 to 6 of 20 rounds had a winner and 14 to 19 had none). It is safe and not live, and the ETag it relies on is the unreliable part (above). |
| A `412` means a second head, the program MUST NOT retry blindly | **Rejected as stated.** A `412` or `423` carries no content and no place to put the loser's bytes: the second head does not exist anywhere unless the writer has already put it somewhere. What works is the converse: every version is its own created file, a conflict is two files with the same parent, and the only conditional the binding needs is create-if-absent. |
| Create-if-absent (`If-None-Match: *`) | **Atomic: never two winners.** Six batches of 20 rounds of two writers of one account creating one name at once (both builds, three runs): in 115 of 120 rounds exactly one got `201` and the other `423` (once `412`), in 5 rounds both got `423`, in none did both succeed. With two *different users* creating the next lease record the same race had a winner in 6 to 12 of 20 rounds (eight batches) and `423` for both in the others (0 double wins in every batch). |
| A lease SHOULD be a WebDAV `LOCK` where the server advertises it | **Rejected.** Core Nextcloud has no lock provider (`LOCK` is `501` when the bundled `files_lock` app is disabled). With it, a lock is authorised by its token alone (another user unlocked it), changes the file's ETag, is not seen by a federated writer, and `LOCK` over a share token is `500`. A lease built on **create-only records in a log folder** passed the lease journeys on both servers. |
| A DCP principal is an OCM address | **Holds** (`bob@nc-b`), with the receiving server's accept step and with Nextcloud writing `alice@http://nc-a` (a scheme) in the notification. |
| L2 and L3 need a channel OCM does not define | **Confirmed**; WebDAV has none either. Not offered. |

The deciding result is the conformance run (spec section 12 written once, [`dcp-conformance`](../../tools/collab-spike/dcp-conformance/)):

| Topology | Passed | Failed | Skipped (capability stated) |
|---|---|---|---|
| alice, bob, carol on one Nextcloud (34.0.5 and 35.0.2) | **9** | 0 | 2 (`unknown_algorithm_falls_back_to_versions`, `update_stream_gap_recovery`: no L2) |
| alice on Nextcloud A; bob and carol on Nextcloud B, through OCM shares they accepted (34.0.5 and 35.0.2) | **8** | 1 (`lease_expiry_releases`, a 4 s lease: failed in 7 of the 8 federated runs of the final code, passed once; passes at 20 s in every run, see below) | 2 (same) |

The conditions:

1. **A document is two folders, not one file** (`<id>.v`, append-only, and `<id>.l`, the lease log); the version graph is files, the roles are the shares' permission masks. The spec must say so ([Proposed 8.3](#proposed-83)).
2. **`parents` live in a sidecar JSON.** Custom WebDAV properties exist but belong to the file and the *user*, not to a version (measured).
3. **The only atomic primitive is creating a name that is free.** Lease and version publication use nothing else.
4. **Authorship is not authenticated by WebDAV.** `created_by` is whatever the writer wrote. An editor wrote a version naming the owner and the server accepted it; only Nextcloud's Activity app (asynchronous) says who really created the file. Spec section 11's "authorship comes from the binding's signatures" has no carrier here.
5. **A lease shorter than about 10 s is not meaningful**, over a federation 20 s. The clock is whole seconds, a request is 50 to 120 ms on one server and more through two, and the settle interval is 500 ms. (The 4 s journey passed on one server in 8 of 8 runs and over a federation in 1 of 8; 20 s passed in all.)
6. **Across a federation the receiver's role is read or write.** The receiving server shows every federated mount the share right, and a create-only grant looks like full write. The sender still enforces the real mask.
7. **A federated writer is not stopped by a lock** the sender's local user holds (Sabre lock: not stopped; `files_lock` user lock: `500`).

What this does not show: other servers, HTTPS, HTTP message signature *verification* by a receiver we wrote (the helper records, it does not verify), the OCM code flow, invites, Nextcloud Office or Collabora editing a DCP document, load, or anything over a real network.

## Method

`tools/collab-spike/` is the standalone Cargo workspace of the other spikes. `dcp-webdav` adds:

- `WebdavStore`, a `dcp_core::DocumentStore` over WebDAV (`/remote.php/dav/files/<user>/`) and OCS sharing (`/ocs/v2.php/apps/files_sharing/api/v1/`). It has no `PROPPATCH`, no `LOCK`, no versions-endpoint code on purpose: those were measured (below) and rejected as the carrier.
- `dav`, `xml`: an HTTP client that returns status, headers and body, and a namespace-aware multistatus reader (unit test).
- `clock::ServerClock`: the server's clock to the millisecond, found by locating the instant its `Date` header ticks over (the header is whole seconds).
- `ocm::OcmReceiver`: a receiving OCM server with no file store: it serves discovery, accepts `POST /ocm/shares` and `/ocm/notifications`, and records every request verbatim. `accept_pending_remote_shares` drives a Nextcloud receiver's own OCS to accept what arrived. There is no sender (Nextcloud was the only one).

Everything runs in `.github/workflows/collab-spike.yml` (the jobs `webdav-probe` and `webdav`; the spike ran them as `collab-spike-e.yml`, merged at integration):

```
# the two servers (scripts/nextcloud-up.sh a b): containers nc-a, nc-b + a PostgreSQL each,
# one docker network, /etc/hosts entries so the runner and the containers use the same URL
NEXTCLOUD_IMAGE=docker.io/library/nextcloud:34.0.5-apache@sha256:1f9212adecae32010cdbf137d7e0db78d08c696dc3e77f1eddcbf902d10209b4
NEXTCLOUD_IMAGE=docker.io/library/nextcloud:35.0.2-apache@sha256:89af7ee7a96ff5e9668280566209268ae6b2d02327f6ef6d56f6d68228cf9431
POSTGRES_IMAGE=docker.io/library/postgres:16-alpine@sha256:f4610795587492cd4fb887c2dc3de301b813278bb6f616992061141b5f15ac36
# occ settings: allow_local_remote_servers=true (private-network federation), brute-force
# protection and rate limiting off, users alice bob carol dave (dave only for the receiver)
scripts/nextcloud-up.sh a b
cargo test -p dcp-webdav --test conformance_over_nextcloud --test conformance_over_ocm \
     --test nextcloud_semantics --test federation --test lease_timing -- --nocapture --test-threads=1
scripts/nextcloud-probe.sh            # raw requests and responses, no assertions
```

Two methodological points that changed results and are worth knowing:

- **The account password is slow.** Every request authenticated with the account password cost about **350 ms** (PROPFIND, median of 15: 410 ms on 34.0.5 and 351 ms on 35.0.2 in the final run) because Nextcloud verifies the password hash each time. An **app password** (`GET /ocs/v2.php/core/getapppassword`) cost **51 ms** (52 ms on 35.0.2); an unauthenticated `GET /status.php` 16 ms (final run; the account-password figure was 294 to 410 ms across runs). The tests use app passwords; the latency test measures both. Before that change the first conformance run took 140 s and now takes 60 s.
- **`occ` changes are invisible to a running Apache** until it restarts (the enabled-apps list is cached in the server's own memory). The probe restarts `nc-a` after disabling `files_lock`; a run that did not showed `LOCK` still working. (This is a harness fact, not a Nextcloud finding.)

Test environment per CI run: `ubuntu-24.04` GitHub-hosted runner (2 cores), docker, Rust stable, debug build with line tables. Servers and tests share the machine; every latency below includes that contention and none includes a network.

## Results

### The document layout, and why it is not "one file and its ETag"

```
DCP-<12 hex>.v/        append-only: shared create-only (permissions 5) with editors
    meta.json              DCP metadata (4.1), created once
    <vid>.bin              the file of one version
    <vid>.json             the DCP version object (4.2): parents, file reference, created_by
DCP-<12 hex>.l/        the lease log: shared with update and delete (15)
    00000000.json          one lease record per file; the highest number is the lease
```

Alternatives measured and why they lost:

- **One file, version = state at an ETag.** Rejected by the ETag measurements above.
- **One file plus Nextcloud's versions endpoint** (`/remote.php/dav/versions/<user>/versions/<fileid>/`). It is a history of *replaced states keyed by Unix seconds*, not of saves: six overwrites in 1.7 to 1.8 s left **2 or 3 listed versions** (3 in the final run, 2 in two earlier ones: roughly one per second in which a state was replaced); the current state is not listed; each entry has `getlastmodified`, `getcontentlength`, `getetag` (the same number as its name) and **`nc:version-author`, the authenticated writer (`alice`)**, a real authenticated author. It cannot be written to (`PUT` is `403`) and carries no parents. List latency 44 to 63 ms. It is read-only evidence of authorship for the old states; it is not a version graph.
- **`parents` in a custom WebDAV property** (`PROPPATCH`). `PROPPATCH` of an arbitrary namespaced property works (`207`, read back by `PROPFIND`). But the property **belongs to the file, not to a state of it** (still there after the file was overwritten, so the old parents now describe new bytes), is **absent on the versions endpoint**, and is **per user**: bob, who may edit the file, did not see alice's property, and his own `PROPPATCH` (accepted `207`) was invisible to her. A property cannot carry a version's parents between principals. Test: `custom_properties_are_per_user_and_per_file`.
- **A sidecar `<vid>.json`**, written after the bytes (version rule 2): chosen. A reader lists `*.json`; an orphan `.bin` (an upload that was verified and failed, or a crash) is never listed.

The cost of the chosen layout is requests: a version listing is one `PROPFIND` plus one `GET` per version (8 in flight): **2 versions 127 ms, 10 versions 306 ms, 30 versions 754 ms** (34.0.5; 138, 349, 836 ms on 35.0.2, final run; the other runs were within 25% of these).

### Writing and verifying a version

`publish_version` is: `PUT <vid>.bin` with `If-None-Match: *`; `GET` it back and compare bytes (version rule 2); `PUT <vid>.json` with `If-None-Match: *`. Nextcloud's own checks do not help with step 2:

- **A body shorter than its `Content-Length` is refused.** A `PUT` that declared 1000 bytes and sent 500, then half-closed, got `400 Bad Request` and **no file existed afterwards**. Nothing partial is stored. (`a_cut_off_upload_is_not_stored_and_checksums_are_not_checked`)
- **`OC-Checksum` is stored, not checked.** A `PUT` with `OC-Checksum: SHA256:000…0` over 11 bytes got `201` and `PROPFIND oc:checksums` returned the bogus value. The checksum is a client claim.
- So the upload answers `201` to whatever arrived; the journey's truncating upload fault (half the bytes, sent whole-framed) is caught only by the read-back. The orphan `.bin` remains: a create-only editor cannot delete its own upload (`DELETE` `403`).

Latency, medians of the final run (34.0.5; 35.0.2 in brackets; the same runner, so a difference between builds is noise: the order of the two builds swapped between runs): `publish_version` end to end for a small file **297 ms** (340; 271 to 340 ms over all runs), a 5 MiB file **414 ms** to publish and **65 ms** to fetch (495, 71), `create_document` with a first version **552 ms** (625), `grant` of one editor (two shares) **316 ms** (358), a lease read **52 ms** (56), a lease write **104 ms** (123), a 1 KiB `PUT` **101 ms** (112), a `GET` **55 ms** (58), a `PROPFIND` of depth 0 **51 ms** (52).

### The lease

**`LOCK` first**, as the outline said (test `webdav_lock_semantics`, the probe's `lock` and `nolockapp` sections):

- `OPTIONS` `Allow` lists `LOCK, UNLOCK` and `DAV:` includes class `2`, **only while the bundled `files_lock` app is enabled** (it is enabled by default in both images). With `occ app:disable files_lock` (and a server restart): `DAV:` has no `2`, capabilities lose `files.locking`, and `LOCK` is **`501 Not Implemented`: "There was no plugin in the system that was willing to handle this LOCK method"**, for the plain and the `X-User-Lock` form alike. There is no lock provider in core.
- With it: `LOCK` exclusive/write with `Timeout: Second-60` is `200` with a `Lock-Token` header and an `activelock` body; the lock's `d:owner` is **whatever the client wrote** (`dcp-spike`); `nc:lock-owner` is the authenticated user (`alice`), `nc:lock-owner-type` `2`, `nc:lock-timeout` `60`, `nc:lock-time` whole seconds.
- Another user's `PUT` and `LOCK` are `423`; the holder's own `PUT` without the token is `423`; with `If: (<token>)` it is `204`. Refresh (`LOCK` with `If` and a new `Timeout`) is `200`. A `Second-2` lock expired: a `PUT` 3.5 s later was `204`.
- **The token alone authorises.** bob `UNLOCK`ed alice's lock with her token: `204`. Same as WOPI's lock string (spec 8.2 rule 5).
- **Taking a lock changes the file's ETag** (`"5ab8…"` to `"6add2c82b2022"`, a different format), so a lock breaks an `If-Match` chain.
- `LOCK` on a name that does not exist creates an **empty file** (`201`, `getcontentlength` 0).
- `X-User-Lock: 1` (the `files_lock` form the desktop client uses): `nc:lock-owner-type` `0`, `nc:lock-owner` the authenticated user, `nc:lock-token` `files_lock/<uuid>`, `nc:lock-timeout` `-60` (sign not explained); another user's `PUT` and `UNLOCK` are `423`, the owner's `UNLOCK` is `200`.

None of this is a DCP lease: no holder principal and session that other principals can read as DCP objects, no `base_version`, no renewal counter, no `duration_ms` in the 1 s to 1 h range DCP states (the timeout is the client's request, honoured), and cross-user `UNLOCK` by token. It is also **not visible through a federation** (below).

**The lease log** is what the store uses: lease record *n* is the file `<n:08>.json` in `<id>.l`; a writer that last read *n* creates *n + 1* with `If-None-Match: *`; `201` means it won, `412` or `423` is `StoreError::Conflict`. A reader that holds record *n* asks for *n + 1* and so on until `404`: **one request (about 52 ms)** when nothing changed, a `PROPFIND` and a `GET` the first time. Records are never changed or deleted (the growth, one file per write, is a cost: a holder renewing every 15 minutes writes about 100 files a day per document; compaction by the owner is not implemented).

- Under real concurrency (20 rounds, two handles of two different users both having read the same record, both creating the next): **0 rounds had two winners; 12 had one and 8 none on 34.0.5, 6 and 14 on 35.0.2** (the earlier runs gave 9 to 10 winners; the losers, and in the no-winner rounds both writers, got `423`); every loser got `StoreError::Conflict`. Safe, not live: the lease rules' read-back and a retry cover it. Test `lease_log_is_a_compare_and_set_under_concurrency`.
- The lease journeys passed: `lease_exclusion_and_readback` (spec test 4: both write over "no lease", exactly one wins, the owner breaks the editor's lease, the editor cannot) and `lease_expiry_releases` (spec test 5) with `racy_lease: false`, because the write is a real compare-and-set.
- **The clock is whole seconds.** `Date`, `Last-Modified` and `getlastmodified` are floored to the second. A 4 s lease renewed at 2 s cannot be judged to better than ±1 s from them. `ServerClock` finds the instant `Date` ticks (a 5 ms poll, 0.6 s once) and counts forward; each lease record carries that estimate as `at_ms`, accepted only if the file's `Last-Modified` agrees to within 2 s, and the reader then uses `at_ms`. Over 3 s of samples the estimate stayed inside the second the `Date` header named (test `the_clock_has_whole_second_resolution`). It is a calibrated estimate, not a reading of the binding's clock, and drift over a long run was not measured. A deployment must treat a lease shorter than the whole-second error as meaningless, spec lease rule 1.
- **Duration.** `lease_expiry_releases` fixes `D = 4000` and, after renewing at `D/2`, needs the 500 ms settle, two read-backs and two more reads to fit before `D` is up. On one server it passed in 8 of 8 runs (four runs, both builds; the margin is about 0.26 s by arithmetic from the measured read and write times, not measured). **Over a federation it failed in 7 of 8 runs**, and the same journey with `D` a parameter ([`tests/lease_timing.rs`](../../tools/collab-spike/dcp-webdav/tests/lease_timing.rs)) **passed at 20 s on both topologies and both builds in every run, passed at 4 s on one server every time, and at 4 s over a federation passed in some runs and failed in most (final run: failed on both builds).** It is the journey's clock, not the lease logic: see [Spec corrections](#spec-corrections).
- **Round 3: the one-instance run also uses 20 s.** At 4 s the one-instance journey's margin was about 0.26 s by arithmetic (above), and it was spent once: `lease_expiry_releases` on one Nextcloud 35.0.2 failed with "renewal did not keep the lease alive" in CI run 40180662063 (a slow runner; 34.0.5 and the other runs passed). `tests/common/mod.rs` now sets `Fixture::lease_ms` to `NEXTCLOUD_LEASE_MS` (20 s, what the federated factory already used) for both topologies; the journey and its assertions are unchanged.

### Roles, groups, revocation

Roles are the share permission masks of the two folders (test `share_permissions_are_the_roles`; the fourth column is what the recipient's `PROPFIND` of the mount root reports as `oc:permissions`, which is how the store reads a role back; the share masks themselves are the second and third):

| DCP role | `.v` mask | `.l` mask | Recipient's `oc:permissions` on `.v` | Measured |
|---|---|---|---|---|
| viewer | 1 | 1 | `SGDN` | create `403`, overwrite `403`, `MKCOL` `403`; reshare `403` |
| editor | 5 (read, create) | 15 (+update, delete) | `SGDNCK` | create `201`, create again `412`, **overwrite `403`, delete `403`, `MOVE` `403`** |
| owner | 31 | 31 | `SRGDNVCK` | reshare allowed |
| commenter | | | | **no equivalent**: comments are a separate app any reader may use |

**Create-only works.** An editor with mask 5 can add versions and cannot rewrite or delete one, whether it is this store or a hostile client: version rules 1 and 4 are enforced by the server. The same editor with mask 15 *can* overwrite history (`204`), which is why `.v` is not shared with 15. The roles journey also checked that `grant` by an editor or viewer is refused (`403` from OCS; the store checks `role()` first), and that a demoted viewer is refused again.

A **group** share of the same folder worked (probe `sharing` section: `shareType=1`, `permissions=15`; a member got `SGDNVCK` on the folder, `oc:share-types` listing both 0 and 1). The store addresses users only; group principals are not implemented.

**Revocation by unsharing** (`DELETE` on the share): the removed user's next request is `404` for the folder. The journey needed a mapping: a write by a removed member gets `404` ("the folder does not exist"), which the store reports as `Forbidden` (a document the caller already had an id for). Spec test 7's "removed members lose new content" holds as access control; it is **not** the encrypted-binding property the table assumes: the server holds the plaintext, and content already downloaded stays. `revocation: true` is declared with that stated.

### Authorship

`oc:owner-id` of a file a recipient created is the **folder's owner** (`alice`), not the uploader; no DAV property names the uploader. Nextcloud's Activity app does: `GET /ocs/v2.php/apps/activity/api/v2/activity/filter?object_type=files&object_id=<id>` returned `file_created` with `"user":"bob"`, after **56 ms** in the final run (44 to 253 ms over the runs; it is written after the request and its latency under load was not measured). So WebDAV carries no author, `Stamped.sender` in this binding is `created_by.principal` as the object claims it, and an editor **can claim to be the owner**: bob wrote a version naming alice, the server accepted it, `versions()` listed it with sender `alice@nc-a`, and the Activity API named bob (test `an_editor_can_claim_to_be_the_owner`). The viewer's forgery is another matter: a viewer cannot write at all (`403`), so the shared forgery journey passes for a different reason than on Matrix.

### Federation (OCM)

#### Discovery

`GET /.well-known/ocm` answers `200` on both servers, and so do `/ocm-provider`, `/ocm-provider/` and `/index.php/ocm-provider/` (identical, 816 bytes on 34.0.5); `/.well-known/ocm.json` and `/ocm` are `404`. Differences between the builds:

| | 34.0.5 | 35.0.2 |
|---|---|---|
| `apiVersion` / `version` | `1.0-proposal1` / `1.1.0` | `1.0-proposal1` / `1.1.2` |
| `capabilities` | `notifications`, `shares` | `notifications`, `shares`, **`exchange-token`, `http-sig`** |
| `tokenEndPoint`, `jwksUri` | absent | present (`/apps/cloud_federation_api/api/v1/access-token`, `.../jwks`) |
| `resourceTypes[0].protocols` | `{"webdav": "/public.php/webdav/"}` | same |
| `webapp`, `webapp-receive`, `webdav-receive` | absent | absent |
| `publicKey` | RSA, `keyId` `https://nc-a/ocm#signature` | same; the RFC 9421 signature below names an ECDSA key `…/ocm#ecdsa-p256-sha256-1` |

**OCM web-app access mode (the spec's `webapp` protocol): not offered by either stock image.** The discovery document lists only `webdav` under `protocols`; there is no `webapp` and no `webapp-receive`, so nothing here could open a shared document in the sender's own editor (iframe or blank) over OCM. The spec's `webapp` protocol also needs `exchange-token` (35.0.2 advertises it, 34.0.5 does not; whether 35's token endpoint works was not exercised). **That is a statement about these two images, not about Nextcloud 35:** the `cs3org/ocm-test-suite` capability registry (read, not run) marks `flow.webapp-share` sender and receiver `supported` for Nextcloud v35 and "requires Nextcloud v35" for v33 and v34, and its Nextcloud cookbook runs a JupyterHub sidecar on the sender side with its own images. Web-app sharing therefore appears to need an app or a service these images do not have; which one was not determined. Test `ocm_discovery_documents`.

#### What crosses the wire

A Rust receiver with no file store (`OcmReceiver`) was the target of a Nextcloud share (`shareWith=dave@http://dcp-host:<port>`; the `http://` is needed because Nextcloud otherwise tries only `https://`, measured: without it the share failed with "could not find dave@nc-b, maybe the server is currently unreachable or uses a self-signed certificate"). Recorded verbatim (test `a_receiver_with_only_the_share_secret`, `measure.jsonl` `ocm_share_creation_notifications`):

1. `GET /.well-known/ocm` from `Nextcloud-Server-Crawler/34.0.5` (and `/35.0.2`).
2. One `POST /ocm/shares` **per shared folder** (a DCP document is two shares): `Content-Type: application/json`, `Digest: SHA-512=…`, `Date`, and `Signature: keyId="https://nc-a/ocm#signature",algorithm="rsa-sha512",headers="(request-target) content-length date digest host",signature="…"` (draft-cavage HTTP signatures, not RFC 9421) on both builds. Body keys: `shareWith`, `shareType` (`"user"`), `name` (`DCP-….v`), `resourceType` (`"file"`, for a folder), `description`, `providerId`, `owner`, `ownerDisplayName`, `sharedBy`, `sharedByDisplayName`, `sender`, `senderDisplayName`, `protocol`. **`owner`, `sender`, `sharedBy` are `alice@http://nc-a`**: the address includes a scheme (the OCM spec's OCM Address MUST NOT).
3. `protocol` on 34.0.5: `{"name":"webdav","options":{"sharedSecret":"<15 chars>","permissions":"{http://open-cloud-mesh.org/ns}share-permissions"}}`: the deprecated "Option 1" shape; **the permissions are the literal property name, not a list**: they cross as a pointer to a WebDAV property the receiver may read, not as data.
4. **A receiver that advertises `http-sig` and `exchange-token` changes what a 35.0.2 sender sends** (test `a_receiver_that_advertises_the_newer_capabilities`): `Content-Digest`, `Signature-Input: ocm=("@method" "@target-uri" "content-digest" "content-length");created=…;keyid="https://nc-a/ocm#ecdsa-p256-sha256-1";tag="ocm"`, `Signature: ocm=:…:` (RFC 9421), and the protocol object in the newer shape `{"name":"webdav","webdav":{"permissions":["{http://open-cloud-mesh.org/ns}share-permissions"],"sharedSecret":"…","uri":"https://http://dcp-host:<port>/public.php/webdav/"}}`. **That `uri` is malformed (`https://http://…`)**: with an `http://` address the 35.0.2 sender builds an unusable absolute URI. 34.0.5 sends the same cavage form whatever the receiver advertises. No `requirements: ["must-exchange-token"]` was sent to our receiver.
5. **A change of permission is not notified.** Downgrading the share (15 to 1) produced **no request to the receiver**; the sender enforced it at once (the receiver's next `PUT` was `403`), but the receiver was not told. (OCM defines `SHARE_CHANGE_PERMISSION`; Nextcloud did not send it.)
6. **Removing the receiver is notified:** `POST /ocm/notifications` `{"notificationType":"SHARE_UNSHARED","resourceType":"file","providerId":"94","notification":{"sharedSecret":"…","message":"file is no longer shared with you"}}`, once per share; the secret stopped working at once (`401`).

#### What a receiver holding only the secret can do

The receiver's access is WebDAV at `/public.php/webdav/` on the sender (test `a_receiver_with_only_the_share_secret`):

- **`Authorization: Basic <secret>:` works (`207`); `Authorization: Bearer <secret>`, which the OCM spec's Resource Access section specifies, is `401`**, on both builds.
- The create-only mask (5) holds for a federated receiver: create `201`, create again `412`, **overwrite `403`, delete `403`**. The lease folder (15): create `201`, overwrite `204`.
- **`LOCK` through the secret is `500 Internal Server Error`** on both folders.
- A PROPFIND shows `oc:permissions` `RGCK` for the mask-5 share and **no** `ocm:share-permissions` property on the sender's side of the share.

#### Against the OCM specification

Read: `IETF-OCM.md` (`draft-ietf-ocm-open-cloud-mesh-07`) and `spec.yaml` (version 1.5.0) of `cs3org/OCM-API`, `develop` branch. What the two Nextcloud builds did differently from that text, as measured (nothing here is a judgement of which is right):

| Spec text | Nextcloud 34.0.5 / 35.0.2 |
|---|---|
| Discovery at `/.well-known/ocm`, `apiVersion` such as `"1.5.0"`, `endPoint`, `resourceTypes[].protocols.webdav` | `/.well-known/ocm` and the older `/ocm-provider` both answer; `apiVersion` `"1.0-proposal1"`, `version` `1.1.0` (34) or `1.1.2` (35); `webdav` is `/public.php/webdav/` |
| `webdav-receive`, `webapp`, `webapp-receive` in `protocols` | absent in both |
| `capabilities` includes `exchange-token`, `http-sig`, `invites` as offered | 34: `notifications`, `shares`; 35 adds `exchange-token` and `http-sig`; `invites` absent in both |
| Share notification signed with RFC 9421 HTTP Message Signatures | 34 always draft-cavage (`Signature: keyId=…,algorithm="rsa-sha512"`); 35 cavage by default and RFC 9421 (`Signature-Input: ocm=…`, `Content-Digest`) only when the receiver advertised `http-sig` |
| OCM Address has no scheme | `owner`, `sender`, `sharedBy` carry `alice@http://nc-a` (a scheme) when the sender is on `http` |
| `resourceType` `folder` for a folder (SHOULD) | `"file"` for a folder share |
| `protocol.webdav.permissions`: array of `read`, `write`, `share` | 34: the string `{http://open-cloud-mesh.org/ns}share-permissions`; 35: an array holding that same string; the real list is `ocm:share-permissions` read by `PROPFIND` on the receiver's mount (`["share","read","write"]`), and a create-only grant reads as `write` |
| Resource access: `Authorization: Bearer <sharedSecret>` | `401`; `Basic <secret>:` works |
| `SHARE_CHANGE_PERMISSION` notification | not sent when the permission changed; `SHARE_UNSHARED` is sent |
| `uri` an absolute or relative URI to the resource | 35 with an `http://` address: `https://http://host:port/public.php/webdav/` (malformed) |

#### Two Nextcloud instances, no helper

With alice on `nc-a` and bob, carol on `nc-b` (test `conformance_over_ocm`, the same `run_all` and the same store pointed at the receiving server):

- The sender creates the share; the receiving user **must accept it on the receiving server** (`POST /ocs/v2.php/apps/files_sharing/api/v1/remote_shares/pending/<id>`); nothing auto-accepts. Then the folders are mounted by name at the receiver's root (`DCP-….v`, `DCP-….l`), so the document id is the same on both sides, and all of the receiver's requests go through its own server, which proxies them to the sender: reads, creates, `If-None-Match`, `If-Match` all behave as on the sender (`412` and `204` through the proxy), and `ETag` values pass through unchanged.
- **The receiver's role is only read or write.** B's `PROPFIND` on a federated mount reports `oc:owner-id` `alice@nc-a` (the owner's cloud id), `oc:permissions` `SRGDN` for a viewer and `SRGDNVCK` for an editor and for an owner-level grant, and `ocm:share-permissions` `["share","read"]` or `["share","read","write"]`; its own `remote_shares` record says permissions `25` or `31`. The `R` (reshare) is present for every federated mount, so reading `R` as "owner" (what works locally) made an editor an owner in the first run and let it break the lease. The store now reads the OCM list for a mount whose owner id contains `@`. A create-only (5) grant is reported as full write; an owner-level grant is indistinguishable from an editor's. The sender still enforces the real mask (the receiver's overwrite of a version file is `403`). Test `what_a_federated_mount_says_about_the_role`.
- The journeys that pass: convergence, second head, corrupt file, truncated upload, lease exclusion and read-back (including the owner breaking the editor's lease and the editor being refused), roles enforced (including a promotion and a demotion reaching the receiver), the viewer's forgery, revocation. **`lease_expiry_releases` fails at 4 s** (above).

#### Locks across a federation

(`locks_do_not_cross_a_federation`, both builds, same results.)

- bob takes a `LOCK` through `nc-b` on a file that lives on `nc-a`: `200`, and **alice's `PUT` on `nc-a` is `204`**: the lock is nc-b's and nc-a never sees it (`lockdiscovery` on alice's side is empty).
- alice takes a Sabre `LOCK` on `nc-a`; bob's `PUT` through `nc-b` is **`204`**: a federated write ignores a lock held by a local user.
- alice takes a `files_lock` user lock (`X-User-Lock: 1`) on `nc-a`; bob's `PUT` through `nc-b` is **`500`**: it is stopped, with a server error and not `423`.
- The sender's version history is not offered to a receiver: `PROPFIND /remote.php/dav/versions/dave/versions/<id>` on `nc-b` for a federated file was `404` (probe, `oc-dave-versions`). The ledger layout does not need it.

### Nextcloud Office / Collabora (a note, not a measurement of editing)

Both images ship `files_lock` and an `office` app (`info.xml`: "Office overview", 1.0.0 on 34, 1.1.0 on 35); **Nextcloud Office (`richdocuments`) is not in the image**, and `occ app:install richdocuments` from the app store succeeded in the probe (11.1.2 on 34.0.5, 12.0.1 on 35.0.2). With it installed the capabilities report an empty `wopi_url` and `direct_editing: false` until a Collabora server is configured, and none was. So whether Nextcloud Office, which edits through WOPI, takes a `files_lock` lock on the file (the `nc:lock-owner-editor` property exists for that), and what it would do to a DCP version folder, **was not observed**. The WOPI adapter of spec 8.2 and Spike D is what would bridge a DCP document to Collabora; nothing here tested that over Nextcloud.

## What OCM and WebDAV cannot express

| DCP concept | On this binding |
|---|---|
| Heads, parents, merges | **Expressible**, as files: the sidecar JSON. Not natively: ETags, file versions and properties carry none (above). |
| Immutable versions (rules 1, 4) | **Enforced** for editors by a create-only share (mask 5). An editor with update or delete rights, or the owner, can rewrite them. |
| Lease with holder, base version, renewal, duration | Expressible as the create-only log; **not** as `LOCK` (no holder or duration DCP can read; token authorises; not federated). |
| Version authorship (`created_by` authenticated, section 11) | **Not** by WebDAV. Activity (asynchronous, retention not measured) names the creator; the versions endpoint has `nc:version-author` for replaced states only. |
| Roles | `viewer`, `editor` and local `owner` only. `commenter` has no carrier. Across a federation, read or write only. |
| Break a lease (owner, rule 5) | Any holder of update/create rights on `.l` can write the next record; the store checks `role()` first. The server does not enforce *owner-only*. |
| Removed member loses new content | Yes, as access control (`404`); not encryption. |
| L2 updates, L3 presence | No channel. Not offered. |
| Notification of a permission change | **None** (not sent). Unshare is notified. |
| Encrypted documents | None. The server sees everything it stores. |

## Spec corrections

For section 8.3 and the sections it touches. Status says how each was established.

1. **8.3 is replaced** by the [Proposed 8.3](#proposed-83) below. *Established by the measurements above.*
2. **4.2.1: a `webdav` locator is an `https://` URL of the owner's path.** No other principal can fetch it (their path differs), and the CI servers speak `http`. The store writes a canonical `https://` name and never fetches it; a reader resolves `<vid>.bin` relative to the document. Either `webdav` MUST NOT require an `https://` URL that readers fetch, or it should be a document-relative reference. *Established: every version in the conformance run carries a locator no reader used.*
3. **4.3 lease rule 1** ("MUST use the binding's own timestamp"): WebDAV's is whole seconds. Say what a binding with a coarse clock does (a calibrated estimate stored in the record, checked against the stamp) and that a lease shorter than the clock's error is not meaningful. *Established.*
4. **4.3 lease rule 3 and spec 12 test 4** assume a binding "without atomic compare-and-set" or with one that has an `If-Match`. A create-if-absent log is a compare-and-set with no ETag. *Established: 0 double wins in 280 concurrent rounds (14 batches of 20, both builds, creator races and lease-log races).*
5. **12, journey `lease_expiry_releases`**: `D = 4000` with a 500 ms settle fits a store that answers in tens of milliseconds. Proposed exact change: add `pub lease_ms: u64` to `Fixture` (default 4000) and have the journey use it; a binding with a slower path sets a longer one and states it. *Established: passes at 4 s on one server in 8 of 8 runs, fails at 4 s over a federation in 7 of 8 runs, passes at 20 s on both in every run.* A message was sent to the coordinator; `tests/lease_timing.rs` carries a copy of the journey with the parameter.
6. **5 (roles) and 8.3**: `commenter` has no carrier; an owner across a federation cannot be told from an editor. *Established.*
7. **9/11 (authorship)**: state that on a binding with no sender authentication, `created_by.principal` is a claim, and either require the reader to check an out-of-band source or say the binding offers no authorship. *Established: the editor's forgery.*
8. **10 (failure behaviour), "Lease expired while editing... save as a version anyway"**: holds on this binding (version publication needs no lease).
9. **13, open question 6** ("Whether OCM and WebDAV can express leases and heads, or only versions"): answered: yes, both, as files; not through ETags or `LOCK`.
10. **13, new**: the receiver must accept a federated share; a DCP client on the receiving side needs to say so, and the document is two shares.

### Proposed 8.3

> ### 8.3 OCM and WebDAV binding (measured once)
>
> Measured on Nextcloud 34.0.5 and 35.0.2, over WebDAV and OCS sharing, one server and an OCM federation of two. [Spike E results](rfc/0004-spike-e-results.md). Verdict: **go** for L0 and L1 for `viewer` and `editor`; L2 and L3 are not offered. The ETag and `LOCK` mapping of draft 1 is withdrawn.
>
> **Document.** A document is two collections in the owner's home: `<id>.v` and `<id>.l`, where `<id>` is `DCP-` followed by 12 lowercase hexadecimal digits. The identifier is the name; a share keeps it on the receiving side. A principal is an OCM address `user@host[:port]`.
>
> **Versions (`<id>.v`).** `meta.json` is the document metadata (4.1), written once. Each version is two files named by the same version identifier: `<vid>.bin`, the file, and `<vid>.json`, the version object (4.2) with `parents`. A writer MUST create each with `If-None-Match: *`, MUST read `<vid>.bin` back and compare it with what it sent before creating `<vid>.json` (the upload's own status and `OC-Checksum` do not verify the bytes), and MUST NOT overwrite or delete a version. A reader lists the `*.json` files and MUST ignore `<vid>.bin` files that have no `.json`. A `412` or `423` on a create means the name is taken; it is not a second head. Two heads are two version files with a common parent. An ETag, Nextcloud's file versions and WebDAV properties MUST NOT be used to carry a version, its parents or its order.
>
> **Lease (`<id>.l`).** A lease record is the file `<n>.json`, `<n>` a zero-padded 8 digit decimal, holding `{by, at_ms, lease}`: the writer, its estimate of the server's time in milliseconds, and the lease (4.3). The newest record (highest `<n>`) is the lease. A writer that last read `<n>` MUST create `<n+1>` with `If-None-Match: *`; `412` or `423` is a lost write and the writer MUST read back (lease rule 3). Records are never changed or deleted by a writer. A reader that holds `<n>` asks for `<n+1>` and so on until `404`. The time of a record is `at_ms` if the file's `Last-Modified` agrees with it to within 2 s, else the `Last-Modified`. WebDAV `LOCK` MUST NOT be used as the lease: it is absent unless the `files_lock` app is enabled, is authorised by its token alone, changes the ETag, and is not honoured across a federation. Leases SHOULD last at least 10 s (20 s over a federation).
>
> **Roles.** `<id>.v` and `<id>.l` are shared with the following permission masks: `viewer` 1 and 1; `editor` 5 (read, create) and 15; `owner` the owner of the folders, or 31 and 31 for a local user. `commenter` is not expressible. An editor MUST NOT be given update or delete on `<id>.v`: that is what makes versions immutable on the server. A role is read from the share: for a local mount from `oc:permissions` of the mount root (`R` owner, `C` editor, else viewer); for a federated mount (its `oc:owner-id` contains `@`) from `ocm:share-permissions`: `write` is editor, else viewer, and `owner` is not distinguishable from `editor`. A removed member gets `404`; a store SHOULD report it as `forbidden` for writes.
>
> **Time.** WebDAV times are whole seconds. A binding MUST estimate the server's clock (the instant `Date` ticks, then a monotonic clock) or accept a lease error of 1 s.
>
> **Authorship.** WebDAV does not name the uploader. `created_by.principal` is a claim by the writer. A reader that needs it authenticated MUST check the Activity API (`file_created`, `user`) or another source; the binding does not provide one.
>
> **Federation.** A document shared to `user@host` is two OCM shares, each needing acceptance by the receiving user on the receiving server. Each share is signed by the sender and carries a secret; the receiving server proxies the receiver's WebDAV requests. A change of permission is not notified (the sender enforces it); removal is (`SHARE_UNSHARED`). Locks and the sender's version history are not shared. The OCM web-app access mode is not offered by Nextcloud.
>
> **Not offered.** L2, L3, encryption.

## Not verified

- **ownCloud** (which the outline named) and **other OCM servers**, and a Nextcloud sender to a non-Nextcloud receiver with a real file store. Read: `IETF-OCM.md` (draft-07) and `spec.yaml` (1.5.0) of `cs3org/OCM-API`, and the `ocm-test-suite` README, capability registry and Nextcloud adapters. **Not read or run:** the OCM-stub reference implementation, and the test suite itself (its flows are browser-driven Cypress runs against its own images).
- **HTTPS**, certificates, real DNS. All traffic is plain HTTP on one docker network; Nextcloud was configured with `allow_local_remote_servers`. The Nextcloud `sharing.federation.allowHttpFallback` key was set and made no difference (the `http://` in the cloud id did).
- **Verification of the HTTP signature by a receiver** (the helper records and accepts), the **OCM code flow** against 35.0.2's token endpoint, and **invites**. 35.0.2's `https://http://…` URI was observed, not diagnosed.
- **Web-app access mode**: absent from the two stock images' discovery; how the `ocm-test-suite`'s Nextcloud 35 does it (its own images, a JupyterHub sidecar) was not read in detail or run.
- **Nextcloud Office / Collabora**: the app installs; no Collabora server was configured and nothing edited a DCP document.
- **Group and circle principals** in `grant`: the group share worked at the HTTP level (probe) and the store does not address groups.
- **Activity API latency and retention under load**: 55 to 105 ms in single trials; not repeated, not under load, retention unknown.
- **SQLite**: PostgreSQL only. Whether the concurrent-create results hold on SQLite was not run; Nextcloud discourages it for concurrency.
- **Redis or APCu file locking.** The default database locking was used; `423` on a concurrent create may behave differently with a memory-cache lock backend.
- **Large version counts** (listing is one `GET` per version), **large files** beyond 5 MiB, `chunked` uploads, **clock drift** over hours, **many documents per account**, **lease log growth** and compaction.
- **Desktop and mobile Nextcloud clients** syncing a document folder (two folders per document would appear as two sync roots).
- **The `Date`-calibrated clock against a server whose clock is not the runner's** (here both are one machine).
- Each result is from **one image per version**, in up to eight CI runs of the final code (four runs, two builds); the final run's figure is quoted and a range where runs differed. Latencies in particular moved 20 to 25% between runs on the same build (a shared 2-core runner), so a difference between the two builds is not a finding. The ETag trials are 20 per variant per build per run.

## Reproducing

```
# the servers (docker; set the digests above)
cd tools/collab-spike
NEXTCLOUD_IMAGE=... POSTGRES_IMAGE=... scripts/nextcloud-up.sh a b

# the tests (a missing server FAILS; DCP_SKIP_NEXTCLOUD=1 skips and prints SKIPPED)
DCP_MEASURE_OUT=/tmp/measure.jsonl cargo test -p dcp-webdav --lib
cargo test -p dcp-webdav --test conformance_over_nextcloud -- --nocapture --test-threads=1
cargo test -p dcp-webdav --test conformance_over_ocm -- --nocapture --test-threads=1
cargo test -p dcp-webdav --test nextcloud_semantics -- --nocapture --test-threads=1
cargo test -p dcp-webdav --test federation -- --nocapture --test-threads=1
cargo test -p dcp-webdav --test lease_timing -- --nocapture --test-threads=1

# the raw probe: every request and response, no assertions
PROBE_OUT=/tmp/probe scripts/nextcloud-probe.sh
```

`MEASURE {...}` lines in the logs and `measure.jsonl` are the numbers quoted here, by `measure` name.
