# [P1] Tables: enforce format-safe saves and prove multi-sheet daily-driver journeys

Audited on `e7e4df6`. Keep the existing WorkbookController/IronCalc architecture and the completed sheet/undo fixes.

## Current gap
`tables/src/window.rs` accepts XLS/XLSX/ODS/CSV imports, while its Save paths call `save_engine_to_xlsx`, including saving to the existing path. Verify and prevent XLSX bytes being written over an imported CSV/ODS file under its old extension. The older roadmap promises preferred-ODF journeys, but import support does not imply ODS write support.

## Architecture and scope
WorkbookController remains the sole mutation gateway. Stable sheet identities bind formulas, selection and undo. IronCalc owns calculation/dependency semantics; the view projects sparse state and does not implement a second formula engine. Format capability is explicit in the session: imported read-only formats require Save As to a writable format until their writers exist.

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
- [x] Two-sheet journey: edit/formula → rename/reorder/delete/undo → switch → save → reopen; no cross-sheet overwrite or retargeted history.
      `TablesTwoSheetJourneySmoke` (#1204): a value on Sheet1, a formula on Sheet2 reading it, rename through the
      dialog, move first, delete and undo, switch to Sheet1 through the sheet switcher, Save As; the saved xlsx must
      have both sheets in the new order under the new names, the formula with its cached value, and no formula on
      Sheet1. The file is then reopened in a fresh `tables two-sheets.xlsx` process and both sheets are walked
      through the switcher again, each showing its own content.
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

Dependencies: P0 save durability, #354, #374, #400. Exit: passing XLSX create/edit/save/reopen journeys and honest import/export capabilities for every offered extension; no unsupported format is silently overwritten.

