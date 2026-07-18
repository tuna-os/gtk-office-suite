# Testing & Automation

Three tiers, cheapest first. A change should be caught by the cheapest tier
capable of catching it.

## Tier 1 — unit tests (gate, milliseconds)

Plain `cargo test` in GTK-free code (`suite-common-core`, and the logic
modules of each app). This is where TDD happens: new parsing, formatting,
undo, layout, or model logic starts as a failing test here.

```bash
cargo test --workspace          # everything
cargo test -p suite-common-core # core only — no GTK headers needed
```

If you can't unit-test a behavior because it's welded to a widget, that's
an extraction signal, not an excuse — see the layering rule in
[DEVELOPMENT.md](DEVELOPMENT.md).

## Tier 2 — GUI smoke tests (gate, ~10 seconds)

`tests/gui/test_smoke.py`, run by `tests/gui/run_gui_tests.sh`. Deterministic
AT-SPI assertions through dogtail: app launches, window appears, typing
reaches the editor, the word count updates. No API keys, no screenshot
judging — these must never flake.

```bash
# one-time deps (Ubuntu)
sudo apt-get install xvfb dbus at-spi2-core python3-dogtail \
  python3-pytest python3-pil python3-requests
python3 -m pip install --break-system-packages mss

# run (starts Xvfb itself if you have no display; uses yours if you do)
tests/gui/run_gui_tests.sh test_smoke.py
```

The runner compiles GSettings schemas, starts a private D-Bus session, and
enables accessibility — the three things GTK apps need that a bare CI runner
lacks. Add a smoke test when you add user-visible behavior that AT-SPI can
assert deterministically (a label's text, a node's existence, focus). Keep
them shallow; depth belongs in tier 1.

Known limitation: use system `/usr/bin/python3` (the runner does this) —
apt's dogtail 0.9.11 lives in dist-packages and differs from pip dogtail 2.x.

## Tier 3 — VLM visual audit (non-gating, scheduled)

`tests/gui/test_letters.py`, `test_tables.py`, `test_decks.py` assert
screenshots via Gemini (`framework/base.py: assertVision`). They run in the
scheduled `vlm-audit` CI job with `continue-on-error` — informative for
visual/HIG regressions, never a merge blocker, because model judgments flake
and need `GEMINI_API_KEY`. Locally they skip without a key.

## CI map

| Workflow | Trigger | Gates? | Contents |
|---|---|---|---|
| `ci.yml` | push, PR | yes | cargo check, clippy, unit tests; coverage on main; Flatpak builds |
| `gui-tests.yml` → `smoke` | push/PR to main | yes | tier 2 under Xvfb |
| `gui-tests.yml` → `vlm-audit` | daily 06:00 UTC, manual | no | tier 3 + screenshot artifacts |

House rule: **never `|| true` a test step.** The GUI workflow ran that way
for weeks while pytest wasn't even installed, and three launch-blocking bugs
(apps exiting at startup, the Letters editor orphaned from its window)
shipped behind a green badge. Honest red is the product; see PR #86.

## Automation notes for agents

- Iterate locally under Xvfb before spending CI rounds; the runner script
  gives CI-equivalent conditions.
- When driving CI on a branch: `gh workflow run "GUI Tests" --ref <branch>`,
  then check conclusions before pulling logs.
- Debug AT-SPI by dumping the tree (`dogtail.tree`) rather than guessing
  role names; GTK4 role mappings are surprising, and a widget missing from
  the tree usually means a real allocation/mapping bug in the app.

## Oracle coverage target (adopted 2026-07-18)

The bar for through-LibreOffice coverage (I4): **every green Tier-1/2
PARITY row that persists data has at least one oracle assertion per
format direction it claims** — numerically 25+ Letters, 20+ Tables,
20+ Decks (~65–70 tests). Each test writes our file, has LibreOffice
read/rewrite it, and re-reads the result through our own readers,
asserting the *attribute*, not just the text.

Rules of engagement:
- New oracle tests are written red-first; a wave that comes back all
  green earns another probe into an uncovered row.
- Above ~70 hand-written tests, breadth comes from the LO-authored
  corpora (`lo_parity.rs`), which are ratcheted and cover many features
  per file at lower CI cost.
- Display-precision differences (Calc CSV rounding, Impress soft line
  breaks) are normalized in the test, not in the engines.
