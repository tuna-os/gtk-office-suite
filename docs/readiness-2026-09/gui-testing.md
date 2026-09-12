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
- [x] Isolate defaults for every test, retaining explicit recovery/restart
      overrides — the shared `/tmp/gtk-office-schemas` is a per-run
      temporary directory, and every journey now launches with its own
      `XDG_CONFIG_HOME`/`DATA`/`CACHE`/`STATE` and the keyfile GSettings
      backend. `isolate_gsettings`, `isolate_autosave_state` and
      `isolate_xdg` still work for the journeys that name them: they set
      `launch_env` before `setUp`, and explicit `launch_env` wins over the
      defaults. See below for what the default was hiding.
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
- [x] Test harness isolation itself with injected failures, concurrent runs,
      an unrelated app sentinel, and repeated launch/close —
      `tests/test_gui_harness.py` holds the unrelated-app sentinel, the
      process-ownership property, the readiness budgets and
      `InjectedSetupFailures`, which breaks one part of setup at a time and
      asserts the runner exits nonzero, names the cause, and still cleans
      up. `HarnessRepeatedLaunchSmoke` runs four launch/close cycles and
      asserts exactly one live copy is owned after each.
      `GUI_TEST_SLEEP_SCALE` is a related instrument: it perturbs the
      harness's own timing to find where the journeys depend on it.
- [x] Required journeys: all-app create/open/edit/undo/redo/save/reopen;
      Save As/cancel/error; dirty close; kill/recover/restart; two-app
      clipboard; keyboard/screen-reader workflows — every app covered, 58
      journeys recorded on video per pull request. Save As error and cancel
      arrived with #436, the two-app clipboard with #552.
- [x] Run changed-app journeys on PRs; shared/harness/unknown paths
      conservatively run all apps. No VLM judgments in required gates —
      the classification moved out of `gui-tests.yml` into
      `tests/gui/select_journeys.sh` so it can be tested, and
      `tests/test_journey_selection.py` covers every fallback: shared
      crate, workspace lockfile, harness path (the selector included),
      unrecognised path, and an empty diff. `vlm-audit` is not a required
      gate.

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

Nine of the ten items are complete. The one that is not is the sleep
conversion: the runner's setup sleeps are gone and the three journeys that
measurably depended on a fixed wait use predicates, but the remaining
`time.sleep` calls in the other journeys — measurably not load-bearing, see
below — are still there. This issue stays open for that mechanical
follow-up.

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

One observability gap turned out to be a product defect, and is now
fixed: **the grid's accessible description did not follow a name-box
jump** — it reported only the last *committed* cell. Jumping by cell
reference or to a defined name moved the selection and announced nothing,
so a screen-reader user was never told where they had landed.
`refresh_grid_a11y` already existed and its own documentation warns about
exactly this staleness for sheet switches; the name box was the path that
still had it, and `jump_to_reference` now calls it.
`TablesNameBoxAccessibilitySmoke` asserts it, and fails against the
unfixed binary with `last observed: 'cell B2: seven'` after a jump to D7.

That also supplies the signal the remaining Tables waits need, so they can
become predicates rather than fixed intervals.

The other 228 sleeps are not load-bearing at a quarter of their length,
which does not make them safe to simply shorten — it makes them dead
weight to be replaced with predicates where they precede an assertion.

## What opt-in isolation was hiding

Only **2 of 38** journey classes called `isolate_gsettings`, so the rest
launched against the real XDG stores. The private D-Bus session does not
help: GSettings' dconf backend keeps its storage in
`$XDG_CONFIG_HOME/dconf/user`, which no bus setting redirects. Measured
with two probe journeys reading the preference *as the launched app sees
it*:

| | before | after |
|---|---|---|
| first journey starts at | `false` — left by a *previous run* | `true` — the schema default |
| it changes the value to | `false` | `false` |
| next journey starts at | `false` — inherited | `true` |

So a journey that touched a preference rewrote the machine's real settings,
every later journey in the run inherited them, and the next run started
from whatever the last one left. On a developer's machine that silently
edits their actual app preferences; on CI the fresh runner hid it
completely. The stress campaign's randomised journey order would have
surfaced it as a flake with no visible cause.

`configure_deterministic_environment` now gives every test its own XDG
root and the keyfile backend, so isolation is a default rather than
something a journey remembers to ask for.
`LettersSettingsIsolationSmoke` and its follower assert it — each expects
the schema default, so neither depends on the other having run, while
together they catch the regression. Against the unisolated harness the
follower fails with the inherited `false`.

One probe-design note worth keeping: the first version of this measurement
read the setting with a bare `gsettings` subprocess and so reported the
*harness's* environment, not the app's — it showed a leak even after the
fix. Reading through `launch_env`, the way the app is launched, is what
makes the measurement mean anything.

