# [P0] Replace completeness claims with a revision-bound capability and test evidence ledger

Audited on `e7e4df6`. The checked-off #95 backlog, stale ROADMAP.md, PARITY tables, and current implementation disagree. This checkout was initially 128 commits behind main; all new evidence must identify its revision.

## Design
Maintain a machine-readable capability manifest with stable feature IDs, app/format/direction, admitted scope, owner issue, required test IDs by layer, and status: implemented-unverified / verified / failing / deferred. Record revision, environment, command, test IDs, skips and artifact URLs with each test run. Validate references against collected tests, not source-string existence alone. Derive the published roadmap/status table from this evidence.

## Status (2026-09-11)

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
- [ ] Preserve existing conformance/validate_parity.py and corpus ratchets; extend their schema rather than create a competing scorecard.
- [ ] Report model, persistence, controller/bridge, GUI, interoperability, accessibility/visual and performance separately.
- [ ] No marker, closed issue, skipped test, missing oracle, or --no-run report may count as observed success.
- [ ] Detect duplicate Python test classes/functions and missing/uncollected test references.
- [ ] Every waiver names an issue, reason, scope and review date; release-critical skipped tests fail the release gate.
- [ ] Reconcile README, ROADMAP.md, docs/ROADMAP.md, TESTING.md and the historical implementation plans with the new tracker.
- [ ] Add mutation tests for the validator: nonexistent tests, duplicated IDs, omitted required layers, stale revision, failed or skipped results must be rejected.

Exit: a reviewer can select any advertised feature and follow its current-revision evidence through the required layers. A green model corpus never implies a green GUI save journey. Coordinate #313 for running validator tests in CI and #326 for release consumption.

