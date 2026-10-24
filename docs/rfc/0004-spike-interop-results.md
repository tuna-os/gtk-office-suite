# RFC-0004 interop: the Matrix and WOPI bindings against each other

Date: 2026-10-24 · Tracks: [RFC-0004](0004-interoperable-document-collaboration.md), DCP draft 0 §8.1 and §8.2 · Code: [`tools/collab-spike/dcp-interop/`](../../tools/collab-spike/dcp-interop/) · Inputs: [Spike A results](0004-spike-a-results.md), [Spike D results](0004-spike-d-results.md)

**What this shows and does not show.** Spike A measured `MatrixStore` on Synapse; spike
D measured `WopiHost` and a real Collabora over an in-memory store, and listed "the host
over the Matrix binding" as not verified. This closes that gap with our own test clients:
a WOPI client *simulator* (`WopiClientStore`, plus a step-by-step raw HTTP client) over
`WopiHost<MatrixStore>`, and native `MatrixStore` users in the same room. **Collabora is
not in these tests** (it is in the `collabora` job, over memory, as before), and no Matrix
client other than ours opened a room. One Synapse (1.163.2, relaxed rate limits), loopback,
debug builds, one run per statement unless stated.

## Result

**No bug was found in either binding or in the spec by these tests.** All 12 cross-binding
checks, both conformance runs, the conformance negative control and all 7 Synapse-backed tests of `negative_control.rs` passed on GitHub Actions
(run [40088025176](https://github.com/tuna-os/gtk-office-suite/actions/runs/40088025176),
commit `1ec91bb`, all seven jobs green) and again locally on Synapse 1.163.2 under podman
(12 + 3 + 8 tests). Nothing needed an expected-failure marker.

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
| `MatrixStore` (relaxed) | all 10 journeys, in spec order | none |
| `WopiClientStore -> WopiHost -> MatrixStore` | `convergence_on_one_head`, `corrupt_file_rejected`, `truncated_upload_never_published`, `roles_enforced_at_store` | `second_head_shown_not_hidden`, `lease_exclusion_and_readback`, `lease_expiry_releases`, `revocation_hides_new_content`, `unknown_algorithm_falls_back_to_versions`, `update_stream_gap_recovery` |

The WOPI-over-Matrix list is the same six as spike D's WOPI-over-memory list, for the
same reasons (a WOPI save has one parent; the lock string is all of a lease WOPI carries;
no grant/revoke; no L2 stream). The stack behind the host does not change what WOPI can
express. Roles and membership for that stack are set up natively, because WOPI cannot.
A defective viewer behind the host (`HideNewest`) makes `convergence_on_one_head` fail
through WOPI-over-Matrix.

## Cross-binding checks

`tests/cross_binding.rs`, one test per check in `src/checks.rs`, each on a fresh room.

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
| `removed_members_wopi_token_gets_no_new_content_and_no_write` | See next section. |

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

`tests/negative_control.rs`. Each wraps one handle in a `Defective` store and asserts that
the check aimed at it returns a `Failure`:

| Defect | Handle | Check that fails |
|---|---|---|
| `HideNewest` | Matrix reader | `wopi_publish_is_read_by_a_matrix_user` |
| `HideNewest` | the host's backing store for the editor | `native_publish_between_getfile_and_putfile_is_a_409` |
| `HideSiblingHeads` | Matrix reader | `native_offline_publish_shows_as_a_conflict_to_the_matrix_reader` |
| `IgnoreRoles` (writes as the owner) | native viewer | `viewer_token_cannot_write` |
| `RevokeIsNoop` | native owner | `removed_members_wopi_token_gets_no_new_content_and_no_write` |
| `FrozenClock` | the host's backing store for the editor | `native_lease_expiry_frees_the_wopi_lock` |

plus `a_defective_rig_still_passes_the_checks_the_defect_cannot_touch` (a rig whose
revoke is a no-op still passes `native_publish_opens_through_wopi_getfile`), and the
conformance control above. Four of the six fail by timing out after 8 s waiting for an
effect that never arrives; that is a legitimate failure for a convergence property, but it
is a weaker signal than an assertion.

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

- **Collabora against `WopiHost<MatrixStore>`.** The `collabora` job runs the real
  Collabora over the in-memory store; nothing here puts Collabora in front of the Matrix
  host. Collabora's save frequency against Matrix's limits remains open.
- **The default (rate-limited) Synapse profile.** `dcp-interop` requires `relaxed` and
  asserts it. Under the default profile a WOPI save costs three timeline/state events
  (LOCK, version, UNLOCK) plus reads, so it would meet spike A's 0.2 events/s limit; not run.
- **A WOPI PutFile on a conflicted document.** CheckFileInfo says read-only and the tests
  assert that; whether the host *refuses* a PutFile from a client that ignores
  `UserCanWrite` was not probed (from reading `do_put`, which checks the role, the lease and the timestamp but not the head count, it would accept it; [INFERENCE], not run).
- **Two WOPI sessions on one document, a lease whose WOPI holder is another principal's
  token, `UNLOCK_AND_RELOCK` and `REFRESH_LOCK` over Matrix**: the host tests cover them
  over memory; not repeated here. LOCK refresh over Matrix is untested (it depends on
  spike A's identical-state-event dedupe workaround).
- **L2 updates and L3 presence through WOPI**: none exist.
- **Other homeservers, other WOPI clients, federation, a Matrix client that is not ours.**

## Reproducing

```
cd tools/collab-spike
eval "$(scripts/synapse-up.sh relaxed)"          # docker or podman
cargo test -p dcp-interop -- --nocapture          # all of it
cargo test -p dcp-interop --test cross_binding    # the 12 cross-binding checks
cargo test -p dcp-interop --test negative_control # the controls
scripts/synapse-down.sh relaxed

DCP_SKIP_SYNAPSE=1 cargo test -p dcp-interop      # explicit skip; prints SKIPPED
```

Without `DCP_SYNAPSE_URL` (and without `DCP_SKIP_SYNAPSE=1`) every Synapse test fails; the
`interop` job checks that before it starts a Synapse. The workflow is
[`.github/workflows/collab-spike.yml`](../../.github/workflows/collab-spike.yml); it
replaces the three per-spike workflows.
