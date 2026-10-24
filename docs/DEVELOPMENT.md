# Development Guide

How to set up, build, run, and change this codebase. For testing specifics
see [TESTING.md](TESTING.md); for module layout see
[ARCHITECTURE.md](ARCHITECTURE.md); agents start at the repo-root
[AGENTS.md](../AGENTS.md).

## Setup

If something below doesn't work, [USER-TROUBLESHOOTING.md](USER-TROUBLESHOOTING.md)
covers the common setup and build failures.

Requirements: Rust stable ≥ 1.80 (edition 2021, lockfile v4), GTK4 ≥ 4.14
and libadwaita ≥ 1.5 dev headers.

```bash
# Debian/Ubuntu
sudo apt-get install libgtk-4-dev libadwaita-1-dev libglib2.0-bin

# check
cargo check --workspace
```

If `cargo` reports a lockfile-version error, an old distro cargo is shadowing
rustup on your PATH — `which cargo` should print `~/.cargo/bin/cargo`.

## Building and running

```bash
cargo build --bin letters --bin tables --bin decks
```

The apps read GSettings and **abort at startup if their schemas aren't
compiled**. Outside Flatpak, point them at a compiled schema dir:

```bash
mkdir -p /tmp/gtk-office-schemas
cp flatpak/*.gschema.xml /tmp/gtk-office-schemas/
glib-compile-schemas /tmp/gtk-office-schemas
GSETTINGS_SCHEMA_DIR=/tmp/gtk-office-schemas cargo run -p letters
```

Flatpak builds (the release path) use the manifests in `flatpak/` and the
GNOME 50 runtime; CI builds all three on every push.

## Where code goes

The load-bearing rule: **widget code must not own logic.**

| Layer | Crates | May depend on GTK? | Tested by |
|---|---|---|---|
| Core logic | `suite-common-core` (and future `*-core` per app) | **No** | unit tests, fast, everywhere |
| GTK shell helpers | `suite-common` | Yes | sparingly |
| Apps | `letters`, `tables`, `decks` | Yes | GUI smoke tests |

If a function can be written against plain data (`&str`, offsets, model
structs) instead of `GtkTextBuffer`/`TabView`, write it that way and put it
in a core crate with tests. The app crates' `engine.rs`/`undo.rs` modules
are the current homes for per-app logic; the direction of travel is to
extract them into GTK-free `letters-core`/`tables-core`/`decks-core` crates.

Engine choices (deliberate, don't churn): IronCalc for spreadsheet
formulas/model, rdocx for DOCX I/O, pulldown-cmark for Markdown, Typst
(in-process through `suite-export`) for PDF export, zspell for spell
checking, zip + quick-xml for PPTX.

## Workflow

Use one topic branch per issue. The base branch is `main`; do not commit
directly to it.

1. Update local `main` before starting:

   ```bash
   git switch main
   git pull --ff-only origin main
   ```

2. Create a topic branch. Replace `docs/my-change` with a short
   `<type>/<description>` name appropriate to the change:

   ```bash
   BRANCH=docs/my-change
   git switch -c "$BRANCH"
   ```

3. Keep commits focused, use the commit style in
   [CONTRIBUTING.md](CONTRIBUTING.md), and run the checks in
   [TESTING.md](TESTING.md) that cover the changed surface. Before publishing
   the branch, check the diff:

   ```bash
   git diff --check origin/main...HEAD
   ```

4. Publish the branch:

   ```bash
   git push --set-upstream origin "$BRANCH"
   ```

5. Open a pull request against `main`. Link the issue, describe evidence and
   known limits, and wait for required checks before merge.

6. If `main` advances during review, refresh the branch before requesting
   another review:

   ```bash
   git switch "$BRANCH"
   git fetch origin
   git rebase origin/main
   git diff --check origin/main...HEAD
   ```

   Resolve conflicts locally, rerun relevant checks, then publish the
   refreshed branch. Do not rebase while another contributor is changing the
   same branch.

- PRs run check, clippy, unit tests, Flatpak builds, and GUI smoke tests. A
  red smoke test means an app is broken for real users, not a flaky test.
- Before pushing: `cargo test --workspace` and
  `tests/gui/run_gui_tests.sh test_smoke.py`.
- GitHub Issues track work; `gh` CLI is the expected interface.
