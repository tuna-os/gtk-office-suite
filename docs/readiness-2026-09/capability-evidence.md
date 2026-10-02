# [P0] Replace completeness claims with a revision-bound capability and test evidence ledger

Audited on `e7e4df6`. The checked-off #95 backlog, stale ROADMAP.md, PARITY tables, and current implementation disagree. This checkout was initially 128 commits behind main; all new evidence must identify its revision.

## Design
Maintain a machine-readable capability manifest with stable feature IDs, app/format/direction, admitted scope, owner issue, required test IDs by layer, and status: implemented-unverified / verified / failing / deferred. Record revision, environment, command, test IDs, skips and artifact URLs with each test run. Validate references against collected tests, not source-string existence alone. Derive the published roadmap/status table from this evidence.

## Status (2026-09-12)

The ledger exists: `conformance/capabilities.json`, validated by
`conformance/validate_capabilities.py` with mutation tests in
`tests/test_validate_capabilities.py`, checked in the fast PR lane and in
both test lanes against collected tests and recorded outcomes.

Done here: the schema and statuses, validation against collected tests
rather than source strings, per-layer reporting, rejection of skipped or
missing results behind a verified claim, duplicate-test detection (which
immediately found #507), dated waivers, and validator mutation tests.

Not done here, deliberately: back-filling every PARITY.md row (an entry
asserts someone watched those tests pass at a named revision — writing
that from prose would reproduce exactly the unfounded claims this
replaces), deriving the published roadmap table from the ledger, and
reconciling README/ROADMAP/TESTING wording beyond pointing at it.

## Work
Each row below is ticked from the instrument that implements it, not from
this document's prose. `[~]` means part of the row holds and the rest is
named; the reasoning is under the row so a reader can disagree with it.

- [~] Preserve existing conformance/validate_parity.py and corpus ratchets; extend their schema rather than create a competing scorecard.
  - Holds: `validate_parity.py` and its E1–E4 ratchet are untouched and still
    run on every pull request (`ci.yml`, including the `--base` ratchet
    comparison), and `tests/test_validate_parity.py` still guards it.
  - Does not: the capability ledger is a second file with its own schema
    rather than an extension of PARITY.md's. That was deliberate — the two
    answer different questions, and the Status note above says so — but the
    row as written asked for an extension, so it is not ticked.
- [x] Report model, persistence, controller/bridge, GUI, interoperability, accessibility/visual and performance separately.
  - Holds: `LAYERS = ("model", "format", "bridge", "gui", "a11y", "performance")`,
    required per capability and reported per layer; evidence for a layer a
    capability does not require is rejected.
  - Does not: **persistence** was not a distinct layer, so save durability
    landed under `format` or `model` and a green format layer could hide an
    unproven save transaction. **Now it is** (#1207): `persistence` is in
    `LAYERS`, and `suite.save.atomic-replace` is the first capability to cite
    it. Checked when this row was ticked, no save-durability evidence was
    left under `format` or `model`, and a structure check keeps it that way:
    a test from the `atomic_save`, `autosave` or `save_transaction` modules
    cited under either is rejected as belonging to `persistence`
    (`StructureTests.test_save_durability_evidence_must_be_filed_under_persistence`).
- [x] No marker, closed issue, skipped test, missing oracle, or --no-run report may count as observed success.
  - C4 in `validate_capabilities.py`, with mutation tests for a skipped
    result, a failed result and a test that never ran. Evidence must be a
    collected test with a recorded outcome, so a source marker or a closed
    issue cannot stand in for one by construction.
- [x] Detect duplicate Python test classes/functions and missing/uncollected test references.
  - Duplicate classes and duplicate methods within a class are rejected; a
    renamed or deleted test is rejected; a test no lane collects is rejected.
    This found the duplicate `TablesNamedRangeSmoke` (#507) on its first run.
- [x] Every waiver names an issue, reason, scope and review date; release-critical skipped tests fail the release gate.
  - Holds: `check_waivers` requires issue, reason, scope and review date,
    rejects an expired review date, a missing field, a `deferred` capability
    with no waiver, and a waiver for a capability that is not in the ledger.
  - **The consumption half is done** (#1207). A capability can be marked
    `"release_critical": true`. `validate_capabilities.py --release-critical`
    (C7) refuses unless each one is verified, or deferred under a waiver
    that passes C5, and unless every test it cites in the namespaces the
    report covers passed. A skipped one is a refusal. It runs in the PR
    `test` lane on every revision, and in `release-revision.yml`'s new
    `release-critical-tests` job, which runs the workspace at the release
    revision; `publish-flatpak.yml` calls that workflow before it
    publishes. Mutation tests: `tests/test_validate_capabilities.py::ReleaseCritical`.
    The journeys' half now runs there too: `release-critical-tests` builds
    the apps, runs the GUI tests the release-critical capabilities cite,
    and adds their pytest JUnit to the same check
    (`collect_test_inventory.py --pytest-junit`, which names them in the
    ledger's path form and speaks only for the tests it ran, so a few
    journeys cannot vouch for the whole file). A failed, skipped or missing
    journey refuses the release like a Rust test does; checked by running
    `LettersSaveFailureSmoke` with its report then marked failed.
- [x] Reconcile README, ROADMAP.md, docs/ROADMAP.md, TESTING.md and the historical implementation plans with the new tracker.
  - #1285. ROADMAP.md's status columns are gone: an item's state is its
    issue's, and progress is this ledger's. The figures left in README,
    ROADMAP.md and `docs/TESTING.md` are each checked by
    `tests/test_roadmap_figures.py`: the corpus scores against the ratchet
    baselines and the corpus sizes (recorded in `*-total.txt` beside each
    baseline and held there by each corpus's own test), the oracle floors
    against the oracle suites, and `StatusDocuments` fails on any other
    `n/m` figure. Two were stale when checked: ROADMAP.md's CommonMark
    630/652 (the baseline is the whole spec) and `conformance/scorecard.py`'s
    Letters corpus out of 104 (it has 109). `docs/ROADMAP.md`, the July
    product roadmap and backlog and the v1.0 test plan now open by saying
    they are history, which the same test requires.
- [x] Add mutation tests for the validator: nonexistent tests, duplicated IDs, omitted required layers, stale revision, failed or skipped results must be rejected.
  - All five, each as its own test in `tests/test_validate_capabilities.py`,
    plus a mutated copy of the real ledger that must be rejected.

Exit: a reviewer can select any advertised feature and follow its current-revision evidence through the required layers. A green model corpus never implies a green GUI save journey. Coordinate #313 for running validator tests in CI and #326 for release consumption.

