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

### The four fields, all now closed

The four fields #839's audit measured and left open, each in all four
directions (self and cross-format, both ways). Every one is now fixed,
in the order of severity the audit gave them; the sections below record
what each turned out to be, because the four had four different causes:

| field | state |
|---|---|
| `tab_stops_pt` | **fixed** — see below |
| `Document.footnotes` | **fixed** — see below |
| `PageGeometry.columns` | **fixed** — see below |
| `page_break_before` | **fixed** — see below |

Tab stops and columns were missing features rather than silent strips in
one direction; footnotes and the page break were losses in a single
direction. A document lost all four on save either way, which is why
they were listed here rather than in a backlog nobody reads.

One thing measured and still unread: a strict-OOXML indent inside a
table cell. rdocx exposes table paragraphs separately from the body
stream, and the positional scan that reads the strict indents and tab
stops walks the body only, skipping `w:tbl` subtrees.

### Strict-OOXML indents (`w:ind w:start`), found by a CI-only failure

The oracle test added for the two fixes above passed locally and failed
on CI, at the same assertion twice, with the indent reading zero after
`odt -> Writer -> docx`. LibreOffice was the same version on both sides
(4:24.2.7-0ubuntu0.24.04.6), which ruled out a version divergence and
pointed at the one thing left: `--convert-to docx` does not name a
filter, and LibreOffice has **two** docx exporters.

| filter | `w:pPr` |
|---|---|
| `MS Word 2007 XML` (transitional) | `<w:ind w:left="720"/>`, `<w:jc w:val="left"/>` |
| `Office Open XML Text` (strict) | `<w:ind w:start="720"/>`, `<w:jc w:val="start"/>` |

ISO/IEC 29500 strict names the horizontal indents by axis rather than by
side. rdocx parses only the transitional pair, so every indent in a
strict .docx read as zero — the CI runner's LibreOffice simply picked the
other filter than this container's, and the environment difference was
exposing a real reader gap rather than causing one. `w:firstLine`,
`w:hanging` and `w:jc`'s `start`/`end` are already handled, so the gap is
exactly `w:start`/`w:end`.

The reader now supplements the strict spelling from `word/document.xml`
where rdocx reports no indent, positionally and only when its paragraph
count agrees with rdocx's. Two guards: a unit test that re-spells our own
output and needs no LibreOffice, and the oracle test, which now names
both filters instead of accepting whichever one the local build prefers.

Not covered: a strict indent inside a table cell. rdocx exposes table
paragraphs separately from the body stream, and this scan walks the body
only, skipping `w:tbl` subtrees.

### Footnotes now cross the boundary both ways

`text:note` appeared nowhere in the odt module — neither writer nor
reader — so a footnote's text was dropped on every odt save. That is
authored content, not layout, which makes it the most severe of the four.

ODF nests a note *inside* the referencing paragraph:

```xml
<text:p>Body<text:note text:id="ftn1" text:note-class="footnote"
  ><text:note-citation>1</text:note-citation
  ><text:note-body><text:p>the note</text:p></text:note-body
></text:note> and after</text:p>
```

Three things follow from that nesting, each its own guard:

- the note's `text:p` children must stay out of the body stream, or the
  referencing paragraph splits and the note's text lands in the body;
- `text:note-citation` holds the rendered marker, which a reader that
  takes all the text turns into a leading `1` on the footnote;
- the reference run carries no text of its own — it marks a position,
  and the text lives in `Document::footnotes`.

The docx → Writer → odt direction was broken for the same reason, so
fixing the reader also made us able to read Writer's own notes. Both
directions are now asserted against a real Writer, and all four guards
above fail independently under mutation.

### Columns: one missing writer call, and one reader looking in the wrong part

`PageGeometry.columns` was carried by the odt writer and dropped by the
docx one — `w:cols` appeared nowhere in the module, so a two-column
document saved as `.docx` came back as one column. rdocx has exposed
`set_columns` and `section_properties().columns` all along, exactly as it
had exposed the indent builders. One column still writes no `w:cols`:
that is the default section layout, so an explicit `w:num="1"` is noise.

The cross-format test then failed in **one** direction only, and the
two-step attribution mattered again:

| step | finding |
|---|---|
| what did we write? | `<w:cols w:num="2" w:space="480"/>` — correct, 480 twips is 24pt |
| did Writer read it? | yes: docx → docx keeps `w:num="2"`, elaborated with `w:equalWidth` and `w:sep` |
| where did it go in the odt? | `styles.xml` had no `style:columns` at all |
| so who lost it? | **nobody** |

