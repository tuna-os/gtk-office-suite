## Measurable readiness slice: performance, rendering and accessibility

Use this issue for measured performance work; #354 owns automated journeys, #423/#421/#389/#306 remain related accessibility planning. Audit reference `e7e4df6`; reuse tables-core/tests/performance_budgets.rs, tests/gui/visual_golden.py and existing screen-reader checklist.

Architecture: benchmark core open/edit/recalculate/save separately from GTK input-to-frame and scrolling. Shared model/geometry drives canvas, print and accessible bounds. Tests must exercise populated scenes, not just empty launch windows.

- [x] Define representative small/medium/large Letters documents, sparse multi-sheet workbooks and image-heavy decks.
      Letters (#1208): `letters-core/tests/performance_budgets.rs` builds small, medium and large documents (20, 500
      and 5,000 paragraphs) shaped like real writing, with headings every twenty paragraphs, bold and italic runs, and a
      bulleted list in each section. Tables has sparse and dense fixtures (`tables-core/tests/performance_budgets.rs`).
      Decks: `decks-core/tests/performance_budgets.rs` builds 5- and 30-slide decks, each slide with a title, a text box
      and a ~360 KB picture, saved and reopened as PPTX and ODP; writing it found the PPTX writer deflating every
      picture again (2.8 s for 30 slides, now 0.08 s).
- [x] Record baseline hardware/runtime, sample count, p50/p95 latency, peak memory and output semantics; gate measured regressions with explicit budgets.
      For Letters, DOCX and ODT save and open, Markdown serialize and parse, and typing 200 characters through the model
      each run 7 samples per size and print them with p50 and p95. Each fails when p95 exceeds a budget that scales from
      small to large, with the runtime and measurements it was set against recorded in the file. It runs in the PR lane.
      **It found a real defect on its first run:** saving the large document as DOCX took **30 s**, because rdocx's
      `add_bullet_list_item` clones the whole document on every call, making a save quadratic in list items.
      `docx::write` now allocates each list definition once and writes later items as paragraphs on its numId, which
      is the same XML. The save now takes 3.8 s in a debug build, and its 15 s budget fails the old behaviour. Decks
      has the same p50/p95 budgets for its image-heavy fixture.
      **Peak memory** (#1208): `peak_memory.rs` in each core crate installs `suite_common_core::peak_heap::CountingAlloc`
      and holds each operation's peak heap above its starting point to a budget, printed beside it. It counts what the
      program allocates rather than RSS, which an allocator's retained pages and the harness's threads would blur.
      Measured, debug build: the large Letters document's DOCX save peaks at 127 MB for a 7 MB model (rdocx's
      document clones; ODT save peaks at 4 MB), DOCX open 89 MB; the 30-slide deck's ODP save 21 MB, PPTX save 12 MB.
      Tables: the sparse million-row grid holding 10,000 values peaks at 0.9 MB, the dense sheet's XLSX open at 31 MB.
      **Writing it found opening an xlsx quadratic in its cells**: every loader recalculated the whole workbook after
      each cell it set, so a 512×128 sheet did not open in twenty minutes. Loaders, paste and PDF export now recalculate
      once (`TablesEngine::put_cell_text`); it opens in 0.8 s, and `performance_budgets.rs` now times that open.
- [ ] Test virtualized viewport work scales with visible data, not maximum row/column coordinates.
- [ ] Fixed-font visual matrix: widths 400/800/1280, light/dark/high contrast, scale 1/2, editor/selection/dialog/error; retain expected/actual/diff plus snapshot.
- [ ] Keyboard-only edit/save/undo and AT-SPI names/roles/states/bounds match the model after scroll/resize/zoom.
- [ ] Reactivate the closed #137 navigation crash reproduction only after live verification; include inspector/object selection and screen-reader traversal.
- [ ] Complete the manual Orca checklist for release; record environment and deviations rather than treating an automated role check as full usability.

Completion is measured behavior and reproducible artifacts, not a new benchmark strategy document. VLM visual review stays advisory.
