# CLAUDE.md — agent guide for gtk-office-suite

Three GTK4/libadwaita apps in Rust: **Letters** (word processor), **Tables**
(spreadsheet), **Decks** (presentations), plus shared crates. Flatpak is the
ship vehicle. Human-oriented docs: `docs/DEVELOPMENT.md`, `docs/TESTING.md`.

## Commands

```bash
cargo build --bin letters --bin tables --bin decks   # build apps
cargo test --workspace                               # unit tests (~90, fast)
tests/gui/run_gui_tests.sh test_smoke.py             # GUI smoke tests (~10s, needs Xvfb deps)
cargo clippy --workspace                             # lint (many pre-existing warnings)
```

Running an app outside Flatpak needs compiled GSettings schemas or it aborts:

```bash
mkdir -p /tmp/gtk-office-schemas
cp flatpak/*.gschema.xml /tmp/gtk-office-schemas/
glib-compile-schemas /tmp/gtk-office-schemas
GSETTINGS_SCHEMA_DIR=/tmp/gtk-office-schemas cargo run -p letters
```

## Architecture rule (the one that matters)

**No business logic in widget code.** If a function does not need a GTK type
to do its job, it belongs in `suite-common-core` (GTK-free, unit-testable) —
or a future `letters-core`/`tables-core`/`decks-core` crate. `window.rs`
files wire signals and render; they must not own document models, parsing,
or algorithms. When you touch logic currently trapped in a `window.rs`,
prefer extracting it over growing it.

Crate map: `suite-common-core` (pure Rust: undo, formats, events, search,
units) ← `suite-common` (GTK helpers: dialogs, toasts, SuiteApp/SuiteWindow)
← app binaries.

## Testing expectations

- New logic ⇒ unit test in the same (GTK-free) crate. TDD preferred.
- New user-visible behavior ⇒ extend `tests/gui/test_smoke.py` only if it can
  be asserted deterministically via AT-SPI (no VLM assertions in gating tests).
- CI: `ci.yml` (check/clippy/test/flatpak) and `gui-tests.yml` (smoke job
  gates; VLM audit is scheduled + non-gating). Never add `|| true` to a test
  step — that is how three launch-blocking bugs shipped undetected in June
  2026 (see PR #86).

## Gotchas that have bitten before

- GTK4 custom widgets: allocate children in `size_allocate`, never in
  `snapshot()` — snapshot-time allocation breaks mapping, focus, and AT-SPI
  (PageContainer bug, PR #86).
- Don't `set_content`/parent a widget twice; `gtk_overlay_set_child` fails
  silently as a GTK-CRITICAL and orphans the subtree.
- `main()` must end in `suite.run()`; a stray early `return` compiles fine
  (clippy warns unreachable) and ships a binary that exits with code 0.
- The find `GtkSearchBar` captures window-level keystrokes; editors must
  `grab_focus` when mapped or typing goes to the search entry.
- AT-SPI test framework is dogtail 0.9.11 (apt), not pip dogtail 2.x — no
  `tree.Accessible`, no `retry=` kwarg on `findChildren`.
