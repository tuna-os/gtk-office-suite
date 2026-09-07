## Implementation contract for the September readiness roadmap

Audited at `e7e4df6`. Reuse this issue as the execution owner for deterministic GUI infrastructure (related #367, #361/PR #362, #241). An ADR alone is not completion.

Architecture: separate dependency-free model/controller tests, GTK bridge tests dispatched on one initialized GTK main thread, and real-process AT-SPI journeys. Give every run a private display and D-Bus session and every test isolated XDG config/data/state/cache and GSettings. Never kill apps by name. Track and reap only launched PIDs. Keep the process runner alive long enough to clean up on success, test failure, and setup failure.

Confirmed harness gaps: fixed :99 display, shared /tmp schemas, EXIT traps bypassed by exec, global pkill, snapshot artifacts read from the parent environment instead of launch_env, and duplicate TablesNamedRangeSmoke classes (one silently replaces the other). The far-navigation regression remains skipped although #137 is closed.

- [ ] Replace fixed display startup with automatic allocation and mandatory WM/input dependencies; fail visibly if setup is unavailable.
- [ ] Isolate defaults for every test, retaining explicit recovery/restart overrides.
- [ ] Register cleanup immediately after process launch; preserve stdout/stderr, AT-SPI tree, screenshot, input trace and normalized state on setup/test failures.
- [ ] Replace sleeps with bounded state predicates; timeout reports the last observed state.
- [ ] Detect duplicate test definitions before collection; reconcile and reactivate the #137 regression after live verification.
- [ ] GTK widget/bridge tests share one GTK main-thread dispatcher; a mutex or --test-threads=1 alone is not a substitute for thread ownership.
- [ ] Upload JUnit and failure artifacts on every CI outcome; nonzero pytest and setup failures propagate.
- [ ] Test harness isolation itself with injected failures, concurrent runs, an unrelated app sentinel, and repeated launch/close.
- [ ] Required journeys: all-app create/open/edit/undo/redo/save/reopen; Save As/cancel/error; dirty close; kill/recover/restart; two-app clipboard; keyboard/screen-reader workflows.
- [ ] Run changed-app journeys on PRs; shared/harness/unknown paths conservatively run all apps. No VLM judgments in required gates.

Local work has begun on isolation, owned process cleanup and snapshot artifacts. Full GUI verification is still pending; the host currently lacks Xvfb/Matchbox and its toolbox cannot start. Do not mark this issue complete from syntax checks.
