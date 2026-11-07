# RFC-0004 interop: the Matrix and WOPI bindings against each other

Date: 2026-10-24 · Tracks: [RFC-0004](0004-interoperable-document-collaboration.md), DCP draft 1 [§4.3](../DOCUMENT-COLLABORATION-SPEC.md), §8.1 and §8.2 (the first version of this document, run 40088025176, was written against draft 0; [Draft 1 update](#draft-1-update) says what changed) · Code: [`tools/collab-spike/dcp-interop/`](../../tools/collab-spike/dcp-interop/) · Inputs: [Spike A results](0004-spike-a-results.md), [Spike D results](0004-spike-d-results.md)

**What this shows and does not show.** Spike A measured `MatrixStore` on Synapse; spike
D measured `WopiHost` and a real Collabora over an in-memory store, and listed "the host
over the Matrix binding" as not verified. This closes that gap two ways: with our own
test clients (a WOPI client *simulator*, `WopiClientStore`, plus a step-by-step raw HTTP
client) over `WopiHost<MatrixStore>` next to native `MatrixStore` users in the same room,
and, since the draft 1 update, with **a real Collabora Online editing through
`WopiHost<MatrixStore>`** (4 tests, see [Collabora over a Matrix room](#collabora-over-a-matrix-room)).
No Matrix client other than ours opened a room. One Synapse (1.163.2), loopback, debug
builds, one run per statement unless stated.

## Result

**No bug was found in either binding or in the spec by these tests, before or after the
draft 1 update.** Latest run: [40093629440](https://github.com/tuna-os/gtk-office-suite/actions/runs/40093629440)
(commit `f5cf477`), all seven jobs green: `cargo check --workspace --all-targets --locked`,
`core`, `matrix` (relaxed and default rate limits), `wopi-sim`, `interop` and `collabora`
(Collabora over memory: 4 tests; Collabora over Matrix: 4 tests). In `interop`: 5
conformance tests, 18 cross-binding tests, 10 negative-control tests passed. The first run
(draft 0, run [40088025176](https://github.com/tuna-os/gtk-office-suite/actions/runs/40088025176),
commit `1ec91bb`) was also green. Nothing needed an expected-failure marker.

## How it is built

`WopiHost<MatrixStore>` is one `WopiHost` whose access tokens are each backed by that
principal's own `MatrixStore` (spec 8.2: "stores documents through another binding"). A
`Rig` is one Synapse room with an owner, an editor and a viewer; every request a test
makes is either *native* (a `MatrixStore` call) or *WOPI* (HTTP to the host, which calls
the same principal's `MatrixStore`). All clients and the host's backing stores reach
Synapse through spike A's fault proxy, so "a truncated upload is never published" also runs
through WOPI. Files are opaque bytes, so the host runs with `validate_packages: false`
(spike D's own conformance test does the same).

The checks return a `Failure` instead of panicking. That is what lets the negative
controls (`tests/negative_control.rs`) wrap one handle in one of `dcp-conformance`'s
`Defective` stores and assert that the check *fails*. A control that fails for any other
reason than the defect would not count: each was read (log line "failed as it must").

## Conformance journeys on both stacks

`tests/conformance.rs`. The skipped lists are asserted with `assert_skipped_exactly`, so a
journey that stops running, or one that starts running, fails CI.

| Stack | Passed | Skipped (asserted exactly) |
|---|---|---|
| `MatrixStore` (relaxed) | all 11 journeys, in spec order | none |
| `WopiClientStore -> WopiHost -> MatrixStore` | `convergence_on_one_head`, `corrupt_file_rejected`, `forged_version_from_a_viewer_is_not_accepted`, `roles_enforced_at_store`, `truncated_upload_never_published` | `second_head_shown_not_hidden`, `lease_exclusion_and_readback`, `lease_expiry_releases`, `revocation_hides_new_content`, `unknown_algorithm_falls_back_to_versions`, `update_stream_gap_recovery` |

The WOPI-over-Matrix list is the same six as spike D's WOPI-over-memory list, for the
same reasons (a WOPI save has one parent; the lock string is all of a lease WOPI carries;
no grant/revoke; no L2 stream). The stack behind the host does not change what WOPI can
express. Roles and membership for that stack are set up natively, because WOPI cannot.
A defective viewer behind the host (`HideNewest`) makes `convergence_on_one_head` fail
through WOPI-over-Matrix.

## Cross-binding checks

`tests/cross_binding.rs`, one test per check in `src/checks.rs` (15 checks, 18 tests: the two forgery checks also run in a draft-0 room, and one test checks that every check has a test), each on a fresh room.

| Check | What is asserted |
|---|---|
| `wopi_publish_is_read_by_a_matrix_user` | A save by `WopiClientStore` is a Matrix event (`$...` id). A different principal (the viewer) reading natively sees it as the only head, with the WOPI writer's parent, `sender` and `created_by` the WOPI token's principal, the same `ts_ms`, and the host's `WOPI PutFile (...)` message. `assert_same_view` holds between the Matrix reader and a WOPI reader (same heads, bytes, rejections; identifiers agree because the WOPI id *is* the Matrix event id), and between two Matrix readers; `assert_same_content` holds across the two bindings. The WOPI session's lease reads `released` on Matrix afterwards. |
| `native_publish_opens_through_wopi_getfile` | A native publish is what CheckFileInfo (`Version`, `LastModifiedTime` = the binding timestamp, `Size`, `UserCanWrite`, `DcpVersion` with author and message) and GetFile (bytes, `X-WOPI-ItemVersion` = the event id) return, for an editor and a viewer. |
| `native_publish_between_getfile_and_putfile_is_a_409` | Editor: CheckFileInfo, GetFile, LOCK. Owner publishes natively; the WOPI lock is untouched (lease rule 6: advisory). The editor's PutFile with Collabora's timestamp is **409, body `COOLStatusCode` 1010, no `X-WOPI-Lock`**, and publishes nothing (every principal still lists exactly `[first, native]`). The lock is still held; re-opening and saving then works, as a child of the native version. |
| `forced_wopi_save_forks_and_matrix_shows_the_conflict` | The same, but the PutFile has no timestamp header (a forced save): it is accepted and forks. All three Matrix readers show two heads as a conflict, both bytes present; WOPI shows the document read-only (`UserCanWrite` false, `DcpHeads` lists both, `X-DCP-Heads: 2`); a native merge of both heads makes it writable again. |
| `native_offline_publish_shows_as_a_conflict_to_the_matrix_reader` | WOPI saves v2 from v1; the owner then publishes natively from v1 (an offline edit coming back). The viewer, natively, sees both heads, newest first, as a conflict; the Matrix view equals the WOPI view; the WOPI editor gets a read-only session. |
| `viewer_token_cannot_write` | The viewer's token: CheckFileInfo says read-only, GetFile works; LOCK, UNLOCK and PutFile (with and without a lock, with the timestamp) are 403; `WopiClientStore` save is `Forbidden`, so is a native publish. An editor's token on the same document saves. Nothing the viewer tried left a version or a lease. |
| `wopi_lock_blocks_native_acquire_and_the_reverse` | While WOPI holds `LOCK`, `lease::acquire` by the owner, and by the editor's principal in another session, is `Held`; the record names the WOPI principal, the lock string, 30 minutes and `base_version`. After UNLOCK a native session acquires; then WOPI's LOCK is **409 carrying the native token** and GET_LOCK shows it. Released natively, WOPI locks again. |
| `native_lease_blocks_wopi_save` | A native lease held by the owner: WOPI LOCK, PutFile with its own string and PutFile with no lock are all 409 with the native token; `WopiClientStore` gets `Conflict`; nothing is published. A native *editor* publishes anyway and the lease is unchanged: rule 6, only WOPI is stopped by a lease. |
| `owner_break_invalidates_the_wopi_lock` | Lease rule 5: the owner breaks a WOPI holder's lock natively. The WOPI session finds out as WOPI says it must: GET_LOCK shows the breaker's token, PutFile with the old string is 409 carrying it, UNLOCK is 409, nothing is published, the breaker's lease is intact. |
| `native_lease_expiry_frees_the_wopi_lock` | A 6 s native lease: LOCK is 409 while active, succeeds after expiry (7.1 s after the acquire in the CI run, 27 attempts; 7.8 s locally); the lease is then the WOPI one and a native acquire is `Held`. |
| `wopi_lock_refresh_is_a_new_record_on_matrix` | Three refreshes (`LOCK`, same string) are three new events with renewal 1, 2, 3 and later timestamps; UNLOCK then LOCK starts again at 0. |
| `wopi_lock_on_an_empty_document_omits_base_version` | See [Draft 1 update](#draft-1-update). |
| `removed_members_wopi_token_gets_no_new_content_and_no_write` | See next section. |
| `forged_version_is_dropped_by_matrix_readers`, `forged_version_is_dropped_by_wopi_readers` | See [Draft 1 update](#draft-1-update); each runs in a room with the spec's table and in a room with the draft-0 table. |

### A removed member's WOPI token

The owner removes the editor natively (`revoke`: kick plus power-levels cleanup) and
publishes a new version. The editor's WOPI token is still in the host's registry. What the
code does, observed in CI and locally and now asserted:

**Every request through the token is refused with 403, reads included.** CheckFileInfo,
GetFile (head and `?dcp_version=` of the new version), LOCK, and PutFile (with and without
a lock) all answer `HTTP 403`, body `join !<room>: the server returned an error: [403 /
M_FORBIDDEN] You are not invited to this room.` The removed member gets no new content and
also no content they could read before; `WopiClientStore` for that token fails with
`Forbidden`. The refusal comes from `MatrixStore::room_for`, which tries to re-join the
room it was kicked from and is refused; the WOPI host has no notion of membership and never
learns of the removal (401 would mean it had forgotten the token). Nothing was published
and no lease was written by the removed member's attempts, and a member who stayed reads
the new version through WOPI. What the removed member already holds on their own device is
outside this check (spec section 11; spike A's 0 of 70 decryption trials).

### Negative controls

`tests/negative_control.rs`. Each builds a rig with one defect and asserts that the check
aimed at it returns a `Failure` **whose message states what it observed instead of what it
expected**; each control names the phrase its check uses. (The first version of these
controls only asserted failure, and four of the six failed by waiting 8 s for an effect
that never arrived. `Rig::until` now reports `expected X; still observed: Y`, and the
controls assert on that phrase, so a control that failed only because Synapse was slow
would itself fail.)

| Defect | Where | Check that must fail | The failure must say |
|---|---|---|---|
| `HideNewest` | Matrix reader | `wopi_publish_is_read_by_a_matrix_user` | `still observed: heads` |
| `HideNewest` | the host's backing store for the editor | `native_publish_between_getfile_and_putfile_is_a_409` | `still observed: Version` |
| `HideSiblingHeads` | Matrix reader | `native_offline_publish_shows_as_a_conflict_to_the_matrix_reader` | `still observed: heads` |
| `IgnoreRoles` (writes as the owner) | native viewer | `viewer_token_cannot_write` | `must be Forbidden` |
| `RevokeIsNoop` | native owner | `removed_members_wopi_token_gets_no_new_content_and_no_write` | `must be refused by the Matrix store` |
| `FrozenClock` | the host's backing store for the editor | `native_lease_expiry_frees_the_wopi_lock` | `still observed: LOCK still 409` |
| draft-0 power-level table and readers that trust the sender (`MatrixConfig::reader_checks_senders = false`) | every Matrix store | `forged_version_is_dropped_by_matrix_readers` | `a viewer's forged version was accepted by a Matrix reader` |
| the same | every Matrix store | `forged_version_is_dropped_by_wopi_readers` | `through WOPI the heads are` |

plus the conformance-level controls (a viewer behind the host that hides the newest version
fails `convergence_on_one_head`; the draft-0 room with trusting readers fails
`forged_version_from_a_viewer_is_not_accepted` on both stacks, "forged version was
accepted"), and `a_defective_rig_still_passes_the_checks_the_defect_cannot_touch` (a rig
whose revoke is a no-op still passes `native_publish_opens_through_wopi_getfile`; a rig with
no reader checks still passes `wopi_publish_is_read_by_a_matrix_user`). Two of the controls
(the two `HideNewest` ones and the expiry one) are still failures to *converge*, reported as
a stated mismatch after a 6 to 10 s wait; that is the nature of the property, not an
assertion on a value.

## Draft 1 update

The spec moved to draft 1 (branch `spike/collab-spec-fixes`: `dcp` 1, lease `renewal`,
optional `base_version`, rewritten 8.1/8.2). What changed in the code and what that showed:

- **The renewal counter replaces the alternation hack.** `MatrixStore::write_lease` no
  longer rewrites `duration_ms` by 1 ms; it writes what it is given. `Claim::lease_over`
  builds the record over what the session just read: renewal 0 over nothing or a released
  record, previous + 1 over a held one (renewal, takeover after expiry, break). In the
  conformance journeys (4 and 5) and the interop checks that is asserted record by record,
  on Synapse: 0 on a fresh lease, 1 on a break and on a takeover of an expired record, +1 on
  every renewal. `wopi_lock_refresh_is_a_new_record_on_matrix` shows the case that mattered:
  Collabora's refresh (`LOCK`, same string) over Matrix is now three consecutive refreshes
  with renewal 1, 2, 3, each a new event ID and a later timestamp, where draft 0 would have
  been silently dropped by Synapse. Over a released record the count starts at 0 again.
- **Lease rule 3 is two read-backs and a settle interval** (500 ms, `lease::MATRIX_SETTLE_MS`).
  `lease::acquire` and `confirm` do it; `WopiHost` does it too (`HostConfig::lease_settle_ms`),
  so a WOPI `LOCK` now takes at least 500 ms. A unit test shows why a single read-back is
  not enough (a write landing during the settle interval is caught only by the second
  read). The memory store refuses a stale `renewal` as `Conflict`. **Not run:** the 40-round
  race of spike A with the two-read rule; journey 4 runs one race per job, and both
  sessions' outcomes were exactly one acquirer on Synapse. Journey 5 uses 4 s leases now
  (2 s was shorter than the settle interval's cost).
- **`base_version` is omitted, not `"none"`.** `wopi_lock_on_an_empty_document_omits_base_version`:
  a WOPI `LOCK` on a Matrix document with no version writes a lease with no `base_version`,
  `renewal` 0 and 1 800 000 ms that validates against the schema, and the same session can
  then save the first version (parents `[]`). The host's duration is no longer configurable.
- **`PutFile` from a token that read no version is refused** (spec 8.2 rule 1, a SHOULD that
  the host now implements): 409, empty `X-WOPI-Lock`, nothing published.
- **Forged versions.** New journey `forged_version_from_a_viewer_is_not_accepted` (spec
  test 6, second half; capability `forgery`) and two cross-binding checks. A viewer's client
  sends a version event straight to Synapse, around `MatrixStore`'s role check, once under
  its own name and once with `created_by` naming the owner. In a room with the spec's table
  (`m.room.encrypted` 50) Synapse refuses both with 403; the checks assert it. In a room
  created with the draft-0 table Synapse **accepts both** (asserted: otherwise the check
  would prove nothing) and every reader drops them: Matrix readers (sender below the editor
  level; `created_by` is not the sender) and WOPI readers over the host (single head in
  `DcpHeads`, `UserCanWrite` unchanged, GetFile serves the legitimate save, the
  `DcpVersions` listing has no forgery). A viewer's own WOPI token cannot add one (PutFile
  and LOCK are 403). The negative control above shows the forgery becoming a second head
  when the reader rule is off. The in-memory store gained the same two behaviours
  (`gates_timeline`, `reader_checks_senders`) so the journey also runs, and has mutation
  tests, with no Synapse.
- **No reader-side rule on updates was exercised.** The reader rule covers updates too
  (`reader_checks_senders` applies to both), but no test forges an update.

## Collabora over a Matrix room

`tests/collabora_over_matrix.rs` (job `collabora`). A real Collabora Online
(`collabora/code:latest`, image `sha256:751ec74aeb6d...`, created 2026-10-23) is told the
WOPISrc of a document in a Matrix room; `WopiHost<MatrixStore>` serves it. The driver is
dcp-wopi's own (`tests/shared/collabora_driver.rs`, moved out of `collabora_e2e.rs`), so
what changed relative to spike D is only the store behind the host. All four passed in 54 s
(run 40093629440; the in-memory twins, 4 tests, took 45 s):

| Test | What was asserted |
|---|---|
| `collabora_over_matrix_edits_and_saves_a_version_a_matrix_user_reads` | Collabora granted `perm: edit`, a user typed, `save`: the Matrix room holds a second version, a Matrix event, whose only parent is the version Collabora opened, with `sender` and `created_by` the token's principal, whose `content.xml` contains the typed text. A second Matrix principal sees it as the single head; `assert_same_view` holds between that principal and the owner, and between that principal and a WOPI reader. While open, the lease in the room is Collabora's lock string, held by the editor, 1 800 000 ms, with a `renewal`, and validates; `UNLOCK` when the tab closes makes it `released`. |
| `collabora_over_matrix_save_after_a_native_matrix_publish_is_a_conflict` | A native Matrix user publishes a child of the version Collabora has open (the lease is advisory). Collabora's save is `409`, `COOLStatusCode` 1010, no `X-WOPI-Lock`; only `[first, native]` exist; Collabora told its user ("documentconflict"). |
| `collabora_over_matrix_opens_a_viewer_read_only_and_never_locks_or_saves` | `perm: readonly` for a viewer; no WOPI operation reached the host; one version and no lease in the room. |
| `collabora_over_matrix_refresh_is_a_new_lease_record_with_renewal_plus_one` | With `storage.wopi.locking.refresh` 10 s, Collabora's second `LOCK` (same string, 200) makes the room's lease renewal 1, a new event ID, a later timestamp. |

Resources: the runner had 15,988 MB of memory; with Synapse and Collabora up and before the
tests, 1,698 MB were in use (`free -m` in the job); nothing was killed or slow. So the
"exceeds runner memory" fallback was not needed and the in-memory Collabora run stays as
the comparison, not as a fallback. This is one run of one Collabora build; autosave and
exit-save `PutFile` (version churn against Matrix's limits, on the default profile) and two
Collabora sessions on one document are still not exercised.

## Observations that are not bugs

- **Lease rule 6 is asymmetric in practice.** A lease (WOPI's or native) stops a *WOPI*
  writer (409 with the current lock string, which is what WOPI requires) and does not stop
  a native writer, who is only advised. The spec says so (rule 6) but rule 5's "MUST NOT
  save" reads stronger than what any binding does.
- **A WOPI save without Collabora's timestamp header forks.** That is the documented
  forced-save path (spike D), not a defect, but it means the lock does not protect a
  non-Collabora WOPI client from a native writer; only the timestamp precondition does.
- **Lease expiry through WOPI lags.** A 6 s native lease took 7.1 s (CI) to 7.8 s (local)
  to free the WOPI lock. The host's clock is the editor's `MatrixStore` clock, which falls
  back to the HTTP `Date` header (1 s resolution, lagging) until that device has written;
  this is spike A's clock finding showing up in the WOPI path, plus polling interval.

## Not verified

- **The default (rate-limited) Synapse profile for `dcp-interop`** (it requires and asserts
  `relaxed`, also for the Collabora-over-Matrix tests). Under the default profile a WOPI
  save costs three timeline/state events (LOCK, version, UNLOCK) plus the settle interval,
  so it would meet spike A's 0.2 events/s limit; not run. Collabora's autosave and
  exit-save churn against that limit is spec question 10 and is still open.
- **The two-read lease rule under stress.** One race per journey run; spike A's 40-round
  race with the settle interval was not repeated.
- **A WOPI PutFile on a conflicted document.** CheckFileInfo says read-only and the tests
  assert that; whether the host *refuses* a PutFile from a client that ignores
  `UserCanWrite` was not probed (from reading `do_put`, which checks the role, the lease,
  the base-version rule and the timestamp but not the head count, it would accept it;
  [INFERENCE], not run).
- **Two WOPI sessions on one document, `REFRESH_LOCK` and `UNLOCK_AND_RELOCK` over Matrix**
  (Collabora uses neither; host tests cover them over memory). `LOCK` refresh over Matrix is
  covered, by the simulator and by Collabora.
- **A forged update**, and a forged version from a principal who was an editor and was then
  demoted (the reader rule uses the current level when the server does not gate).
- **A new device of an existing member** reading a WOPI-served document (spike A's finding
  that it reads no history is unchanged).
- **L2 updates and L3 presence through WOPI**: none exist.
- **Other homeservers, other WOPI clients, other Collabora builds, federation, a Matrix
  client that is not ours.**

## Reproducing

```
cd tools/collab-spike
eval "$(scripts/synapse-up.sh relaxed)"          # docker or podman
cargo test -p dcp-interop --test conformance --test cross_binding --test negative_control -- --nocapture
scripts/synapse-down.sh relaxed

# Collabora over Matrix (needs both services, one test at a time: they share the WOPI host port)
DCP_WOPI_HOST_PORT=18080 scripts/collabora-up.sh
eval "$(scripts/synapse-up.sh relaxed)"
DCP_WOPI_HOST_PORT=18080 cargo test -p dcp-interop --test collabora_over_matrix -- --nocapture --test-threads=1

DCP_SKIP_SYNAPSE=1 cargo test -p dcp-interop      # explicit skip; prints SKIPPED (DCP_SKIP_COLLABORA=1 likewise)
```

Without `DCP_SYNAPSE_URL` (and without `DCP_SKIP_SYNAPSE=1`) every Synapse test fails; the
`interop` job checks that before it starts a Synapse. The workflow is
[`.github/workflows/collab-spike.yml`](../../.github/workflows/collab-spike.yml); it
replaces the three per-spike workflows.