## A timeout that is only just long enough

The display handshake this work introduced replaced blind sleeps with
bounded predicates — the right shape — but sized each budget at 100 polls
of 0.1s. Ten seconds passed every run it was developed against, and then
failed a cold CI runner:

```
21:13:03  setup starts
21:13:13  GUI setup failed: Xvfb never reported a display number.
21:13:16  The XKEYBOARD keymap compiler (xkbcomp) reports: ...
```

Xvfb announced itself three seconds *after* setup declared it dead. The
predicate was correct and the budget was not, which is the worst
combination to debug: the error names the right condition and blames the
wrong cause, on a machine that is merely slower than the one that wrote
it.

Setup now has generous budgets — `GUI_TEST_READY_SECONDS` (default 60) for
the probes, `GUI_TEST_XVFB_SECONDS` (default 60) for the server start —
across all three waits — the `-displayfd` number, the display answering
`xdpyinfo`, and matchbox claiming `_NET_SUPPORTING_WM_CHECK`. Each still
breaks the instant its condition holds, so the change costs a fast machine
nothing: a full smoke run's setup is still immediate, and a server that
dies outright is still reported at once by the `kill -0` check rather than
waiting out the budget. `ReadinessBudget` in `tests/test_gui_harness.py`
holds the line, because the regression is invisible in a passing run:
it fails on a default under 30s and on any wait that carries its own
hardcoded count.

## Testing the harness by breaking it

Two of these items asked for tests of the harness rather than of the apps,
and both name the same kind of defect: a check that reports success without
having checked.

`InjectedSetupFailures` breaks one piece of setup per case and asserts all
three things that matter — nonzero exit, a message naming the actual cause,
and cleanup. The cases are an inherited `DISPLAY` that answers nothing, an
explicitly requested display number already in use, and a readiness probe
that never succeeds while setup's own Xvfb is running. The third is the
interesting one: it is the only failure path with something of ours left
running to leak, and it asserts the server is gone afterwards by asking the
*display* whether anything still serves it — the harness may not identify
processes by name (#241), and neither may its own tests.

Cleanup is checked by giving the runner a private `TMPDIR` and listing it
afterwards. Both of the runner's temporary things — the per-run schema
directory and the display-number file — are mktemp'd into it, so "did the
EXIT trap run on the setup-failure path?" becomes a directory listing
instead of a hunt. That path is the one most likely to leak, because it is
the one nobody exercises on purpose.

`HarnessRepeatedLaunchSmoke` does four launch/close cycles. The apps are
`GtkApplication`s, so one surviving copy owns the bus name and hands its
window to every later launch — a journey then passes or fails against a
process some earlier test started, and the failure surfaces somewhere else
entirely. Writing it found exactly that hazard in the harness: all five
journeys that restarted an app inlined the launch and none registered the
replacement with `owned_processes`. `tearDown` terminates `self.process`,
whichever copy is current, so a journey that restarted twice left the
middle one running, unregistered and therefore invisible to the pre-launch
sweep as well. `relaunch_app` is now the one path, it registers, and the
five inline copies are gone. One of them had also dropped the test's
`launch_env`, so it relaunched against the real XDG stores while its own
first launch was isolated.

The journey-selection fallback could not be tested at all while it lived
inline in `gui-tests.yml`. Getting selection wrong in the cheap direction
is loud — a filter naming an app that does not exist collects nothing and
the run fails. The other direction is silent: too few apps still produces a
green run, just one that never launched the app the change broke. So the
classification moved into `select_journeys.sh`, which reads changed paths
and prints the `-k` expression, and the tests cover the fallbacks.

One of those tests is load-bearing in a way worth recording: on a diff that
names no app, dropping a path from the shared pattern lands in the
unrecognised-path fallback and runs everything anyway, so the first version
of these tests passed against a selector that no longer recognised the
harness at all. Pairing each shared or harness path with an app path is
what makes the pattern itself observable.

One budget was not enough, and the reason is worth recording: the harness
self-tests shorten it so a deliberately broken readiness probe reports
quickly instead of waiting a minute. While the `-displayfd` handshake
shared that variable, shortening it also starved Xvfb's own startup — so on
a cold runner setup failed at the handshake rather than at the gate under
test, and `test_a_display_that_never_answers_reaps_the_server_we_started`
went red with

```
AssertionError: 'no X display at' not found in
'GUI setup failed: Xvfb never reported a display number within 1s.'
```

It passed locally and in its own CI run, and failed later on a slower
machine — the same marginal-timeout shape these tests exist to catch,
reproduced here by putting a `Xvfb` stub that sleeps three seconds ahead of
the real one on `PATH`. Starting a server and probing one that has already
started are different costs, so they now have different budgets, and
`ReadinessBudget` checks both for a marginal default and asserts they stay
independent.
