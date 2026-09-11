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
      still open. Malformed/truncated corpus cases and retained minimized fuzz failures are still open: the cargo-fuzz
      targets in `fuzz/` cover docx/xlsx/pptx only, run 100 iterations a night with `--sanitizer none`, and have no seed
      corpus and no mechanism to turn a finding into a committed regression. The ODF read side has no fuzz target at
      all.
- [ ] Treat missing oracle as failure in required interop/release lanes (REQUIRE_SOFFICE=1), never as observed compatibility.
- [ ] Promote a format feature only when model, live journey and independent-reader evidence all exist.

Depends on #436/#437 and #354; feeds #438/#439/#440 and the capability ledger #441.
