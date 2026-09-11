# Recorded journey evidence in CI

Every GUI journey runs against a real application on a real display. This
adds a recording of that display to the run, so a change can be reviewed
by watching the application do the thing, not only by trusting a green
check.

Three pieces:

| Piece | Path | What it is |
|---|---|---|
| Test container | `tests/gui/container/Containerfile` | The environment the journeys are true in: GTK4 build deps, Xvfb, Matchbox, AT-SPI, fonts, ffmpeg, dogtail, pinned Rust |
| Recorder | `tests/gui/framework/recorder.py` | ffmpeg x11grab around each journey, plus GIF/poster rendering |
| Evidence bundle | `tests/gui/collect_evidence.py`, `tests/gui/pr_comment.py` | Videos → GIFs + `summary.md` + `evidence.json` → the pull-request comment |

## Recording a journey

Recording is controlled by two environment variables and is off unless
asked for:

```bash
GUI_TEST_VIDEO=all       # keep every clip
GUI_TEST_VIDEO=failures  # record everything, keep only failed journeys
GUI_TEST_VIDEO=off       # default
GUI_TEST_VIDEO_DIR=…     # where clips land (default tests/gui/videos)
```

Setting `GUI_TEST_VIDEO_DIR` alone implies `all`.

```bash
just verify                       # all smoke journeys, recorded, evidence/ built
just verify 'test_smoke.py -k Letters'
just verify-container             # the same, inside the CI image
```

Recording never changes a verdict. A missing `ffmpeg`, a recorder that
cannot start, an encode that produces nothing — each prints a warning and
the journey's own assertions still decide pass or fail. That is the
point: video is evidence *about* a result, never the result.

Clips are recorded from the same Xvfb display the AT-SPI assertions ran
against, and start before the application launches, so a window that
never appears is on tape too.

## Getting a video onto a pull request

Add the **`verify-video`** label to the pull request (or run the
*Feature verification video* workflow manually with a journey selection).
The workflow runs the journeys in the container, records them, and posts
a single comment — updated in place on later runs — with a result table
and each clip inline.

The clips have to be fetchable by URL for GitHub to render them, and
there is no API for attaching media to a comment, so they are pushed to
the orphan `ci-evidence` branch under `pr-<n>/<run-id>/`. Only the newest
run per pull request is kept. That branch is generated output, not
source: nothing builds from it and it can be deleted at any time.

It is opt-in because a recorded run costs several minutes of runner time.
The deterministic smoke gate in `gui-tests.yml` still runs on every push,
and now records failures: when it goes red, the artifacts include a video
of each failing journey.

Fork pull requests are skipped with a reason in the job log — their token
cannot publish media or comment. Run the workflow manually against the
branch to produce the same evidence.

## The container

`tests/gui/container/Containerfile` is the single definition of the GUI
test environment; `.github/workflows/gui-container.yml` builds it, proves
it can build, display and encode, and publishes
`ghcr.io/tuna-os/gtk-office-suite/gui-test:main` from `main` (weekly
rebuild for base-image updates; pull requests build and check it without
publishing).

```bash
tests/gui/container/run.sh tests/gui/run_gui_tests.sh test_smoke.py
tests/gui/container/run.sh --build bash     # build the image locally first
```

Container runs put Cargo output in `target/container/` so a host build and
a container build do not overwrite each other; the harness honours
`CARGO_TARGET_DIR` when it looks for the binaries.

`run_gui_tests.sh` still starts its own Xvfb, window manager and D-Bus
session inside the container — the image supplies the tools, the script
owns the session. `GUI_TEST_PYTHON` overrides the interpreter for
environments whose `/usr/bin/python3` is not the one dogtail and the GTK
introspection bindings were built for.

Note the smoke gate in `gui-tests.yml` still installs its dependencies
with apt rather than using this image. Moving it over is a one-line
change once the image is published from `main`, and is deliberately left
as a separate step so a container problem cannot take the merge gate down
with it.

## Working through the roadmap with this

For a roadmap item ([docs/readiness-2026-09](readiness-2026-09/README.md)):

1. Write the journey that asserts the behavior — AT-SPI assertions, no
   VLM judgment, same as any gating test.
2. Implement until it passes locally (`just verify 'test_smoke.py -k …'`).
3. Open the pull request and label it `verify-video`.
4. The comment shows the journey passing on the real application at that
   revision. The video is the evidence the readiness roadmap asks for;
   the assertions are what keeps it honest.

A recording is not a substitute for an assertion. A journey that proves
nothing produces a video that shows nothing, and no amount of footage
turns "it looked fine" into evidence.
