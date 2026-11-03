# Tables implementation specification

Status: **specification, not implementation or release certification**. Assessed
2026-10-13 against the source and evidence described below. Stable package IDs
are `TBL-001` through `TBL-020`; proposed APIs and test names are explicitly
marked. This document specifies all remaining Tables product work in the current
readiness/roadmap scope, including incompletely satisfied historical issues.

## Authority, checkpoint, and execution rules

Read these before implementing a package:

- [Tables readiness](../readiness-2026-09/tables-readiness.md), especially
  **Direction and delivery order**, is the product order: safe editing and
  persistence, calculation authority, native workflows, analysis, graduation.
- [ADR 0007](../adr/0007-tables-advanced-analysis.md) admits richer charts,
  pivots, protection and upstream-dependent arrays; it does not certify them.
- [ROADMAP](../../ROADMAP.md), [render roadmap](../RENDER-PARITY-ROADMAP.md),
  [performance/accessibility](../readiness-2026-09/performance-accessibility.md),
  [recovery](../readiness-2026-09/recovery.md), and
  [interoperability](../readiness-2026-09/interoperability.md) supply the other gates.
- [#439](https://github.com/tuna-os/gtk-office-suite/issues/439) is open.
  [#112](https://github.com/tuna-os/gtk-office-suite/issues/112),
  [#113](https://github.com/tuna-os/gtk-office-suite/issues/113), and
  [#114](https://github.com/tuna-os/gtk-office-suite/issues/114) are closed, but
  their acceptance criteria remain relevant. Closure is not runtime evidence.
  The older unchecked #439 issue body must not erase the more recent checked
  journeys in the readiness ledger.

**Verified checkpoint versus pending work.** Main checkpoint is PR #1550,
`6dacab4d`. Its assessment records controller cross-tab pivots, narrow SEQUENCE
coverage, chart-render evidence and atomic protection rejection. The historical
protection operation-boundary evidence is `af1ef767` and
[GUI run 38426268309](https://github.com/tuna-os/gtk-office-suite/actions/runs/38426268309).
  Protection persistence **PR #1552 merged as `5485dd97`** (evidence revision
  `3b97d980`, [GUI run 38439078711](https://github.com/tuna-os/gtk-office-suite/actions/runs/38439078711)
  smoke pass plus core `test`, oracle, tables corpus, interop and all three Flatpak builds green; only the
  pre-existing RustSec advisory #1549 fails, unchanged by that PR). Its importer/exporter, loss warnings, budget
  additions, GUI journeys, undo/redo accessible-description refresh, readiness evidence and
  `tables.protection.persistence` ledger entry are on main. TBL-004 consumes that merged work rather than asking
  another implementer to recreate it.

No builds, tests, formatters, reference applications or manual Orca sessions were
run to author this specification. Existing test names below are inspected test
locations, not claims that this revision passes. Historical results are evidence
only for the revision recorded by their source. RustSec
[#1549](https://github.com/tuna-os/gtk-office-suite/issues/1549) remains a shared
release blocker through Typst's direct serde_yaml path; Tables cannot certify a
release while that gate is unresolved.

### Invariants all packages inherit

1. `WorkbookController` is the user-mutation gateway and owns the single
   `History<Op>`. Address operations by stable `sheet_id`, never by a stale
   tab index. Selection/focus is session state; document changes are operations.
2. `TablesEngine`/IronCalc owns input/formulas, calculation and dependencies.
   `SheetModel` owns admitted sheet metadata and view state; its calculated
   values are a projection, not another formula engine. Do not turn spill
   results or formatted display strings into independent user inputs.
3. One accepted user operation is one history group. Rejected/cancelled operations
   leave engine, metadata, dirty state, selection where appropriate, and redo
   intact. Trusted undo/recovery restores previously authorized state; it does
   not route through new user authorization and fail halfway through.
4. Keep calamine + rust_xlsxwriter until a same-corpus comparison justifies an
   explicit replacement. IronCalc's separate XLSX adapter is not automatically
   used by Tables. No second production writer or local expression evaluator.
5. Supported content must survive load → unrelated edit → save → Tables reopen
   and independent-reader reopen. Unsupported destructive changes must be
   specifically disclosed before any write. Cancel keeps source bytes,
   dirty state, identity and recovery. ODS/XLS/CSV/TSV remain import-only;
   `.xlsm` is not a writable macro-preserving format; `.xlsb` is refused.
6. GTK/libadwaita actions/dialogs and existing grid remain. No ribbon clone,
   Numbers free-form canvas, business logic in widget callbacks, or blanket
   engine rewrite. Do not raise `window.rs`'s 2300-line ceiling to add features.
7. Every package updates affected existing feature/format/readiness evidence when
   its behavior actually lands. Preserve checked historical regressions. New
   test IDs in this document are **proposed**, not already collected tests.

### CI contract used by each package

The package's `CI` field selects these existing lanes; shared infrastructure and
release policy are owned by [shared specifications](shared.md), not reimplemented
here.

- **Core:** `.github/workflows/ci.yml` workspace checks/clippy/nextest/docs,
  Python/corpus validators and relevant Tables tests. An implementer may use
  `cargo test -p tables-core --test <existing-target>` for focused diagnosis;
  required CI must still collect the full affected suite and collab-feature
  contracts. Never replace a failing test with a narrower claim.
- **GUI:** `.github/workflows/gui-tests.yml`, existing
  `tests/gui/run_gui_tests.sh` and `tests/gui/test_smoke.py` harness; deterministic
  state waits, private display/bus/XDG directories, owned-process cleanup,
  screenshots/logs/JUnit on failure. `visual-matrix.yml` supplies wide/narrow,
  light/dark/high-contrast and scale coverage. A D-Bus action proves action
  routing, not keyboard reachability; require actual keys for keyboard claims.
- **Interop:** `tables-core/tests/soffice_oracle.rs`, `package_consistency.rs`,
  `xlsx_loss_budget.rs`, corpus lane and `.github/workflows/file-corpus.yml`.
  Required oracle lanes set `REQUIRE_SOFFICE=1`; a missing oracle is failure,
  not a pass. Record LibreOffice version. Inspect XML/relationships independently
  of Tables' importer for every new persistence claim.
- **Render:** `.github/workflows/render-parity.yml` Tables A/B jobs and export
  lab, `render-real.yml` for real documents; preserve budgets and reviewed
  baseline ratchets. Attach app/reference/diff images, semantics and metrics.
  New visual features get a fixture before parity claims. Keep builds and
  per-app jobs parallel in CI, not competing local full-workspace builds.
- **Installed:** exact-revision Flatpak Tier C, release/upgrade/soak/evidence
  workflows and manual Orca under shared gates. These are external/runtime
  prerequisites, not work that can be replaced by a source-only assertion.

## Coverage and dependency map

| Authoritative checklist / admitted work | Remaining packages | Existing evidence intentionally retained |
|---|---|---|
| Readiness order 1: individual XLSX losses, restoration, later sheets, recovery, independent reader | TBL-001–005, TBL-017 | `xlsx_loss_budget`, format-safe save/loss GUI journeys; atomic operation protection is already checked |
| Order 2: one authority, operation audit, arrays/function compatibility | TBL-006–008 | Engine SEQUENCE and blocked-spill cases only; stable-ID sheet undo |
| Order 3: coherent selection/edit/reference/F4/clipboard/fill/sort/filter/freeze/names/search/sheets | TBL-007, TBL-009–011, TBL-018 | Two-sheet, structure, fill/filter/name-box/clipboard journeys remain regression tests |
| Order 4 / final unchecked Work and acceptance item / ADR 0007 / #114 | TBL-002, TBL-004–005, TBL-008, TBL-012–013 | Existing five chart kinds and static cross-tab values, not full chart/pivot persistence |
| Order 5: four daily-driver scenarios, measured workloads, installed evidence | TBL-015–020 | Existing microbudgets and prior installed checks, not current-release signoff |
| Readiness checked CSV/ODS/XLS safety; #113 ODS interop | TBL-001, TBL-017, TBL-020 | Source bytes unchanged and correctly suffixed XLSX Save As; no ODS writer promised |
| Readiness checked Unicode/offset regression | TBL-001, TBL-015, TBL-019 | `io::load::tests::xlsx_round_trip_keeps_a_lone_bottom_right_cell`, `xlsx_round_trip_keeps_content_at_its_own_coordinates`, `offset_start.ods`; retain full Unicode generator |
| #112 sparse live storage; insert/delete/move metadata; cumulative sizes; Pango; budgets | TBL-007, TBL-015–016, TBL-019 | Prefix geometry exists; standalone SparseGrid benchmarks do not prove live sparse workbook storage |
| #113 print area, page setup, hiding, row height, names/formulas | TBL-007, TBL-011, TBL-014 | Existing model/controller/dialog/IO paths, independently test their composition |
| ROADMAP #168 GUI architecture debt | TBL-009–014 as touched | Existing `chart_dialog`, `sheet_actions`, `persistence` seams; no unrelated mass decomposition |
| Render Phase 3, remaining amber fixtures, screen/PDF differences | TBL-012, TBL-014, TBL-019 | Baseline currently has `number-formats`, `wrap-text`, `wrap-merged` amber; pivot fixture A/B green |
| Shared save/recovery/stress/interop/a11y/security/release | TBL-017–020 plus shared SH-001–SH-008 | No duplicate atomic-save, autosave-envelope, harness or release system |
| ROADMAP collaboration/extensions/enterprise/CLI proposals | Explicit boundary below | Not blockers for dependable everyday Tables editing; preserve existing collab operation compatibility when changed |

**Delivery graph:** TBL-001 starts the loss ledger. Land and verify TBL-004
without bundling chart or credential work. TBL-002/003 use TBL-001's ledger;
TBL-005 builds on TBL-004. TBL-006 defines ownership before TBL-007/008/015.
TBL-009–011 consume those controller contracts. TBL-012 follows chart persistence;
TBL-013 follows safe operation/persistence semantics. TBL-014 follows shared
render semantics. TBL-016 measures TBL-015 and all operation changes.
TBL-017/018/019 extend incrementally with each feature; TBL-020 is the integration
exit, not a reason to defer feature-specific recovery/a11y tests until the end.
Independent scoped PRs may run CI concurrently; merge a dependency before claiming
its dependent acceptance. No dates or effort estimates are assigned.

## TBL-001 — Complete the XLSX loss ledger and save consent

**Goal/problem and evidence.** `tables-core/tests/xlsx_loss_budget.rs::BUDGET`
currently asserts broad chart existence rather than individual fields and
explicitly loses later-sheet charts/rules and filter-vs-manual-hide identity.
`io/loss.rs::content_a_save_drops` combines model and original-package scanning.
`tables/src/loss_ui.rs::save_after_asking` deduplicates labels for the existing
save question. This is a useful base, not proof that every unsupported worksheet
attribute is reported. `persistence.rs::save_engine_to_xlsx` is the common writer
and extension guard.

**Targets/ownership/dependencies.** Existing `io/{loss,load,save,format}.rs`,
`tables/src/{loss_ui,persistence,window}.rs`, `xlsx_loss_budget.rs`,
`save_capability.rs`, `package_consistency.rs`. No dependency beyond shared save
contract. Core owns loss feature IDs and locations; UI renders that report and
never decides independently what a writer can preserve.

**Ordered steps.** (1) Split budget entries into chart owner/series/order/name/
color/ranges/title/axis/legend/anchor/size, each protection flag, credentials,
conditional-rule ownership, filter criteria, formula/cached-value/array state,
names/scopes, notes/validation, hidden state, dimensions, print settings and safe
opaque parts. (2) For every `Lost` entry assert the feature existed before save
and the loss report names its location and consequence; retained fixtures must
not spuriously warn. (3) Extend package scan to model-insufficient content,
including unsupported charts and rule shapes instead of treating every drawing
containing only chart references as preserved. (4) Present useful affected-sheet
and feature detail, retaining Cancel as default and Save Anyway as destructive.
(5) Exercise Ctrl+S, Save As and close-guard Save through the same report/serialize/
commit contract. Save As confirmation describes the target loss and that the
original stays untouched. (6) Fail closed or retain an unresolved compatibility
state when source parts cannot be inspected; inability to read the original must
not silently mean no losses. Consume shared save-generation/race policy.

**Edges/data loss.** Missing/replaced source, mixed-chart/picture drawing,
unsupported extension, unsaved model-only features, repeated saves after an
explicit lossy conversion, ODS conversion, malformed relationships, Cancel after
editing, and failed remote commit. Do not blindly carry old worksheet XML over
fresh edits; opaque retention is only safe under existing package rules.

**Tests/GUI/render/interop/CI.** Extend existing budget/format/package tests and
`TablesLossQuestionSmoke`, `TablesOpaquePartsSmoke`, `TablesCsvSaveSmoke`,
`TablesOdsSaveSmoke`, `TablesXlsSaveSmoke`. Proposed IDs:
`tbl001_each_loss_has_a_location`, `tbl001_cancel_all_save_routes`,
`tbl001_unreadable_source_is_not_loss_free`. Compare source hashes, dirty/history
and recovery state, then saved XML and independent reader. Core + GUI + Interop;
no new render claim from a textual warning.

**Acceptance/PR boundary/non-goals.** One inventory-and-consent PR can land before
fidelity fixes; every declared loss is observable and cancellable, every retained
field has both-reader assertions. Do not add a new format writer, macro execution,
external refresh, picture editor or unsupported-content preservation fiction.

## TBL-002 — Relationship-correct, field-complete chart persistence

**Goal/evidence.** `io/charts.rs::read_charts_from_xlsx` enumerates drawing/chart
ZIP names, pairs anchors by enumeration index, reads the first category/value
range, clears series/axis titles and defaults legend Right. It discards sheet
qualifiers in `parse_range`. `load_xlsx_workbook` is the common open path; repair
it, not a GUI sidecar. `ChartSpec` has both `series` and legacy `cat`/`val`, while
`save_sheets_to_xlsx_bytes` writes richer chart fields.

**Targets/contracts/dependencies.** `io/{charts,load,save,props,loss}.rs`,
`sheet.rs::{ChartSpec,ChartSeries}`, controller chart metadata consumers, and
existing chart tests. Depends TBL-001. Resolve workbook→worksheet→drawing→chart
relationships and anchor relationship IDs; ZIP order/filenames are not identity.
Keep chart ownership separate from referenced data-sheet identity. Normalize
admitted single-column same-sheet series into one canonical series vector;
migrate all current fallback callers rather than maintaining conflicting copies.
If chart ranges gain sheet IDs, use stable IDs internally and quoted names at IO.

**Steps.** (1) Author a two-sheet, two-drawing fixture with shuffled ZIP member
order/nonstandard part names and multiple charts per drawing. (2) Parse scoped
XML elements with existing bounded XML/package facilities, preserving every
series, literal name, admitted color, chart title, axis titles and legend.
(3) Resolve one-cell/two-cell anchors using actual column/row geometry; explicitly
report unsupported anchor offsets or kinds rather than silently substituting
480×280. Either model/round-trip offsets or warn with exact limitation.
(4) Validate category/value lengths and finite bounded geometry; admit
cross-sheet/row-oriented ranges only with a model and writer contract, otherwise
report the precise unsupported range. (5) Serialize canonical data and relationships;
retain unsupported chart details only when safe, else TBL-001 consent.
(6) Tighten each loss-budget row only after Tables and independent reopen agree.

**Edges.** Quoted/Unicode/apostrophe sheet names, rename/reorder/delete/undo,
multiple series with identical names, pie's series restrictions, scatter numeric
X values, unknown combo/3-D charts, external references, missing rel targets,
malformed EMUs, negative anchors and empty/cached-only ranges. No wrong-sheet
fallback or chart disappearance without report.

**Tests and CI.** Extend `io/charts.rs::chart_tests`, `xlsx_loss_budget.rs`,
`snapshot_fidelity.rs`, `soffice_oracle.rs`, `package_consistency.rs`;
proposed `tbl002_relationship_order_independent`,
`tbl002_all_chart_fields_later_sheet`, `tbl002_chart_unsupported_report`.
Add fixtures in `tools/render-lab/fixtures.py` before visual claims, including
multi-series legends and axes. Core + Interop + Render; GUI save/reopen journey
joins TBL-012. Foreign writer plus Tables writer fixtures are both required.

**Acceptance/boundary/non-goals.** A parser/model/IO PR preserves every admitted
field on later sheets and recovery, with a separate chart UI PR. No broad
“charts retained” acceptance, no full Excel chart catalog, no new canvas model.

## TBL-003 — Worksheet-scoped conditional rules and persistent filters

**Goal/evidence.** `io/condrules.rs::read_cond_rules_from_xlsx` has a first-sheet
reader; the loss budget marks later-sheet rules lost. `SheetModel.hidden_rows`
is filter-derived visibility; `hidden_rows_manual` is a separate set. Current
XLSX load writes all saved hidden rows into the manual set. Consequently Clear
Filter after reopen cannot restore the same semantics. Frozen rows currently
act as sort headers, which is not an explicit data-range/header contract.

**Targets/ownership/dependencies.** `io/{condrules,props,load,save,loss}.rs`,
`sheet.rs::{CondRule,SheetModel}`, `controller/{view,ops}.rs`, `column_menu.rs`
and `window_dialogs.rs`. Depends TBL-001; range transforms align with TBL-007.
Core owns rule/filter definitions and derives visibility; UI only edits them.
Add a **proposed** explicit filter range/header/column-criteria model instead of
trying to reconstruct criteria from hidden rows. Retain independent manual hides.

**Steps.** (1) Read rules per resolved worksheet and preserve admitted comparison,
range, threshold(s), fill and priority semantics; unknown expressions/styles/
stop-if-true behavior must warn, not be approximated silently. (2) Introduce
filter definitions and an atomic controller operation; derive hidden rows for
all active column criteria together. (3) Serialize/import admitted XLSX autoFilter
range and value criteria, with explicit header exclusion. (4) Keep manual hide
state through clear/reset and save; unsupported Excel table filters remain
specific losses. (5) Recompute criteria after authorized cell/row changes;
undo restores definition and resulting visibility. (6) Expose criteria/range in
existing native column/filter controls, including clear one/all behavior.

**Edges.** Two differently filtered sheets, empty cells/duplicate labels/Unicode,
numeric-vs-text equality, multiple simultaneous columns, manually hidden matching
rows, zero visible rows, filtered header, deleted range endpoints and protected
sheets. Unsupported top-N/date/color filters must not be reported as preserved.

**Tests/GUI/render/CI.** Extend `xlsx_loss_budget.rs`, controller `tests.rs`,
`snapshot_fidelity.rs`, `soffice_oracle.rs`; proposed
`tbl003_rules_resolve_worksheet`, `tbl003_filter_clear_after_reopen`,
`tbl003_multi_column_filter_undo`. Extend `TablesFilterSmoke`,
`TablesColumnMenuSmoke`, `TablesSheetStructureSmoke`; add a later-sheet rule
fixture and visible filtered/cleared comparison. Core + GUI + Interop + Render.

**Acceptance/boundary/non-goals.** Split conditional-rule ownership and explicit
filter persistence into separately green PRs; each updates only its ledger rows.
No general Excel-table authoring or arbitrary conditional-format formula engine.

## TBL-004 — Land and verify modeled protection persistence

  **Goal/status/evidence.** **PR #1552 merged as `5485dd97`; do not recreate it.** Merged
  `io/xlsx_styles.rs::XfStyle` carries `CellProtection`; `parse_protection` uses locked=true/hidden=false defaults,
  `load_xlsx_workbook` includes style-only blank extents, and `save.rs::cell_format` writes locks/hidden flags.
  Merged `loss.rs::protection_drops` warns about legacy/salted credentials and unmodeled permissions. Verified tests
  include `imported_protection_survives_later_sheet_edits_and_undo_in_real_xlsx`,
  `every_modeled_sheet_permission_combination_has_ooxml_disallow_semantics`,
  `inactive_cell_protection_survives_until_a_sheet_is_protected`, and `TablesProtectionPersistenceSmoke`
  (GUI run 38439078711; ledger `tables.protection.persistence` at evidence revision `3b97d980`).
  TBL-004's remaining work is TBL-001-contract conformance and any follow-up the merged evidence left open —
  credential-bearing round trips, hidden-formula UI and workbook structure protection stay pending.

**Targets/contract/dependencies.** Candidate files `io/{xlsx_styles,props,load,
save,loss}.rs`, `pivot_and_protection_tests.rs`, `xlsx_loss_budget.rs`,
`tests/gui/test_smoke.py`; depends TBL-001 loss contract and existing operation
protection. Scope is `locked`, `hidden_formula`, protected boolean and the five
currently modeled `allow_*` booleans. OOXML permission attributes generally mean
**disallow**, the inverse of model `allow_*`. Preserve effective defaults and
style inheritance. Protection is restriction, never encryption.

**Steps.** (1) Inspect candidate CI at its exact SHA and repair failures there;
only merge after Core/GUI/Interop gates, without marking all protection complete.
(2) Require XML-level assertions independent of the importer for every
permission combination, styled blank cells and a later sheet. (3) Add any missing
recovery/LibreOffice rewrite coverage; check inherited cell-XF protection,
explicit false/default values, and metadata-only extents. (4) Retain credential
and unsupported-permission loss warnings across all save routes. (5) Update
readiness/ADR wording after merge to say which flags are preserved and which UI
restrictions are actually enforced; do not remove password limitations.

**Edges.** Unprotected sheet with inactive cell flags, nondefault permissions on
an unprotected sheet (candidate explicitly warns they reset), sheet-only/default
styles, style-only far blank, imported salted credential bundle, locked/unlocked
mixed compound operation, undo after protecting, and pathological style extents.
Preserving a hidden flag is not proof the formula bar hides the formula (TBL-005).

**Tests/CI/UI/interop.** Candidate named tests plus existing
`controller::protection_tests`, `snapshot_fidelity.rs`,
`TablesSheetStructureSmoke` and proposed
`tbl004_protection_snapshot_later_sheet`,
`tbl004_calc_rewrite_preserves_modeled_permissions`. Core + GUI + Interop.
No new render parity claim; GUI must show refusal and successful unlocked edit.

**Acceptance/boundary/non-goals.** Scoped persistence PR plus evidence completion.
Exact candidate CI green and independent reopen before readiness status changes.
**Does not implement password verification, credential-preserving save, new
password dialogs, every Excel permission, or document encryption.**

## TBL-005 — Complete protection UI semantics and credential preservation

**Goal/evidence.** `SheetProtection` currently has opaque
`password_hash: Option<String>` and only selection/cell-format/row-insert/delete
permissions. `controller/view.rs::set_sheet_protection` toggles fields;
`sheet_actions.rs` provides a toggle, not verified password authentication.
The inspected formula/editor action sources do not consume `hidden_formula` or
selection permission fields. TBL-004 persistence alone must not imply enforcement.

**Targets/contracts/dependencies.** `sheet.rs::{SheetProtection,CellProtection}`,
`controller/{protection,view,ops,editing}.rs`, `io/{props,load,save,loss}.rs`,
`tables/src/{sheet_actions,formula_bar,window,grid_area,format_inspector}.rs`.
Depends TBL-004, TBL-006. Replace the ambiguous hash string with a **proposed typed
credential** distinguishing no credential, legacy verifier and modern salted
OOXML algorithm/hash/salt/spin-count tuple. Retain imported unsupported tuples
without asserting they can be verified. Never reinterpret a hash as plaintext
for rust_xlsxwriter's plaintext-password API; keep secrets out of logs/snapshots
except the format's intended verifier, never the entered plaintext.

**Steps.** (1) First implement hidden-formula presentation and selection permissions
through common controller/view eligibility helpers; show calculated values but
not hidden formula text while protected, including accessible descriptions and
clipboard formula extraction. (2) Expose native protection settings and cell
locked/hidden controls with one controller transaction and focus restoration.
(3) Enumerate OOXML protection attributes and effective defaults; either add
precise model/enforcement/IO support per attribute or retain an exact loss report.
(4) Separately implement credential preservation using bounded XML serialization
or a writer API verified to retain the tuple. Do not enable a password prompt
before verifier support exists. (5) Implement legacy and explicitly admitted
modern verifier algorithms according to public format specifications and known
fixtures, with bounded work for hostile spin counts; unsupported algorithms
produce a visible inability-to-unprotect, not successful unprotection.
(6) Wrong password/Cancel changes no state; successful unprotect is an explicit
operation; existing authorized undo/recovery remains possible. Explain that this
is not a security boundary or file encryption.

**Edges.** Missing half of modern tuple, invalid base64, huge spin count,
Unicode password rules, empty password vs none, credentials without sheet=true,
selection via mouse/keys/name-box, both selection options false, hidden spill
anchor, multi-cell copy, and Save Anyway credential loss. A permission to sort
must not bypass ordinary locked-cell rules by accident.

**Tests and CI.** Extend `controller/protection_tests.rs`,
`pivot_and_protection_tests.rs`, `io/loss.rs` tests; proposed
`tbl005_hidden_formula_all_surfaces`, `tbl005_selection_permissions`,
`tbl005_credential_tuple_roundtrip`, `tbl005_wrong_password_is_noop`.
Add **proposed** `TablesProtectionSettingsSmoke` in `test_smoke.py`; independent
LibreOffice fixtures verify preserved verifier and actual unprotect behavior.
Core + GUI + Interop + manual Orca for dialog/hidden-state announcements.

**Acceptance/boundary/non-goals.** UI/enforcement and credential support are
separate mergeable PRs; keep credential-loss warning until end-to-end support
lands. All admitted restrictions survive restart; no plaintext leakage or false
password success. No encrypted-workbook format or broad cryptography project.

## TBL-006 — Define and enforce calculation/model authority

**Goal/evidence.** `WorkbookState` holds both `TablesEngine` and `SheetModel`
matrices. `cell_input` falls back to a displayed engine value, `sync_active_sheet`
refreshes only the active projection, and `mutate_sheet` clones a sheet with only
a debug assertion against data/size edits. `set_cell_inputs` currently calls
`set_cell_text` (which evaluates) per cell then evaluates again. Engine write and
reference-translation wrappers can discard/fallback from upstream errors.
These are concrete audit points, not proof of one tested editing contract.

**Targets/ownership/dependencies.** `engine.rs`, `controller/{state,core,ops,
editing,collab}.rs`, `sheet.rs::sync_from_engine`, `snapshot.rs`,
`io/{load,save}.rs`. No feature dependency; coordinate TBL-001. Inputs, formulas,
names, dependencies and calculated values belong to IronCalc. Sheet metadata is
authoritative only for fields Tables owns. A projection may be cached but cannot
be written back as input. Reuse `engine.input_at` for editable input semantics.

**Steps.** (1) Inventory every write/read path including import, clipboard,
structural edits, sort, fill, pivot output, collab application, recovery and save;
record its upstream API and error semantics in code/tests. (2) Make engine writes
and projection refresh transactional with controller operation groups; propagate
errors instead of silently appearing successful. (3) Batch user inputs with
`put_cell_text` and a single calculation boundary where upstream permits it;
refresh all affected sheet projections, eagerly or by revision-keyed invalidation
before any consumer reads them. (4) Separate read-only displayed value and raw
input APIs; migrate save, snapshots, formula editor and copy consumers.
(5) Ensure `mutate_sheet` cannot silently bypass the content contract in release
builds; use typed operations for each affected field. (6) Keep one controller
history; do not introduce upstream/UI parallel undo stacks.

**Edges.** Inactive dependent sheet displayed after edit, switching during pending
input, cross-sheet cycles/errors, failed multi-op engine mutation, dead sheet ID,
no-op preserving redo, formula-string vs text beginning with `=`, and derived spill
cells. Loading must not commit partial workbook into the current window on error.

**Tests/GUI/interop/CI.** Extend `controller/{tests,ops_tests,collab_tests}.rs`,
`tests/stateful.rs`, `determinism.rs`, `xlsx_loss_budget.rs`; proposed
`tbl006_inactive_dependents_refresh`, `tbl006_failed_group_keeps_redo`,
`tbl006_projection_never_becomes_input`. GUI extends `TablesTwoSheetJourneySmoke`
and snapshot journeys to edit a precedent on another sheet and save without
visiting every dependent sheet first. Core + GUI + Interop; Render on projection
changes. **Acceptance/PR/non-goals:** one contract/error/batching PR, followed by
consumer migrations if needed, each fully compiling without duplicate authority.
No engine replacement or local recalculation/dependency implementation.

## TBL-007 — Formula-affecting operations and structural metadata

**Goal/evidence.** IronCalc wrappers already use `insert_rows`, `delete_rows`,
`insert_columns`, `delete_columns`, `move_sheet`, `extend_copied_value` and
`move_cell_value_to_area`. Reuse them. `SheetModel::shift_row_metadata` shifts
merge anchors but not interior spans; metadata transformation requires explicit
intersection behavior. `paste_fragment` clips outside existing dimensions;
pivot output does too. #112 also admits moving rows/columns, not just inserting
and deleting them.

**Targets/contracts/dependencies.** `engine.rs`, `controller/{editing,ops,view,
sheets}.rs`, `sheet.rs::{insert_rows,delete_rows,insert_cols,delete_cols,
shift_row_metadata,shift_col_metadata}`, `fill.rs`, `fragment.rs`.
Depends TBL-006; coordinate TBL-002/003/013 metadata. Core preflights entire source
and destination, grows safely or rejects visibly, then applies one operation.
Engine owns formula reference semantics; Tables transforms its owned metadata.

**Steps.** (1) Build an operation matrix: edit/clear/copy/paste/cut/move/fill/sort,
row/column insert/delete/move, sheet rename/reorder/delete and named-range change.
For each establish upstream capability and distinguish copy-relative from true
move/dependent-relink semantics. (2) Define exact interval transforms for before,
inside, through and after merges/rules/filter ranges/chart ranges/anchors/pivot
source/output/print area/frozen boundaries. Deletion may shrink/remove metadata;
never produce inverted ranges or resurrect deleted labels. (3) Add/finish native
row/column move operations via a supported engine API, or a tested engine-owned
composition if no atomic API exists; never regex-rewrite formulas. (4) Preflight
bounds, overlap, protection, spill ownership and validations before changing any
cell. (5) Capture original input and complete metadata in inverse operations;
restore sheet identity/dependent formulas exactly across undo/redo and reopen.
(6) Extend stateful generator to these combined operations and preserve minimized
failing traces.

**Edges.** Cut overlaps destination, partial merge deletion, moving across frozen
boundary, deleting all rows/last sheet, maximum coordinates, fill up/left,
absolute/mixed/quoted/cross-sheet references, hidden rows, validation/protection
metadata moving with cells and protected destination on a later sheet.

**Tests/GUI/render/CI.** Existing `controller/{tests,ops_tests,protection_tests}.rs`,
`tests/{stateful,property,snapshot_fidelity}.rs`; proposed
`tbl007_structural_metadata_intersections`, `tbl007_move_updates_dependents`,
`tbl007_paste_never_truncates`. Extend `TablesSheetStructureSmoke`,
`TablesFillHandleSmoke`, `TablesAutofillSeriesSmoke`, `TablesTwoSheetJourneySmoke`.
Independent reopen must assert formula strings and recalculated values, not just
cached text. Core + GUI + Interop + Render for merges/anchors/freeze changes.

**Acceptance/boundary/non-goals.** Split structural transforms, move, and
copy/fill transaction work into independently tested PRs under this ID. Every
admitted operation has atomic failure and one undo step. No hand-written formula
evaluator or “fix” that drops metadata to simplify transforms.

## TBL-008 — Dynamic arrays and function-by-function consumer compatibility

**Goal/evidence.** Existing `engine.rs` tests establish `SEQUENCE(3)` and occupied
`SEQUENCE(2,2)` only. `TablesEngine::to_grid` scans configured bounds;
`formula_edit.rs::FUNCTIONS` is a static suggestion catalog, not runtime support.
Upstream IronCalc 0.8.3 availability does not establish grid/edit/export behavior.

**Targets/contracts/dependencies.** `engine.rs`, `controller/{state,editing,
ops}.rs`, `sheet.rs`, `io/{load,save,loss}.rs`, `formula_edit.rs`, formula/grid UI.
Depends TBL-006/007 and growth contract TBL-015 for large spills. IronCalc alone
creates and computes spill ranges. Tables needs explicit origin/derived-cell
ownership from upstream APIs or observable upstream state; do not infer a second
spill engine from formula text.

**Steps.** (1) Add authored exact-result cases for FILTER, SORT, UNIQUE, LET,
LAMBDA, SEQUENCE and `A1#` spill references, ordinary references into spill
results, cross-sheet references, blanks/text/errors and nested formulas.
(2) Record support per function and layer: evaluation, projection, edit/undo,
XLSX emission/reload, independent reader. An unsupported layer remains an honest
limitation even if another passes. (3) Probe deterministic boundary sizes and
upstream metadata/errors; configure safe model growth instead of clipping a
successful engine result at 100×26. (4) Block edits to derived cells as a complete
user operation with actionable feedback; allow editing/removing anchor and
occupied blockers via engine semantics. Recalculation shrinking/expanding spill
must remove stale projected values without erasing independent inputs.
(5) Serialize formulas/array metadata/caches in the form required by XLSX;
never flatten derived cells into independent constants without explicit loss
consent. (6) Align suggestions/help with tested capabilities, not invented runtime
capability queries; upstream defects receive minimal reproduction and visible
limitation, not local calculation emulation.

**Edges.** Spill past row/column limits, merged/protected/hidden blockers, source
change causing empty result, overlap of two spills, indirect dependency, deleted
anchor, undo blocker addition/removal and imported stale caches. LAMBDA recursion
and runtime limits must fail visibly and remain cancellable where applicable.

**Tests/GUI/render/CI.** Extend `engine.rs::tests`, `openformula.rs`,
`xlsx_loss_budget.rs`, `soffice_oracle.rs`, controller tests; proposed
`tbl008_filter_sort_unique_consumer`, `tbl008_let_lambda_consumer`,
`tbl008_spill_reference_reopen`, `tbl008_spill_edit_resize_undo`.
Proposed `TablesDynamicArraySmoke` drives anchor edit, rejected child edit,
block/unblock, undo/redo, save/restart and later-sheet values. Add render fixture
for spill growth beyond initial grid. Core + GUI + Interop + Render + performance
case for bounded large spill.

**Acceptance/boundary/non-goals.** Land compatibility matrix/tests first with
honest unsupported outcomes, then end-to-end fixes per admitted function group.
Do not call matrix publication alone support completion. No universal Excel
formula compatibility claim or new expression engine.

## TBL-009 — Native formula editing, reference picking and F4

**Goal/evidence.** `formula_bar.rs::attach` implements colored reference tokens,
autocomplete and argument hints using `formula_edit.rs::reference_tokens`.
`window.rs` separately owns selection/pointer/key handling. Inspected action/key
sources have no F4/reference-picking state contract. Highlighting typed references
is not picking references while an edit remains attached to its origin cell.

**Targets/contracts/dependencies.** `formula_edit.rs`, `formula_bar.rs`,
`window.rs::FormulaRefs` and editor/selection wiring, `sheet_bar.rs`,
`grid_render.rs`. Depends TBL-006/007. Add a **proposed formula edit session** with
origin stable sheet ID/cell, original input, draft text, cursor/selection and
reference-picking span. Keep draft/session UI state outside committed workbook;
only controller commit mutates content. Token manipulation is editing, not formula
evaluation, and must respect string literals and Unicode character/byte indices.

**Steps.** (1) Specify Enter/Tab commit, Escape cancel, F2 edit, mouse/keyboard
selection and reference-picking transitions. (2) While picking, grid clicks/drags
insert/replace a cell/range token at cursor; switching sheet changes preview,
not edit origin. Quote sheet names correctly. (3) Implement F4 cycling `A1` →
`$A$1` → `A$1` → `$A1` → `A1` on the active reference, including range endpoints;
no-op outside a reference, strings, or unsupported token shapes with no corruption.
(4) Keep highlights tied to displayed sheet and repeated references consistently
colored; no text-only or color-only indication of active reference. (5) Restore
focus/selection on commit/cancel/error and never commit to whichever sheet was
last viewed. Extract coherent input/session wiring from `window.rs` if necessary,
not model logic into the widget.

**Edges.** IME composition, combining marks before cursor, quoted apostrophes,
name resembling A1, unfinished formula, autocomplete Tab vs commit Tab, protected
origin and later sheet deleted during draft. Draft cancellation creates no undo.

**Tests/GUI/render/CI.** Extend `formula_edit.rs` unit tests and
`TablesFormulaReferenceHighlightSmoke`, `TablesFormulaAutocompleteSmoke`;
proposed `tbl009_f4_token_cycle_unicode`, `TablesFormulaPickingSmoke` in
`test_smoke.py`. Drive keys/pointer across two sheets, Escape, re-enter/commit,
undo, save and independent formula inspection. GUI at 400px and wide; screenshot
outlines/focus, AT-SPI labels and manual Orca. Core + GUI + Render when drawing
changes. **Acceptance/PR/non-goals:** one editor-session PR then picking/F4 PR,
each preserving existing shortcuts. No Excel ribbon or proprietary interaction
assumption without recorded observation.

## TBL-010 — Selection, clipboard, fill and editing as one workflow

**Goal/evidence.** Existing `TablesKeyboardOnlySmoke`, `TablesClipboardSmoke`,
fill journeys and `GridArea` provide components. `paste_fragment` presently clips
outside current bounds and copies formula strings without an explicit source-
origin translation contract. Whole workflow coverage must distinguish native
fragment, external TSV and text; formula presence after paste is insufficient.

**Targets/contracts/dependencies.** `controller/editing.rs::{paste_fragment,fill,
clear_selection}`, `fragment.rs`, `fill.rs`, `window.rs` key/clipboard wiring,
`grid_area.rs`, `grid_render.rs::follow_selection`. Depends TBL-006–009;
TBL-015 supplies sparse growth. Controller owns effective destination and
transaction; clipboard adapter preserves source identity/origin only when trusted
and available. External spreadsheet text has no assumed source formula origin.

**Steps.** (1) Define navigation/selection for arrows, Shift-extension, Tab/
Shift-Tab, Enter, Home/End and page movement, retaining range anchor and keeping
active cell visible through hidden/frozen/merged regions. (2) Implement complete
range copy/cut/paste input semantics through engine APIs; native internal copy
adjusts relative references and retains supported format/validation/protection
metadata. Expose paste-as-values if offered as a distinct explicit operation.
(3) Parse external rectangular TSV/text with documented newline/tab/empty-tail
behavior, preflight dimensions and grow or reject before any writes.
(4) Fill all directions with existing number/date/list series semantics and
upstream formula-copy translation, preserving applicable metadata.
(5) Validate all destinations first; do not paste the unlocked portion of a
mixed protected range or silently clear errors. Refresh a11y, statistics and
focus once per operation.

**Edges.** Clipboard ownership loss, empty rows/trailing tabs, CRLF and multiline
cells, data interpreted as formula versus literal text, overlapping cut,
protected cells, invalid validation value halfway through range, selection past
maximum bounds, merged cells and spill children. Decide/document cut cancellation
semantics; never delete source before a paste can commit.

**Tests/GUI/interop/CI.** Existing controller editing/fill/property tests and
`TablesClipboardSmoke`, `TablesFillHandleSmoke`, `TablesAutofillSeriesSmoke`,
`TablesKeyboardOnlySmoke`; proposed `tbl010_clipboard_relative_absolute`,
`tbl010_paste_validation_atomic`, `TablesRangeEditingJourneySmoke`.
Journey: range select → copy/fill → external paste → undo/redo → save/reopen,
compare input/formats and visible result. Core + GUI + Interop; render snapshots
for selection/fill handles at matrix widths, not formula correctness by pixels.

**Acceptance/boundary/non-goals.** Clipboard semantics and combined keyboard/fill
journey may be separate PRs; no partial writes/truncation, one history step per
operation. No unsupported rich external clipboard fidelity claim.

## TBL-011 — Sort/filter, names, search and sheet management

**Goal/evidence.** `controller/view.rs::sort_column` sorts full-sheet rows after
`frozen_rows`; name creation delegates to IronCalc but name-box
`parse_defined_name_range` discards sheet qualification. `window_dialogs.rs`
explicitly defers name deletion. Source inspection found no workbook Find action
in Tables' current window/formula/sheet action modules. Existing two-sheet and
named-range journeys are retained, not treated as complete management coverage.

**Targets/contracts/dependencies.** `controller/{view,sheets,ops}.rs`,
`engine.rs::{defined_name,set_defined_name}`, `sheet.rs::parse_defined_name_range`,
`sheet_bar.rs`, `column_menu.rs`, `window_dialogs.rs`, `window.rs`.
Depends TBL-003/006/007. Named ranges resolve through engine scope/stable sheet
identity; filter/sort range and explicit header policy are independent of freeze
view state. Search reads model/engine, not only allocated cell widgets.

**Steps.** (1) Offer visible sort/filter target range and header inclusion,
stable equal-key order and no accidental movement of adjacent unrelated lists;
retain current simple column action as a shortcut to the same contract, not a
second implementation. (2) Add native name management for list/define/update/
delete, workbook/sheet scope only to the extent engine/IO support it; name-box
navigation selects the actual referenced sheet. Unsupported formulas-as-names
remain usable by engine but not falsely navigable. (3) Add Find next/previous with
explicit current-sheet/workbook scope and values/formulas mode, case option,
wrap feedback and stable selection; replacing content, if exposed, uses a
preflighted controller group, not string surgery on display caches.
(4) Complete sheet add/rename/reorder/delete keyboard paths and clear validation
errors; keep one last sheet, stable identity, undo and per-sheet selection.
(5) Expose freeze both rows and columns, manual hide/unhide and sizing with native
controls and persistence; freeze must not secretly redefine the data header.

**Edges.** Duplicate/case-insensitive names, scope collision, #REF! after sheet
delete, Unicode sheet names, invalid rename, empty search, hidden matches,
formula matching versus result matching, protected destinations and no results.

**Tests/GUI/render/CI.** Extend controller tests, `soffice_oracle.rs`,
`TablesNamedRangeSmoke`, `TablesNamedRangeStatsSmoke`,
`TablesTwoSheetJourneySmoke`, `TablesSheetStructureSmoke`, `TablesFilterSmoke`;
proposed `tbl011_scoped_name_targets_sheet`, `tbl011_sort_explicit_header`,
`TablesFindWorkbookSmoke`, `TablesNameManagerSmoke`. Save/restart and independent
reopen required for names/filter/freeze, not for transient search text. Core +
GUI + Interop + Render for changed frozen geometry; manual Orca for result/focus.

**Acceptance/boundary/non-goals.** Separate sort/header, names, and search PRs,
all sharing controller contracts. No Power Query, database connection refresh,
or implicit selection-only sort that detaches related columns.

## TBL-012 — Rich chart editing and one visible chart result

**Goal/evidence.** `chart_dialog.rs::opener` reads active column against column A,
returns silently with no numeric values, and inserts a default chart with an
empty series vector. `ChartSpec::points` uses only the first series.
`tables/src/charts.rs` reexports shared chart rendering; five admitted kinds are
already visible. The readiness requires editable series/ranges/titles/axes/
legend/placement with matching screen, file and export, not only type selection.

**Targets/contracts/dependencies.** `chart_dialog.rs`, `grid_render.rs`,
`sheet.rs::{ChartSpec,ChartSeries,points}`, `controller/{ops,view}.rs`,
`io/save.rs`, existing shared chart renderer reached through `charts.rs`.
Depends TBL-002/006/007; coordinate shared renderer changes with Decks rather
than forking it. A dialog draft is not committed model state. One confirmed edit
or completed placement gesture is one controller operation; preview/Cancel is
not history. Data range resolves through engine projection, not dialog-cached data.

**Steps.** (1) Replace single-column guessing with explicit series list/add/remove/
reorder, name/category/value ranges and optional admitted color. Seed from the
selection without including headers/totals by unstated heuristics.
(2) Add title, axis titles, legend position and numeric anchor/size controls;
select existing chart for editing/deletion, not insertion-only. (3) Use canonical
series rendering for all series with correct scatter X data, category mapping,
legend labels and axis layout; specify pie's supported series restriction.
(4) Show actionable invalid/empty-data errors and disable commit when invalid.
(5) Provide keyboard-accessible chart selection/movement/resizing through native
controls; pointer drag may supplement, not replace them. (6) Compare edited
screen, saved XLSX reopened by both readers, and production PDF (TBL-014).

**Edges.** No numeric data, blanks/errors/negative/nonfinite values, unequal
ranges, deletion of referenced cells/sheets, hidden rows, multiple overlapping
charts, long/Unicode titles, colors in dark/high-contrast and narrow dialogs.
Never silently coerce unsupported multi-series layout into first-series output.

**Tests/GUI/render/CI.** Extend `TablesChartDialogSmoke`, `TablesChartTypesSmoke`,
controller tests, `xlsx_loss_budget.rs`; proposed `TablesChartEditingSmoke`,
`tbl012_chart_edit_cancel_one_undo`. Add `tables/chart-multi-series`,
`tables/chart-axis-legend`, `tables/chart-placement` **proposed fixture IDs** in
`tools/render-lab/fixtures.py`, require A/B and independently authored XLSX.
Core + GUI + Interop + Render; shared rendering changes run Decks regressions too.

**Acceptance/boundary/non-goals.** Series renderer and native inspector can be
separate PRs after persistence; admission is per kind/field with matching output.
No Numbers canvas, arbitrary drawing editor or full Excel chart catalog.

## TBL-013 — Safe pivot summaries, refresh and truthful persistence

**Goal/evidence.** `controller/pivot.rs::{add_pivot_table,evaluate_pivot_table}`
implements row/column grouping into ordinary cell inputs. It clips destination
to existing rows/cols, counts only parsed numbers and formats nonintegral results
to two decimals. `PivotTableSpec` has no `filter_fields` despite the ADR's proposed
model. GUI controls are absent; XLSX drops the definition and `loss.rs` warns.
`pivot_and_protection_tests` and the green `tables/pivot-crosstab` fixture prove
only their scoped controller/output cases.

**Targets/contracts/dependencies.** `sheet.rs::{PivotTableSpec,PivotField,
PivotDataField,PivotAggFunc}`, `controller/{pivot,ops,protection}.rs`,
`io/{loss,save,load}.rs`, native dialog/action wiring beside `sheet_actions.rs`.
Depends TBL-006/007 and TBL-001; GUI follows core semantics. Retain calculation
aggregation in the existing core pivot implementation, not widgets. Source and
output identities must be explicit, with stable sheet IDs if crossing sheets.
A registered definition and its output update atomically as one history group.

**Steps.** (1) Define source header and field bounds, Sum/Count/Average/Min/Max
semantics for text/blanks/errors, deterministic category order and full numeric
precision (format display separately). Reject invalid fields outside source.
(2) Precompute entire output shape; grow safely or reject. Detect source/output
overlap, existing data, protection and merged/spill destinations before mutation;
require explicit overwrite consent, never clip. (3) Store an owned output extent
and implement edit/refresh/remove semantics: refresh replaces only prior owned
output, clears stale tail on shrink and preserves unrelated cells. Source edits
mark result stale visibly until explicit refresh; undo restores both definition
and output. (4) Add filter-field model/criteria if exposing ADR filter controls;
otherwise document its absent implementation and keep controls unavailable until
semantics/tests land. (5) Provide native source, rows, columns, values/aggregation,
output and refresh controls, with accessible summary of selection.
(6) Label output **static summary on XLSX save/reopen** until definition
persistence exists; use existing loss consent. Preserve definition in recovery
under TBL-017. Ordinary result values must round-trip and match independent
reader. A later full OOXML pivot-definition writer/reader requires its own
reviewed format scope and fixtures; current accepted static-summary boundary
must not be advertised as Excel pivot fidelity.

**Edges.** Duplicate field labels, multi-value cross-tabs, no data rows, empty
intersection, errors, nonfinite numeric input, fractional sums, changed categories,
shrinking output, imported existing Excel pivot/cache, later sheet and protected
cell anywhere in destination. Do not modify source from an overlapping refresh.

**Tests/GUI/render/CI.** Extend `pivot_and_protection_tests.rs`,
`controller/protection_tests.rs`, `snapshot_fidelity.rs`, `soffice_oracle.rs`;
proposed `tbl013_refresh_shrink_atomic`, `tbl013_no_output_truncation`,
`tbl013_fractional_aggregation_precision`, `TablesPivotSummarySmoke`.
GUI creates, changes source, refreshes, undoes, cancels loss warning, saves copy,
reopens static result; render edited cross-tab with original existing fixture.
Core + GUI + Interop + Render.

**Acceptance/boundary/non-goals.** First safe core/refresh PR, then native summary
UI with explicit static-XLSX label; both must preserve recovery state. No invented
Excel pivot-definition fidelity, slicers or external data. Full OOXML pivots are
an explicitly unimplemented capability, not a hidden acceptance shortcut.

## TBL-014 — Production printing/PDF that matches the workbook

**Goal/evidence.** `grid_render.rs::render_sheet_pdf` already implements the
single-page, viewport-clipped **test hook** described in
[TABLES-EXPORT-PARITY](../TABLES-EXPORT-PARITY.md); that document's “spec only”
wording is historical. In contrast the production `export-pdf` action calls
`tables_core::export::to_pdf_with_setup`, whose input is `TablesEngine` grid text
and print setup, not the full styled `SheetModel`. Passing the three hook fixtures
is not proof production PDF includes cell styling/charts or a whole workbook.

**Targets/contracts/dependencies.** `tables/src/{grid_render,window_dialogs,
window}.rs`, `tables-core/src/export.rs`, `sheet.rs::{print_area,page_setup}`,
`controller/view.rs::{set_print_area,set_page_setup}`, render/export lab.
Depends TBL-006/012 where charts are exported, shared rendering contract. Reuse
one sheet-content painter/geometry for screen and print; pagination is a print
layout adapter, not a second independent cell/style/chart renderer. GTK/Pango
painting stays out of GTK-free core; core may describe print pages/ranges.

**Steps.** (1) Define production output scope: selected sheet/explicit selected
sheets, print area or used extent, paper/orientation/margins/scale; exclude editing
chrome. (2) Extract reusable content painting from `draw_grid` without duplicating
formatting; frozen panes are viewport state, not a reason to omit scrolled-away
cells from a printed sheet. (3) Paginate full requested ranges with actual widths,
heights, merges, wraps, charts and hidden-row policy; implement repeat-header
behavior only when explicitly modeled. (4) Route production Export/Print/preview
to same output contract, keep narrow lab hook semantics separately named.
(5) Handle invalid/empty ranges and write failures visibly, with no damage to
source or clean state. (6) Add production action export fixtures before enlarging
export parity claims; retain existing hook budgets rather than redefining them.

**Edges.** Multiple pages both axes, merge spanning a page boundary, chart outside
cell used extent, tiny margins/scale, landscape, manually hidden/filter rows,
empty sheet, very large print area and remote export failure. State whether
objects straddling pages split or repeat, and test it rather than dropping them.

**Tests/GUI/render/CI.** Existing `grid_render.rs` PDF unit tests,
`tests/test_export_parity.py`, `tables-core/src/export.rs` tests and
`tools/render-lab/{export_render,export_compare}.py`; proposed
`tbl014_production_pdf_full_range`, `tbl014_multi_page_styled_chart`,
`TablesPrintExportSmoke`. Drive Page Setup/Set Print Area/Export and inspect PDF
page count, page boxes, extracted text and rasterized app/reference/diff, including
later sheet. Core + GUI + Render/export + Interop. Depends on shared #1549 fix for
release if Typst remains in any shipped path.

**Acceptance/boundary/non-goals.** Painter extraction then production pagination
PRs; no release claim based solely on test-export-pdf. Do not silently expand the
historical three-fixture amendment's metric scope or rewrite the entire suite's
export infrastructure.

## TBL-015 — Sparse live workbook storage, not just a sparse benchmark

**Goal/evidence.** `sparse.rs::SparseGrid` exists, but inspected
`SheetModel::new` allocates dense matrices for data/formula flags/formats/styles/
borders/protections/validation/notes. `engine::to_grid` also scans a rectangle.
`performance_budgets.rs`'s million-row sparse object is a standalone SparseGrid;
its GUI viewport uses separate dense SheetModels. Thus #112's “without
preallocating every cell” acceptance is not established for the live app.

**Targets/contracts/dependencies.** `sparse.rs`, `sheet.rs::SheetModel`,
`controller/{state,ops,editing,format,pivot,view,collab}.rs`, `io/{load,save}.rs`,
`fragment.rs`, `snapshot.rs`, `tables/src/{grid_area,grid_render,window,
chart_dialog,format_inspector}.rs`. Depends TBL-006; coordinate operation and
array contracts. IronCalc remains sparse input/calculation authority. Store only
nondefault owned metadata and a bounded/lazily populated value projection; do
not add a second persistent sparse formula store. Dimensions and default
protection (locked=true) remain metadata even without a cell allocation.

**Steps.** (1) Introduce access/iteration APIs on existing SheetModel that separate
logical bounds, used content/metadata extent and visible cells; enumerate and
migrate every direct matrix consumer before changing backing storage.
(2) Reuse SparseGrid where it matches metadata/default semantics, extend bounded
growth as needed; never allocate rows×cols to set a far blank style.
(3) Replace dense projection synchronization with engine reads/invalidation
limited to changed/visible/occupied cells; enumerate actual formula/spill output
when saving, not an entire maximum sheet rectangle. (4) Serialize nondefault
blank metadata as well as values; do not lose unlocked blank cells, validations,
notes, merges, charts or row/column properties. (5) Migrate inverse operations,
collab conversion, snapshots and accessible cells to sparse access; remove
obsolete dense copies once callers migrate. (6) Keep prefix geometry correctness;
measure axis-cache memory separately from cell storage.

**Edges.** Edit XFD1048576 without filling the rectangle, two distant cells,
metadata-only extent, delete/reinsert far rows, sparse spill result, default-style
reset removing a sparse entry, stale a11y slots and source dimensions claiming
huge content. Reject malformed bounds without OOM; never silently truncate.

**Tests/GUI/render/CI.** Extend `performance_budgets.rs`, `peak_memory.rs`,
`property.rs`, `stateful.rs`, `malformed_inputs.rs`, `snapshot_fidelity.rs`;
proposed `tbl015_live_controller_far_sparse_edit`,
`tbl015_sparse_metadata_only_reopen`, `tbl015_no_rectangular_projection`.
Extend `TablesNamedRangeSmoke` and `TablesScreenReaderTraversalSmoke` to a far
coordinate backed by the real controller, not only standalone SparseGrid.
Core + GUI + Interop + Render + TBL-016 whole-app measurements.

**Acceptance/boundary/non-goals.** Accessor migration is one behavior-preserving
PR, sparse cutover a dependent PR with every caller migrated and old storage
removed. Acceptance measures real app/model memory proportional to occupied
state plus dimension/viewport overhead, not rows×cols. No speculative storage
engine dependency or coordinate cap used to disguise the scalability defect.

## TBL-016 — Whole-app performance and responsiveness budgets

**Goal/evidence.** `performance_budgets.rs` uses seven samples, a 2s p95 bound,
256 inputs plus 256 formulas for recalculation, dense 512×128 save/open and
separate top/bottom prefix-geometry tests. `peak_memory.rs` measures allocations
for sparse/dense components. Those useful gates neither prove UI responsiveness
nor a large mixed multi-sheet workload; `set_cell_inputs`' per-cell evaluation
and `mutate_sheet` whole-sheet clones are inspected hot-path candidates.

**Targets/contracts/dependencies.** Existing performance/peak-memory tests,
`engine.rs`, `controller/{state,ops,editing}.rs`, `sheet.rs` geometry/cache,
`persistence.rs`, `grid_area.rs`, `grid_render.rs` and GUI performance harness.
Depends TBL-006/015; keep small gates while adding end-to-end measurements.
Shared SH performance/evidence policy owns artifact schema/hardware metadata.

**Steps.** (1) Commit deterministic workload generators with dimensions,
nonempty counts, formula/dependency depth, sheets, metadata and file size:
existing dense fixture, actual sparse far-cell workbook, multi-sheet report,
large paste and large spill. (2) Time actual file open, first painted frame,
edit→recalculate→visible dependent, paste, top/middle/bottom scroll, save and
reopen; measure peak process memory and allocator data separately.
(3) Record build profile, CPU/RAM, OS/GTK/runtime, commit/lockfile, cold/warm runs,
p50/p95/max and raw samples. (4) Use measurements to remove per-cell evaluation,
full-workbook copying/scanning and excess accessible-node allocation; preserve
transaction/error semantics. (5) Adopt recorded per-workload budgets in CI with
explicit baseline review, keeping current bounds until replacements have evidence.
(6) Long operations must preserve GTK main-thread ownership and visible progress/
error behavior; any background serialization operates on an immutable owned
snapshot and obeys shared save-generation rules.

**Edges.** Slow oracle distinct from app latency, CI noise, stale cache after
resize/hide/undo, cold font cache, metadata-heavy blank cells, paste failure,
save while edit continues, cancellation and memory pressure. No artificial warmup
that removes the operation under test or relaxed threshold to hide regression.

**Tests/GUI/render/CI.** Existing named core performance tests plus proposed
`tbl016_multisheet_edit_paste_save_budget` and
`TablesPerformanceJourneySmoke` (new class; use the existing harness conventions).
Retain `a_frame_costs_the_same_at_the_top_and_the_bottom_of_a_million_rows`.
Core + GUI + scheduled/Installed measured runs; exact measurement artifact linked
from release evidence. Visual snapshots confirm measured frames carry real data.

**Acceptance/boundary/non-goals.** Measurement PR first; each measured bottleneck
fix separately reviewed with same workload. No whole-app scalability claim from
geometry microseconds alone, no arbitrary hardware-independent latency promise.

## TBL-017 — Recovery and save safety for every newly admitted field

**Goal/evidence.** `persistence.rs::autosave_bytes` writes ordinary XLSX bytes;
`TablesWindow::recover_from_snapshot` reopens a scratch XLSX. Existing
`snapshot_fidelity.rs` covers layout and later-sheet isolation, not every future
field. Anything the XLSX writer cannot represent (notably pivot definitions)
cannot magically survive this recovery representation. `loss.rs::source_drops`
also depends on the original file continuing to exist.

**Targets/contracts/dependencies.** `persistence.rs::{autosave_bytes,
save_engine_to_xlsx}`, `window.rs::{recover_from_snapshot,open_path}`,
`io/{load,save,loss}.rs`, snapshot tests and existing GUI recovery classes.
Depends each newly admitted feature; shared SH-002/SH-003 own save races and
recovery-envelope/ownership mechanics. Tables owns the lossless workbook payload
and source-derived compatibility provenance; use the existing shared envelope,
not a parallel autosave system.

**Steps.** (1) Enumerate all authoritative Tables state against recovery encoding:
engine input/formulas/names, per-sheet metadata, protection, charts, filter
definitions and session-only pivot definitions. (2) For admitted state not
representable in XLSX, add a versioned Tables recovery payload/sidecar inside the
shared atomic envelope, or a lossless snapshot representation; version and test
migration from existing XLSX snapshots. Never degrade a checkpoint silently.
(3) Preserve compatibility facts/retained source parts needed after original is
renamed/missing; recovery Save As cannot forget a source loss merely because its
scratch file lacks the original feature. (4) Recover complete state before
publishing it, keep dirty status/identity correct, and retain checkpoint until
adopted safely; historical undo stack need not persist, but restored state must
allow new correct undoable edits. (5) Extend killed-mid-save/failure/cancel paths
with later-sheet metadata and multiple windows.

**Edges.** Second crash before next timer, old payload version, corrupt sidecar
next to valid candidate, live owner, renamed original, read-only import source,
remote conflict and credential/protection metadata. Recovery is not user consent
to flatten pivots or drop unsupported source content.

**Tests/GUI/interop/CI.** Extend `snapshot_fidelity.rs`, existing
`TablesRecoveryIsItselfProtectedSmoke`, `TablesEveryCrashedDocumentSmoke`,
`TablesRenamedOriginalSmoke`, `TablesKilledMidSaveSmoke`,
`TablesLegacySnapshotUpgradeSmoke`, `TablesAutosaveFailureSmoke`;
proposed `tbl017_all_admitted_state_snapshot`,
`TablesRichWorkbookRecoverySmoke`, `tbl017_source_loss_survives_missing_original`.
Reopen recovered saved XLSX independently; compare supported state and explicit
loss report, not merely one marker cell. Core + GUI + Interop + Installed upgrade.

**Acceptance/boundary/non-goals.** Payload completeness PR coordinated with shared
recovery owner; feature-specific additions can land with their feature PRs.
No newly supported state lost on crash; no rewrite of shared autosave envelopes
or false promise to preserve an entire old undo history.

## TBL-018 — Accessible native workflows and manual usability

**Goal/evidence.** `GridArea` provides virtual cells and shared geometry;
`TablesAccessibleGeometrySmoke`, `TablesScreenReaderTraversalSmoke` and
name-box accessibility journeys exist. GTK accessible sibling linking was fixed
with `set_accessible_parent`; do not reintroduce the transfer-none unref defect.
Automated roles/bounds are not manual Orca signoff; the shared readiness's manual
checklist is still unchecked.

**Targets/contracts/dependencies.** `grid_area.rs::{sync_cells,set_geometry}`,
`grid_render.rs::follow_selection`, controller snapshot geometry,
`formula_bar.rs`, `sheet_bar.rs`, `sheet_actions.rs`, chart/pivot/protection dialogs,
`docs/MANUAL-SCREEN-READER-CHECKLIST.md`, `docs/ACCESSIBILITY.md`.
Depends TBL-009–013, TBL-015 changes; shared SH-006 owns cross-app/manual policy.
Accessible value/state derives from the same selected sheet/geometry/calculation
projection; hidden formulas follow protection contract, not a separate a11y cache.

**Steps.** (1) Define cell announcement containing coordinate, displayed value,
selection/editability, formula presence without disclosing protected formula,
validation/note/error as applicable; announce sheet/range changes and rejection.
(2) Keep accessible children bounded and geometrically correct after scroll,
freeze, hide/filter, resize, sheet switch, growth/shrink and sidebar changes.
(3) Complete keyboard entry/exit/focus order for formula suggestions, column menu,
validation list, names/search, chart/pivot inspectors and protection dialogs;
Escape returns to originating control. (4) Test at narrow/wide, scale 1x/2x,
light/dark/high-contrast, with long translated labels; use icons+text/state rather
than only reference colors. (5) Run the manual Orca checklist on the exact
packaged revision, recording GNOME/GTK/Orca/version/backend and deviations.

**Edges.** Far-jump then shrink and two successive tree traversals, empty/unloaded
sheet, hidden selection, modal focus trap, row/column limits, stale descriptions
after undo, unsupported chart selection and validation Alt+Down vs column menu.

**Tests/GUI/render/CI.** Extend all named existing accessibility smoke classes,
`TablesButtonsNamedSmoke`, `TablesAccessibilityDocShortcutsSmoke`;
proposed `TablesProtectedCellAccessibilitySmoke`,
`TablesAnalysisKeyboardAccessibilitySmoke`. Assert AT-SPI roles/names/states and
pixel bounds against `snapshot::cell_rects`; assert real key input, not only app
action activation. GUI + display matrix + manual Installed evidence; Render for
visible focus/error/selection. Core geometry regressions remain required.

**Acceptance/boundary/non-goals.** Each new workflow ships automated accessible
coverage in its own PR; manual evidence is a separate exact-candidate gate.
No claim of screen-reader usability based solely on synthetic traversal.

## TBL-019 — Remaining rendering tails and real-document interop

**Goal/evidence.** Inspected `tools/render-lab/baseline.json` records Tables
`number-formats`, `wrap-text`, `wrap-merged` amber; `pivot-crosstab` is A/B green.
`docs/features/tables.md` still describes long numbers showing `###` where
LibreOffice rounds to fit. `grid_render.rs` already uses Pango and auto-fit;
`persistence::attach_xlsx_sidecars` auto-fits rows on open. These concrete tails
must be distinguished from historic metric-artifact acceptance and real content
loss. Phase 3 real documents remains a separate gate.

**Targets/contracts/dependencies.** `grid_render.rs::{draw_grid,
fit_rows_to_content}`, `style.rs`, `io/{numfmt,xlsx_styles,ods_styles,load}.rs`,
`sheet.rs` geometry, `tools/render-lab/{fixtures,edit_journeys,baseline,
baseline-real,fetch_real_corpus}.py/json` as applicable (existing actual filenames
are `fixtures.py`, `edit_journeys.py`, `baseline.json`, `baseline-real.json`,
`fetch_real_corpus.py`), `real_corpus/`, existing file-corpus tests.
Depends affected TBL-002/003/012/014; shared SH-005 owns render infrastructure.
Use the existing shared number-format/geometry paths; preserve cell numeric value
while choosing display text. Never round stored values to fit a column.

**Steps.** (1) Read each current amber's artifact/linked issue and classify
implementation defect, admitted Excel-vs-Calc semantic difference or measured
metric artifact. Do not require a fake green by weakening thresholds.
(2) Add minimized cases for long general-format numbers, precision, wrap/merged
row height and explicit imported heights; compare loaded model dimensions before
and after auto-fit so opening does not silently rewrite authored geometry.
(3) Fix root rendering/import behavior and rerun app/reference/diff with current
fonts; update baseline only with reviewed evidence. (4) Inventory every Tables
real-corpus entry and its losses/results; fix all blocking admitted-feature
failures, minimize them into feature fixtures, preserve fixture provenance/license.
(5) Add ordinary budget/list/report/chart documents combining formulas, styles,
notes, names, filters and later sheets, with edit→undo→save→reopen and independent
reader. (6) Confirm import-only ODS conversion preserves admitted semantics while
source bytes remain intact; no ODS write claim.

**Edges.** Non-Latin/RTL/combining text, empty A1 offset content, locale/date/time
formats, long numeric precision, styled blanks, fonts missing in Flatpak,
merged wrapping, explicit row height versus auto-fit and charts beyond cell extent.

**Tests/GUI/render/CI.** Existing `property.rs`, `soffice_oracle.rs`,
`tests/gui/test_file_corpus.py`, `tests/gui/test_edit_render.py`,
`tests/test_export_parity.py`; proposed fixture IDs
`tables/general-number-fit`, `tables/wrap-explicit-height`,
`tables/real-budget-edited`. Preserve existing Unicode/offset tests and generators.
Core + GUI + Interop + Render A/B/real/export + Installed Tier C.

**Acceptance/boundary/non-goals.** One minimized rendering/interop fix per PR,
plus reviewed real-document evidence. Amber metric artifacts may remain only
with exact recorded rationale and no hidden missing content; no baseline is a
substitute for fixing a genuine defect. No blanket Excel/Numbers fidelity claim.

## TBL-020 — Exact-revision daily-driver graduation

**Goal/evidence.** Readiness order 5 requires four real workflows from fresh
Flatpak install through restart, not a demonstration assembled from unrelated
passing widgets. Existing format-safe save, two-sheet journey, protection,
keyboard, recovery and installed checks are reusable regressions, not certification
of the pending capabilities or current package.

**Targets/contracts/dependencies.** `tests/gui/test_smoke.py`,
`tests/gui/test_file_corpus.py`, `tools/render-lab/edit_journeys.py`,
`tools/render-lab/vm/` installed/upgrade hooks, existing readiness/feature/format
and capability evidence records. Depends accepted TBL-001–019 behavior and shared
SH-001–SH-008 gates. Runtime/reference/human prerequisites are listed below.
The selected packaged commit and dependency lock identify every artifact.

**Ordered journeys (all proposed new scenario IDs).**

1. **`TBL-DD-BUDGET`**: fresh install, create Budget and Assumptions sheets,
   currency/percentage inputs, named tax/rate and cross-sheet totals; fill month
   formulas, edit a precedent, protect calculated cells and leave inputs unlocked;
   attempt rejected compound edit, undo authorized edit, save XLSX, restart and
   independently reopen. Assert formulas, numeric precision, names, protection
   flags and no wrong-sheet data.
2. **`TBL-DD-EXPENSES`**: open provenance-recorded CSV/ODS expense list; add notes,
   validation and dates, explicit header sort, two-column filter, manual hide,
   clear/reset, paste rows, undo/redo and search. Ctrl+S cannot overwrite import;
   Save As offers `.xlsx`; cancel any loss question first, then consent only to
   recorded losses, reopen and verify criteria/manual-hide identity.
3. **`TBL-DD-REPORT`**: open multi-sheet XLSX with ranges, charts/rules on a later
   sheet and scoped names; insert/delete/move rows/columns, rename/reorder/delete/
   undo sheet; change a source and verify dependent formulas/spills. Crash after
   a checkpoint, recover all admitted metadata, make a new undoable edit, save
   and restart; compare original-supported fields and independent recalculation.
4. **`TBL-DD-CHART`**: select explicit data, create/edit multiple series/title/
   axes/legend, move/size chart, change input data, inspect native accessible
   chart settings, create and refresh a pivot summary with explicit static-save
   warning, undo/redo, save/reopen and production PDF export. Compare screen,
   XLSX and PDF with render/semantic budgets and no unexplained truncation.

**Edges/observations.** Each scenario includes cancellation/error recovery,
close-guard behavior and a narrow-window keyboard segment; use deterministic
snapshots/reader assertions, not videos alone. Record source bytes and every
permitted loss. Run external reader and packaged app with their actual versions;
manual Orca is separate evidence. Do not silently remove a failed feature from
the scenario to declare daily-driver readiness.

**CI/acceptance/PR/non-goals.** Add one scenario per scoped PR after dependencies;
run Core/GUI/Interop/Render and fresh Installed/upgrade/soak on selected candidate.
Release evidence contains collected test IDs/JUnit, screenshots/reference/diffs,
workbooks/PDFs, hardware/latency/memory, capability/loss matrix, logs and manual
accessibility result at that same SHA. No known P0 crash/data loss, unacknowledged
loss, silently skipped release-critical test or unresolved required security gate.
Publishing to Flathub is the shared rollout action after signoff, not acceptance
by itself. This is daily-driver scope, not all Excel features.

## Open questions, decisive experiments and external prerequisites

These are bounded implementation decisions, not excuses to pause all work. Each
has an owner package and an exact next source/test action. Do not claim an answer
from an upstream announcement or a closed issue.

| Question / inspected evidence | Owner and resolution |
|---|---|
| Which IronCalc APIs expose spill origins/extents and errors for each admitted function? Existing wrappers expose calculated values and only SEQUENCE regressions. | TBL-008: inspect pinned 0.8.3 implementation, write consumer tests for every matrix row and bound, contribute minimized upstream defects; retain truthful unsupported status until result. |
| Can imported modern/legacy sheet credentials be emitted unchanged? Candidate `loss.rs` explicitly says writer takes plaintext, not stored credentials. | TBL-005: inspect pinned rust_xlsxwriter API/public OOXML contract, test independent writer/reader tuple preservation; no password UI until valid verifier implementation. |
| Does full cross-sheet/chart range fidelity fit current tuple model? `ChartSeries.cat/val` have only rows/column; parser strips sheet qualifier. | TBL-002: add stable-sheet-qualified range contract or disclose unsupported forms; relationship/rename/reopen fixture decides. |
| How should Count and errors behave in pivot summaries? Current core counts parsed numeric cells and rounds fractions. ADR lists Count without detailed semantics. | TBL-013: choose and visibly label numeric Count vs nonempty Count based on authored oracle fixture, then pin tests; never silently alter numeric semantics or round stored results. |
| How are pivot definitions preserved after crash if normal XLSX deliberately drops them? `autosave_bytes` currently uses same XLSX writer. | TBL-017: versioned lossless Tables payload coordinated with SH-003; static XLSX boundary is not license to lose crash-recovered definitions. |
| Are current amber fixtures actual differences or accepted metric artifacts? Baseline stores verdicts, not enough reasoning to infer cause. | TBL-019/SH-005: retrieve associated run/issue/artifacts, inspect images, minimize genuine defects and retain recorded acceptable deviations only. |
| What are meaningful whole-app latency/memory limits on release hardware? Existing 2s test and standalone sparse measurements are narrower. | TBL-016/SH-001: measure recorded real controller/GTK workloads before choosing new per-workload bounds; retain old gates meanwhile. |
| Would IronCalc's XLSX adapter reduce total loss? Current production IO is calamine/rust_xlsxwriter. | Optional bounded research under TBL-001/006: compare identical field/loss/malformed/performance corpus in an isolated experiment, publish tradeoffs; no adoption or second writer unless explicitly reviewed. |

**Human/infrastructure prerequisites (separate from code):** CI workers with GTK/
AT-SPI/display isolation, LibreOffice and pinned fonts; rendering containers and
GNOME/KVM environment for Flatpak Tier C; a known prior package for upgrade;
recorded hardware for performance; an Orca-capable human running the checklist;
access to original real-corpus files with provenance; and shared release/security
signoff. These are pending evidence until exercised on the candidate. Legitimate
Excel desktop/web or Numbers/macOS access is optional for a bounded unresolved
behavior question, never a prerequisite for implementing the public-format and
LibreOffice-tested core. No proprietary reference app was exercised here. REA is
optional observation tooling, not a runtime dependency, source of copied code or
proof of desktop behavior.

**Scope boundaries maintained honestly:** VBA, Power Query, external-data refresh,
full `.numbers` fidelity, encrypted workbooks, extension marketplaces, enterprise
policy and a general suite-convert product are not silently added to Tables'
daily-driver definition. ODS is import-only until a separately admitted writer
exists. Full OOXML pivot-definition fidelity remains explicitly unsupported under
the accepted static-summary boundary; features not admitted must be disclosed,
not advertised as implemented. Existing collaboration operation codecs must be
kept correct when models change, but collaboration product rollout follows its
own accepted RFC and is not substituted for this remaining-work inventory.
