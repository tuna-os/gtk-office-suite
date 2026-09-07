# Deterministic GUI journeys

`framework/base.py` is the shared AT-SPI harness for the smoke journeys. Each
test launches with a stable locale, UTC timezone, fixed scale, theme, font
aliases, disabled animations, and `SOURCE_DATE_EPOCH=0`. Tests that touch user
data should layer `isolate_xdg()`, `isolate_gsettings()`, or
`isolate_autosave_state()` onto that environment.

Use `wait_for_condition()` and `wait_for_node()` for observable readiness
instead of sleeping for a guessed duration. `trigger_snapshot()` invokes the
test-only `test-snapshot` action and waits for the JSON write to complete. The
action is registered by the application only when `GTK_OFFICE_TEST_MODE=1`,
so no diagnostic backdoor is present in production launches.

On failure, the base class retains the screenshot, AT-SPI tree, application
log, input trace, and snapshot (when configured) under
`tests/gui/failure_artifacts/`.
# Repeated crash and flake campaigns

Build current binaries first, then run existing journeys repeatedly in a
replayable order. Every failed attempt remains a failure even if later attempts
pass. Output directories must be new; each retains revision, binary hashes,
seed, per-attempt logs, JUnit and failure artifacts.

```sh
python3 tests/gui/stress.py --repeat 20 --seed 20260907 --output /tmp/office-baseline
python3 tests/gui/stress.py --repeat 1 --matrix display --output /tmp/office-displays
```

The display matrix requests 400/800/1280 logical pixels, light/dark/high contrast
and 1x/2x scaling. It needs Xvfb, Matchbox, xdotool and gsettings-desktop-schemas
in addition to the existing GUI dependencies. `--app tables` limits the campaign
to one app; `--timeout` bounds each attempt. This repeats existing tests; seeded
stateful edits and save-fault injection are tracked separately in #442.

The `GUI crash and flake campaign` workflow runs baseline repetitions weekly and
supports manual baseline/display campaigns. These expose failures, not retry
them away. The broader architecture and release criteria are tracked in #443.
