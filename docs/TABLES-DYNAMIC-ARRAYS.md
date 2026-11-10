# Tables dynamic arrays: what is admitted and what is not

IronCalc owns evaluation and spilling; Tables projects the result into the
grid and never reimplements either (ADR 0007). This page is the contract
for the pinned engine (`ironcalc_base` 0.9.1), measured rather than
inferred from the formula suggestion list. Every row and limit below is
pinned by a test, so an engine upgrade that changes one fails CI and this
page is updated with it:

- `tables-core/tests/dynamic_arrays.rs` (engine, controller, XLSX writer)
- `tables-core/tests/soffice_oracle.rs`, `a_calc_24_2_rewrite_keeps_the_formulas_and_loses_the_cached_values`
- `tests/test_tables_dynamic_arrays.py` holds the table below to the
  functions those tests exercise (`ADMITTED` in
  `tables-core/tests/support/mod.rs`).

## Admitted functions

Each is exercised for the shapes and values in its row of `CASES`, from a
single anchor cell.

| Function | Shapes exercised |
|---|---|
| `SEQUENCE` | column, 2-D, with start and step |
| `TRANSPOSE` | block to its transpose |
| `FILTER` | column by condition |
| `SORT` | column, descending |
| `UNIQUE` | column of text |
| `HSTACK` | two columns side by side |
| `VSTACK` | two columns end to end |
| `TAKE` | leading rows |
| `DROP` | leading rows removed |
| `TOCOL` | block to a column, row-major |
| `TOROW` | block to a row |
| `MMULT` | 2×2 product |
| `TEXTSPLIT` | delimited text to a row |
| `RANDARRAY` | shape and range only; the values are random |

Array arithmetic (`=A1:A3*2`) spills, and an aggregate over an array
(`=SUM(A1:A3*2)`, `=INDEX(SEQUENCE(3),2)`) reduces to one value. Other
functions that return arrays are not admitted until they have a row here.

## Behaviour that is tested

- **Blocked.** A constant or a formula in the destination makes the anchor
  `#SPILL!`, for a column, a row and a 2-D spill; clearing the blocker lets
  it spill, and the blocker is never overwritten.
- **Empty result.** `FILTER` with no match and `SEQUENCE(0)` give `#CALC!`.
- **Recalculation.** A spill follows edits to its sources (a `FILTER` grows
  and shrinks) and resizes when a cell sizes it (`SEQUENCE(A7)`). A source
  on another sheet is followed.
- **References.** Other cells read a spilled cell like any value
  (`=E2*10`, `=SUM(E1:E3)`).
- **History.** Entering a spill is one undo step that takes the whole
  projection out; redo brings it back; a blocked spill clears when the
  blocker's entry is undone.

## Limits, pinned by tests

- **Clipped at the grid edge.** The grid shows the sheet's configured rows
  and columns. A spill that runs past them is clipped there with no
  `#SPILL!`, where Excel would refuse it. Reopening sizes the grid to the
  stored cells or 100 × 26, whichever is larger, and the cells a spill
  derives are not stored, so a spill past both is clipped on reopening.
- **No spill reference operator.** `=E1#` is not implemented by the pinned
  engine: it yields the anchor cell's value alone, with no error, where
  Excel yields the whole spill.
- **Only the anchor is saved.** See below.

## In the window

Two AT-SPI journeys (`TablesDynamicArraySmoke`) drive the visible grid with
no spill logic in the widget code: a spill, `#SPILL!` when a cell blocks it,
its clearing, and undo and redo; and a `FILTER` that shows `#CALC!` while
nothing matches, spills once its source matches, and shows `#CALC!` again
when those entries are undone.

## XLSX

| | Excel and Calc ≥ 24.8 | Calc 24.2 (the CI runner's) |
|---|---|---|
| Formula | Written as an array formula marked as a dynamic array (`cm="1"`), with `_xlfn.` and `_xlws.` prefixes for the newer functions. `TRANSPOSE` and `MMULT` have no prefix. | Kept, as written, in lower case. |
| Cached value | The anchor's first value. | `#NAME?` for the newer functions, which Calc gained in 24.8. `TRANSPOSE` and `MMULT` are known to it and give the first value alone. |
| Spill range | The file records the anchor only (`ref="E1"`); the derived cells are not stored, so they cannot block the spill on reopening. | The same, rewritten as `ref="E1:E1"`. |
| Reopened here | Every admitted function's spill is recomputed in full. | The same: the formulas survive a Calc rewrite and the spills recompute here. |

Until this page's change, `TRANSPOSE`, `MMULT` and array arithmetic were
written as plain formulas, which Excel and Calc evaluate in a single cell:
the spill came back as its first value. A formula the engine shows spilling
is now always written as a dynamic array formula.

## Not verified

- **Excel itself.** The encoding follows `rust_xlsxwriter`'s documented
  dynamic-array support; no Excel is available to CI.
- **Calc 24.8 and later.** The Calc test fails on a Calc that may evaluate
  these functions, so the table above is revisited when the runner's
  LibreOffice moves.
- **Dynamic-array functions the engine does not implement**, such as `SORTBY`,
  `XLOOKUP` spills and `LET`-built arrays.
