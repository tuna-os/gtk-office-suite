# RFC-0004 Spike Q15: the power level of a sender AT an event

Date: 2026-10-25 · Tracks: [RFC-0004](0004-interoperable-document-collaboration.md), DCP draft 1 [§8.1](../DOCUMENT-COLLABORATION-SPEC.md) (reader rule) and §13 question 15 · Code: [`tools/collab-spike/dcp-matrix/`](../../tools/collab-spike/dcp-matrix/) (`src/levels.rs`, `read_report_with` in `src/store.rs`, `tests/levels.rs`), [`dcp-interop/src/power_at_event.rs`](../../tools/collab-spike/dcp-interop/src/power_at_event.rs) and `tests/power_at_event.rs` · Workflow: [`collab-spike.yml`](../../.github/workflows/collab-spike.yml) (the spike ran it as its own `collab-spike-q15.yml`, merged into it at integration; now the job `q15`, and the spike's `conformance` job is the `matrix` and `interop` jobs) · Input: [Spike F](0004-spike-f-results.md), section "The previously unverified items" (the finding this closes)

**Every number below is from one CI run ("run A"), [40186028967](https://github.com/tuna-os/gtk-office-suite/actions/runs/40186028967)
(workflow `collab-spike-q15`, branch `spike/collab-q15`, commit `92c7466`), 7 of 7 jobs green, unless another run is named. The whole workflow was run again on the
final code ("run B", [40192035297](https://github.com/tuna-os/gtk-office-suite/actions/runs/40192035297), commit `fb2dfa4`, which only adds `level_for`/`state_default`): 7 of 7 green, the same verdicts, and costs given
beside run A's where they differ.**
Synapse 1.163.2, started by `scripts/synapse-up.sh` from `docker.io/matrixdotorg/synapse:v1.163.2@sha256:c3c21919a322f74d177a43bce8031adc81ef839ddd117a8dd03cfec350e76e02`
(image ID `sha256:260cd9c0…`; SQLite, one homeserver, no federation, loopback), and once from `matrixdotorg/synapse:latest`
(also 1.163.2, image ID `sha256:e0d446e9…`, the one Spike F used); matrix-sdk 0.19.1; `ubuntu-24.04` runner (image 20261012.334.1,
4 vCPU, 16 GB), docker 28.0.4, rustc 1.99.1, debug builds. The profiles are those of `scripts/synapse-up.sh`: `relaxed` (every `rc_*` limit 10 000/s),
`default` (no rate-limit keys: Synapse's own, `rc_message` 0.2/s burst 10 per user) and `default-drained` (`default`, and every principal's message
budget spent before each check, `DCP_DRAIN_BURST=1`). One run per statement; nothing here says anything about other homeservers, a real network,
federation, or release builds.

## Result

1. **The question has an answer, and four ways of getting it work.** A reader that judges a sender by the power level in force at the event
   gets every verdict right in all six scenarios, on `relaxed`, `default`, `default-drained` and on `latest`: `GET /rooms/{id}/context/{eventId}?limit=0`
   through raw HTTP and through matrix-sdk's own `Room::event_with_context`, a forward walk of the `m.room.power_levels` events of the
   `/messages` pass the reader already makes, and a backward walk of the same pass through `unsigned.prev_content` (and the hybrid of section 5). **In each job
   65 reader runs (13 per rule) got 0 verdicts wrong, and in scenarios 1 to 5 the rules agreed on every level they found.** The one exception is by design: in scenario 6
   the forward walk places 3 of the 5 events of a reader's window and leaves 2 unresolved (dropped), which is the failure the other rules do not have.
   The negative controls fail where they must: **draft 1's current-table reader got 24 verdicts wrong in 10 runs per job and the reader that trusts the sender 20 in 8
   runs per job** (identical in all four jobs), and the current-table reader fails scenarios 1 and 2 as the brief demands (and 3, 4, 5).
2. **`/context` costs nothing in rate limits.** 620 requests (300 one after the other, 320 from 8 tasks) got 0 `M_LIMIT_EXCEEDED` on `default` and on
   `default-drained` (and on `relaxed`); every cold read of 31 versions made 31 requests and met 0. On `default` a user who made 300 `/context` requests had
   the same send budget left as one who made none (9 sends before the first 429 for a fresh user; 3 sends, 300 requests, then 6 more; control: 3 + 5 sends, then 1 more).
   Source (read, not the measured image): Synapse's `RoomEventContextServlet` on `develop`, fetched 2026-10-25, makes no rate-limit call; ruma-client-api 0.25.1's `get_context` is `rate_limited: false`.
3. **The cost is small and per event, and the profile does not change it.** Listing 30 versions (31 with the first), cold cache, adds a median of **84 to 173 ms (2.7 to 5.6 ms a version) over a reader that checks nothing, in 31
   requests and 134 KB (4.3 KB each, state filtered to the two events that matter)**: run A 106 / 152 / 162 ms on `relaxed` / `default` / `default-drained` (86 on `latest`), run B 142 / 105 / 84 (173 on `latest`). The same job
   differs by up to 1.9 times between the two runs and the profiles do not order consistently, so the spread is the runner's, not the rate limit's. matrix-sdk's own call is 3.6 to 4.8 times slower per event for the same request
   (11 to 20 ms a version). A warm read (the cache is keyed by event ID, state at an event never changes) makes 0 requests. The walks add 0.0 to 4.9 ms for all 31 versions, which is noise: the current-table reader, which does
   no extra work, shows 4.8 ms in the same job of run A.
4. **A walk is exact only if the reader can see every power-levels change, and a plain walk fails silently when it cannot** (scenario 6, not in the brief, found
   while choosing the default). With `history_visibility: joined` a late joiner does not see the demotion sent before it joined; the plain forward walk then judges the
   ex-editor's forged event by the table that demotion had replaced and **accepts it**. Linking each table to the next through `unsigned.replaces_state` leaves those
   events unresolved instead; the backward walk (the hidden table is in the next event's `prev_content`) and `/context` get all five events of the window right.
5. **Chosen: the hybrid** (`SenderRule::Hybrid`, now the default of `MatrixConfig`): the linked forward walk for a room created with `m.federate: false`
   (what the binding creates), `/context` (cached by event ID) for each event the walk cannot place and for every event of a room that federates. Zero extra requests
   in the usual case (0 in the 12 hybrid reader runs of scenarios 1 to 5 per job; 2 in scenario 6, the two events the walk could not place), authoritative where it
   matters. An event whose level cannot be established is dropped, never judged by the current table.
6. **Offline from cached state: yes for the walks, no for a cold `/context`.** The walks are pure functions of the cached events (`levels::judge_transcript`, 17 events,
   16 KB as a string): the offline levels equal the online ones for all 4 events of the test under both walks, say no to the ex-editor's forged event and yes to
   the earlier version; a cache without the power-levels events judges nothing. `/context` needs the server for every event not yet cached by ID.
7. **Nothing regressed.** `dcp_conformance::run_all` passes through `MatrixStore` (7 tests of `dcp-matrix/tests/conformance.rs`, 10 of `journeys.rs`) and through
   `MatrixStore` and `WopiHost<MatrixStore>` (5 of `dcp-interop/tests/conformance.rs`) on `relaxed` and `default`; the draft-0 room runs under each of the five at-event rules (the default,
   the hybrid, in the existing literal-table test, the other four in tests of their own). The existing demoted-editor check and its negative control pass (the check's draft-0 branch
   now asserts the corrected behaviour, section 6), and so do the other existing suites that ran on this branch except for failures that are not about Matrix (section 7).

## What was run

Workflow [`collab-spike.yml`](../../.github/workflows/collab-spike.yml) (job `q15`), all in `tools/collab-spike` after `eval "$(scripts/synapse-up.sh <profile>)"`:

| Job | Commands |
|---|---|
| `check` | `cargo check --workspace --all-targets --locked`; `cargo test -p dcp-matrix --lib --locked` (23 unit tests, 13 of them new in `levels.rs`: both walks, the chain check, the level arithmetic including `state_default`, `judge_transcript`, the federation test) |
| `levels relaxed` / `default` / `default-drained` / `relaxed-latest` | `cargo test -p dcp-matrix --locked --test levels -- --nocapture --test-threads=1` (5 tests: the measurements); `cargo test -p dcp-interop --locked --test power_at_event -- --nocapture --test-threads=3` (8 tests: the six scenarios, scenario 1 also in a gated room, and the registry test); `cargo test -p dcp-interop --locked --test cross_binding -- a_demoted_editor_cannot_publish_and_readers_drop_what_slips_through a_draft0_server_accepts_a_demoted_editors_events_and_readers_drop_them`; on `relaxed` and `relaxed-latest` also `--test negative_control` (13 controls) |
| `conformance relaxed` / `default` | `cargo test -p dcp-matrix --locked --test conformance` (7), `--test journeys` (10); `cargo test -p dcp-interop --locked --test conformance` (5) |

A test that needs Synapse fails when it is missing and skips only under `DCP_SKIP_SYNAPSE=1`, printing `SKIPPED (DCP_SKIP_SYNAPSE=1)`: the `levels relaxed` job runs both
new test files without `DCP_SYNAPSE_URL` (must fail with `DCP_SYNAPSE_URL is not set`) and with the skip variable (must print `SKIPPED` 5 and 7 times, once per Synapse
test). `cargo check --locked` passes: no dependency was added. The runs on this branch, with their reasons: [40175835722](https://github.com/tuna-os/gtk-office-suite/actions/runs/40175835722)
(`a56db6c`) failed at once: `--locked` refused the **Cargo.lock of main** (section 6); [40176167691](https://github.com/tuna-os/gtk-office-suite/actions/runs/40176167691)
(`00c56df`) all green, four rules, five scenarios; [40178500692](https://github.com/tuna-os/gtk-office-suite/actions/runs/40178500692) (`626094b`, with the hybrid and scenario 6) failed
only in scenario 6, my test counted the hybrid's requests against a cache the previous reading had filled; [40181742542](https://github.com/tuna-os/gtk-office-suite/actions/runs/40181742542)
(`c11f5c4`) failed only in `levels default-drained`, scenario 6 sent its history-visibility change without waiting out a 429 (a spent budget answers 429 before anything else,
Spike F); [40186028967](https://github.com/tuna-os/gtk-office-suite/actions/runs/40186028967) (`92c7466`, run A) and [40192035297](https://github.com/tuna-os/gtk-office-suite/actions/runs/40192035297)
(`fb2dfa4`, run B) are green. Both failures were defects of the new test. In run B the `/context` requests met 0 `M_LIMIT_EXCEEDED` again (620 per job, four jobs; 6 more sends after 3 + 300 requests, 1 more after 3 + 5 sends, on `default` and `default-drained`).

## 1. The ways to get the level at an event

What each costs and needs (costs: median of 5 cold reads of a room with 31 versions and 4 power-level changes made between them, `levels::listing_30_versions`; "extra" is over a
read with `SenderRule::Trust`, which took 343.7 / 382.6 / 385.4 / 229.8 ms on relaxed / default / default-drained / `latest` in run A and 380.0 / 348.0 / 235.7 / 390.2 ms in run B: that is the runner noise the extras sit in. Extras are run A, run B):

| Way | Requests for 31 versions | Extra ms, relaxed / default / default-drained (per version) | Offline from cached events | Needs the history back to the room's creation | Right in a federating room |
|---|---|---|---|---|---|
| `GET /rooms/{id}/context/{eventId}?limit=0&filter={"types":["m.room.power_levels","m.room.create"]}`, one after the other | 31 (134 KB) | A 105.8 / 152.1 / 161.8 / 86.2 (2.8 to 5.2 a version); B 142.2 / 104.9 / 83.5 / 172.9 (2.7 to 5.6) | only for event IDs already cached | no | yes (the server's own state) |
| the same, 4 / 8 in flight | 31 | A 100.2 / 130.9 / 142.2 / 73.0 and 90.0 / 128.8 / 159.1 / 73.3; B 127.4 / 90.6 / 72.6 / 151.3 and 121.7 / 89.9 / 67.5 / 136.2 | as above | no | yes |
| matrix-sdk 0.19.1 `Room::event_with_context(id, false, 0, None)` | 31 | A 502.6 / 545.6 / 581.1 / 338.4 (10.9 to 18.8); B 548.2 / 464.7 / 342.9 / 609.7 (11.1 to 19.7) | as above | no | yes |
| forward walk of `m.room.create` and `m.room.power_levels` in the `/messages` pass | 0 | A 0.0 to 4.9, B 0.0 (noise) | **yes** | **yes**, and every change visible | not claimed [INFERENCE: stream order is not state resolution; not run] |
| backward walk through `unsigned.prev_content` from the newest table | 0 | A 0.0 to 2.4, B 0.0 to 0.5 (noise) | **yes** | no: only back to the oldest event judged | not claimed [INFERENCE, not run] |
| `GET /rooms/{id}/state` snapshots | 1 per read | n/a | n/a | n/a | the CURRENT table: draft 1's rule, wrong |
| `GET /rooms/{id}/state?at=<token>` | 1 | n/a | n/a | n/a | **ignores `at`**: returns the current table (probe below) |

How the facts were established:

- **matrix-sdk has no other state-at-event call.** `grep -rn "state_at" src | wc -l` in `matrix-sdk-0.19.1` and `grep -rln "state_at\|StateAt" src | wc -l` in `matrix-sdk-base-0.19.1` print 0; the
  only candidate is `Room::event_with_context` (`src/room/mod.rs:989`), which builds a `get_context::v3::Request` and returns its `state`. Its extra cost over the raw request
  (16 against 3.4 ms a version, same 31 requests) is local: it decrypts the target event and builds the push context inside the call.
- **There is no `/state_at`, and `at` does nothing on `/state`.** `levels::the_alternatives_to_context_probed` (every job): after the editor is demoted, `/context` at the editor's earlier
  version shows the editor at level 50 (asserted, so the probe can tell the past from the present; the SDK's call shows 50 too), while `/state?at=<start token>` and
  `/state?at=<end token>` from that `/context` answer 200 with the current table, the editor absent (unchanged: 2 of 2). `/members?at=` is the only endpoint that honours `at`
  (Synapse's `RoomMembershipRestServlet` in develop, read, not measured); it returns member events only: 404 for one of the two tokens, 200 and `m.room.member` events for the other.
- **`/context` with `limit=0`** returns `state` as the state AT the event (Synapse: `get_state_for_events([event_id])` when no later event is returned). Filtering the state to the
  two event types (`filter` is a JSON string in the query, applied to `state` too) cut the answer for a room of two members from 10 events and 7 769 to 7 804 bytes to 2 events and 4 244 to
  4 256 bytes (4 jobs); the unfiltered state grows with the room's membership [INFERENCE: it holds one `m.room.member` event per member], the filtered one does not.
- **The walks are only as good as the server's `/messages`.** Synapse 1.163.2 sends `unsigned.prev_content` and `unsigned.replaces_state` on the power-levels events of
  `/messages` (the linked forward walk and the backward walk each resolved every event of scenarios 1 to 5 and of the 31-version read; had either field been missing, the
  events would have been unresolved and dropped). Other servers: not measured.

## 2. The five scenarios and their controls

Code: `dcp_interop::power_at_event`, each scenario on a fresh `Rig` (owner, editor, viewer; room created with the literal draft-0 table, `events["m.room.encrypted"]` absent, so Synapse
accepts what the demoted or never-promoted user sends, and each such forgery's acceptance by the server is asserted). Per scenario a ledger says what a correct reader does with
every version and update. Two live readers read the room under all five at-event rules and **must get every verdict right, find nothing unresolved, show the expected head, and agree
on every level**; then each control reads the same room and **must get named events wrong or the check fails**. All 8 tests passed on all 4 jobs.

| # | History | Correct reader | Control: current-table reader (draft 1) | Control: trusting reader |
|---|---|---|---|---|
| 1 | editor publishes v2; editor demoted; owner publishes o1 on v2 | accepts v2 and o1 | drops v2, then o1 (unknown parent): **2 wrong per reader**, document reads as its first version | not needed (no forgery) |
| 1, gated room | the same in a room with the spec's table | all rules accept both | **right** (trusts the server, which refused the ex-editor): asserted | |
| 2 | v2; demoted; ex-editor sends a forged version and a forged update (server accepts both); owner publishes o1 | accepts v2 and o1, drops both forgeries | drops v2 and o1: **2 wrong** | accepts both forgeries: **2 wrong** |
| 3 | viewer forges a version and an update (accepted); later promoted; publishes a | drops both forgeries, accepts a | accepts both forgeries retroactively: **2 wrong** | **2 wrong** |
| 4 | the history of scenario 5; then a new user is invited, joins, reads | same verdicts, same levels, same heads as a live reader, under all five rules | **4 wrong** (a1, a2, a3, o1) | **4 wrong** |
| 5 | an editor's event a1; then five changes of the editor's level (demoted, promoted, demoted, promoted, demoted for good) with an event from the editor after each (a forged f1, a2, f2 and a forged update fu2, a3, f3); then the owner publishes o1 on a3 | a1, a2, a3 and o1 accepted; f1, f2, fu2, f3 dropped | **4 wrong** per reader (a1, a2, a3, o1) | **4 wrong** per reader (the four forgeries) |

Totals per job, from the `READING` lines of the job's log: 65 reader runs (13 per rule: two readers in each of scenarios 1, 2, 3, 5 and in scenario 1 again in the gated room; the live and the late
reader in scenario 4; the late reader in scenario 6) with 0 wrong verdicts (scenario 6's lines do not print the count, the check asserts it); controls: current-table 10 runs, 24 wrong verdicts;
trusting 8 runs, 20 wrong. The totals are identical in the four jobs because the scenarios are deterministic and every job ran the same code against Synapse 1.163.2.

Scenario 4 needs the late joiner to decrypt events of two senders sent over several demotions: it did (`undecryptable` 0, all 9 events judged before the readings), under `shared` history
visibility, with the history shared by the invite as in Spike A.

## 3. What a read costs, and the limits

Measured by `levels::state_at_event_requests_are_not_rate_limited` and `levels::listing_30_versions`:

| | relaxed | default | default-drained | relaxed, `latest` |
|---|---|---|---|---|
| 300 `/context` one after the other: 429s / s / requests per second | 0 / 0.94 / 318 | 0 / 1.33 / 226 | 0 / 1.64 / 183 | 0 / 0.72 / 416 |
| 320 from 8 tasks: 429s / s / requests per second | 0 / 0.84 / 379 | 0 / 1.22 / 263 | 0 / 1.44 / 223 | 0 / 0.71 / 449 |
| cold read of 31 versions: 429s met | 0 (5 reads × 9 cases) | 0 | 0 | 0 |
| cold read, `/context`: extra over trust-sender, run A | 105.8 ms | 152.1 ms | 161.8 ms | 86.2 ms |
| the same, run B | 142.2 ms | 104.9 ms | 83.5 ms | 172.9 ms |
| cold read, `/context`: median total (trust-sender), run A | 449.6 (343.7) | 534.7 (382.6) | 547.3 (385.4) | 315.9 (229.8) |
| warm read (cache): requests / cache hits | 0 / 31 | 0 / 31 | 0 / 31 | 0 / 31 |
| sends before the first 429: fresh user / after 3 + 300 `/context` / control after 3 + 5 sends | not run (no limit) | 9 / 6 more / 1 more | 9 / 6 more / 1 more | not run |

- **`/context` is not counted against `rc_message`**: a user who made 300 requests could send 9 events in all before the first 429, like one who made none; the control (5 extra sends) leaves
  5 fewer (1 against 6), so the comparison can see spent tokens. `rc_message` is the only limit tried; `rc_joins`, `rc_invites` and the others are unrelated to this call.
- **The 429 path of the new requests was never taken** (0 of 620 per job), so the retry on `retry_after_ms` in `MatrixStore::context_raw` and the SDK path is [untested]; it
  is the same logic as the binding's other reads (`retry_rl`) in the SDK path and a copy of it for the raw request.
- **Concurrency saves little on a loopback server** (extra over trust-sender with 1 / 4 / 8 requests in flight, run A: relaxed 106 / 100 / 90 ms, default 152 / 131 / 129, default-drained 162 / 142 / 159, `latest` 86 / 73 / 73;
  run B: 142 / 127 / 122, 105 / 91 / 90, 84 / 73 / 68, 173 / 151 / 136): 2 to 21 % in these eight pairs of series, and run A's `default-drained` 8-in-flight series is slower than 4; a 2 to 5 ms request is mostly server time. On a real network, where a request is dominated by its round trip, it would matter (31 sequential round trips of, say, 80 ms are 2.5 s: derived, not run).
- A reader that pages the room's history **already** pays for `/messages`; the walks reuse that pass, which is why they cost nothing.

## 4. A power-levels change the reader cannot see (scenario 6)

Not in the brief; it came out of choosing the default. The owner sets `history_visibility: joined` before anything else is sent, so an event is visible only to the members who were joined
when it was sent. History: editor publishes a1; **editor demoted** (hidden from whoever joins later); editor forges f1; a new user joins. **In its window**: the ex-editor forges f1b and the
owner publishes o0, both *before any change of the table the new user can see*; then the editor is promoted again, publishes a2, is demoted, forges f2; the owner publishes o1.

| Rule (the late joiner's five events: f1b, o0, a2, f2, o1) | f1b (forged) | o0 | a2 | f2 (forged) | o1 | Requests |
|---|---|---|---|---|---|---|
| `/context`, matrix-sdk's call, backward walk, hybrid | dropped | accepted | accepted | dropped | accepted | 5 / 5 / 0 / **2** |
| forward walk, tables linked through `replaces_state` | **not placed** (dropped) | **not placed** (dropped, a loss) | accepted | dropped | accepted | 0 |
| control: forward walk **without** the link check | **accepted (wrong)** | accepted | accepted | dropped | accepted | 0 |
| control: current-table reader | dropped (3 events dropped in all) | accepted | **dropped (wrong; asserted)** | dropped | accepted | 0 |
| control: trusting reader (0 events dropped) | **accepted (wrong; asserted)** | accepted | accepted | **accepted (wrong; asserted)** | accepted | 0 |

(The verdicts of the four correct rules and of the linked walk are asserted event by event; for the controls the asserted cells are the marked ones, the others follow from the printed counts of events dropped.)
The `READING` lines of scenario 6 are identical in the four jobs (timings aside). [INFERENCE for why the walk without the link check is wrong: the tables the late joiner sees are those sent while the history was
`shared` (the room's creation and the editor's grant) and those in its window; the hidden demotion lies between the last of the first kind and PL2, whose `replaces_state` therefore names it and not
the earlier table; the linked walk's refusal to place f1b and o0 and the unlinked walk's acceptance of f1b are consistent with that and with nothing else tried.] The backward walk never needs the hidden
event: PL2's `prev_content` is the table the demotion left. Each reading of this scenario took 5.0 to 5.4 s against 0.07 to 0.3 s elsewhere: the binding waits `utd_wait` (5 s) for room keys of an event the late joiner could not decrypt [INFERENCE: the first version, sent before the history became `joined`;
not identified in the run].

This is the reason the default is not a plain walk, and the reason a reader MUST NOT reuse the last table it saw across an unlinked change.

## 5. The choice

The function: `levels::level_for(power, sender, event_type, is_state)` returns the sender's level (`users[sender]`, else `users_default`; a room version 12 creator has unbounded power) and what the table asks for the event type
(`events[type]`, else `events_default` for a timeline event and `state_default` for a state event); `Level::permitted()` is "level at least 50 and at least what the table asks". `levels::power_from_state` turns a state-at-event answer
into the table (`None` when it holds no power-levels event: no evidence), the walks and the cache feed the same function, and `MatrixStore::read_report_with(doc, rule)` applies it to every version and update.
DCP versions and updates are timeline events; `is_state` is exercised only by a unit test.

**Default: `SenderRule::Hybrid`.** It is the only rule that is both free in the usual case and right when the walk is not. In rooms that do not federate (the binding creates every document with
`m.federate: false`: all events come from the creator's homeserver, so the order of the pass is the order of the room's state) it walks the pass the reader already makes, trusting a table only while the next
one names it in `unsigned.replaces_state`; whatever it cannot place (a window, a broken link, a pass without the create event) it asks the server for, one request per event, filtered, cached by event ID,
honouring `retry_after_ms`. In a room created to federate it asks the server for every event (`levels::a_room_that_federates_is_judged_by_the_server_and_not_by_the_walk`: 3 events, 3 requests, the same levels as
the walk, the forged event dropped; not tested across two homeservers).

Why not the others as the default: `/context` for everything costs 3 to 6 ms and 4 KB an event for what the pass already holds; matrix-sdk's call is the same request at 3.6 to 4.8 times the local cost; the plain forward walk
is silently wrong in scenario 6; the backward walk is sound there but depends on `prev_content` being present on every event and gives up older history for nothing the hybrid does not also cover. All four stay as rules
because the tests compare them (and the negative controls are rules too); `SenderRule::CurrentTable`, `Trust` and `TimelineWalkUnverified` exist only for the controls.

The fallback the brief asks for if every approach failed (trust only events after the last observed change, clients re-publish) is **not needed**: nothing failed, and the cost on `default` and `default-drained`
is not distinguishable from `relaxed`'s (the two runs differ more from each other than the profiles do).

## 6. What changed in the code and the existing suites

- `dcp-matrix`: new `levels` module and `MatrixStore::read_report_with(doc, rule)`; `ReadReport` gains `unauthorised_ids`, `rejected_unresolved`, `checks` and `level_stats`;
  `MatrixConfig::reader_checks_senders` (a bool) is replaced by `sender_rule` (default `Hybrid`), plus `level_concurrency` and `federate`. The two places that set `reader_checks_senders = false` (the interop rig's
  `draft0_power_levels_and_readers_that_trust_senders` and `dcp-matrix/tests/conformance.rs`) now set `sender_rule = Trust`.
- **The existing check `a_demoted_editor_cannot_publish_and_readers_drop_what_slips_through` changed its draft-0 branch.** It asserted Spike F's finding (the earlier version dropped, the owner's save
  rejected, the readers showing the first version); it now asserts the corrected behaviour (the earlier version listed, the owner's save the head, exactly the events sent after the demotion dropped).
  Passes on the four jobs, and its negative control (`readers_that_trust_the_sender_fail_the_demoted_editor_check`; 13 of 13 controls, run on `relaxed` and `relaxed-latest`) still fails the check as it must.
- **`tools/collab-spike/Cargo.lock` on main was stale**: `dcp-webdav` asks for `parking_lot` 0.12 and `dcp-core` for 0.13, and the lock named one unqualified `"parking_lot"` in
  `dcp-webdav`'s entry, so every `cargo ... --locked` of the workspace failed ("cannot update the lock file", run 40175835722). One line is changed (`"parking_lot 0.12.5"`), produced by
  `cargo metadata --offline` without any version bump; the integration branch will meet the same line. **`dcp-p2p/Cargo.lock` is stale the same way** (`collab-spike.yml`'s `check` job, run
  [40186028902](https://github.com/tuna-os/gtk-office-suite/actions/runs/40186028902), step "cargo check --all-targets --locked (tools/collab-spike/dcp-p2p)": cannot update the lock file); this spike does not touch it.
- `dcp-core`'s in-memory reference store (`MemoryStore::visible_versions`) still filters by the sender's CURRENT role. It is the simulator the shared conformance suite and its mutation tests
  run on and no journey has a demoted editor in it; it was left alone, and the journey that would catch it (S1 as a `dcp-conformance` journey) is not written.

## 7. The existing suites on this branch

`collab-spike.yml` also runs on every push to a `spike/collab-*` branch, so the unchanged suites ran against the new default rule: run
[40186028902](https://github.com/tuna-os/gtk-office-suite/actions/runs/40186028902), commit `92c7466` (the code this document measures). **Green:** `dcp-matrix against Synapse (relaxed + default rate limits)`
(`cargo test -p dcp-matrix` with all its tests, journeys and measurements files); all ten `interop` jobs (relaxed, default and default-drained × conformance, cross_binding with its 21 checks and their draft-0 variants,
latency; relaxed controls, 13 negative controls); `lease race` on both profiles (200 rounds each); `dcp-core, dcp-conformance, schemas`; `dcp-webdav` against two Nextcloud versions and the Nextcloud probes; `dcp-streams`;
the p2panda upstream tests. **Red, none of it on Matrix code:** `cargo check ... (both workspaces)`, `p2p - dcp-p2p journeys...` (4 failing steps) and `p2p - sync through an in-process iroh relay`, all from
`dcp-p2p/Cargo.lock` not satisfying `--locked` (section 6); `WopiHost over MemoryStore (simulator, no Collabora)`, step "DCP_SKIP_COLLABORA=1 skips explicitly and says so" (the job counts `SKIPPED collabora_e2e` lines and expects 4;
dcp-wopi, not touched here, cause not investigated). Skipped by the workflow's own conditions: the Collabora, measure and Flatpak jobs.

## Spec corrections (proposed; `docs/DOCUMENT-COLLABORATION-SPEC.md` is not edited)

**8.1, replacing from "A reader MUST check the room's power-levels event on joining." to the end of that paragraph** (lines 457 to 470 of draft 1):

> A reader MUST judge a version or an update by its sender's power level **in force at the event**, never by the room's current table. The level is `users[sender]`, else `users_default`
> (a room version 12 creator has unbounded power), from the room's power-levels state at the event; the event is accepted only if that level is at least 50 and at least what the same table asks for
> the event type the server saw (`events[type]`, else `events_default`; `m.room.encrypted` in an encrypted room). This applies whether or not the table gates `m.room.encrypted`: with the
> entry the server has already refused an unauthorised sender at send time and the rule changes nothing; without it the server accepts the event and only the reader stands between it and the document.
> A demotion MUST NOT hide what the editor published before it, and a promotion MUST NOT validate what the user sent before it (measured: draft 1's current-table rule dropped the ex-editor's earlier
> version and then the owner's save in a draft-0 room, and accepted a viewer's earlier forgery after a promotion; Spike Q15 scenarios 1 to 5).
>
> A reader obtains the power levels at an event in one of two ways, and uses the second for every event the first cannot place:
> 1. **From the room's own events.** Page the room's timeline (`/messages`): it carries `m.room.create` and every `m.room.power_levels` event in order. Walk them from the oldest; judge each event
>    by the table held when the walk reaches it. The walk MUST trust a table only while the next power-levels event it meets names it in `unsigned.replaces_state`: a change the reader cannot see
>    (history visibility `joined`, a reader who joined after it) would otherwise leave it judging by a table that is no longer in force, silently (Spike Q15 scenario 6: it accepted the ex-editor's
>    forged event). It MAY read the pass from the newest event backwards instead, undoing each power-levels event through its `unsigned.prev_content`. This needs no request, works from cached events
>    and is exact only in a room created with `m.federate: false` (all events come from one homeserver, so stream order is state order); a DCP room SHOULD be created that way.
> 2. **From the server.** `GET /_matrix/client/v3/rooms/{roomId}/context/{eventId}?limit=0`, whose `state` is the room state at the event; a `filter` of
>    `{"types":["m.room.power_levels","m.room.create"]}` keeps the answer to about 4 KB however many members the room has. The answer for an event never changes: cache it by event ID. The call was
>    not rate limited on Synapse 1.163.2 (0 of 620 requests per run on the default profile) and spends none of the sender's `rc_message` budget; it costs 2.7 to 5.6 ms and 4 KB per event on a loopback server
>    (listing 30 versions: 84 to 173 ms extra in eight job runs). Honour `retry_after_ms` on a `429` as for any request.
>
> An event whose level at the event cannot be established MUST be dropped; a reader MUST NOT fall back to the current table.

**8.1, "Roles to power levels" table or the note after it:** add that the owner SHOULD create a document room with `m.federate: false` (the binding does; the hybrid reader's zero-request path depends on it).

**§13 question 15, replacing the item** (line 1394): "15. *Answered by Spike Q15.* The level in force at an event is obtained from the room's own `m.room.power_levels` events in the `/messages` pass
(linked through `unsigned.replaces_state`; exact in a room created with `m.federate: false`) or from `GET /rooms/{id}/context/{eventId}?limit=0` (any room, cached by event ID); neither costs a
rate-limit token; see [Spike Q15](rfc/0004-spike-q15-results.md). Open: a room that federates across real homeservers, other homeservers, and the path when a server answers 429 to `/context`."

**Changelog** (the "Behaviour changes that are not wire changes" bullet): "the 8.1 reader rule judges a sender by the level at the event and says how a reader obtains it (Spike Q15); a reader MUST drop an event whose level it cannot establish."

## Not verified

- **Federation.** Every room had one homeserver. The walk's exactness for `m.federate: false` is an argument from where events originate, tested only as agreement with `/context` on one Synapse; a room whose
  events arrive from several homeservers (forks, state resolution, `/context` for an event another server sent) was not tried. The hybrid's choice for a federating room was tested on a single homeserver
  (3 requests, agreement), not across two.
- **Other homeservers.** `unsigned.prev_content` and `unsigned.replaces_state` on `/messages` power-levels events, `/context` with `limit=0` and a `filter` on `state`, the absence of rate limits: all
  Synapse 1.163.2 (two image builds of it). Dendrite, Conduit and others: not run. A server without `replaces_state` would make the linked walk place nothing, and the hybrid would ask for every event.
- **Rate limits beyond `rc_message`, and any 429 on `/context`.** None was met, so the retry path of the new requests (raw and SDK) never ran. A homeserver or proxy that does limit it is not covered.
- **Real network.** All requests were loopback and 2 to 5 ms; the cost per event on a WAN is derived (round trips), not measured, and so is the benefit of requests in flight.
- **Large rooms and long histories.** The rooms had 3 to 5 members, 31 versions at most and at most 7 power-levels changes (scenario 5); the size of an unfiltered answer (grows with membership), the 100-event page loop of a long
  history, and a table with thousands of entries were not measured (rooms of 3 to 5 members).
- **Room version 12.** Creators with unbounded power are handled and unit-tested (both walks, `m.room.create` as the source), not run against a v12 room (the room version of the CI Synapse's default was not recorded).
- **Reader paths other than a fresh `/messages` pass.** Incremental `/sync` or sliding-sync readers keeping the table up to date as it changes, a reader with a persistent event cache on disk (`judge_transcript` was
  run on a string, not on matrix-sdk's cache), and the Flatpak/GNOME client were not tried.
- **`history_visibility` other than `shared` and `joined`**, and a reader who is invited but has not joined.
- **Whether an `m.room.power_levels` change by a user with state-event power, other than through `grant`** (which sends the whole table) can make `prev_content` and the walk disagree; every change here was sent by `grant`.
- `MemoryStore` (dcp-core) still uses the current level; no `dcp-conformance` journey covers a demoted editor (section 6).
- The jobs of `collab-spike.yml` that were red on this branch (section 7: the `dcp-p2p` lock, 4 jobs; `wopi-sim`'s SKIPPED count) were not investigated beyond naming the failing step; the Collabora, Flatpak and measure jobs of that workflow did not run, so
  nothing says whether Collabora over the new default rule behaves as in Spike F.
