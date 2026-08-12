# ADR 0005: Sparse tables and measurable interaction budgets

## Decision

Tables keeps worksheet coordinates independent from allocation. Empty regions
are represented by sparse coordinate storage, while structural row/column
operations are model operations that shift cell state and every range-bearing
feature together. Rendering and hit testing use the same cumulative geometry,
and cell text is laid out by Pango inside the cell clip rectangle.

The performance fixture gate uses two deterministic profiles:

| Profile | Shape | Populated cells |
| --- | ---: | ---: |
| sparse | 1,000,000 × 16,384 | 10,000 |
| dense | 10,000 × 256 | 2,560,000 |

The benchmark runner must report open, first-visible scroll, single-cell edit,
formula recalc, and save separately. Release gates are p95 budgets of 2 s,
16 ms, 50 ms, 500 ms, and 2 s respectively for the sparse profile; the dense
profile is allowed 5 s, 16 ms, 50 ms, 2 s, and 5 s. A benchmark that cannot
produce a timing for one operation fails rather than silently dropping that
measurement. These are wall-clock budgets for CI hardware and are intended to
be calibrated only by changing this ADR and the checked-in fixture manifest.

## Consequences

The sparse representation makes navigation to distant cells affordable, but
code that needs to enumerate cells must use the populated-cell iterator or a
bounded viewport. Structural edits remain centralized in `SheetModel`, which
also gives importers, undo commands, and GTK actions one place to preserve
formats, merges, validations, charts, pivots, filters, and print areas.
