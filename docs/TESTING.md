# Testing & Automation

Three tiers, cheapest first. A change should be caught by the cheapest tier
capable of catching it.

## Tier 1 — unit tests (gate, milliseconds)

Plain `cargo test` in GTK-free code (`suite-common-core`, and the logic
modules of each app). This is where TDD happens: new parsing, formatting,
undo, layout, or model logic starts as a failing test here.

```bash
xvfb-run -a cargo test --workspace  # everything
cargo test -p suite-common-core     # core only — no GTK headers, no display
```

Tier 1 is not entirely GTK-free: the workspace also holds widget tests that
go through `suite_common::gtk_test::run`, which needs a display to
initialise GTK — hence `xvfb-run` on the whole-workspace form. Those tests
**fail** without one rather than skipping. That is deliberate: they used to
skip and pass, so the PR job ran them with no display and 36 of them
stopped running while the harness still reported `0 ignored` (#241). If you
genuinely cannot give the run a display, opt out explicitly and they are
skipped:

```bash
SUITE_GTK_TESTS=skip cargo test --workspace
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
sudo apt-get install xvfb dbus at-spi2-core matchbox-window-manager python3-dogtail \
  python3-pytest python3-pil python3-requests
python3 -m pip install --break-system-packages mss

# run (always starts its own private Xvfb on a display it allocates;
# GUI_TEST_REUSE_DISPLAY=1 to watch it on yours instead)
tests/gui/run_gui_tests.sh test_smoke.py
```

The runner compiles GSettings schemas, starts a private D-Bus session, and
enables accessibility — the three things GTK apps need that a bare CI runner
lacks. Add a smoke test when you add user-visible behavior that AT-SPI can
assert deterministically (a label's text, a node's existence, focus). Keep
them shallow; depth belongs in tier 1.

Known limitation: use system `/usr/bin/python3` (the runner does this) —
apt's dogtail 0.9.11 lives in dist-packages and differs from pip dogtail 2.x.
If the interpreter those packages were built for is not `/usr/bin/python3`
on your machine, point `GUI_TEST_PYTHON` at the one that is. The symptom is
`ImportError: cannot import name '_gi'` at collection, and the extension
module's filename names the version it wants:

```bash
ls /usr/lib/python3/dist-packages/gi/_gi.cpython-*.so   # e.g. ...-312-...
GUI_TEST_PYTHON=/usr/bin/python3.12 tests/gui/run_gui_tests.sh test_smoke.py
```

Setup fails loudly rather than handing the journeys a display that is not
there: it waits for Xvfb to report its number, for the display to answer,
and for the window manager to claim it, and exits naming whichever step
failed. `GUI_TEST_READY_SECONDS` (default 60) budgets the probes and
`GUI_TEST_XVFB_SECONDS` (default 60) the server start — two knobs because
starting a server and probing one that already started are different costs,
and a test that shortens one must not starve the other. Each wait breaks
the moment its condition holds, so raising either costs a fast machine
nothing and a slow one gets the room it needs.

Journeys can record themselves. `GUI_TEST_VIDEO=all` keeps a clip of every
journey, `failures` keeps only the ones that failed, and
`tests/gui/collect_evidence.py` turns a directory of clips into GIFs and a
summary. `just verify 'test_smoke.py -k Letters'` does both. The recording
never influences a verdict — see
[CI-VIDEO-EVIDENCE.md](CI-VIDEO-EVIDENCE.md).

## Tier 3 — VLM visual audit (non-gating, scheduled)

`tests/gui/test_letters.py`, `test_tables.py`, `test_decks.py` assert
screenshots via Gemini (`framework/base.py: assertVision`). They run in the
scheduled `vlm-audit` CI job with `continue-on-error` — informative for
visual/HIG regressions, never a merge blocker, because model judgments flake
and need `GEMINI_API_KEY`. Locally they skip without a key.

## The capability ledger (what is actually proven)

`conformance/capabilities.json` records, per capability, which tests prove
it at which layer and the revision that was observed at.
`conformance/validate_capabilities.py` checks those claims against the
tests CI actually collected and their recorded outcomes, so evidence
cannot quietly stop meaning anything:

```bash
python3 conformance/validate_capabilities.py          # structure, waivers, duplicates
python3 conformance/collect_test_inventory.py --junit "=target/nextest/ci/junit.xml" \
    --out inv.json --results-out res.json
python3 conformance/validate_capabilities.py --collected inv.json --results res.json
```

A renamed or deleted test, a layer with no test, a skipped or failing test
behind a `verified` claim, an expired waiver, or two test classes sharing a
name all fail the build. Add an entry when you can name the tests that
prove a capability *and* the revision you watched them pass on; leave the
status at `implemented-unverified` until then. `docs/PARITY.md` remains the
human-facing scorecard — the ledger is what a reviewer can check.

## CI map

| Workflow | Trigger | Gates? | Contents |
|---|---|---|---|
| `ci.yml` | push, PR | yes | cargo check, clippy, unit tests; coverage on main; Flatpak builds |
| `gui-tests.yml` → `smoke` | push/PR to main | yes | tier 2 under Xvfb |
| `gui-tests.yml` → `vlm-audit` | daily 06:00 UTC, manual | no | tier 3 + screenshot artifacts |
| `gui-container.yml` | container file changes, weekly, manual | yes (for itself) | builds/verifies/publishes the GUI test image |
| `feature-verification.yml` | `verify-video` label, manual | yes | records journeys in the container, posts the clips on the PR |

The capability ledger is checked in three places: structure in the fast PR
lane, against collected Rust tests and their outcomes in `ci.yml`'s test
job, and against collected journeys in `gui-tests.yml`'s smoke job.

The versioned office interoperability contract lives in
[`interop/corpus.json`](../interop/corpus.json). Its cheap structural check
runs on every PR and can be run locally with `python3
interop/validate_corpus.py`. It validates metadata, both directions for DOCX/
ODT, XLSX/ODS, and PPTX/ODP, and package relationships/content types without
comparing ZIP bytes. The OnlyOffice conversion lane is opt-in via
`ONLYOFFICE_BIN`; LibreOffice remains the required behavioral oracle in the
nightly workflow.

Format importers also expose a GTK-free structured compatibility report. It
classifies detected content as must-preserve, opaque pass-through, warn-on-loss,
or hard-error. `OpaquePackage` carries uninterpreted ZIP members across an
unrelated edit, while `CompatibilityReport::validate_save` blocks hard errors
and requires explicit confirmation for destructive loss. The report and
pass-through behavior are unit-tested in `suite-common-core/src/interop.rs`.

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

The formula oracle compares supported formula results after LibreOffice
recalculation. Exact comparisons are used for text, integers, and booleans;
floating-point results use an absolute tolerance of `0.01` to avoid treating
formatting or implementation rounding as a functional mismatch. Importer
fuzzing separately exercises malformed ZIP/XML inputs; see [`fuzz/README.md`](../fuzz/README.md).
