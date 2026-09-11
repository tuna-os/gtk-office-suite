# [P1] Tables: enforce format-safe saves and prove multi-sheet daily-driver journeys

Audited on `e7e4df6`. Keep the existing WorkbookController/IronCalc architecture and the completed sheet/undo fixes.

## Current gap
`tables/src/window.rs` accepts XLS/XLSX/ODS/CSV imports, while its Save paths call `save_engine_to_xlsx`, including saving to the existing path. Verify and prevent XLSX bytes being written over an imported CSV/ODS file under its old extension. The older roadmap promises preferred-ODF journeys, but import support does not imply ODS write support.

## Architecture and scope
WorkbookController remains the sole mutation gateway. Stable sheet identities bind formulas, selection and undo. IronCalc owns calculation/dependency semantics; the view projects sparse state and does not implement a second formula engine. Format capability is explicit in the session: imported read-only formats require Save As to a writable format until their writers exist.

## Work and acceptance
- [ ] Reproduce CSV/ODS/XLS open → edit → Ctrl+S; preserve the original and offer a correctly suffixed Save As when unsupported.
- [ ] Test formulas, cached values, styles, charts, rules, names, protection and hidden/filter state against a declared XLSX loss budget.
- [ ] Two-sheet journey: edit/formula → rename/reorder/delete/undo → switch → save → reopen; no cross-sheet overwrite or retargeted history.
- [ ] Exercise row/column edits, fill, sort/filter, named ranges and protection through actual GUI actions plus controller
      invariants. (Named ranges and the name box now also pass at 400px width: `Ctrl+G` opens a Go to Cell dialog when the
      narrow breakpoint hides the name box — #516, found by the display matrix.)
- [ ] Resolve the Unicode XLSX property regression tracked in #377/#371/#358/#324 using minimized fixtures; do not weaken the generator just to turn CI green.
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

