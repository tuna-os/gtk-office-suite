# [P1] Tables: enforce format-safe saves and prove multi-sheet daily-driver journeys

Assessment updated 2026-10-13. Keep the Rust `WorkbookController`/IronCalc
architecture and native GTK/libadwaita interface. The direction is viable,
but passing individual feature tests is not evidence of an everyday Excel
replacement.

## Current gap
Format-safe Save As and several multi-sheet journeys now have evidence below.
The remaining priority is trustworthy workbook editing: precise XLSX loss
accounting, calculation/edit consistency, accessible interaction, and measured
performance. ODS is import-only; neither an Excel-compatible UI nor preferred
ODF wording can substitute for a working writer.

## Architecture and scope
WorkbookController remains the sole mutation gateway. Stable sheet identities bind formulas, selection and undo. IronCalc owns calculation/dependency semantics; the view projects sparse state and does not implement a second formula engine. Format capability is explicit in the session: imported read-only formats require Save As to a writable format until their writers exist.

## Direction and delivery order

The product target is a GNOME-native spreadsheet for budgets, lists, formulas,
multi-sheet workbooks and ordinary charts exchanged as XLSX. It is not a clone
of Excel's ribbon or Numbers' free-form canvas. VBA, Power Query, external-data
refresh and full `.numbers` fidelity are not current capabilities; surface
unsupported content rather than implying it survives.

Work in this order; unchecked gates remain requirements, not claims:

