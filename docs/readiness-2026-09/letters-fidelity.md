# [P1] Letters: preserve complete document state through editing, layout, and every save path

Audited on `e7e4df6`. Follow-up to closed #109–#111; existing models are foundations, not proof of GUI fidelity.

## Evidence
`letters/src/bridge.rs::capture_from_buffer` constructs a Document with `header: None, footer: None, page: None`. `StructuredEditor` operations recapture and rerender the buffer. The review workflow contract in `docs/LETTERS-REVIEW-WORKFLOWS.md` admits comments/revisions with warn-until-mapped semantics. Core round-trips alone cannot prove those states survive editing or export.

## Architecture
A per-tab session owns the complete Document, review state, imported-package context, selection and save revision. GTK's buffer is an editing adapter: operations use explicit character offsets and preserve non-buffer state. Keep the established StructuredEditor and engines; first establish lossless capture/render adapters, then migrate commands incrementally. Do not create another unsynchronized document copy.

## Dependency-ordered slices
- [~] Record and test a field-by-field Document ↔ buffer/sidecar mapping, including tables, lists, run styles, images, footnotes, headers/footers and page geometry. **Tables done** (`letters-core/src/table_text.rs`, bridge render/capture, DOCX chain); lists, images, footnotes and geometry already mapped; page geometry and review state ride on buffer sidecars.
- [ ] Preserve fields across unrelated edits and structured commands; distinguish user text from rendered list markers.
- [ ] Apply model operations and undo through one live session; keep review anchors correct across Unicode edits.
- [ ] Drive editor pagination and print/export from styled paragraph/run metrics; remove byte/character-offset ambiguity.
- [ ] Wire admitted review/TOC/bidi workflows or report them as unavailable until GUI and format tests pass.
- [ ] Reconcile duplicate ADR numbers and the conflicting old/new advanced-feature scope without silently expanding scope.

## Known remaining double-writes (observed 2026-09-11)

The table commands now edit the document and let the bridge render it. The
list and page-break commands still do both jobs themselves — they write
literal text into the buffer *and* call the editor — which is the same
defect class. Observed by driving the real app and reading the editor
back after each action:

| Action | Editor text afterwards | Expected |
|---|---|---|
| `bullet-list` | `- • intro` | `- intro` (one rendered marker) |
| `list-indent` | 8 spaces of indent | 4 spaces = one level (`capture_list_marker` reads 4) |
| `insert-page-break` | a literal `---` paragraph at the top of the document | `page_break_before` on the cursor's paragraph |

Each is user-visible and each survives into saved files as text. They need
the same treatment: one model operation, rendered by the bridge, with the
marker mapping already in `capture_list_marker` as the contract.

## Acceptance
For each admitted field: model tests, buffer round-trip on the GTK main thread, ODT and DOCX save/reopen, and LibreOffice rewrite assertions. GUI journeys open a styled Unicode document, change one paragraph, undo/redo, Save As, reopen, and verify both edited and untouched metadata. Include CJK, combining marks, emoji and RTL. Geometry/render tests cover page boundaries, tables and headers at multiple zooms.

Depends on the P0 Letters save transaction, #354 and #374. Collaboration, macro compatibility and a wholesale editor rewrite are outside this issue.

