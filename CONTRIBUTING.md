# Contributing to GTK Office Suite

Thanks for your interest. This repository holds three GNOME apps written in
Rust with GTK4 and libadwaita: **Letters** (word processor), **Tables**
(spreadsheet) and **Decks** (presentations). They ship as Flatpaks.

This page is the short version. The detailed guides are:

- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): setup, building, where code goes.
- [docs/TESTING.md](docs/TESTING.md): the test lanes and what each one proves.
- [AGENTS.md](AGENTS.md): the commands CI runs and the pitfalls that have
  already bitten us. It's written for agents, but it helps humans too.

<!-- hive-contribute-plea: donated-compute appeal, keep in sync across repos -->
## Contribute compute — no code needed

**[Donate time with your AI CLI](https://hub.tunaos.org/#contribute).**

Your machine runs one task at a time using your signed-in AI coding CLI.
The relay alternates between eligible **School** (OS) and **Reef** (apps)
hives after each task. If one hive has no work, the other can continue.

The guide covers prerequisites, verified `just` setup, credentials, provider
costs, and how to stop with Ctrl+C. Your subscription quota or API charges apply.
Never post account credentials or API keys in issues or pull requests.

## What we're working on

The current priority is the
[Render Parity Roadmap](docs/RENDER-PARITY-ROADMAP.md): make each app draw
documents the way LibreOffice does, measured by screenshots in CI. The UI
direction (ideas from iWork and Google's editors, built with the GNOME HIG
and libadwaita) is in [docs/DESIGN-UI.md](docs/DESIGN-UI.md). Designs that
haven't been decided yet are RFCs in [docs/rfc/](docs/rfc/README.md).

Looking for somewhere to start? Try issues labelled
[`good first issue`](https://github.com/tuna-os/gtk-office-suite/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22).

## Set up

You need Rust stable 1.80 or newer, GTK ≥ 4.14 and libadwaita ≥ 1.5 with
their development headers:

```bash
# Fedora
sudo dnf install gtk4-devel libadwaita-devel
# Debian / Ubuntu
sudo apt-get install libgtk-4-dev libadwaita-1-dev libglib2.0-bin
```

A Nix flake (`flake.nix`) is also provided.

## Build and run

```bash
cargo build --bin letters --bin tables --bin decks
```

The apps read GSettings and abort at startup if the schemas aren't
compiled. Outside Flatpak, compile them once and point the app at them:

```bash
mkdir -p /tmp/gtk-office-schemas
cp flatpak/*.gschema.xml /tmp/gtk-office-schemas/
glib-compile-schemas /tmp/gtk-office-schemas
GSETTINGS_SCHEMA_DIR=/tmp/gtk-office-schemas cargo run -p letters
```

## Test

The workspace contains GTK widget tests, and GTK can't start without a
display, so run the unit tests under Xvfb:

```bash
xvfb-run -a cargo test --workspace         # or: cargo test -p tables-core
tests/gui/run_gui_tests.sh test_smoke.py   # GUI smoke journeys (needs Xvfb and AT-SPI)
```

If a build or test step fails in a way that looks like your setup, see
[docs/USER-TROUBLESHOOTING.md](docs/USER-TROUBLESHOOTING.md).

CI also runs clippy with warnings denied, so run it before you push:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

If you change what a document looks like on screen, add or update a
render-lab fixture and check it against LibreOffice with
`tools/render-lab/run.sh --app <app>`. The
[roadmap](docs/RENDER-PARITY-ROADMAP.md#running-it) explains how. It's
fine to push a draft pull request and let CI run the heavy jobs: CI
renders every app and uploads the report.

### Add or update a render-lab fixture

A fixture is one small document that isolates one visual feature, so a
regression names the feature (`letters/bold` is red) instead of "the report
is 3% different".

1. **Write it** in `tools/render-lab/fixtures.py`, in the function for its
   app (`letters()`, `tables()` or `decks()`). Build the document with the
   foreign writer the file already uses (python-docx, openpyxl or
   python-pptx), not our own writers, so our reader sees input we didn't
   produce. Finish with that function's `save(...)` helper (or
   `add(app, feature, path, expect)`), giving a short feature id and a
   one-line `expect` saying what must be visible. Keep one feature per
   fixture. To update a fixture, edit its existing block and keep its id,
   or the baseline entry no longer matches.
2. **Opt in to export checks** only if the fixture should also compare our
   PDF export with LibreOffice's: add `"<app>/<feature>"` to the `EXPORT`
   set in the same file. Do that only once its screenshot is green.
3. **Render it.** `tools/render-lab/run.sh --app <app>` builds the
   fixtures, renders them with LibreOffice and with our app, and writes
   `render-lab-out/report.html` (git-ignored). Open the report and look at
   your fixture's images; a green verdict on a page that isn't drawn is not
   a pass. The script re-runs itself inside the `render-lab` container, so
   build that first with `podman build -t render-lab tools/render-lab`.
   Add `--tier A` to run one tier.
4. **Lock it in.** The baseline `tools/render-lab/baseline.json` is a
   ratchet: CI fails if a fixture gets worse (`regressed`) and also if one
   gets better or is new and the baseline doesn't say so (`stale`). After
   checking the images, run
   `tools/render-lab/run.sh --app <app> --update-baseline` and commit the
   changed `baseline.json` with your change. If you'd rather not render
   locally, push a draft PR, download the `render-parity-report` artifact
   (`gh run download <run> -n render-parity-report`) and commit its
   `baseline.proposed.json` as `baseline.json`.

Real-document fixtures (published forms, spreadsheets and decks) live in
`tools/render-lab/real_corpus/`, are run with `RENDER_LAB_CORPUS=real`, and
ratchet against `baseline-real.json`; see
[its README](tools/render-lab/real_corpus/README.md). The design is in
[docs/RENDER-PARITY-ROADMAP.md](docs/RENDER-PARITY-ROADMAP.md).

## Where code goes

Keep logic out of widget code. Anything that doesn't need a GTK type
belongs in a GTK-free core crate (`suite-common-core`, `letters-core`,
`tables-core`, `decks-core`), with a unit test next to it. The app crates
(`letters`, `tables`, `decks`) and `suite-common` wire up widgets and
signals. Each app's `window.rs` has a line ceiling (see
[ROADMAP.md](ROADMAP.md)), so extract code into a module rather than
growing it.

## Pull requests

1. Open or find an issue for the change.
2. Branch from `main`.
3. Write commit messages in the existing style:
   `feat(tables): …`, `fix(letters-core): …`, `docs: …`.
4. Make sure the tests and clippy pass, then open the pull request against
   `main`. Every CI check has to be green before a merge.

To report a security vulnerability, don't open a public issue: follow
[SECURITY.md](SECURITY.md).

GTK Office Suite is licensed under GPL-3.0-or-later (see [LICENSE](LICENSE)),
except `lifafa/`, which is AGPL-3.0-or-later (see `lifafa/LICENSE`). By
contributing, you agree that your contributions are licensed the same way as
the directory they change: new files under `lifafa/` carry
`SPDX-License-Identifier: AGPL-3.0-or-later`, everything else
`GPL-3.0-or-later`.
