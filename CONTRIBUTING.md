# Contributing to GTK Office Suite

Thanks for your interest. This repository holds three GNOME apps written in
Rust with GTK4 and libadwaita: **Letters** (word processor), **Tables**
(spreadsheet) and **Decks** (presentations). They ship as Flatpaks.

This page is the short version. The detailed guides are:

- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): setup, building, where code goes.
- [docs/TESTING.md](docs/TESTING.md): the test lanes and what each one proves.
- [AGENTS.md](AGENTS.md): the commands CI runs and the pitfalls that have
  already bitten us. It's written for agents, but it helps humans too.

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

GTK Office Suite is licensed under GPL-3.0-or-later (see [LICENSE](LICENSE)).
By contributing, you agree that your contributions are licensed the same way.
