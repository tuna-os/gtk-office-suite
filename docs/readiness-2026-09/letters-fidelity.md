# [P1] Letters: preserve complete document state through editing, layout, and every save path

Audited on `e7e4df6`. Follow-up to closed #109–#111; existing models are foundations, not proof of GUI fidelity.

## Evidence
`letters/src/bridge.rs::capture_from_buffer` constructs a Document with `header: None, footer: None, page: None`. `StructuredEditor` operations recapture and rerender the buffer. The review workflow contract in `docs/LETTERS-REVIEW-WORKFLOWS.md` admits comments/revisions with warn-until-mapped semantics. Core round-trips alone cannot prove those states survive editing or export.

## Architecture
A per-tab session owns the complete Document, review state, imported-package context, selection and save revision. GTK's buffer is an editing adapter: operations use explicit character offsets and preserve non-buffer state. Keep the established StructuredEditor and engines; first establish lossless capture/render adapters, then migrate commands incrementally. Do not create another unsynchronized document copy.

## Dependency-ordered slices
- [ ] Record and test a field-by-field Document ↔ buffer/sidecar mapping, including tables, lists, run styles, images, footnotes, headers/footers and page geometry.
- [ ] Preserve fields across unrelated edits and structured commands; distinguish user text from rendered list markers.
- [ ] Apply model operations and undo through one live session; keep review anchors correct across Unicode edits.
- [ ] Drive editor pagination and print/export from styled paragraph/run metrics; remove byte/character-offset ambiguity.
- [ ] Wire admitted review/TOC/bidi workflows or report them as unavailable until GUI and format tests pass.
- [ ] Reconcile duplicate ADR numbers and the conflicting old/new advanced-feature scope without silently expanding scope.

## Acceptance
For each admitted field: model tests, buffer round-trip on the GTK main thread, ODT and DOCX save/reopen, and LibreOffice rewrite assertions. GUI journeys open a styled Unicode document, change one paragraph, undo/redo, Save As, reopen, and verify both edited and untouched metadata. Include CJK, combining marks, emoji and RTL. Geometry/render tests cover page boundaries, tables and headers at multiple zooms.

Depends on the P0 Letters save transaction, #354 and #374. Collaboration, macro compatibility and a wholesale editor rewrite are outside this issue.

