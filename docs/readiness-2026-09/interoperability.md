## Bind the existing compatibility model to actual open/save paths

September audit at `e7e4df6`. Use this issue as the execution owner; related #316/#386 and existing PR #317 are context. suite-common-core already has CompatibilityReport, validate_save and opaque-package helpers. The missing proof is their use in the live app paths, not the existence of another policy document.

Architecture: readers return complete semantic document state plus source-package context and a structured compatibility report. The document session retains all three. A save planner computes target-format loss, blocks MustPreserve failures, requires explicit confirmation for admitted loss, and preserves opaque parts only when relationships/content types remain valid. A writer cannot bypass that decision.

- [ ] Trace each GUI Open/Save/Save As/recovery/export path to the compatibility boundary.
- [ ] Define exact readable/writable formats per app; prohibit overwriting CSV/ODS with XLSX bytes (#439).
- [ ] Version fixtures with author/version, format, expected semantics, feature IDs and permitted losses.
- [ ] Test supported text/styles/images/links/notes/geometry/formulas/sheet order in both directions through LibreOffice.
- [ ] Verify opaque relationships/content types after unrelated edits; refuse unsafe partial pass-through.
- [ ] GUI cancel on a loss warning preserves original bytes and dirty state.
- [ ] Harden ZIP/XML/image readers with size/count/decompression limits and malformed/truncated corpus cases; retain minimized fuzz failures.
- [ ] Treat missing oracle as failure in required interop/release lanes (REQUIRE_SOFFICE=1), never as observed compatibility.
- [ ] Promote a format feature only when model, live journey and independent-reader evidence all exist.

Depends on #436/#437 and #354; feeds #438/#439/#440 and the capability ledger #441.
