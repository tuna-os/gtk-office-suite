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
- [x] Test virtualized viewport work scales with visible data, not maximum row/column coordinates.
      Tables (#1208): the grid renderer called `row_on_screen` and `row_y` for every row from the first, and each
      re-summed every row above it, so a frame was quadratic in the sheet's rows (and columns); the divider hit-tests
      and the accessibility spans did the same. `sheet::visible_rows` / `visible_cols` now place what shows in one pass,
      and the renderer draws only those. `visible_lists_and_spans_match_the_per_index_geometry` holds them to the old
      per-index geometry across hidden, frozen, resized and scrolled sheets; `a_frame_at_the_bottom_of_a_tall_sheet`
      (performance_budgets.rs) requires a 100,000-row frame to draw only a screenful, within 300 ms p95 (38 ms p50 in a
      debug build). The rest is closed by #1282:
      - **Tables**: row and column offsets are prefix sums. They are cached on the sheet and rebuilt when a size or
        hidden set changes; those fields are `Tracked`, so every write, including the dozens that go straight to the
        public field, marks the cache stale. `row_y`/`col_x` are a lookup, and hit-testing and `visible_rows` are a
        binary search. The 100,000-row frame went from 42 ms to 15 µs p50.
        `a_frame_costs_the_same_at_the_top_and_the_bottom_of_a_million_rows` requires the top and bottom of a
        million-row sheet to agree (both ~15 µs p50). `prefix_geometry_matches_summing_and_follows_direct_writes` holds
        the cache to the summing definition across direct writes, inserts, deletes and clones.
      - **Letters**: the Print Layout view tested every page each frame and, for each, re-summed the heights of the
        pages above it, as did the click and drag hit-test: quadratic in the pages. `layout::PageStack` keeps the page
        tops as prefix sums and finds the visible pages and the nearest page by binary search
        (`page_stack_lookups_match_testing_every_page`). `a_frame_of_a_500_page_document_draws_only_the_visible_pages`
        draws a 515-page document's frame at its first and last page in ~2 ms p50 each, 250 ms p95 budget.
      - **Decks**: the slide strip rendered every slide's thumbnail on each rebuild. Rows now start as placeholders,
        and a row's thumbnail is drawn from the live deck when it comes within a screen of the viewport.
        `a_300_slide_strip_renders_thumbnails_only_in_view_and_the_canvas_one_slide` requires a 300-slide rebuild to
        render none (15 ms p50) and the canvas to draw its one slide within 100 ms p95 (1 ms p50).
- [ ] Fixed-font visual matrix: widths 400/800/1280, light/dark/high contrast, scale 1/2, editor/selection/dialog/error; retain expected/actual/diff plus snapshot.
- [x] Keyboard-only edit/save/undo and AT-SPI names/roles/states/bounds match the model after scroll/resize/zoom.
      **Keyboard-only edit, undo and save is done for all three apps** (#1208): `LettersKeyboardOnlySmoke`,
      `TablesKeyboardOnlySmoke` and `DecksKeyboardOnlySmoke` (`KeyboardOnlyMixin` in `tests/gui/test_smoke.py`) open
      a document at launch and edit, undo and save it with keys alone (no pointer, no D-Bus action), asserting on the
      snapshot and on the saved file. Decks inserts through the command palette (Ctrl+K). Writing them found that
      **Decks' Ctrl+Z and Ctrl+Shift+Z worked only while the canvas had focus**, so an object inserted from the
      palette or the Insert menu could not be undone from the keyboard; Decks now binds them app-wide, as Letters and
      Tables do. **The AT-SPI half is done too** (#1283): `TablesAccessibleGeometrySmoke`,
      `DecksAccessibleGeometrySmoke` and `LettersAccessibleGeometrySmoke` check every visible accessible node's name,
      role, states and bounds, to the pixel, against the snapshot after each step. Tables: every visible cell after
      a jump that scrolls and after the Format panel narrows and widens the grid, against the snapshot's
      `cell_rects`. Decks: every slide object as the canvas narrows and widens, against the snapshot's object boxes
      and the canvas's fit rule. Letters: the page view (one text box, read through GtkAccessibleText) at 150% and
      60% zoom and scrolled to the end, against the snapshot's page sizes, zoom and scroll. Writing the Tables
      journey found that **an opened workbook had no accessible cells until the selection moved**; opening now
      builds them. Limits: the harness's window manager (matchbox) sizes every window to the screen and undoes an
      external resize, so "resize" is the view's widget resizing, not the window's. Tables and Decks have no zoom. Letters can't report
      per-character bounds until the suite builds against GTK 4.16 (AccessibleText extents).
- [ ] Reactivate the closed #137 navigation crash reproduction only after live verification; include inspector/object selection and screen-reader traversal.
- [ ] Complete the manual Orca checklist for release; record environment and deviations rather than treating an automated role check as full usability.

Completion is measured behavior and reproducible artifacts, not a new benchmark strategy document. VLM visual review stays advisory.
