# [P1] Letters: preserve complete document state through editing, layout, and every save path

Audited on `e7e4df6`. Follow-up to closed #109–#111; existing models are foundations, not proof of GUI fidelity.

## Evidence
`letters/src/bridge.rs::capture_from_buffer` constructs a Document with `header: None, footer: None, page: None`. `StructuredEditor` operations recapture and rerender the buffer. The review workflow contract in `docs/LETTERS-REVIEW-WORKFLOWS.md` admits comments/revisions with warn-until-mapped semantics. Core round-trips alone cannot prove those states survive editing or export.

## Architecture
A per-tab session owns the complete Document, review state, imported-package context, selection and save revision. GTK's buffer is an editing adapter: operations use explicit character offsets and preserve non-buffer state. Keep the established StructuredEditor and engines; first establish lossless capture/render adapters, then migrate commands incrementally. Do not create another unsynchronized document copy.

## Dependency-ordered slices
- [~] Record and test a field-by-field Document ↔ buffer/sidecar mapping, including tables, lists, run styles, images, footnotes, headers/footers and page geometry. **Tables, lists and page breaks done** (`letters-core/src/table_text.rs`, the `page-break` tag, bridge render/capture, DOCX chain); images, footnotes and geometry already mapped; page geometry and review state ride on buffer sidecars.
- [ ] Preserve fields across unrelated edits and structured commands; distinguish user text from rendered list markers.
- [ ] Apply model operations and undo through one live session; keep review anchors correct across Unicode edits.
- [ ] Drive editor pagination and print/export from styled paragraph/run metrics; remove byte/character-offset ambiguity.
- [ ] Wire admitted review/TOC/bidi workflows or report them as unavailable until GUI and format tests pass.
- [ ] Reconcile duplicate ADR numbers and the conflicting old/new advanced-feature scope without silently expanding scope.

## GUI commands that used to write their own markers (fixed 2026-09-11)

Every structured command wrote presentation text into the buffer *and*
edited the model, usually at a hardcoded paragraph or table index. Each is
now a single model operation rendered by the bridge, acting on the caret's
paragraph:

| Action | Editor text before | Now |
|---|---|---|
| `insert-table` | a literal pipe grid *plus* cells appended at the end of the document | one table, at the caret, cells empty |
| `table-insert-row-*` / `table-delete-*` | `| New Cell 1 |` at the caret, against table id 1 | the caret's table, or nothing |
| `bullet-list` | `- • intro` (the action's bullet plus the rendered marker) | one rendered marker, on the caret's paragraph |
| `list-indent` / `list-outdent` | 8 spaces per level (4 written, 4 rendered) | one level, `list_level` in the model |
| `insert-page-break` | a literal `---` paragraph at the top of the document | `page_break_before` on the caret's paragraph, shown as a shaded rule |

Page breaks needed a buffer mapping of their own to survive capture — they
ride on a `page-break` paragraph tag, the same mechanism headings and
alignment already use, because anything written into the text would be
indistinguishable from a user typing it.

## Acceptance
For each admitted field: model tests, buffer round-trip on the GTK main thread, ODT and DOCX save/reopen, and LibreOffice rewrite assertions. GUI journeys open a styled Unicode document, change one paragraph, undo/redo, Save As, reopen, and verify both edited and untouched metadata. Include CJK, combining marks, emoji and RTL. Geometry/render tests cover page boundaries, tables and headers at multiple zooms.

Depends on the P0 Letters save transaction, #354 and #374. Collaboration, macro compatibility and a wholesale editor rewrite are outside this issue.

## Paragraph indents and spacing (the cross-format audit, 2026-09)

The sweep that closed #322's fidelity row for Decks, and found #716's
defect unfixed in the ods reader for Tables, was pointed here next.
Letters came out of it better than either: its oracle already crosses
formats (`through_lo_to_docx`, `odt_styles_survive_lo_conversion_to_docx`)
where Decks' and Tables' only ever rewrote a file in its own format. All
27 of those tests passed before any change.

What the audit did find is what those 27 do not ask about. Six model
fields had nothing testing them at all; two are fixed here and four are
recorded below rather than left implicit.

**Indents were written by one writer and not the other.** `left_indent_pt`,
`right_indent_pt` and `first_line_indent_pt` were carried by the odt
writer since it was written, and the docx module mentioned `w:ind`
nowhere — neither writer nor reader. So the same document kept its layout
saved as `.odt` and lost it saved as `.docx`, with no warning. rdocx has
exposed `indent_left`, `indent_right` and `first_line_indent` the whole
time; they were simply never called. An unindented paragraph still writes
no `w:ind` at all, because OOXML reads an absent one as "inherit from the
style" and an explicit zero as "override the style with nothing".

**Paragraph spacing was written where only we could read it.** The odt
writer emitted `fo:space-before`/`fo:space-after` on
`style:paragraph-properties`. ODF spells paragraph spacing
`fo:margin-top`/`fo:margin-bottom` — the same XSL-FO properties this
writer's own page geometry already used a hundred lines further down. Our
reader understood the ones we wrote, so the self round trip passed and the
spacing was gone the moment the file reached Writer: LibreOffice's
converted docx carried no `w:spacing` in the body *or* in the `Normal`
style, which is how the loss was traced to the attribute rather than to
its converter. The reader still accepts the old spelling so a document
saved by an earlier build keeps its spacing.

That is the same defect as the odp paragraph break in #790 and the
run-joining convention in #783: **our writer and our reader agreed with
each other, and nothing else agreed with either.** A self round trip
cannot see it; only a real consumer can.

### Still open, measured but not fixed

Four fields the audit found and this change does not address. Each was
measured in all four directions (self and cross-format, both ways):

| field | state |
|---|---|
| `tab_stops_pt` | lost by **both** formats, including each self round trip — neither writer persists it at all |
| `Document.footnotes` | docx carries them; the **odt** writer and reader drop them entirely |
| `PageGeometry.columns` | odt carries them; the **docx** module has no `w:cols`, so a two-column document saves as one |
| `page_break_before` | survives everywhere except `odt -> Writer -> docx`, which needs the same which-side-dropped-it check the spacing got |

Tab stops and columns are missing features rather than silent strips in
one direction, but a document loses them on save either way, so they
belong on this list rather than in a backlog nobody reads.
