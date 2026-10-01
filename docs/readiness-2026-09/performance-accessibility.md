## Measurable readiness slice: performance, rendering and accessibility

Use this issue for measured performance work; #354 owns automated journeys, #423/#421/#389/#306 remain related accessibility planning. Audit reference `e7e4df6`; reuse tables-core/tests/performance_budgets.rs, tests/gui/visual_golden.py and existing screen-reader checklist.

Architecture: benchmark core open/edit/recalculate/save separately from GTK input-to-frame and scrolling. Shared model/geometry drives canvas, print and accessible bounds. Tests must exercise populated scenes, not just empty launch windows.

- [~] Define representative small/medium/large Letters documents, sparse multi-sheet workbooks and image-heavy decks.
      **Letters is done** (#1208): `letters-core/tests/performance_budgets.rs` builds small, medium and large documents
      (20, 500 and 5,000 paragraphs) shaped like real writing, with headings every twenty paragraphs, bold and italic
      runs, and a bulleted list in each section. Tables already had sparse and dense fixtures
      (`tables-core/tests/performance_budgets.rs`). Still open: image-heavy decks.
- [~] Record baseline hardware/runtime, sample count, p50/p95 latency, peak memory and output semantics; gate measured regressions with explicit budgets.
      For Letters, DOCX and ODT save and open, Markdown serialize and parse, and typing 200 characters through the model
      each run 7 samples per size and print them with p50 and p95. Each fails when p95 exceeds a budget that scales from
      small to large, with the runtime and measurements it was set against recorded in the file. It runs in the PR lane.
      **It found a real defect on its first run:** saving the large document as DOCX took **30 s**, because rdocx's
      `add_bullet_list_item` clones the whole document on every call, making a save quadratic in list items.
      `docx::write` now allocates each list definition once and writes later items as paragraphs on its numId, which
      is the same XML. The save now takes 3.8 s in a debug build, and its 15 s budget fails the old behaviour. Still
      open: peak memory, and the image-heavy deck fixture.
- [~] Test virtualized viewport work scales with visible data, not maximum row/column coordinates.
      Tables (#1208): the grid renderer called `row_on_screen` and `row_y` for every row from the first, and each
      re-summed every row above it, so a frame was quadratic in the sheet's rows (and columns); the divider hit-tests
      and the accessibility spans did the same. `sheet::visible_rows` / `visible_cols` now place what shows in one pass,
      and the renderer draws only those. `visible_lists_and_spans_match_the_per_index_geometry` holds them to the old
      per-index geometry across hidden, frozen, resized and scrolled sheets; `a_frame_at_the_bottom_of_a_tall_sheet`
      (performance_budgets.rs) requires a 100,000-row frame to draw only a screenful, within 300 ms p95 (38 ms p50 in a
      debug build). Still open: that one pass is linear in the rows rather than independent of them (prefix sums
      would remove it), and Letters' and Decks' viewports are unmeasured.
- [ ] Fixed-font visual matrix: widths 400/800/1280, light/dark/high contrast, scale 1/2, editor/selection/dialog/error; retain expected/actual/diff plus snapshot.
- [ ] Keyboard-only edit/save/undo and AT-SPI names/roles/states/bounds match the model after scroll/resize/zoom.
- [ ] Reactivate the closed #137 navigation crash reproduction only after live verification; include inspector/object selection and screen-reader traversal.
- [ ] Complete the manual Orca checklist for release; record environment and deviations rather than treating an automated role check as full usability.

Completion is measured behavior and reproducible artifacts, not a new benchmark strategy document. VLM visual review stays advisory.
