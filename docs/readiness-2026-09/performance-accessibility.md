## Measurable readiness slice: performance, rendering and accessibility

Use this issue for measured performance work; #354 owns automated journeys, #423/#421/#389/#306 remain related accessibility planning. Audit reference `e7e4df6`; reuse tables-core/tests/performance_budgets.rs, tests/gui/visual_golden.py and existing screen-reader checklist.

Architecture: benchmark core open/edit/recalculate/save separately from GTK input-to-frame and scrolling. Shared model/geometry drives canvas, print and accessible bounds. Tests must exercise populated scenes, not just empty launch windows.

- [ ] Define representative small/medium/large Letters documents, sparse multi-sheet workbooks and image-heavy decks.
- [ ] Record baseline hardware/runtime, sample count, p50/p95 latency, peak memory and output semantics; gate measured regressions with explicit budgets.
- [ ] Test virtualized viewport work scales with visible data, not maximum row/column coordinates.
- [ ] Fixed-font visual matrix: widths 400/800/1280, light/dark/high contrast, scale 1/2, editor/selection/dialog/error; retain expected/actual/diff plus snapshot.
- [ ] Keyboard-only edit/save/undo and AT-SPI names/roles/states/bounds match the model after scroll/resize/zoom.
- [ ] Reactivate the closed #137 navigation crash reproduction only after live verification; include inspector/object selection and screen-reader traversal.
- [ ] Complete the manual Orca checklist for release; record environment and deviations rather than treating an automated role check as full usability.

Completion is measured behavior and reproducible artifacts, not a new benchmark strategy document. VLM visual review stays advisory.
