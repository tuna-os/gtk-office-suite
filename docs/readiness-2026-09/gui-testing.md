## Implementation contract for the September readiness roadmap

Audited at `e7e4df6`. Reuse this issue as the execution owner for deterministic GUI infrastructure (related #367, #361/PR #362, #241). An ADR alone is not completion.

Architecture: separate dependency-free model/controller tests, GTK bridge tests dispatched on one initialized GTK main thread, and real-process AT-SPI journeys. Give every run a private display and D-Bus session and every test isolated XDG config/data/state/cache and GSettings. Never kill apps by name. Track and reap only launched PIDs. Keep the process runner alive long enough to clean up on success, test failure, and setup failure.

Confirmed harness gaps: fixed :99 display, shared /tmp schemas, EXIT traps bypassed by exec, global pkill, snapshot artifacts read from the parent environment instead of launch_env, and duplicate TablesNamedRangeSmoke classes (one silently replaces the other). The far-navigation regression remains skipped although #137 is closed.

- [x] Replace fixed display startup with automatic allocation and mandatory
      WM/input dependencies; fail visibly if setup is unavailable —
      `run_gui_tests.sh` asks Xvfb to allocate the number (`-displayfd`)
      instead of defaulting to `:99`, waits on `xdpyinfo` and on the window
      manager's `_NET_SUPPORTING_WM_CHECK`, and exits 1 naming the cause
      when any of it fails. `xprop` joined the container's dependency
      check-list.
- [ ] Isolate defaults for every test, retaining explicit recovery/restart
      overrides — **partly done**: the shared `/tmp/gtk-office-schemas` is
      now a per-run temporary directory. Per-test XDG/GSettings isolation
      is still opt-in (`isolate_gsettings`, `isolate_autosave_state`) rather
      than the default.
- [x] Register cleanup immediately after process launch; preserve
      stdout/stderr, AT-SPI tree, screenshot, input trace and normalized
      state on setup/test failures — `_capture_failure_artifacts`, and
      `addCleanup`/`tearDown` for every launched process.
- [ ] Replace sleeps with bounded state predicates; timeout reports the last
      observed state — **partly done**: the runner's setup sleeps are gone,
      `BaseGUITestCase.wait_until` reports the last observed value on
      timeout, and the three journeys that measurably depended on a fixed
      wait now use predicates. The remaining `time.sleep` calls in the
      other journeys are measurably *not* load-bearing (see below), so
      they are dead weight rather than a flake risk, and converting them is
      a mechanical follow-up.
- [x] Detect duplicate test definitions before collection; reconcile and
      reactivate the #137 regression after live verification —
      `conformance/validate_capabilities.py` (C6) rejects a duplicate class
      or method name, and
      `TablesNamedRangeSmoke.test_jump_far_and_back_to_a_range_no_longer_crashes`
      is active and passing on video.
- [x] GTK widget/bridge tests share one GTK main-thread dispatcher —
      `suite_common::gtk_test`, and since #241 a test that cannot reach a
      display fails rather than skipping. See
      [gtk-threading.md](gtk-threading.md).
- [x] Upload JUnit and failure artifacts on every CI outcome; nonzero
      pytest and setup failures propagate — `if: always()` uploads in
      `ci.yml` and `gui-tests.yml`; the runner is not `exec`ed, so its exit
      status is forwarded and its EXIT trap still runs.
- [ ] Test harness isolation itself with injected failures, concurrent runs,
      an unrelated app sentinel, and repeated launch/close — **partly
      done**: the unrelated-app sentinel and the concurrent-run case are
      covered (`tests/test_gui_harness.py`, and three simultaneous runner
      invocations each allocating a distinct display). Injected setup
      failures and repeated launch/close are not. `GUI_TEST_SLEEP_SCALE`
      is a related instrument: it perturbs the harness's own timing to
      find where the journeys depend on it.
- [x] Required journeys: all-app create/open/edit/undo/redo/save/reopen;
      Save As/cancel/error; dirty close; kill/recover/restart; two-app
      clipboard; keyboard/screen-reader workflows — 54 journeys, recorded
      on video per pull request. Save As error and cancel arrived with
      #436, the two-app clipboard with #552.
- [ ] Run changed-app journeys on PRs; shared/harness/unknown paths
      conservatively run all apps. No VLM judgments in required gates —
      the path-based selection exists in `gui-tests.yml` and `vlm-audit` is
      not a required gate; the conservative fallback has no test of its own
      yet.

## The display collision this closed

The runner defaulted to `:99`. A second concurrent run did not fail — it
quietly joined the first run's display, measured:

```
run A owns :99
run B: Xvfb -> "Fatal server error: Server is already active for display 99"
run B proceeded on DISPLAY=:99
geometry seen by run B: 800x600     (A's size, not the 1920x1080 B asked for)
```

Two runs' windows then shared a screen, `xdotool` activation fought over
focus, and one run's synthetic keystrokes landed in the other's editor. The
stress campaign's narrow/wide matrix would have recorded at the other run's
geometry. `stress.py` had already met this from the caller's side — its
`free_display_number()` docstring described the exact failure — and worked
around it by scanning `/tmp` for a free number, which is itself a race
between two campaigns scanning at once. The runner now uses `-displayfd`, so
the X server allocates under its own locking and reports back; the scan is
deleted. Three simultaneous invocations with `:99` already taken allocate
`:0`, `:1`, `:2` with no setup failures.

An explicitly requested `GUI_TEST_DISPLAY_NUM` still works and now goes
through the same handshake, so a taken number fails loudly
(verified: exit 1, "Display 77 was requested explicitly; is it already in
use?") instead of silently sharing.

`GUI setup failed` joined the campaign's infrastructure markers, so an
attempt that never started a journey is classified as infrastructure rather
than as an unexplained product result.

Six of the ten items are complete and three are partly complete; the item
list above says which, and what is left. This issue stays open for the
remaining work: per-test isolation by default, the `time.sleep` calls inside
the journeys, injected setup failures and repeated launch/close, and a test
for the conservative journey-selection fallback.

## Which fixed waits were actually load-bearing

The journeys held 231 `time.sleep` calls — 163 seconds of unconditional
waiting per run. Shortening them on a hunch is how a real race gets
papered over, so `GUI_TEST_SLEEP_SCALE` (conftest; default 1.0, patches
nothing) scales every fixed wait, and a run at 0.25 says empirically which
ones the journeys depend on:

| run | before | after |
|---|---|---|
| `GUI_TEST_SLEEP_SCALE=1` | 54 passed in 276s | 54 passed in 266s |
| `GUI_TEST_SLEEP_SCALE=0.25` | **3 failed**, 51 passed in 134s | **54 passed** in 134s |

Three of 54. Every one was a Tables journey, and every one failed with a
message that pointed at the product rather than at the clock:

* `TablesMultiSheetSmoke` — `'2' not found in 'cell A1: 6'`, which reads
  as the data-isolation bug the journey exists to catch. The dropdown had
  not opened, so Up/Return went nowhere and it never left Sheet2.
* `TablesNamedRangeStatsSmoke` — `'Sum 60' not found in 'A1:A3 · Sum 50 ·
  Avg 25 · Count 2'`, which reads as a formula-engine defect. The Define
  Name dialog was still up when Ctrl+G fired, so the keys went to the
  dialog.
* `TablesFilterSmoke` — `hidden_rows: []`, which reads as filtering not
  working. The filter had not been applied when the snapshot was sampled.

All three now use predicates. The whole suite passes at 0.25 as well as at
1.0, and the three journeys pass at 0.25 across three consecutive runs on
their own. The observables were established
by probing the live AT-SPI tree rather than guessed, and two of them were
not obvious:

* Return does **not** advance the active cell in the Tables grid, so the
  name-box jump is genuinely required for each cell. An earlier attempt to
  type down a column put both values in A1 — caught immediately, because
  `wait_until` reported `last observed: 'cell A1: 20'`.
* GtkDropDown's popup list is not exposed in the app's AT-SPI tree at all,
  so there is nothing to wait *for* before driving it. The sheet switcher's
  own accessible name mirrors the selection, so the gesture is retried
  against that outcome instead.

One observability gap is worth recording as product work rather than test
work: **the grid's accessible description does not follow a name-box jump**
— it reports only the last *committed* cell, so "the selection moved" has
no accessible signal. That is why these journeys reach for fixed waits in
the first place. A selection-changed signal on the grid would let the
remaining Tables waits become predicates too.

The other 228 sleeps are not load-bearing at a quarter of their length,
which does not make them safe to simply shorten — it makes them dead
weight to be replaced with predicates where they precede an assertion.
