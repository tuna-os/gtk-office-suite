<!-- See CONTRIBUTING.md for the full guide. -->

## What and why

<!-- What the change does and the problem it solves. -->

Fixes #

## How it was tested

<!-- Which of these you ran, and anything else. For a bug fix, say whether the
new test fails without the fix. -->

- [ ] `xvfb-run -a cargo test --workspace` (or the changed crates)
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] GUI journeys: `tests/gui/run_gui_tests.sh test_smoke.py -k <name>`
- [ ] Render or export parity: `tools/render-lab/run.sh --app <app>` (if what a document looks like changed)
- [ ] `python3 -m pytest tests/ --ignore=tests/gui` (if docs, scripts or the roadmap changed)

## Checklist

- [ ] Commit messages follow the existing style (`fix(letters-core): …`, `docs: …`)
- [ ] Docs, docs/PARITY.md and the readiness ledger are updated if behaviour changed
- [ ] No `window.rs` grew past its ceiling in ROADMAP.md
- [ ] Any change to saved files keeps round-trips intact (or says what is lost)
