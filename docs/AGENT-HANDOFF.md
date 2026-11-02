# Office suite agent handoff — 2026-10-13

## User instruction at handoff

The user explicitly requested: write the handoff, commit it, then stop after
writing the agent instructions. Do not interpret this handoff as authorization
to resume implementation in the current session. Future work should start with
the user's next instruction.

## Start here

The complete implementation specifications were merged in
[PR #1554](https://github.com/tuna-os/gtk-office-suite/pull/1554), merge revision
`9f089bf5a5fcebb6bea98c91f78be9b9310ffabe`:

- `docs/implementation/README.md`: handoff contract, execution protocol,
  parallel ownership, verification commands, deferred-scope registry.
- `docs/implementation/tables.md`: 20 packages, `TBL-001` through `TBL-020`.
- `docs/implementation/letters.md`: 15 packages, `LET-001` through `LET-014`
  plus `LET-005S` for concrete mixed-section document modeling.
- `docs/implementation/decks.md`: 21 packages, `DCK-001` through `DCK-021`.
- `docs/implementation/shared.md`: eight shared release packages,
  `SH-001` through `SH-008`.

These files are on `main`, but **not yet on the current older implementation
branch**. Read them from the merged revision or integrate main deliberately;
do not recreate them or mistake their absence on this branch for missing work.
The 64 packages specify remaining work, not completed capabilities. Their
coverage maps reconcile readiness gates, historical roadmap items, and issues.
Do not declare the whole roadmap complete or the apps daily-driver-ready.

## Current checkout and protected changes

Repository: `/home/ubuntu/dev/tuna-os/gtk-office-suite`.
Branch: `work/tables-protection-persistence`.
Implementation head before this handoff commit:
`3b97d980fbd33d32c87c11a5da3f812e5c6684b3`.
Open implementation [PR #1552](https://github.com/tuna-os/gtk-office-suite/pull/1552).
This handoff commit is local unless a later agent explicitly pushes it.

The following pre-existing research changes belong to the user. Do not stage,
rewrite, revert, clean, or delete them:

- `docs/research/01-survey.md`
- `docs/research/02-deep-dive.md`
- `docs/research/05-implementation-plan.md`
- untracked `docs/research/08-office-engines-and-design-prior-art.md`

No local Rust build was run. The user requested parallel CI builds to save
local time and disk. Disk previously filled; only verified stale temporary
checkout `/tmp/gtk-office-suite` and our downloaded `/tmp/decks-edited-render`
were removed. Do not generalize that into permission to delete active work.

## Immediate unfinished work: PR #1552

### Implemented and pushed

Commit `add64900` implements XLSX cell protection and worksheet permission
persistence across the real reader/writer path:

- Cell lock/hidden flags, default/style-zero protection, inheritance and
  `applyProtection` semantics, including blank cells and later sheets.
- Modeled sheet permissions with OOXML disallow-bit inversion.
- Export of protection on ordinary cells, formula cells, blank styled cells,
  and merged cells, retaining formula formatting/caches.
- Explicit loss warnings for unsupported imported protection credentials and
  non-representable permission settings. Never treat protection as encryption,
  never silently activate inactive sheet protection, never pretend an imported
  credential is a plaintext password.
- Core and integration tests, including all 32 modeled permission combinations.
- Two actual GUI journeys in `TablesProtectionPersistenceSmoke`:
  `test_unlocked_blank_cell_remains_editable_after_save_and_reopen` and
  `test_password_loss_requires_consent_and_cancel_preserves_source`.

Relevant files: `tables-core/src/io/{props,xlsx_styles,load,numfmt,ods_styles,save,loss}.rs`,
`tables-core/tests/{xlsx_loss_budget,pivot_and_protection_tests}.rs`,
`tests/gui/test_smoke.py`.

### Observed failure and repair — verification still pending

At `add64900`, workspace tests, all three Flatpak builds, render/export jobs,
real-file corpus jobs and oracle passed. The GUI run
[38434400260](https://github.com/tuna-os/gtk-office-suite/actions/runs/38434400260)
reported **72 passed, one failed, two skipped** in the Tables selection.
The password-loss consent/cancel journey passed. The unlocked-cell journey
saved and reopened successfully, edited B1 from 42 to 84, then timed out after
undo because the accessible grid description still reported `cell B1: 84`.
Do not reinterpret this failed run as a pass.

Source inspection showed the undo/redo action in `tables/src/window.rs`
only followed the sheet switcher and queued drawing, without refreshing
selection/accessibility. Pushed repair `3b97d980` invokes both the existing
`refresh_sel` callback and `grid_area::refresh_grid_a11y` after a successful
history step. ROADMAP's measured window size was updated from 2234 to 2237.
The original GUI assertion remains unchanged; no failure was suppressed.

**The repair is not yet verified at handoff.** Its exact-revision GUI run is
[38439078711](https://github.com/tuna-os/gtk-office-suite/actions/runs/38439078711).
A read-only background watcher was started as `bg_42`:
`gh run watch 38439078711 --exit-status --interval 60`.
Session job IDs may not survive; use the GitHub run ID in a future session.
Other new-revision runs include CI `38439078750`, Nightly `38439078776`, and
real-file corpus `38439078840`.

An immediate post-push `gh pr checks --watch` returned old-revision results
before new checks registered. Always confirm `headSha` equals the revision
being claimed; short-SHA filtering with `gh run list --commit` returned no
matches, so use the full SHA.

### Required next steps when implementation resumes

1. Read the exact-revision GUI result and logs. If failed, diagnose the actual
   failure; do not rerun unchanged checks merely hoping for green.
2. Require the unchanged unlocked-cell undo assertion and password-consent
   journey to pass. Keep verification in CI; do not start a local heavy build.
3. Review current CI results for the same full SHA. The unchanged RustSec
   failure described below is not a new regression, but must remain disclosed.
4. Only after observed proof, update `docs/readiness-2026-09/tables-readiness.md`
   with the bounded persistence capability, test IDs, run link and exact
   verified revision. Update the capability ledger according to its existing
   convention if claiming a new capability. Do not check off the broad chart,
   recovery, or all-protection readiness gates from this slice.
5. Reconcile the specs' explicit PR #1552 pending status with actual evidence
   if/when it merges. Keep unverified credential/hidden-formula UI work pending.
6. Run relevant documentation/capability validators, push explicitly staged
   files, and use normal PR merge only after required checks and the changed
   GUI journey pass. Never bypass with admin privileges. Auto-merge is disabled
   in the repository; the branch's configured required context is `test`, which
   alone is weaker than the evidence necessary for this change.

## Prior merged implementation baseline

[PR #1550](https://github.com/tuna-os/gtk-office-suite/pull/1550) merged as
`6dacab4de5a8e5795877f7e27ae3a1fa05c4fb18`. It includes the three-app assessment,
Decks accessible inline text editing with undo, fail-closed source-save safety,
missing-image presentation preflight, and atomic Tables user-operation
protection for compound edits. Protected sheet structure is not workbook
structure protection. Trusted undo/recovery remains exempt from user-operation
rejection. Preserve these regression guarantees.

The last baseline implementation evidence recorded before PR #1552 is
`af1ef767c5cc8cc39ef13989f732073c1b41c38f`, including GUI run `38426268309`.
Use the readiness pages and ledger for the actual bounded claims.

## Architecture and reference decisions

- Tables still uses IronCalc 0.8.3 for calculation. Retain one controller mutation
  authority; audit upstream semantics before adding competing formula logic.
  File I/O currently uses calamine/rust_xlsxwriter; upstream IronCalc I/O fixes
  do not automatically improve this application. The live model's dense storage
  must not be described as solved merely because a separate sparse benchmark
  exists. Follow the Tables specs' migration and performance acceptance.
- Letters: sole document authority and screen/export revision agreement precede
  more features. Mixed sections require the explicit LET-005S model migration,
  not another widget-local representation. Do not assume open PRs #1533/#1546
  are merged without checking their current state.
- Decks: preserve source package relationships safely, establish stable identity
  and transaction semantics, then finish design/presenter workflows. Existing
  text editing, tables/charts and master assets are baselines, not reasons to
  invent duplicate engines. Real-file issue #1397 remains unclosed by this work.
- REA (`https://github.com/morluto/rea`) is optional observation/research tooling,
  not a Rust office engine or an automatic Excel/Numbers implementation.
  Prefer public specs, upstream source and authored reference documents;
  distinguish inventory from observed behavior. Keep provenance and licensing
  for any further BetterOffice/Artcraft/reference-app investigation. Do not
  claim reference products were reverse-engineered merely from this handoff.

## Known release blocker

Issue #1549 tracks unchanged `unsafe-libyaml` advisory RUSTSEC-2024-0449 through
Typst 0.15.1's direct `serde_yaml` dependency. It is an unmaintained dependency
advisory, not evidence of an observed exploit. A Hayagriva-only migration does
not remove the direct path. Do not suppress the advisory to make CI green.

## Specification verification already completed

All five specification files passed local relative-link and duplicate-package-ID
checks: 64 distinct work packages. Local checks passed:

- The retired source-citation assertion passed one test at that revision.
  Current checks use the capability inventory and JUnit outcomes.
- `python3 tests/test_roadmap_figures.py` — 18 tests.
- `python3 tests/test_feature_docs.py` — three tests; existing ResourceWarnings.
- `python3 conformance/validate_capabilities.py` — 19/19 structural entries.

PR #1554 required tests passed in runs `38436939325` and `38436943885`, then
normal squash merge completed. These docs results do not verify the pending
PR #1552 repair or certify the whole suite.

## How to execute future packages

Read repository `AGENTS.md` and `docs/implementation/README.md` first. Select
bounded packages with satisfied dependencies. Keep model/parsing/algorithms in
GTK-free core crates. Give parallel agents independent file ownership and an
explicit shared contract; do not have multiple agents rewrite the same model
or window. Each agent must report exact files, consumer-visible acceptance,
loss/error policy, and observed verification revision. Run shared CI validation
after integration, not repeated builds mid-flight. Keep real GUI/render proof
for changed surfaces and independent reopen proof for format changes. Merge
small complete slices, update evidence honestly, and never count a spec, mock,
skipped oracle, or isolated engine test as a delivered daily-driver workflow.
