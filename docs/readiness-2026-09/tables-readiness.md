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
- [ ] Exercise row/column edits, fill, sort/filter, named ranges and protection through actual GUI actions plus controller invariants.
- [ ] Resolve the Unicode XLSX property regression tracked in #377/#371/#358/#324 using minimized fixtures; do not weaken the generator just to turn CI green.
- [ ] Verify sparse-grid scaling and accessibility far-navigation regression (#137 is closed but the current smoke file still contains a skipped reproduction).
- [ ] Treat advanced analysis per the accepted ADR: prove supported pivots/charts/protection through the live controller and formats; explicitly surface upstream-dependent array limits.

Dependencies: P0 save durability, #354, #374, #400. Exit: passing XLSX create/edit/save/reopen journeys and honest import/export capabilities for every offered extension; no unsupported format is silently overwritten.