1. **Trustworthy editing and saves before more analysis features.**
   - [ ] Expand the existing XLSX loss budget to individual chart series,
     worksheet relationships, axes, legend, placement/size, cell lock/hidden
     flags and protection permissions. A broad “chart retained” or
     “sheet protected” assertion is insufficient.
   - [ ] Restore supported attributes through the real load/edit/save path;
     unsupported changes must trigger the existing loss-confirmation policy.
     Cancel must leave source bytes unchanged. Protection is an editing
     restriction, not document encryption.
   - [ ] Exercise these cases on later worksheets, through undo/redo and after
     crash recovery, then reopen with an independent reader.
   - [x] Verify protection at the user-operation boundary for paste, fill,
     clear, sort, structural edits and pivot output, not just single-cell
     typing. Rejected compound edits leave values, metadata and history
     unchanged; internal undo/recovery can restore authorized state.
     Verified at `af1ef767`: core `controller::protection_tests` covers mixed
     locked/unlocked ranges, later sheets, pivot registration/output and
     modeled formatting/row permissions; the
     [GUI run](https://github.com/tuna-os/gtk-office-suite/actions/runs/38426268309)
     exercises rejected paste, clear, sort and structural actions, native
     feedback, history integrity and successful editing after unprotect.
     This does not certify XLSX persistence of individual protection flags.
2. **One calculation authority with a tested editing contract.**
   - [ ] Audit every formula-affecting operation against IronCalc's existing
     APIs before adding local reference-rewriting, fill or calculation logic.
     Keep one controller undo transaction per user operation; avoid parallel
     engine and UI histories disagreeing.
   - [ ] Add consumer-level cases for FILTER, SORT, UNIQUE, LET, LAMBDA and
     spill references, plus edits inside spill regions, resizing, blockers,
     undo/redo, cross-sheet references and XLSX reopen. Upstream support is a
     candidate for reuse, not certification of Tables' projection or writer.
   - [ ] Define which state is authoritative in IronCalc versus SheetModel.
     Derived visible values must not become independent editable formula data.
3. **Complete everyday GNOME interactions.**
   - [ ] Prove keyboard selection/editing, formula reference picking, F4,
     clipboard ranges, fill, sort/filter with headers, freeze panes, names,
     search and sheet management as coherent workflows, not isolated widgets.
   - [ ] Use native GTK actions, dialogs and libadwaita navigation with visible
     errors, focus restoration, narrow-window behavior and accessible cell
     state. Require deterministic AT-SPI journeys and manual Orca evidence.
4. **Analysis features after persistence is trustworthy.**
   - [ ] Provide chart series/range/title/axis/legend editing with matching
     on-screen, saved and exported results. Add render-lab fixtures before
     claiming visible parity.
   - [ ] Expose pivots only with clear refresh/source/output semantics and
     undo. Until pivot definitions persist, label output as a static summary
     and warn about definition loss; do not market it as Excel pivot fidelity.
5. **Daily-driver graduation on an exact packaged revision.**
   - [ ] Run real budgeting, expense-list, multi-sheet reporting and chart
     workflows from fresh Flatpak install through save, restart and reopen.
   - [ ] Measure open, edit/recalculate, scroll, paste and save latency plus
     memory on recorded dense and sparse workbooks and identified hardware.
     Do not infer whole-app scalability from the virtualized view alone.
   - [ ] Require real-document and installed-app evidence, no unacknowledged
     loss, and the shared release/recovery/accessibility gates. Keep heavy
     builds and per-app render jobs parallel in CI.

## Reuse and reference research

- **IronCalc remains the calculation engine.** The lockfile uses 0.8.3.
  Its [0.8 announcement](https://blog.ironcalc.com/2026/08/01/Changelog-v0.8.html)
  describes dynamic arrays, LET/LAMBDA and substantial formula additions.
  Tables currently uses calamine/rust_xlsxwriter for file I/O, so upstream
  XLSX fixes do not automatically reach Tables. Compare IronCalc's XLSX
  adapter against the same loss corpus before proposing a migration; do not
  introduce a second production writer without evidence of a net improvement.
- **REA is optional research tooling, not a runtime dependency or engine.**
  [REA](https://github.com/morluto/rea) documents browser observation,
  JavaScript analysis and native-provider integration. It cannot turn Excel
  or Numbers into a maintainable Rust implementation automatically. Its
  [Apple inventory projection](https://github.com/morluto/rea/blob/main/docs/apple-application-analysis.md)
  explicitly distinguishes package inventory from observed runtime behavior.
- Start with public specifications, IronCalc source, LibreOffice comparison
  and small authored workbooks. For an unresolved interaction, record inputs,
  action sequence, visible result, exported file, application version and
  provenance from a legitimately accessible reference app. Excel web evidence
  is not proof of desktop Excel behavior.
- A bounded REA pilot should answer one question, such as selection and focus
  transitions during formula-reference picking. Compare ordinary browser
  observation with REA's evidence; retain the tool only if it resolves an
  otherwise unanswered question. Native Excel requires an appropriate target
  environment; native Numbers requires macOS for runtime observation. No
  proprietary reference application was exercised in this assessment.
- Numbers is useful as a reference for understandable chart inspectors and
  approachable formatting. Its multiple-tables-per-canvas model would change
  workbook semantics and is not a shortcut to Excel compatibility.
- Keep reference observations separate from implementation. Do not copy
  proprietary code, assets or branding, bypass licensing/access controls, or
  assume REA's MIT license grants rights to analyzed applications. Check
  applicable terms before binary analysis; prefer behavioral specifications.


## Work and acceptance
- [x] Reproduce CSV/ODS/XLS open → edit → Ctrl+S; preserve the original and offer a correctly suffixed Save As when unsupported.
      `TablesCsvSaveSmoke`, `TablesOdsSaveSmoke` and `TablesXlsSaveSmoke` (tests/gui/test_smoke.py; fixtures written by
      LibreOffice in tests/gui/fixtures/) open each format, edit B2, press Ctrl+S, and require the original's bytes
      unchanged, the "Cannot save in this format" prompt, Save As offering `budget.xlsx`, and that workbook holding the
      edit. With the guards in `save_engine_to_xlsx` and the save action removed, the journey fails (#1204).
- [x] Test formulas, cached values, styles, charts, rules, names, protection and hidden/filter state against a declared XLSX loss budget.
      `tables-core/tests/xlsx_loss_budget.rs` declares the budget, feature by feature, and saves one workbook carrying all
      of them through the real byte path (`save_sheets_to_xlsx_bytes` with the engine, then `load_workbook`). It fails both
      ways: a feature declared kept that comes back wrong, and one declared lost that survives, so the declared losses
      stay exactly the real ones. Writing it found two losses, both fixed here (#1204): **sheet protection** was written
      and never read back, so a protected workbook reopened unprotected and was saved that way; and **charts and
      conditional formats** were attached by the GUI after `load_workbook` returned, so recovery tests and corpus tooling
      opened xlsx files without them. Declared lost, with the reason: a filter's hide reopens as a manual hide (xlsx keeps
      no filter state here), and charts and rules on a sheet after the first (their readers resolve the first worksheet).
      Content outside the budget is no longer dropped silently (#1272): `tables_core::io::loss` reads the model and the
      file the workbook came from (macros, pictures, pivots, threaded comments, slicers, external links, connections,
      OLE, sparklines, and validation rules xlsx has no form for), and every save path — Ctrl+S, Save As and the close
      guard's Save — asks "Save Without This Content?" first. Cancel writes nothing. `TablesLossQuestionSmoke` drives
      it: a workbook with a macro project, edited, Ctrl+S, Cancel leaves its bytes; Save Anyway writes the edit.
  XLSX cell/permission protection now persists across the real load/edit/save path (#1552, observed at
  `3b97d980fbd33d32c87c11a5da3f812e5c6684b3`): cell lock/hidden flags, style-default inheritance and
  `applyProtection` semantics round-trip on ordinary, formula, blank styled and merged cells, including later
  sheets; modeled worksheet permissions serialize with OOXML disallow-bit inversion (all 32 combinations in
  `pivot_and_protection_tests`); unsupported imported credentials and non-representable permission settings raise
  explicit loss warnings and are never treated as encryption. `TablesProtectionPersistenceSmoke` drives both the
  unlocked-cell edit/save/reopen/undo journey and the password-loss consent/cancel journey (GUI run
  38439078711, smoke 13m9s pass; core `test`, oracle and tables corpus green at the same revision; undo/redo also
  refreshes the grid accessible description). Capability ledger: `tables.protection.persistence`. This does not
  certify chart persistence, credential-bearing round trips, or workbook structure protection.
- [x] Two-sheet journey: edit/formula → rename/reorder/delete/undo → switch → save → reopen; no cross-sheet overwrite or retargeted history.
      `TablesTwoSheetJourneySmoke` (#1204): a value on Sheet1, a formula on Sheet2 reading it, rename through the
      dialog, move first, delete and undo, switch to Sheet1 through the sheet switcher, Save As; the saved xlsx must
      have both sheets in the new order under the new names, the formula with its cached value, and no formula on
      Sheet1. The file is then reopened in a fresh `tables two-sheets.xlsx` process and both sheets are walked
      through the switcher again, each showing its own content.
      Follow-up for #439 extends this same journey with delete → undo → redo → undo
      and precedent-edit undo/redo while the dependent sheet is selected. It asserts
      both sheet-local values and recalculated formulas before the existing save/reopen
      checks. These new assertions await the GUI CI lane; they are not new verified evidence.
      Test/export action registration now lives in a dedicated shell module,
      preserving the opt-in test-mode gate while reducing constructor setup code.
- [x] Exercise row/column edits, fill, sort/filter, named ranges and protection through actual GUI actions plus controller
      invariants. (Named ranges and the name box now also pass at 400px width: `Ctrl+G` opens a Go to Cell dialog when the
      narrow breakpoint hides the name box — #516, found by the display matrix.)
      Writing the missing journeys (#1277) found three of these unreachable from the app. The controller had row and
      column insert/delete, frozen rows and sheet protection, but no action or menu item reached them, and an edit to
      a protected cell was dropped without a word. `tables/src/sheet_actions.rs` adds them to the toolbar's extended
      section (the More menu when narrow): insert rows/columns at the selection, delete the selected ones, freeze the
      rows above the selection, and protect the sheet. A refused edit or structural change now says "This sheet is
      protected". Sort now leaves frozen rows in place, the header rule Sheets uses, so a header row is no longer
      sorted in among the data (`sort_keeps_frozen_header_rows_in_place`). `TablesSheetStructureSmoke` drives each
      through the snapshot:
      - insert and delete of rows and columns: formulas are rewritten, recalculate after an edit (no stale value), and
        undo restores the sheet exactly;
      - an ascending then descending sort under a frozen header, undone;
      - a protected sheet refuses an edit with the message, and unprotect allows it.

      Fill, filter and named ranges already had journeys (`TablesFillHandleSmoke`, `TablesAutofillSeriesSmoke`,
      `TablesFilterSmoke`, `TablesColumnMenuSmoke`, `TablesNamedRangeSmoke`).
- [x] Resolve the Unicode XLSX property regression tracked in #377/#371/#358/#324 using minimized fixtures; do not weaken the generator just to turn CI green.
      Resolved by #450 and ticked here on re-verification (#1204). It was neither flaky nor about Unicode: the loader
      read a calamine `Range` at relative coordinates where `get_value` takes absolute ones, so any sheet whose content
      did not start at A1 was read shifted or empty. The Unicode strategy only exposed it because it can emit an empty
      string for A1, which the plain-value strategy never does. The minimized counterexample is pinned as
      `tables_core::io::load::tests::xlsx_round_trip_keeps_a_lone_bottom_right_cell` and
      `…::xlsx_round_trip_keeps_content_at_its_own_coordinates`, plus the committed `offset_start.ods` fixture. The
      generator is unchanged (combining marks, CJK, Hebrew and emoji, empty strings included). Re-run on 2026-10-01 at
      3,000 cases (`PROPTEST_CASES=3000`, against the PR lane's 64): passes.
- [x] Verify sparse-grid scaling and accessibility far-navigation regression: the skipped #137 reproduction now runs as
      `TablesNamedRangeSmoke::test_jump_far_and_back_to_a_range_no_longer_crashes`, alongside the far-jump named-range
      journey that reproduced #507. Root cause was not grid lifetime bookkeeping but GTK itself:
      `gtk_accessible_update_next_accessible_sibling()` unrefs the parent accessible it obtained from
      `gtk_at_context_get_accessible_parent()`, which is transfer-none (a weak pointer), in every GTK from 4.10 through
      main. Each call dropped a reference the app never owned, so a wide enough virtual-cell chain finalized the
      `GridArea` while it was still parented. Both apps now link children with `set_accessible_parent(parent, sibling)`,
      which writes the same ATContext fields without the stray unref.
- [ ] Treat advanced analysis per the accepted ADR: prove supported pivots/charts/protection through the live controller and formats; explicitly surface upstream-dependent array limits.
      Branch `work/roadmap-checkpoint-readiness` adds controller-level cross-tab pivots: `col_fields` now form
      deterministic pivot columns; `pivot_and_protection_tests` verifies cell placement, XLSX result-value round-trip,
      and a compatibility warning that the pivot definition is not retained. Two `SEQUENCE` engine regressions verify
      visible spill values and `#SPILL!` when occupied cells block output. Local targeted tests and CI run 38377144141 pass:
      workspace check/clippy/nextest/docs, interop corpus, Python validators, and all three Flatpak app builds. The RustSec advisory
      gate failed through Typst YAML loading; see #1549. The initial wide fixture was amber and split across two LibreOffice pages;
      after narrowing to the pivot output, render run 38384168878 produced one page and green A/B results. I reviewed the app,
      LibreOffice and diff images: labels and values match, with SSIM 0.844, 93.3% word recall and 2.76pt displacement. The baseline
      now records this fixture green for A/B; the full report has 58 green, 4 amber and 0 red in both tiers (one Letters and three
      Tables fixtures remain amber).
      This remains open: pivot controls are not exposed in the GUI. The
      chart dialog builds one value series from the selected column and uses
      column A as categories. XLSX export can write multiple series, axis
      titles and legend position, but import keeps one category/value range,
      drops those settings, and defaults the legend. Protection import/export now retains per-cell lock/hidden flags
      and modeled permission bits across the real path (#1552, evidence above); password hashes and other credential
      material remain explicitly non-round-tripped with a consent warning, never treated as encryption. These
      chart/protection round trips are XLSX-only;
      other offered document formats are not writable. Dynamic-array evidence is
      deliberately narrower: engine tests cover only `SEQUENCE(3)` and a
      blocked `SEQUENCE(2,2)` spill. Grid projection is bounded to the
      `TablesEngine`'s configured rows and columns; no function-by-function
      dynamic-array inventory or upstream shape/size ceiling has been
      established. The static formula-suggestion catalog is not a runtime
      support matrix.

Dependencies: P0 save durability, #354, #374, #400. Exit: passing XLSX create/edit/save/reopen journeys and honest import/export capabilities for every offered extension; no unsupported format is silently overwritten.

