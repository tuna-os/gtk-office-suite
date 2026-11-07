# RFC-0004 Spike F: the verification gaps of spikes A, D and the interop run

Date: 2026-10-24 · Tracks: [RFC-0004](0004-interoperable-document-collaboration.md), DCP draft 1 [§4.3](../DOCUMENT-COLLABORATION-SPEC.md), §8.1, §8.2, §13 question 10 · Code: [`tools/collab-spike/dcp-interop/`](../../tools/collab-spike/dcp-interop/) (new: `profile.rs`, six checks in `checks.rs`, `tests/lease_race.rs`, `tests/collabora_churn.rs`, `tests/wopi_latency.rs`) and one fix in [`dcp-wopi/src/host.rs`](../../tools/collab-spike/dcp-wopi/src/host.rs) · Inputs: [Spike A](0004-spike-a-results.md), [Spike D](0004-spike-d-results.md), [interop](0004-spike-interop-results.md) (its "Not verified" list is the work list)

**Every number below is from one CI run, [40142775201](https://github.com/tuna-os/gtk-office-suite/actions/runs/40142775201)
(branch `spike/collab-f`, commit `513086d`), all 14 jobs green, unless another run is named.**
Synapse 1.163.2 (`matrixdotorg/synapse:latest`, image `sha256:e0d446e9d9f2e0db07dc093219aee61ae32315a1672f81cdcaa44e21e58325a2`,
SQLite, no federation, loopback, docker 28.0.4); Collabora Online 26.04.5.2
(`collabora/code:latest`, image `sha256:751ec74aeb6d3a39f11b17a5ffe726be92eb97bfc3ccbcc41ce9b8db4e9315e0`, created
2026-10-23); `ubuntu-24.04` runner (image 20261012.334.1, 2 vCPU), rustc 1.99.1, debug builds; matrix-sdk 0.19.1 as in
spike A. One run per statement unless a count of rounds is given. Nothing here says anything about other homeservers,
other WOPI clients, a real network, or release builds.

## Result

1. **Default (rate-limited) Synapse, whole interop suite: everything passes, and no existing assertion was changed for it.** Conformance (11
   journeys on `MatrixStore`, 5 passed and 6 skipped-as-declared through `WopiHost<MatrixStore>`) and all 26 cross-binding
   checks pass on `relaxed`, on `default`, and on `default-drained` (every principal's message budget spent first, see
   below). The harness is profile-aware (explicit settle 300 s and per-test deadline 45 min on the default profile,
   30 s and 15 min on relaxed); the table of recorded default-profile failures is empty because nothing failed. One check
   written in this spike (two hosts racing for a lock) was loosened after its first drained run; the finding it
   exposed is item 4 and the judgment call is described in section 3.
2. **On the plain default profile nothing is ever delayed, and that is itself the finding.** A journey sends 3 to 8
   events in all (3 users), a check 1 to 12; a fresh user's burst of 10 covers it. 0 of 22 journey runs got a 429
   (1 `M_LIMIT_EXCEEDED` in all 25 check runs, 1 s waited). The limit only bites for a user who has already spent the burst:
   `default-drained` journeys took 7.5 to 28.4 s against 3.1 to 14.8 s relaxed (median 3.2 times longer, up to 8.6
   times), checks up to 40 s, the client waited 140 s (journeys) and 237 s (checks) for the limiter in total, all passing.
3. **Collabora's autosave and lock-refresh churn does not exceed 0.2 events/s, burst 10, in a 3-minute session, at its
   own timers or at far tighter ones: 6, 14 and 40 events in 187 s, 0 `M_LIMIT_EXCEEDED`.** One version per `PutFile`
   (autosaves are not coalesced) and one state event per lock refresh. The tightest run is 0.21 events/s, at the
   limit; the burst hides it for the first minutes only. **With the editor's budget already spent the limit is the
   binding one, and the surprise is on Collabora's side: it abandons a WOPI call after 3003 ms** ("Timeout while
   requesting [POST ...] after 3003ms"), twice, for the `UNLOCK` at the end of the session. Over the wire, on
   a spent budget, 30 of 36 WOPI calls take longer than that (median 4.6 to 5.1 s).
4. **New findings, three of them real defects or spec gaps:**
   - `WopiHost` **accepted a `PutFile` on a conflicted document** (observed: HTTP 200 from a client that ignores
     `UserCanWrite: false`). Fixed in `dcp-wopi` (409, `COOLStatusCode` 1010, no `X-WOPI-Lock`), covered by a test in
     `dcp-wopi` and by a cross-binding check against Synapse.
   - **The reader rule of spec 8.1 judges a sender by the room's *current* power levels.** In a room whose table does
     not gate `m.room.encrypted`, demoting an editor makes every reader drop that editor's earlier legitimate versions,
     and then reject the owner's later save built on one of them (unknown parent): the document reads as its first
     version. Not an issue in a room with the spec's table (the server refuses the ex-editor, history stays).
   - **The two-read lease rule does not hold when the homeserver delays a write**: with two WOPI hosts in front of
     one room and spent budgets, both `LOCK`s answered 200 in 3 of 4 rounds (the writes landed 4.5 to 5.5 s apart,
     the settle interval is 0.5 s). The damage is contained: the losing host's next `PutFile` is refused 409 with the
     holder's string and publishes nothing (asserted).
5. **The lease race, 200 rounds on each profile: the second read always resolves it.** First read-back wrong
   (both sessions would have believed they hold it) in 37 of 200 rounds on relaxed and 69 of 200 on default; the
   two-read rule let exactly one session acquire in 200 of 200 on both, and the final state named that session in
   200 of 200. The settle interval is the boundary: a competing write 450 ms behind the first never produced two
   holders (0 of 6), 550 ms behind produced them in 5 of 6, 700 ms and later in 6 of 6.
6. The unverified items are verified: a forged update is dropped (refused 403 in a gated room, dropped by every reader
   in a draft-0 room); a demoted editor is stopped by the server and, where the server does not, by the readers (with
   the retroactive effect above); `REFRESH_LOCK` and `UNLOCK_AND_RELOCK` over Matrix behave as rule 8 says; two WOPI
   sessions on one document never share a lock.

## What was run

Workflow [`collab-spike.yml`](../../.github/workflows/collab-spike.yml) (run as the spike's own workflow `collab-spike-f.yml`, merged into it at integration; now the jobs `interop`, `lease-race`, `wopi-sim` and `collabora`), jobs:

| Job | Command (in `tools/collab-spike`, after `eval "$(scripts/synapse-up.sh <profile>)"`) |
|---|---|
| `interop <tag> / conformance` | `cargo test -p dcp-interop --locked --test conformance -- --nocapture --test-threads=N` |
| `interop <tag> / cross_binding` | `cargo test -p dcp-interop --locked --test cross_binding -- --nocapture --test-threads=N` |
| `interop relaxed / controls` | `... --test negative_control` (13 negative controls, relaxed only) |
| `interop <tag> / latency` | `... --test wopi_latency` |
| `lease race (<profile>)` | `DCP_RACE_ROUNDS=200 cargo test -p dcp-interop --locked --test lease_race -- --nocapture --test-threads=2` |
| `churn` | `./scripts/collabora-up.sh` with each timer set, `scripts/synapse-up.sh default`, `cargo test -p dcp-interop --locked --test collabora_churn -- --nocapture --test-threads=1` |
| `wopi-sim` | `cargo test -p dcp-wopi --locked --lib --test host_semantics --test client_store --test conformance_over_wopi` |

`<tag>` is `relaxed` (`synapse-up.sh relaxed`: every `rc_*` limit 10 000/s), `default` (`synapse-up.sh default`: no
rate-limit key, Synapse's own: `rc_message` 0.2/s burst 10 per user), or **`default-drained`** (`synapse-up.sh default`
plus `DCP_DRAIN_BURST=1`: once a rig is built and read, each of its three principals sends into a scratch room
until the first `M_LIMIT_EXCEEDED`, 4 to 5 events for the owner and 8 to 9 for the editor and viewer in the CI log,
so the bucket is empty when the check or journey starts, and an event then waits for the 0.2/s refill, up to about 5 s). Every run
prints `TIMING` lines and writes them to `$DCP_MEASURE_OUT`: seconds for rig setup and for the check, events
sent, `M_LIMIT_EXCEEDED` answers and seconds the client waited for the limiter (the binding's own counters).

## 1. The interop suites on the default profile

How the harness changed (`dcp-interop/src/profile.rs`, `rig.rs`): the interop tests used to require `relaxed`; they
accept `relaxed` or `default` and refuse any other profile name. `RigOptions::settle` is now optional and defaults per
profile; every check and journey runs under a per-profile deadline (relaxed 15 min, default 45 min); the journeys
run one at a time through `run_all_timed` so each has a timing. An assertion is not changed for the default profile. A
check or journey that really fails there would be listed in `DEFAULT_PROFILE_FAILURES` with the phrase its failure
must contain, and the suite would assert exactly that failure; the table is empty. Two harness changes were
needed to run on a spent budget, and both are about the *test*, not the binding: a forgery check sends an event around
the store's role check with no queue, and on the default profile **Synapse answers `M_LIMIT_EXCEEDED` before it looks at the sender's power
level**, so a user whose budget is spent got `RateLimited` where the check expected 403 (8 of 26 checks failed that way in the drained
cross-binding job of run [40138096615](https://github.com/tuna-os/gtk-office-suite/actions/runs/40138096615)). The forging sends now
repeat on 429 until the server gives its verdict on the event (`send_unchecked_patiently`). And the first version of the drain ran before the rig was built, which let the
editor's and viewer's buckets refill while the owner built the room; it now runs at the end of rig setup.

"slow-on-default" below is a measured label with a stated rule, not a guess: **drained run at least 3 times the relaxed run and at least 10 s longer**.
The harness' own informational label (over 60 s) never fired: the longest run was 40 s.


**MatrixStore (journeys)**

| | relaxed s | default s (429s) | default-drained s (429s) | drained: s waited on the limiter | drained / relaxed | slow-on-default |
|---|---|---|---|---|---|---|
| `convergence_on_one_head` | 5.4 | 7.2 (0) | 10.4 (1) | 1 | 1.9x | no |
| `corrupt_file_rejected` | 4.5 | 4.5 (0) | 7.5 (1) | 3 | 1.7x | no |
| `forged_version_from_a_viewer_is_not_accepted` | 3.6 | 3.6 (0) | 8.1 (0) | 0 | 2.3x | no |
| `lease_exclusion_and_readback` | 4.7 | 5.8 (0) | 19.6 (3) | 12 | 4.2x | yes |
| `lease_expiry_releases` | 14.8 | 15.6 (0) | 19.6 (1) | 4 | 1.3x | no |
| `revocation_hides_new_content` | 9.7 | 9.4 (0) | 14.1 (1) | 4 | 1.5x | no |
| `roles_enforced_at_store` | 3.9 | 3.7 (0) | 11.9 (2) | 7 | 3.0x | no |
| `second_head_shown_not_hidden` | 4.1 | 7.0 (0) | 13.1 (2) | 7 | 3.2x | no |
| `truncated_upload_never_published` | 3.1 | 3.6 (0) | 8.3 (1) | 4 | 2.7x | no |
| `unknown_algorithm_falls_back_to_versions` | 3.7 | 3.4 (0) | 22.5 (4) | 17 | 6.0x | yes |
| `update_stream_gap_recovery` | 3.3 | 3.7 (0) | 28.1 (5) | 22 | 8.6x | yes |

**WopiClientStore -> WopiHost -> MatrixStore (journeys)**

| | relaxed s | default s (429s) | default-drained s (429s) | drained: s waited on the limiter | drained / relaxed | slow-on-default |
|---|---|---|---|---|---|---|
| `convergence_on_one_head` | 6.4 | 8.4 (0) | 21.1 (2) | 8 | 3.3x | yes |
| `corrupt_file_rejected` | 5.3 | 5.7 (0) | 17.9 (3) | 11 | 3.4x | yes |
| `forged_version_from_a_viewer_is_not_accepted` | 5.1 | 4.6 (0) | 18.1 (2) | 8 | 3.5x | yes |
| `lease_exclusion_and_readback` | skipped | skipped | skipped | | | |
| `lease_expiry_releases` | skipped | skipped | skipped | | | |
| `revocation_hides_new_content` | skipped | skipped | skipped | | | |
| `roles_enforced_at_store` | 5.1 | 4.5 (0) | 18.1 (3) | 12 | 3.5x | yes |
| `second_head_shown_not_hidden` | skipped | skipped | skipped | | | |
| `truncated_upload_never_published` | 5.2 | 5.7 (0) | 28.4 (5) | 20 | 5.5x | yes |
| `unknown_algorithm_falls_back_to_versions` | skipped | skipped | skipped | | | |
| `update_stream_gap_recovery` | skipped | skipped | skipped | | | |

**Cross-binding checks**

| | relaxed s | default s (429s) | default-drained s (429s) | drained: s waited on the limiter | drained / relaxed | slow-on-default |
|---|---|---|---|---|---|---|
| `a_demoted_editor_cannot_publish_and_readers_drop_what_slips_through` | 2.2 | 3.7 (0) | 7.3 (1) | 1 | 3.3x | no |
| `a_demoted_editor_cannot_publish_and_readers_drop_what_slips_through@draft0` | 2.8 | 4.0 (0) | 15.1 (2) | 3 | 5.4x | yes |
| `forced_wopi_save_forks_and_matrix_shows_the_conflict` | 2.6 | 4.2 (0) | 8.1 (2) | 5 | 3.1x | no |
| `forged_update_from_a_viewer_is_dropped_by_readers` | 0.9 | 1.3 (0) | 4.0 (0) | 0 | 4.5x | no |
| `forged_update_from_a_viewer_is_dropped_by_readers@draft0` | 1.2 | 1.7 (0) | 8.1 (0) | 0 | 6.5x | no |
| `forged_version_is_dropped_by_matrix_readers` | 0.8 | 1.4 (0) | 4.3 (0) | 0 | 5.3x | no |
| `forged_version_is_dropped_by_matrix_readers@draft0` | 1.1 | 1.7 (0) | 8.2 (0) | 0 | 7.1x | no |
| `forged_version_is_dropped_by_wopi_readers` | 1.3 | 2.1 (0) | 5.0 (0) | 0 | 3.9x | no |
| `forged_version_is_dropped_by_wopi_readers@draft0` | 1.5 | 2.7 (0) | 8.7 (0) | 0 | 5.9x | no |
| `native_lease_blocks_wopi_save` | 1.4 | 1.4 (0) | 2.5 (1) | 1 | 1.9x | no |
| `native_lease_expiry_frees_the_wopi_lock` | 7.0 | 7.5 (0) | 8.9 (1) | 1 | 1.3x | no |
| `native_offline_publish_shows_as_a_conflict_to_the_matrix_reader` | 1.9 | 2.2 (0) | 15.2 (3) | 12 | 8.0x | yes |
| `native_publish_between_getfile_and_putfile_is_a_409` | 1.9 | 2.4 (0) | 14.5 (3) | 11 | 7.5x | yes |
| `native_publish_opens_through_wopi_getfile` | 0.9 | 0.8 (0) | 3.1 (1) | 2 | 3.7x | no |
| `owner_break_invalidates_the_wopi_lock` | 1.5 | 1.6 (0) | 6.0 (1) | 4 | 4.0x | no |
| `put_file_on_a_conflicted_document_is_refused` | 5.2 | 5.7 (0) | 19.5 (4) | 13 | 3.8x | yes |
| `refresh_lock_and_unlock_and_relock_are_new_lease_records_on_matrix` | 3.7 | 3.9 (0) | 34.9 (7) | 28 | 9.4x | yes |
| `removed_members_wopi_token_gets_no_new_content_and_no_write` | 15.1 | 15.2 (0) | 28.2 (3) | 12 | 1.9x | no |
| `two_wopi_hosts_racing_for_one_lock_have_one_winner` | 3.1 | 4.5 (1) | 40.0 (9) | 37 | 13.0x | yes |
| `two_wopi_sessions_on_one_document_never_share_a_lock` | 4.1 | 4.6 (0) | 28.7 (6) | 22 | 6.9x | yes |
| `viewer_token_cannot_write` | 1.4 | 1.5 (0) | 14.6 (3) | 12 | 10.5x | yes |
| `wopi_lock_blocks_native_acquire_and_the_reverse` | 2.3 | 2.3 (0) | 20.0 (4) | 16 | 8.8x | yes |
| `wopi_lock_on_an_empty_document_omits_base_version` | 1.5 | 1.6 (0) | 20.0 (4) | 17 | 13.6x | yes |
| `wopi_lock_refresh_is_a_new_record_on_matrix` | 3.4 | 3.2 (0) | 34.5 (7) | 28 | 10.2x | yes |
| `wopi_publish_is_read_by_a_matrix_user` | 1.7 | 1.5 (0) | 14.8 (3) | 12 | 8.5x | yes |


Totals from the `TIMING` lines (the 25 check runs sent 93 events and the 22 journey runs 85, on every profile),
`M_LIMIT_EXCEEDED` answers for checks / journeys: relaxed 0 / 0; default 1 (waited 1 s) / 0; **default-drained 65 (237 s waited) / 36 (140 s waited)**. Journeys:
relaxed median 4.9 s (3.1 to 14.8), default 5.2 s (3.4 to 15.6), default-drained 18.0 s (7.5 to 28.4); 8 of the 16
journeys that run are slow-on-default by the rule. Checks: 1.9 s (0.8 to 15.1), 2.3 s (0.8 to 15.2), 14.5 s (2.5 to
40.0); 12 of 25 are slow-on-default. Plain `default` against `relaxed`: median 1.06 times (journeys), 1.12 (checks), within the
run-to-run noise of one run.

What this says and does not say:

- **No journey or check fails on the default profile, drained or not, for a reason of the binding.** In particular
  lease expiry (journey 5, 4 s leases; the native 6 s expiry check) passes drained. [INFERENCE for why: the lease
  is stamped with the server's `origin_server_ts` of the event, so a write the limiter delays is stamped late, not early.]
- The two journeys that were slowest relative to relaxed (`update_stream_gap_recovery` 8.6 times, `unknown_algorithm_falls_back_to_versions` 6.0
  times) publish four updates in a row from one user (the editor): one bucket empties, 5 and 4 `M_LIMIT_EXCEEDED` answers, 22 and 17 s waited.
- `default-drained` models a user who has been editing. It is one way to spend the budget (the scratch-room
  loop is in `profile::drain_message_budget`), on one user per principal, one account per test.
- Only `rc_message` is exercised. `rc_joins`, `rc_invites` (Synapse's per-user burst of 5 invites is not reached: a rig
  invites twice), `rc_login` and `rc_registration` have their own defaults and were not pushed; the shared-secret
  user creation never failed on the default profile (as in spike A).

## 2. A real Collabora session against the default limits (spec question 10)

`tests/collabora_churn.rs`, job `churn`. Collabora opens a document served by `WopiHost<MatrixStore>` from a room on
the default Synapse (a fresh editor: burst intact), a user types `DcpChurnNNN ` every 5 s for 180 s (36 `textinput`
messages through the COOL WebSocket, no explicit save), the tab closes, the test waits for `UNLOCK`. Everything is
counted from the room: the owner pages `GET /rooms/{id}/messages` (state events included) and buckets the events
stamped after the load started; next to it the host's request log and the editor store's own counters. The window is
187 s (180 s of typing, the close, 5 s). The effective defaults come from the image's own `coolwsd.xml` (printed by the job):
`autosave_duration_secs` 300, `idlesave_duration_secs` 30, `storage.wopi.locking.refresh` 900.

| Collabora timers | LOCK | PutFile (200) | UNLOCK | events in the room (state + timeline) | per minute (0 to 3) | 429s | budget `10 + 0.2 x 187` |
|---|---|---|---|---|---|---|---|
| **its own** (nothing set) | 1 | 4: 3 autosave, 1 exit-save | 1, 1.0 s after the close | **6** (2 + 4) | 2, 1, 1, 2 | 0 | 47 |
| autosave 30 s, idle save 10 s, refresh 60 s | 3 | 10: 9 autosave, 1 exit-save | 1 | **14** (4 + 10) | 4, 4, 4, 2 | 0 | 47 |
| autosave 5 s, idle save 3 s, refresh 10 s | 18 | 21 autosave | 1 | **40** (19 + 21) | 12, 13, 14, 1 | 0 | 47 |

- Each `PutFile` is one version, a Matrix event of the editor (a 1:1 count: 4, 10 and 21 `PutFile` against 4, 10 and 21
  `m.room.encrypted` events), each `LOCK` refresh one lease state event; the count of events the room holds from the
  editor equals the count the editor's store sent (6, 14 and 40; asserted for the first two, printed for the third,
  which ran in stress mode). Both kinds draw on the same `rc_message` budget, as spike A said. Each version also
  uploads a file; uploads are not events and no limit on them was met.
- **Cadence, from the event timestamps, is not what the XML says.** At Collabora's own timers the saves came at
  +31, +91, +151 s (every 60 s) and an exit-save at +181 s, although `autosave` is 300 s and `idlesave` 30 s and the
  user typed every 5 s; why is not known from these runs. With 30 s/10 s configured the saves came every 20 s; with
  5 s/3 s configured every 10 s (the lease refresh every 10.5 s): Collabora saved no more often than every 10 s
  even when told 5. The lease refresh obeyed its setting (60 s and 10 s). These are measurements of one build.
- **The limit is not exceeded in 3 minutes by any of them.** A token bucket (burst 10, 0.2/s) fed with the editor's
  server timestamps would have been empty for 0 events in all three. The tightest run is 0.21 events/s overall and
  13 to 14 events/min (0.217 to 0.233/s) in minutes 1 and 2, above the sustained limit: at 13 events/min the 10-event burst alone would
  be gone after about 10 minutes of editing ([derived](#derived-numbers), not run). At Collabora's own timers a document
  open costs about 2 events/min, a tenth of the sustained limit, **per open document per user**: the budget is per
  user, so several open documents add.
- **Spent budget, aggressive timers** (`DCP_DRAIN_BURST=1`, same typing; measured, not required to finish cleanly): 38
  events sent in 275 s, 7 `M_LIMIT_EXCEEDED`, 16 s waited, 19 `PutFile` and 17 `LOCK` all answered 200, and the last
  typed text is in the newest version. **But the `UNLOCK` at the end was not answered in time**: Collabora's log
  has two `CheckTimeout: Timeout while requesting [POST .../wopi/files/...] after 3003ms`, each followed by `Failed to UNLOCK docKey ... Notifying client and making session read-only`, then
  `No session left to unlock docKey ... The lock cool-lock... stays held until the host expires it`. The host
  had queued the release behind the limiter: the lease did read `released` when the tail ended, but no `UNLOCK` ever
  completed in the host's log (the dropped request is not logged), and the room holds 37 of the 38 events the store
  counted as sent. The missing one is most likely the second `released` write, which Synapse drops as identical to
  the first ([INFERENCE]: consistent with the two timed-out UNLOCKs, not otherwise checked).
  No `PutFile` or `LOCK` timed out in Collabora's log in that run (0 `CheckTimeout` other than the two), although the
  first lease event is stamped 4.6 s after the load started, so the first `LOCK` took about 4.5 s: the 3003 ms is shown for
  the call Collabora makes at close, not shown to apply to every call.
- **Per operation over the wire** (`tests/wopi_latency.rs`: 6 cycles of `GetFile`, `LOCK`, 2 `PutFile`, `REFRESH_LOCK`,
  `UNLOCK` back to back, 36 calls, no Collabora; latency is from sending the request to the end of the body):

| | relaxed | default (fresh budget, 30 events back to back) | default-drained |
|---|---|---|---|
| `LOCK` median / max | 580 / 585 ms | 5 072 / 6 073 ms | 5 091 / 6 098 ms |
| `PutFile` median / max | 205 / 265 ms | 4 730 / 4 836 ms | 4 737 / 5 702 ms |
| `REFRESH_LOCK` median / max | 587 / 593 ms | 6 073 / 6 084 ms | 5 096 / 6 092 ms |
| `UNLOCK` median / max | 52 / 53 ms | 4 546 / 4 606 ms | 4 559 / 4 562 ms |
| `GetFile` median / max | 134 / 180 ms | 123 / 164 ms | 132 / 186 ms |
| calls over 3003 ms | **0 of 36** | **20 of 36** | **30 of 36** |

  A WOPI `LOCK` is at least 0.5 s on any profile (the settle interval, `HostConfig::lease_settle_ms`); a call that needs an
  event the limiter delays takes about 3.7 to 6.1 s. Not a Collabora cadence: this is the host's own worst case.

**Answer to question 10.** One version per `PutFile`, whether autosave, idle save or exit-save; one lease event per lock
refresh; both against the user's `rc_message`. At Collabora's own timers 6 events in 3 minutes; at timers 6 to 30 times
tighter 14 and 40, still inside the budget for 3 minutes. The limit is therefore not what breaks first. What breaks on a spent budget
is the **3 s the WOPI client allows the host**: an `UNLOCK` that waits for the limiter is abandoned, Collabora reports it
and makes the session read-only, and the lock is released only because the host's queue still delivers the write. Not tried:
a host that answers `LOCK`/`UNLOCK` before the Matrix write lands (write-behind), or one that answers 429/503.

## 3. The previously unverified items

All run on both Synapse profiles and in drained mode (the rows of the check table above); the room has the spec's power-level table
(`m.room.encrypted` gated at 50) unless it says draft-0 (the table of draft 0, no gate).

| Item | What is asserted | Result |
|---|---|---|
| **Forged UPDATE** (`forged_update_from_a_viewer_is_dropped_by_readers`, also `@draft0`) | A viewer sends two schema-valid `org.tunaos.dcp.update` straight to Synapse (`send_unchecked`). | Spec table: both refused 403 by the server. Draft-0 table: both accepted by the server (asserted, so the check cannot pass vacuously), and **every Matrix reader drops them**: each reader's `rejected_unauthorised` is 2, `updates()` lists only the editor's legitimate one. Control: with readers that trust the sender the check fails ("a viewer's forged update was accepted by a Matrix reader"). |
| **Forged version from a demoted ex-editor** (`a_demoted_editor_cannot_publish_and_readers_drop_what_slips_through`, also `@draft0`) | The editor publishes v2, the owner demotes (`grant(.., Viewer)`), the ex-editor tries a store publish, a WOPI `LOCK` and `PutFile`, and sends a forged version and update around the store. | **Spec table: the store refuses (Forbidden), WOPI `LOCK` and `PutFile` are 403, Synapse refuses both forgeries (403), v2 stays listed and the owner's later save is the head.** Draft-0 table: Synapse accepts both forgeries; every reader drops them; and **also drops v2 and rejects the owner's save for its unknown parent: the readers show the first version** (3 of 3 runs, `FINDING` line in the log). Control: readers that trust senders show the forgery as a second head. |
| **`PutFile` on a conflicted document** (`put_file_on_a_conflicted_document_is_refused`; `dcp-wopi` test `put_file_on_a_conflicted_document_is_refused`) | Four attempts (editor with its lock, with and without Collabora's timestamp; owner without a lock, same) after a forced save forked the document. | **Before the fix the host accepted**: the first attempt got 200 (run locally over the in-memory store, against the unmodified host, commit `78f69b2` plus the test; the test then failed at "a PutFile on a conflicted document must be refused, left: 200 right: 409"; the rest of the attempts were not probed). After the fix (`fc6c59a`): all four 409 with `COOLStatusCode` 1010 and no `X-WOPI-Lock`, nothing published (every principal lists the same 3 versions 2 s later), and after a native merge the same session can save (child of the merge). Control: a host whose view hides the second head fails the check ("the forked document must open read-only"). |
| **`REFRESH_LOCK`, `UNLOCK_AND_RELOCK` over Matrix** (`refresh_lock_and_unlock_and_relock_are_new_lease_records_on_matrix`) | Three refreshes: new event, later timestamp, renewal 1, 2, 3, holder, token, base version and 1 800 000 ms kept; wrong string 409 carrying the current one and no record written; relock a to b is one record with renewal 4 over the held one, same holder and base; old string dead (`PutFile` 409 carrying b), wrong `OldLock` 409; refresh and relock on an unlocked file 409 with an empty lock. | Passes on all three profiles (3.7 s relaxed, 34.9 s drained). |
| **Two WOPI sessions on one document** (`two_wopi_sessions_on_one_document_never_share_a_lock`) | Editor and owner tokens through one host: B's `LOCK` while A holds it is 409 carrying A's string, and B's `PutFile` (with and without its string), `UNLOCK`, `REFRESH_LOCK`, `UNLOCK_AND_RELOCK` are 409 carrying A's; the lease record is the **same event, body and timestamp** afterwards; A saves; after A's `UNLOCK` B locks (renewal 0, B as holder) and A's old string is the mismatch. Three rounds of both `LOCK` at once: exactly one 200, the other 409 carrying the winner's string. | Passes on all three profiles. One host serialises per document (`doc_guard`), which is why the race is deterministic here. |
| **Two hosts, one room** (`two_wopi_hosts_racing_for_one_lock_have_one_winner`) | Two `WopiHost`s in front of one room (so no shared mutex), each with a different principal, both `LOCK` at once, four rounds. One 200 and one 409 unless the two lease writes are at least the settle interval apart, which is then printed as `OUTSIDE-TOLERANCE` and the losing host's `PutFile` must be refused 409 with the holder's string. | Relaxed: one winner in 4 of 4 rounds (host 1 three times, host 2 once). Default: one winner in 4 of 4 (2 and 2). **Default-drained: both hosts answered 200 in 3 of 4 rounds (writes 4 504, 5 505 and 5 506 ms apart), one winner in the fourth.** In each double-200 round the losing host's `PutFile` was refused 409 carrying the holder's string and published nothing (asserted). **The check was loosened once:** its first version asserted one winner in every round and failed on the drained profile (run 40136145008: both 200, writes 1 007, 4 506, 5 500 and 4 501 ms apart); it now asserts what the rule promises (one winner when the writes are within the settle interval) plus the containment above, and prints the rest. That is a judgment call; the stricter form fails drained. |

The negative controls (job `controls`, 13 tests, relaxed) all passed: each control fails "by the mismatch it should state"
(including the three new ones above); the log lines are in the job's artifact.

## 4. The lease two-read rule under stress

`tests/lease_race.rs`. Per round: a third principal (the owner) releases the lease; two editors with no client-side
limiter start together (even rounds) or the second 0 to 119 ms late (odd rounds, as in spike A); each runs
`lease::available`, `lease::write`, **one observed read-back** (what draft 0 would have acted on), then the real
rule `lease::confirm` (read back, 500 ms settle, read back again, both must name the session and the `renewal` it
wrote). 1.5 s later the owner reads the final state. 200 rounds per profile; a round where one session saw the other's
active lease at its availability check and did not write is counted separately.

| | relaxed | default |
|---|---|---|
| rounds / total time | 200 / 491 s | 200 / 981 s |
| events sent / `M_LIMIT_EXCEEDED` / time waited | 519 / 0 / 0 s | 531 / 194 / 450 s |
| rounds in which both sessions wrote | 119 | 131 |
| rounds where one session was refused at the availability check | 81 | 69 |
| **first read-back: both sessions believe they hold it** | **37 (18.5 % of rounds, 31 % of rounds where both wrote)** | **69 (34.5 %, 53 %)** |
| first read-back: exactly one | 163 | 131 |
| first read-back: neither | 0 | 0 |
| **two-read rule: both acquired** | **0** | **0** |
| two-read rule: exactly one acquired / none | 200 / 0 | 200 / 0 |
| final state (1.5 s later) names the one session the rule let acquire | 200 of 200 | 200 of 200 |
| first read wrong and resolved by the second | 37 of 37 | 69 of 69 |
| gap between the two writes' `origin_server_ts`, ms: p50 / p95 / max | 1 / 49 / 81 | 1 / 39 / 68 |

Two earlier CI runs of the same test ([40133951142](https://github.com/tuna-os/gtk-office-suite/actions/runs/40133951142), commit `5e59afd`, and
40138474867, commit `dce4832`) gave first reads wrong 45 and 41 of 200 relaxed, 80 and 56 of 200 default, and the same 0 / 200 / 200 / 200
for the rule: in all 1 200 rounds of the three runs (3 runs x 2 profiles x 200), the two-read rule never let two sessions acquire and the
final state always agreed. Spike A's one-read rule: 7 of 40 (17.5 %) with 40 rounds.

On the default profile the limiter did not separate the two writers: both queues waited for about the same
retry-after, and the writes stayed within 68 ms of each other (the same quantity as relaxed). That is the natural case. It
is not the case of the next table.

**Where the settle interval stops being enough** (`two_read_rule_tolerance`): both sessions read the lease free, session
a writes at once, session b writes a chosen time later over the same stale record; each then runs `confirm`.
"Both acquired" is the failure. Measured write gap is bob minus alice from the server timestamps.

| asked gap (ms) | relaxed: measured gap, ms | relaxed: both acquired / exactly one | default: measured gap, ms | default: both acquired / exactly one |
|---|---|---|---|---|
| 100 | 102 to 125 | 0 / 6 | 99 to 129 | 0 / 3 |
| 300 | 298 to 304 | 0 / 6 | not run | |
| 450 | 451 to 454 | 0 / 6 | 450 to 451 | 0 / 3 |
| 550 | 552 to 555 | **5** / 1 | not run | |
| 700 | 699 to 713 | **6** / 0 | 700 to 701 | **3** / 0 |
| 1 000 | 983 to 1 004 | **6** / 0 | not run | |

The earlier runs: 550 ms gave 6 of 6 and 2 of 6 both-acquired (so "about 550 ms"), 450 and below never. The boundary is the settle
interval (500 ms) plus the two read latencies, as the rule's wording says ("at least as long as the gap between
competing writes the binding must tolerate"). **What the rule does not cover is a write the homeserver delays.** The
two-host check in section 3 under a spent budget is that case at the WOPI layer: writes 4.5 to 5.5 s apart, both
`LOCK`s 200. What contains it there is not the rule but the lease record, which every `PutFile` is judged against.

## Spec corrections

Proposed; the spec was not edited.

1. **§8.2 rule 6**: add that the adapter MUST refuse a `PutFile` on a document with more than one head, 409 with
   `COOLStatusCode` 1010 and no `X-WOPI-Lock` (as for a changed document), in addition to not offering a writable session. Measured:
   without it a client that ignores `UserCanWrite` gets 200. Implemented in `dcp-wopi` (`do_put`, step 0).
2. **§8.1 reader rule** ("SHOULD drop DCP timeline events from senders below the editor level whatever the server
   accepted"): as written, the level is the room's *current* one, so a demotion in a room without the gate retroactively
   drops the demoted member's legitimate history and every later version built on it (3 of 3 runs). Either require
   the gate (the spec's own table has it; the draft-0 table is the only way to meet this) and say readers MUST NOT apply the rule to a room that has it,
   or require the level in force **at the event**. Matrix has a way to ask for the state at an event
   (`GET /rooms/{id}/context/{eventId}` returns it; [INFERENCE], not tried). Also: in a *gated* room a reader trusts the server, so a
   forged event that a server accepted would be accepted; the spec's wording makes that explicit by applying the rule only
   when the table lacks the entry, and no test can produce such an event on one Synapse.
3. **§4.3 rule 3 and §8.1 "Lease" / §13 question 3**: replace "The two-read rule itself was not run" with the numbers of section 4
   (1 200 rounds over three runs, 0 double acquisitions, final state always agreeing; the single read-back wrong in
   18.5 to 22.5 % of rounds relaxed and 28 to 40 % default). State the tolerance: the guarantee holds for writes that land less than
   about 450 ms apart and fails from about 550 ms (settle 500 ms). **Add that a binding whose sends can be delayed by
   a rate limiter has no bound on that gap**, observed at 4.5 to 5.5 s through two WOPI hosts on spent budgets; a session therefore
   cannot treat "both read-backs name me" as exclusion on such a binding, and MUST judge every save against the lease record at save time
   (the WOPI host does). Candidate for a stronger rule, not tried: have the writer check what its write replaced
   (`unsigned.replaces_state` / `prev_content` of the event it just sent [INFERENCE: present for state events, not checked
   here]) and treat a replaced record that is not the one it read as a lost race.
4. **§8.2 call order** (and "Not measured"): Collabora gives the host **3003 ms** for the `UNLOCK` it sends when the last
   editing session disconnects, retries it once 3 s later, and then logs `Failed to UNLOCK`, makes the session read-only and says the lock
   stays held until the host expires it. A host over a rate-limited binding MUST answer within that time or accept that the release lands later
   (here the queued write still landed). Measured: `LOCK` 0.57 to 0.59 s, `PutFile` 0.14 to 0.27 s, `UNLOCK` 0.05 s on relaxed; 3.7 to 6.1 s for
   every call that needs an event on a spent budget (`wopi_latency`). The same section's "Not measured" list shrinks: autosave and exit-save
   `PutFile`, the adapter over the Matrix binding, `REFRESH_LOCK`/`UNLOCK_AND_RELOCK`, two sessions (simulator) and a forced save
   after a conflict (simulator) are now measured; two real Collabora sessions are not.
5. **§13 question 10** can be closed with: one version per `PutFile` (autosave, idle save and exit-save alike; none coalesced), one
   lease event per refresh, both charged to the same `rc_message`; 6, 14 and 40 events in 187 s at three timer settings, none near the
   burst; the failure mode on a spent budget is the 3 s above, not the limit.
6. **§8.1 "Ordering and rate"**: Synapse answers `M_LIMIT_EXCEEDED` before it checks the sender's power level, so a 429
   says nothing about authorisation (a forgery check must retry past it); and a fresh user's burst of 10 covers a
   whole conformance journey (3 to 8 events), so a default-profile run proves nothing about the limiter unless the budget is spent first.

## Not verified

- **Two real Collabora sessions on one document** (two users in one document broker; Collabora's behaviour when two
  access tokens save), and Collabora's reaction to the new 1010 refusal on a conflicted document, to a lease broken by an owner during an
  edit, or to a double-200 `LOCK`. All the multi-session checks use our own WOPI client.
- **Churn longer than 3 minutes** and the regime where the limit is exceeded by Collabora's own cadence: the three
  Collabora settings never produced a 429 on a fresh account. The spent-budget run is one run, one timer setting (the
  aggressive one), 35 `textinput`s; the `UNLOCK` timeout was seen twice in that run and not in the other three.
  Whether a 3003 ms limit applies to `LOCK` or `PutFile` is not shown (none timed out, with calls of 4.5 s).
- **Why Collabora saves every 60 s at its own timers** (and every 10 s when told 5): not explained.
- **A host that answers before the write lands (write-behind), or with 429/503**: described, not built. The
  `replaces_state` check of correction 3: not built.
- **Any rate limit other than `rc_message`** (joins, invites, login, registration, media), other homeservers (Conduit,
  Dendrite, Synapse workers, a Synapse with different `rc_message` settings), federation, a real network, release builds,
  any Matrix client other than ours.
- **The spent budget is one model of a busy user** (a scratch-room loop run at the end of rig setup, once per principal). A user
  who spent it in the same room, or whose budget refills while the check runs, is not modelled.
- **`GET /rooms/{id}/context/{eventId}` as a way to judge senders at the event** (correction 2): not tried.
- **The retroactive reader effect in a gated room**: cannot be produced here (the server refuses the ex-editor); only
  the draft-0 room shows it.
- From the interop results' list, **still not verified**: a new device of an existing member reading a WOPI-served document, L2/L3 through
  WOPI (none exist), other homeservers, other WOPI clients, other Collabora builds, federation.

## Derived numbers

Arithmetic on measured values, not run: the budget of `10 + 0.2 x t` events at Synapse's default; the burst lasting
about 10 minutes at 13 events/min (`10 / (13/60 - 0.2)` = 10 / 0.0167 = 600 s); the share of the sustained limit at Collabora's own timers
(2 events/min = 0.033/s against 0.2/s).

## Runs on the way

| Run | Commit | What it showed |
|---|---|---|
| [40133951142](https://github.com/tuna-os/gtk-office-suite/actions/runs/40133951142) | `5e59afd` | First run. Compiled and passed except one check: the ex-editor's pre-demotion version was gone from the owner's own view in a draft-0 room (the retroactive effect, found by my check's assumption failing). Default-profile suites passed with 0 `M_LIMIT_EXCEEDED`: the burst hides the limit. Lease race, tolerance and churn data first appeared here. |
| 40135657197 | `4652610` | Compile error in the new harness (a shadowed name); superseded by the next push. |
| [40136145008](https://github.com/tuna-os/gtk-office-suite/actions/runs/40136145008) | `096d254` | First `default-drained` runs (drained before the rig was built); two-host race 200/200 first seen; churn gave the first cadence data. |
| 40138096615 | `a0bf62e` | Drain moved to the end of rig setup; 8 forgery checks failed on a spent budget (429 before 403), fixed by retrying forged sends; cancelled by the next push. |
| 40138474867 | `dce4832` | All green (no spent-budget churn yet). |
| 40140055278, 40141458607 | `8d36797`, `6e926ad` | Green except the new spent-budget churn run, whose counting assertion failed (38 sent, 37 in the room; Collabora's UNLOCK timeouts found there); `wopi_latency` added in the second. |
| [40142775201](https://github.com/tuna-os/gtk-office-suite/actions/runs/40142775201) | `513086d` | **All 14 jobs green: the numbers above.** |

## Reproducing

```
cd tools/collab-spike
# profiles: relaxed | default; default-drained = default plus DCP_DRAIN_BURST=1
eval "$(scripts/synapse-up.sh default)"
DCP_DRAIN_BURST=1 DCP_MEASURE_OUT=/tmp/out.jsonl cargo test -p dcp-interop --test conformance --test cross_binding -- --nocapture --test-threads=6
DCP_RACE_ROUNDS=200 cargo test -p dcp-interop --test lease_race -- --nocapture
scripts/synapse-down.sh default

# Collabora churn (own timers; add COLLABORA_EXTRA_PARAMS / COLLABORA_LOCK_REFRESH_SECS for the others)
COLLABORA_LOCK_REFRESH_SECS=default DCP_WOPI_HOST_PORT=18080 scripts/collabora-up.sh
eval "$(scripts/synapse-up.sh default)"
DCP_WOPI_HOST_PORT=18080 DCP_CHURN_LABEL=stock cargo test -p dcp-interop --test collabora_churn -- --nocapture --test-threads=1

cargo test -p dcp-wopi --lib --test host_semantics   # includes put_file_on_a_conflicted_document_is_refused
```

Without `DCP_SYNAPSE_URL` (or `DCP_SKIP_SYNAPSE=1`) every Synapse test fails, `lease_race` included (the job checks that); a
spent-budget request on any other profile than `default` panics.
