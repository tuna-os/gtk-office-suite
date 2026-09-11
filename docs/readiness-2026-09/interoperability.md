## Bind the existing compatibility model to actual open/save paths

September audit at `e7e4df6`. Use this issue as the execution owner; related #316/#386 and existing PR #317 are context. suite-common-core already has CompatibilityReport, validate_save and opaque-package helpers. The missing proof is their use in the live app paths, not the existence of another policy document.

Architecture: readers return complete semantic document state plus source-package context and a structured compatibility report. The document session retains all three. A save planner computes target-format loss, blocks MustPreserve failures, requires explicit confirmation for admitted loss, and preserves opaque parts only when relationships/content types remain valid. A writer cannot bypass that decision.

- [ ] Trace each GUI Open/Save/Save As/recovery/export path to the compatibility boundary.
- [ ] Define exact readable/writable formats per app; prohibit overwriting CSV/ODS with XLSX bytes (#439).
- [ ] Version fixtures with author/version, format, expected semantics, feature IDs and permitted losses.
- [ ] Test supported text/styles/images/links/notes/geometry/formulas/sheet order in both directions through LibreOffice.
- [ ] Verify opaque relationships/content types after unrelated edits; refuse unsafe partial pass-through.
- [ ] GUI cancel on a loss warning preserves original bytes and dirty state.
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
- [ ] Treat missing oracle as failure in required interop/release lanes (REQUIRE_SOFFICE=1), never as observed compatibility.
- [ ] Promote a format feature only when model, live journey and independent-reader evidence all exist.

Depends on #436/#437 and #354; feeds #438/#439/#440 and the capability ledger #441.
