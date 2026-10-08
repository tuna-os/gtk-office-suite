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

In the same mode, `GTK_OFFICE_TEST_SAVE_PAUSE=<stage>:<file name>` holds a save
of that file for twenty seconds at `created` (its temporary just made) or
`written` (the new bytes synced, not yet renamed), so a journey can SIGKILL the
real app mid-write instead of racing a write that lasts milliseconds
(`KilledMidSaveMixin`, #1217).

On failure, the base class retains the screenshot, AT-SPI tree, application
log, input trace, and snapshot (when configured) under
`tests/gui/failure_artifacts/`.

## Shards and screenshots

CI splits the journeys across sixteen jobs (`smoke (K/16)` in
`.github/workflows/gui-tests.yml`). Any run can do the same:

```sh
GUI_TEST_SHARD=3/16 tests/gui/run_gui_tests.sh test_smoke.py test_edit_render.py
```

The unit is the test class, balanced by journey count
(`framework/sharding.py`); a class that must follow another names it with
`shard_with`.

With `GUI_TEST_SCREENSHOT_DIR` set, every journey leaves a screenshot of how it
ended, passed or failed, named by its test id (`.failed.png` for a failure).
CI uploads each shard's as `gui-screenshots-K` and merges them into one
`gui-screenshots` artifact per run. To see what changed between two runs:

```sh
GITHUB_TOKEN=... scripts/compare_gui_screenshots.py RUN_BEFORE RUN_AFTER
```

writes `gui-screenshot-diff/report.html`, the most changed journeys first.
Either side may also be a local directory of screenshots.

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

## Visual matrix

`visual_matrix.py` screenshots each app in four states: editor, a selection,
the Keyboard Shortcuts window, and the "Could not open" message. It does this
at widths 400/800/1280, in light, dark and high contrast, at scales 1 and 2,
and compares each screenshot with
`visual/expected/<config>/<app>-<state>.png` (#1284).

```sh
tests/gui/visual_matrix.py                # compare all 18 configs
tests/gui/visual_matrix.py --only w800    # some of them
tests/gui/visual_matrix.py --update       # record new expected images, on purpose
```

- **Pinned rendering.** Fonts are the bundled DejaVu pair (`visual/fonts.conf`
  loads no system fonts), and the renderer is GSK's cairo. The caret doesn't
  blink, and a screenshot is taken only once two frames in a row are identical.
- **Comparison.** Both images are compared at half resolution. A pixel
  differs when a channel is more than 32/255 apart. A cell fails when more
  than its threshold of pixels differ: the default in `visual/thresholds.json`,
  or the cell's own entry there.
- **Evidence.** `visual/out/` (or `--out`) keeps, for every cell, the
  screenshot, the app's state snapshot and the result. On a mismatch it also
  keeps the expected image and a diff, with differing pixels in red. Over
  everything it writes `report.md` and `report.json`.
- **Nightly.** `.github/workflows/visual-matrix.yml` runs it every night in
  the GUI image. It doesn't gate yet. A dispatch with `update=true` records
  expected images from that environment as an artifact.