ODF allows column layout either page-wide or per section, and the two
live in different parts. This writer emits the page-wide form —
`style:columns` inside `style:page-layout-properties` in `styles.xml` —
and Writer, reading a docx `w:cols`, models it as a **section** instead
and writes `style:section-properties` into `content.xml`:

```xml
<style:style style:name="Sect1" style:family="section">
  <style:section-properties style:editable="false">
    <style:columns fo:column-count="2" fo:column-gap="0.3335in"/>
```

Our reader only ever looked at the page layout, so a converted
two-column document read as one column. Nothing was lost in the
conversion; it was recorded somewhere we never read. Had the first check
stopped at "the odt has no `style:columns`", the obvious next move would
have been to change the writer — which was not wrong.

A page-wide count still wins where both are stated. That precedence is
**not separately observable**: this writer never emits a section, so no
fixture can hold both, and it is a stated rule rather than a tested one.

### Tab stops, and a `w:tab` that means three different things

`tab_stops_pt` is settable from the paragraph layout UI and was
persisted by **neither** writer, so a paragraph's stops were lost on
every save in both formats — self round trips included, because writer
and reader agreed the stops did not exist.

Each format resisted in its own way.

**ODF** puts stops in a `style:tab-stops` *child* of the paragraph
properties, not an attribute. Every paragraph style this writer emitted
was a self-closing `<style:paragraph-properties .../>`, so carrying
stops meant giving that element a body. The automatic-style dedup was
keyed on the attribute string alone, which two paragraphs with different
stops share exactly — they would have collapsed onto one style and both
read back with the first one's stops. The key is now the whole element.

**OOXML** spells `w:tab` for three unrelated things:

| where | meaning |
|---|---|
| `w:tabs` child, `w:val="left"` and friends | a real tab stop |
| `w:tabs` child, `w:val="clear"` | **removes** an inherited stop at that position |
| inside a `w:r` | a literal tab character in the text |

A reader that collects every `w:tab@w:pos` invents stops from all three.
The cross-format test caught the `clear` case immediately: Writer emits
`<w:tab w:val="clear" w:pos="1134"/>` to drop its own 2cm default before
listing the real stops, so `[36, 108]` came back as `[56.7, 36, 108]`.
`w:val="bar"` (a vertical rule) and `w:val="num"` (a list's numbering
gap) are not stops either, so the reader keeps an allowlist of the
alignment kinds that are.

rdocx writes stops through `add_tab_stop` but exposes only
`tab_stop_count()` when reading — no positions — so the positions come
from the same positional scan of `word/document.xml` that reads the
strict-spelled indents. One scan now serves both.

### The page break: a third verdict shape

The last of the four, and the two-step attribution came back differently
again. The spacing (#839) was our writer using an attribute nothing else
reads. The columns (#845) were our reader looking in the wrong part of
the package. This one:

| step | finding |
|---|---|
| what did we write? | `fo:break-before="page"` — ODF-correct |
| did Writer read it? | yes |
| is the break in its output? | **yes**, as `<w:br w:type="page"/>` |
| so who lost it? | nobody — OOXML has two spellings and we knew one |

OOXML expresses a page break either as `w:pageBreakBefore` in the
paragraph properties, or as a run-level `<w:br w:type="page"/>`.
LibreOffice writes the second, and puts it as the **last run of the
paragraph before the break** — which is this model's
`page_break_before` on the paragraph that *follows*. Reading it as the
containing paragraph's own break puts the break a page early.

Where the break sits is the whole meaning, so all three placements are
handled and separately tested:

| placement | meaning |
|---|---|
| after all of a paragraph's text | the next paragraph's `page_break_before` |
| before any of its text | that paragraph's own (how Word writes a break inserted at the start of a line) |
| text on both sides | one paragraph split across pages — **not** expressible in this model, so not reported on either paragraph |

The middle case is guessable in a way that would be wrong: assuming the
containing paragraph is the break's owner is exactly the off-by-one
above. The last case is guessable in a way that moves text to the wrong
page, so it is deliberately dropped rather than approximated.

A second, smaller defect came out with it. rdocx renders the break run's
content as a newline, which arrived merged into the preceding run — so
`"first page"` read back as `"first page\n"`, in a model whose
paragraphs never contain a newline. The break is not content, and is
stripped with the flag it produced.
