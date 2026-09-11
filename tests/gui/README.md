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

## Repeated campaigns

`stress.py` answers a different question from the smoke gate. The gate runs
each journey once and asks whether this change broke it; a campaign runs them
many times and asks how often a journey that passes once passes.

```sh
# 20 repetitions of the whole smoke file, replayable from the seed
tests/gui/stress.py --repeat 20 --seed 20260907 --output /tmp/campaign

# one repetition across the display matrix (400/800/1280/1920, light/dark, 1x/2x)
tests/gui/stress.py --repeat 1 --matrix display --output /tmp/matrix

# one app, five times
tests/gui/stress.py --repeat 5 --select Tables --output /tmp/tables
```

Each attempt shuffles the journey order with a generator derived from the
seed, so the same `--seed`, `--repeat`, `--matrix` and `--select` replay the
same campaign — a journey that only fails after another one has run is a real
class of bug, and a fixed order never finds it.

`manifest.json` records the revision, the app binaries' SHA-256, the seed, and
for every attempt its config, order, verdict, duration, JUnit report, log and
retained artifacts. Failures are classified as `product-crash`,
`assertion-mismatch`, `timeout`, `infrastructure` or `unclassified`; the
summary reports the first-attempt failure rate.

There is no retry, and there will not be one: a green rerun does not unfail
the first attempt. Run a diagnostic repeat as a second campaign with its own
output directory, which keeps both results.

`.github/workflows/gui-stress.yml` runs a bounded campaign nightly in the same
container the recorded journeys use, and takes `repeat`, `seed`, `matrix` and
`select` on manual dispatch. The nightly schedule uses the baseline display
only, and the matrix stays opt-in until the narrow-width findings it keeps
turning up are closed: #516 is fixed, but a full 47-journey matrix pass still
reports more (see #520). A known-red nightly teaches people to ignore it.
