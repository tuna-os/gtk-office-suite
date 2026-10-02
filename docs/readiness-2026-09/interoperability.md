## Bind the existing compatibility model to actual open/save paths

September audit at `e7e4df6`. Use this issue as the execution owner; related #316/#386 and existing PR #317 are context. suite-common-core already has CompatibilityReport, validate_save and opaque-package helpers. The missing proof is their use in the live app paths, not the existence of another policy document.

Architecture: readers return complete semantic document state plus source-package context and a structured compatibility report. The document session retains all three. A save planner computes target-format loss, blocks MustPreserve failures, requires explicit confirmation for admitted loss, and preserves opaque parts only when relationships/content types remain valid. A writer cannot bypass that decision.

- [x] Trace each GUI Open/Save/Save As/recovery/export path to the compatibility boundary.
      [`docs/SAVE-PATHS.md`](../SAVE-PATHS.md) (#1273) lists every call, in all three apps, to the functions that read
      or write a document. For each it gives the path that reaches it, how it treats the format's losses (asks, a
      recovery snapshot, an export, or a read whose losses the save asks about) and the test behind it.
      `tests/test_save_paths.py` fails on a call the page doesn't list, and on a row marked `asks` whose call isn't
      inside the loss question's save closure. The trace found **the Decks close guard's Save writing around the
      question**; it asks now (`DecksUnsupportedContentSmoke.test_the_close_guards_save_asks_too`). It also found that the
      Open dialog and drag-and-drop have no journey in any app, and that a drop replaces a document with unsaved
      changes (#1316).
- [ ] Define exact readable/writable formats per app; prohibit overwriting CSV/ODS with XLSX bytes (#439).
- [x] Version fixtures with author/version, format, expected semantics, feature IDs and permitted losses.
      Each fixture set has a `fixtures.json` beside its files (#1275): per file, the authoring app and version, the
      format, the feature IDs it proves, the semantics the round trip checks, the losses it is permitted (each naming
      its issue, e.g. CommonMark example 150 → #1290) and the tests that use it. Feature IDs are PARITY.md's own
      names: `<app>/<slug of the feature row>` or a render fixture PARITY.md cites, so the interop evidence table
      (#1276) can use the same IDs. `conformance/validate_fixtures.py` fails on a set with no manifest, a file with no
      entry or an entry with no file, a missing field, an unknown feature ID, a loss with no issue, or a `used_by` test
      that never names the fixture; `tests/test_fixture_manifests.py` runs it in the Python checks lane. Writing it
      found `tests/fixtures/*.md` unused by any test; `whole_markdown_fixtures_round_trip` now uses them.
- [x] Test supported text/styles/images/links/notes/geometry/formulas/sheet order in both directions through LibreOffice.
      [`docs/INTEROP-EVIDENCE.md`](../INTEROP-EVIDENCE.md) (#1276) has one row per feature, app and format, and two
      columns: ours → LibreOffice, and LibreOffice → ours (LibreOffice's bytes, never a same-format re-save of ours). Every
      cell cites a test, and `tests/test_interop_evidence.py` checks each one exists. The empty cells got new oracle
      tests: our docx's links into Writer's odt, a picture Writer places read from its docx, cell styles from Calc's
      own xlsx, and text and run styling read from Impress's pptx. Letters' odt has no image support at all (#1292),
      the one `n/a` that is a gap. The Calc xlsx test records an upstream loss: LibreOffice 24.2 exports a themed
      workbook's font colour as `theme="1"` and reads it back black.
- [x] Verify opaque relationships/content types after unrelated edits; refuse unsafe partial pass-through.
      [`docs/OPAQUE-PARTS.md`](../OPAQUE-PARTS.md) (#1274) decides, for each kind of part an app doesn't model, whether a
      save carries it or asks about it. Custom XML, thumbnails, custom properties and ODF settings are carried by
      `suite_common::carry` through every save in all three apps. They go in with their content types, relationships or
      manifest entries, and the file is replaced once. Macros, embedded objects, SmartArt and charts are referenced from
      content the apps rewrite, so they go into the loss question instead; Letters gained that scan (`letters_core::loss`).
      Journeys: `tests/gui/test_smoke.py::LettersOpaquePartsSmoke`, `TablesOpaquePartsSmoke` and `DecksOpaquePartsSmoke`
      (open → edit → Ctrl+S, then the parts are byte-identical and `ooxml_package_problems` is empty), and
      `LettersUnreadContentSmoke`. Building the journeys found that adding the parts after the writer replaced the file
      left a moment when the file on disk had the edit without them; the save now writes once.
- [~] GUI cancel on a loss warning preserves original bytes and dirty state. **Letters is done** (#1206): every save
      (Ctrl+S, Save As, the close guard's Save All) builds the target format's `CompatibilityReport` from the
      document *before* writing and, when the format would drop something the document has, asks "Save as <format>?"
      with Cancel as the default. Cancel writes nothing and leaves the tab unsaved; "Save Anyway" writes, and later
      saves to the same path do not ask again. It used to write first and report afterwards ("Saved, with formatting
      this format cannot hold"), so the bytes were already gone when the user found out. Journey:
      `tests/gui/test_smoke.py::LettersLossWarningCancelSmoke::test_cancel_on_a_loss_warning_keeps_the_file_and_the_edit`
      (a bold run in a `.txt` file; on the old code it fails with the file overwritten). Still open: Tables and Decks
      build no loss report for their save formats, so they have nothing to warn from yet.
- [~] Harden ZIP/XML/image readers with size/count/decompression limits and malformed/truncated corpus cases; retain
      minimized fuzz failures. **Limits are done**: `suite-common-core/src/zip_guard.rs` holds one set of bounds —
      member count, uncompressed bytes per member, and uncompressed bytes across the whole archive — and every package
      reader in the suite now reads through it (odt, odp, pptx, the three best-effort xlsx readers, and
      `OpaquePackage::capture`, which is the widest surface because it reads every member the format reader did not
      claim). Before this, all seventeen entry reads called `read_to_end`/`read_to_string` with no bound, so a
      40-kilobyte file could ask a reader for gigabytes. The bound comes from `Read::take` rather than the archive's
      declared size, because a bomb simply lies about that, and the buffer is grown incrementally rather than
      preallocated — preallocating from a declared size *is* the allocation the bomb is asking for.
      `{letters,decks}-core/tests/hostile_packages.rs` build hostile archives in memory and assert the production
      readers refuse them, that an oversized *optional* part is skipped rather than fatal, and that ordinary documents
      still open.
      **Malformed/truncated corpus cases and retained minimized fuzz failures are done.**
      `{letters,decks,tables}-core/tests/malformed_inputs.rs` are deterministic, seeded, PR-lane harnesses: structural
      hostility cases (bounded deep nesting, truncation mid-element and mid-attribute, entity expansion, non-finite and
      out-of-range numeric attributes, thousands of objects or rules in one part) plus two mutation modes — the packaged
      bytes, and one part's XML repackaged into a valid archive so every seed reaches the parser rather than dying at
      the zip layer (measured: only 503 of 2,000 package mutations produced a readable archive). Each harness was
      verified to *detect* a panic by injecting one, because a no-panic test that cannot see a panic asserts nothing.
      It found **three real defects in `read_sheet_props_from_xlsx`** on its first run: `<col min="0">` and
      `<row r="0">` underflowed a 1-based to 0-based conversion (a panic in debug, `usize::MAX` in release, recording a
      nonsense hidden index), and `<col min="1" max="4294967295">` looped over the whole range — a hang rather than a
      crash, measured at over 30 seconds before being killed, now clamped to the grid's own limits
      (`SHEET_MAX_ROWS`/`SHEET_MAX_COLS`).
      Each crate has a `tests/crashes/` directory whose every file is replayed on every pull request: that is the
      retention mechanism, and the replay test prints its count so an empty directory cannot be mistaken for a missing
      wire. The libFuzzer lane grew from three targets to nine, adding odt, odp, ods, the three best-effort xlsx part
      readers, `markdown::parse` and `parse_master_shapes`; its temporary-file names now come from `NamedTempFile`
      rather than pid-plus-length, which collided between workers handling equal-length inputs and would have looked
      like a crash that did not reproduce.
      **The sanitizer and seed-corpus gaps are closed.** The nightly now runs `--sanitizer address`. The four core
      crates contain no `unsafe` at all, so `--sanitizer none` was defensible on its own terms — but what they hand
      attacker-controlled bytes to is not this suite's code: the decompression stack underneath is Rust
      reimplementations of C libraries and is `unsafe`-heavy (zlib-rs 451 occurrences, libbz2-rs-sys 142, lzma-rust2
      16, miniz_oxide 4), and a decompressor fed hostile input is the classic memory-safety target.
      The seed corpus is generated by this suite's own writers
      (`{letters,decks,tables}-core/examples/fuzz_seed_corpus.rs`) rather than committed as binaries: it cannot drift
      from what the writers produce, the generators are reviewable source, and each generator reads every seed back
      and exits non-zero if its own reader rejects one — verified by corrupting a seed and confirming the failure.
      The effect is not marginal. Measured at 200 runs per target, with and without the corpus:
      `letters_odt` 1814 vs 176 coverage, `tables_xlsx` 6883 vs 215, `decks_odp` 1783 vs 176 — roughly 10x to 32x.
      Without seeds libFuzzer never escaped 6-to-9-byte inputs, because a package needs a valid local header, central
      directory and CRC before a reader looks at any XML, and it does not reach that by chance in a bounded run.
      Seeds include the 652 vendored CommonMark examples for the markdown target. No crashes were found in any run.
      Still open here: unbounded nesting depth needs the out-of-process lane because a stack overflow aborts rather
      than unwinding and cannot be caught in-process.
- [x] Treat missing oracle as failure in required interop/release lanes (REQUIRE_SOFFICE=1), never as observed compatibility.
      `tests/test_oracle_lanes.py` (PR lane) finds every test target that starts `soffice` from the sources and checks
      that each one panics instead of skipping when `REQUIRE_SOFFICE` is set, that every workflow step running one sets
      `REQUIRE_SOFFICE: "1"`, and that the pull-request `test` lane, which has no LibreOffice, excludes each of them
      from the evidence it offers (`conformance/lanes.json`). Verified to fail when the variable is removed from the
      nightly oracle step.
- [ ] Promote a format feature only when model, live journey and independent-reader evidence all exist.

Depends on #436/#437 and #354; feeds #438/#439/#440 and the capability ledger #441.
