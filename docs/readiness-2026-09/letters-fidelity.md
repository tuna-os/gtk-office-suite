# [P1] Letters: preserve complete document state through editing, layout, and every save path

Original audit: `e7e4df6`; follow-up to closed #109–#111. The dated assessment below supersedes historical current-state language, not the recorded regressions and fixes.

## Direction assessment — 2026-10-13

**[INFERENCE] A usable GNOME-native word processor is a viable direction on the
current Rust/GTK/libadwaita architecture; general Word or Pages replacement is
not yet evidenced.** Keep the native shell, live model and shared layout rather
than restarting the editor. The largest blockers are preserving imported
semantics, paragraph/table geometry on mixed real documents, and release-level
editing/accessibility evidence—not the choice of GTK. This is a source,
committed-baseline and issue/PR review, not a new test run or a hands-on Word/Pages
comparison. Upstream capability claims are not Letters capabilities.

### What exists, and what that proves

- **One edit authority now exists.** `letters/src/live.rs::LiveModel` owns the
  `Document` and `edit::History`; content edits are model operations and the
  `GtkTextBuffer` is a projection. `foreign_edits` counts writes outside that
  projection. `bridge::document_of`, save and page layout read the model;
  `capture_from_buffer` is test-only. The old capture-on-every-edit description
  below, and buffer-as-authority comments in `doc_tab.rs`/`page_view.rs`, are
  historical, not the current implementation. [#1202's stage-3
  record](https://github.com/tuna-os/gtk-office-suite/issues/1202#issuecomment-5965274243)
  agrees; the issue remains open because the projection buffer is not removed.
  Removing that read-side adapter is not a prerequisite to having one authority.
- **Screen and ordinary PDF/print share layout, not four independent engines.**
  `letters-core/src/layout/mod.rs` produces the `RenderTree`; `layout/pango.rs`
  shapes and draws it. `doc_tab.rs::typeset_for`, `page_view.rs`, `printing.rs`
  and headless PDF export use `Typeset`. Hit testing, selection and caret use
  the same positioned text. `letters-core/tests/pagination.rs` checks PDF page
  count and page text, not full Word-compatible geometry. The separately
  labelled Typst export is an alternative conversion, not WYSIWYG evidence.
- **DOCX and ODT are implemented, but are model projections.** `docx.rs` maps
  rdocx into Letters' model and supplements it with `docx_fields.rs` and other
  XML adapters; `odt.rs` is a separate reader/writer. `model.rs::Document` has
  one page geometry, plain-text note/header/footer bodies, header/footer
  pictures and alignment, heading looks, review marks, and flat paragraphs
  carrying table membership plus widths/heights/spans/fills. It is not an
  arbitrary section tree, general style-definition graph, rich note-body
  model or nested-table tree. `ParaStyle` has a named-style reference, not a
  complete reusable style definition. Representable fields surviving is not
  equivalent to preserving everything an imported package contains.
- **Safety foundations exist; preservation is not complete.**
  `window/saving.rs::save_page_to_path` commits through
  `DocumentSession::save_to` before clearing dirty state, and
  `save_asking_about_loss` asks before known destructive saves.
  `letters-core/src/loss.rs` detects macros, embedded objects, DOCX charts/
  SmartArt and nested tables; safe opaque parts are carried separately.
  That finite scan is not an exhaustive loss detector. Both it and opaque
  capture read the source path at save time, not a complete immutable import
  context owned by `DocumentSession`. Source replacement/removal while editing
  therefore needs an explicit contract and evidence. The package-format
  no-loss branch in `save.rs::compatibility_report` describes the model,
  not all source-file semantics.
- **Real-document evidence is substantially weaker than isolated features.**
  The committed `tools/render-lab/baseline-real.json` contains 30 Letters
  documents: Tier A **4 green / 25 amber / 1 red**, Tier B **3 green / 27 amber /
  0 red**. These are stored ratchet verdicts, not a rerun or latest-CI totals.
  The corpus manifest records hashes/licences for GOV.UK DOCX/ODT publications
  of at most eight pages; it is not a broad long-document or multilingual
  workflow sample. A is GTK render-node output; B is a Broadway browser
  screenshot, not shipped GNOME/Wayland. Current [#1456](https://github.com/tuna-os/gtk-office-suite/issues/1456)
  still reports the SMA email template amber in B despite 98% words found:
  text presence alone does not establish a convincing page.

Recent fixes support incremental repair: merged [#1525](https://github.com/tuna-os/gtk-office-suite/pull/1525)
resolved inherited ODT paragraph text styling, [#1522](https://github.com/tuna-os/gtk-office-suite/pull/1522)
added header/footer pictures and alignment, and [#1532](https://github.com/tuna-os/gtk-office-suite/pull/1532)
kept footer text-box text once. Do not count open work as shipped:
[#1533](https://github.com/tuna-os/gtk-office-suite/pull/1533) supplies table
padding missing from the current constant-padding layout;
[#1546](https://github.com/tuna-os/gtk-office-suite/pull/1546) addresses
[#1545](https://github.com/tuna-os/gtk-office-suite/issues/1545)'s unbounded
non-XML DOCX members; [#1531](https://github.com/tuna-os/gtk-office-suite/pull/1531)
proposes making shipped-Flatpak Tier C gating. All three were open at review.

### Ordered gates to an everyday Word alternative

These are release requirements, not a claim that all work is missing. Preserve
the completed tests below and extend them at each boundary; safety and loss
disclosure precede broader fidelity claims.

1. **Data safety and import provenance.** Retain the existing failed/cancelled
   Save, Save As, close-all and recovery regressions. Close the DOCX binary
   budget gap, and prove hostile media/XML rejection without losing the open
   document. Bind carried parts and loss findings to the imported revision;
   exercise source deletion/replacement, Save As across formats, remote-upload
   failure and crash recovery. Unknown or unmodelled source content must be
   preserved safely, explicitly disclosed before overwrite, or refused—not
   treated as kept because DOCX/ODT themselves can represent it. Exit: original
   bytes and dirty/recovery state survive cancellation/failure; successful
   reopen matches every admitted semantic field and the approved loss report.
2. **Canonical document, commands and history.** Keep `LiveModel` the only
   content authority; `DocumentSession` currently owns identity/recovery, not a
   second content model. Preserve the zero-`foreign_edits` invariant for typing,
   IME, formatting, paste, find/replace, review and structural commands. Define
   import-context and save-revision ownership alongside it. Expand the current
   seeded tests to new model fields and mixed tables/objects; one undo history
   must restore edited and untouched content, review anchors and selection.
   Buffer retirement must simplify adapters, not reintroduce a competing model.
3. **Screen/export agreement.** All ordinary screen, thumbnails, print,
   preview and PDF paths must derive geometry from the same document revision
   and `Typeset`; keep Typst clearly separate. Extend pagination tests to
   image/footnote/table-heavy pages and compare every page after edits, at
   multiple zooms and scales, with explicit missing-font reporting. Exit:
   matching page count, breaks, object bounds and text between screen and PDF,
   plus independently reviewed reference differences; self-agreement alone
   can consistently reproduce the wrong layout.
4. **Paragraph and page typography.** Ratchet real cases for style inheritance,
   fonts/fallback, exact versus proportional line spacing, before/after space,
   indents, tabs/leaders, numbering, keep-with-next, widow/orphan placement,
   footnotes and headers/footers. Current layout already handles many of these;
   global widow/orphan defaults and one `PageGeometry` do not represent
   per-paragraph controls or mixed sections. Add model/I/O/layout support
   together where the corpus requires it. Explicit bidi direction is still
   unavailable (`LETTERS-REVIEW-WORKFLOWS.md`); text retention is not bidi
   editing parity. Cover CJK, combining sequences and RTL cursor movement.
5. **Styles, tables and images as editable content.** Preserve reusable style
   definitions and inheritance, not only an imported effective appearance.
   Extend existing table widths/heights/spans/fills with measured padding,
   border and pagination semantics; require edits in multi-paragraph/merged
   cells to round-trip without losing neighbours. Nested tables currently
   flatten with a warning, not structural fidelity. Test image anchor/size/alt
   text, wrapping expectations and persistence across save/recovery; floating
   image coordinates alone are not Pages-like page-layout support. Each added
   capability needs model + command/undo + DOCX/ODT + screen/export coverage,
   or an honest unsupported/loss disposition.
6. **GNOME keyboard and accessibility workflows.** Extend existing
   `LettersKeyboardOnlySmoke` and `LettersAccessibleGeometrySmoke` beyond
   edit/undo/save and text-box bounds: navigate headings/tables, select and
   format text, insert a link/image, review comments, find/replace and recover
   a document without a pointer. `page_view.rs::AccessibleTextImpl` exposes
   content/caret/selection but returns empty attribute lists; geometry tests
   do not prove formatting, heading/table semantics or Orca usability.
   Require real GNOME/Wayland + Orca/IME journeys, focus and selection continuity
   after zoom/reflow, and per-character bounds on the supported GTK version.
7. **Release evidence on actual documents.** Reduce each meaningful amber/red
   real-document defect to an authored, licensed regression; retain the full
   document as an integration case. Require open → edit → undo/redo → save a
   copy → reopen → export/print with semantic and visual assertions, including
   long reports, forms, letters/CVs and mixed-language documents, not only
   launch renders. Gate the shipped Flatpak in GNOME (Tier C), publish the
   exact revision, fonts, references, budgets, missing cases and residual
   losses, and make the advertised “usable for” set no broader than those
   passing journeys. The current 30-document baseline cannot justify a blanket
   Word/Pages replacement claim.

### Reuse and observation, without another competing engine

- **Start with the implementation already present.** `letters-core/Cargo.toml`
  and `Cargo.lock` select rdocx **0.14.0**. Its [published manifest](https://docs.rs/crate/rdocx/0.14.0/source/Cargo.toml)
  declares `MIT OR Apache-2.0` and already includes layout dependencies.
  [Upstream](https://github.com/tensorbee/rdocx) now advertises a broader native
  document/layout stack; neither that README nor an upgrade proves Letters'
  narrower adapter keeps those features. Inventory usable pinned-version APIs
  before writing more positional XML patches; upstream missing accessors and
  parser fixes where possible. Use a small corpus comparison to decide whether
  a specific upstream component beats the current adapter. Do not silently add
  rdocx PDF pagination beside Pango screen pagination: adopting layout would
  require one end-to-end geometry, editing/hit-test and export contract,
  including ODT. The earlier indent/column fixes below illustrate missed API
  reuse, not evidence for replacing the entire application.
- **LibreOffice Writer is the strongest existing independent oracle and
  format/layout prior art.** Keep `soffice_oracle.rs`, `lo_parity.rs` and the
  render lab. `lo_parity.rs` generates authored HTML scenarios through Writer;
  it does not import the entire Writer test corpus. Study its OOXML/ODF test
  taxonomy and reduce observed failures to own fixtures. For copied code/data,
  check the exact file and third-party notices: [LibreOffice's licensing
  statement](https://www.libreoffice.org/licenses/) describes MPL-2.0 and mixed
  dependencies, not a blanket permission to relabel everything GPL. Embedding
  its engine would be a different, substantial integration decision, not a
  prerequisite established by this review.
- **Keep Pango/Cairo for shaping and drawing; Typst for its labelled use.**
  [Typst](https://github.com/typst/typst) is Apache-2.0 typesetting prior art,
  not proof of DOCX/ODT interactive fidelity. Reuse existing incremental
  shaping/layout and the shared renderer; avoid another source-to-document
  conversion or independently authoritative engine merely to improve PDFs.
- **Observe workflows rather than reconstruct proprietary implementations.**
  Use [Microsoft's public format documentation](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-docx/)
  with OOXML/ODF standards, and the [Pages user guide](https://support.apple.com/guide/pages/welcome/mac)
  for explicit workflow expectations. Pages distinguishes flowing
  word-processing from free page-layout documents; Letters should not imply
  that its word-processing model implements both. Observe licensed Word/Pages
  on authored documents for style edits, page breaks, table growth, image
  placement, review and keyboard behaviour; record app versions, fonts, actions
  and resulting files/PDFs. No such proprietary-app exercise was performed in
  this assessment. [#1534](https://github.com/tuna-os/gtk-office-suite/issues/1534)
  proposes revision-matched publisher/Office PDFs as a second reference, and
  is still an idea rather than an installed Office oracle. Optional
  [REA](https://github.com/morluto/rea) can help organize CLI/MCP investigation
  evidence; it supplies no Office document engine and is not a release
  dependency. Prefer public specs, maintained OSS and behavioral evidence;
  proprietary binary decompilation is neither needed nor proposed.

## Historical implementation record

## Original audit evidence
At `e7e4df6`, `letters/src/bridge.rs::capture_from_buffer` constructed a Document with `header: None, footer: None, page: None`, and `StructuredEditor` operations recaptured and rerendered the buffer. The review workflow contract then admitted comments/revisions with warn-until-mapped semantics. The following slices record the subsequent fixes; their checkmarks cover the cited tests, not arbitrary imported-document fidelity.

## Architecture target and implemented direction
A per-tab live model owns the complete represented Document and edit history; session identity/recovery and imported-package provenance must have explicit ownership alongside it. GTK's buffer is an editing/accessibility projection, not content authority. Keep the established structured operations and shared layout engine, extending their field contracts rather than creating another unsynchronized document copy. The 2026-10-13 assessment identifies the remaining import-context and fidelity boundaries.

## Dependency-ordered slices
- [x] Record and test a field-by-field Document ↔ buffer/sidecar mapping, including tables, lists, run styles, images, footnotes, headers/footers and page geometry.
      [`docs/LETTERS-FIELD-MAP.md`](../LETTERS-FIELD-MAP.md) (#1205) has one row per model field: where the editor keeps it,
      and the test that carries it through LibreOffice Writer for docx and for odt. A self round trip doesn't count.
      `tests/test_letters_field_map.py` fails on a model field with no row or a cited test that doesn't exist. Building it
      found five losses through Writer, all fixed here, with
      `paragraph_and_run_attributes_survive_a_conversion_between_the_two_formats` crossing each both ways:
      - list restarts were carried by neither format;
      - the odt writer flattened nested lists;
      - code blocks and inline code were lost both ways (styles named but never defined, a font never declared);
      - block quotes were lost from docx to odt;
      - the odt reader guessed a Writer list's kind from its style name.

      `heading_styles_and_picture_size_survive_writer_rewriting_a_docx` covers the two docx-only fields. Odt images
      (#1292) now cross both ways (`pictures_in_our_odt_survive_writer_rewriting_it`,
      `we_read_a_picture_writer_places_in_an_odt`). Odt tables (#1296) cross too
      (`tables_in_our_odt_survive_writer_rewriting_it`, `we_read_the_table_writer_writes_into_an_odt`); the second
      found that our docx writer opened every table cell with a blank line, hidden by our reader skipping empty cell
      paragraphs. The heading look now crosses odt too
      (`heading_looks_survive_writer_in_odt`), and `tracked_changes_survive_lo_passes` compares a deletion of a pending
      insertion in all four directions (#1297). It and `comments_survive_lo_passes` had converted each same-format file
      onto itself, which Writer won't do, so those passes read our own file back; they now convert into a directory of
      their own. That audit closed its field-map gaps; it did not establish full DOCX/ODT semantics or coverage of future model fields.
- [x] Preserve fields across unrelated edits and structured commands; distinguish user text from rendered list markers.
      `unrelated_edits_and_commands_leave_every_other_field_alone` (`letters/src/live/tests.rs`, #1278) loads a document
      with every model field set into a live editing session. It then runs 60 seeded sequences of typing (multi-byte
      text and Enter included), deletes, structured commands (list on and off, indent, outdent, restart numbering, page
      break, insert table) and undo and redo in its last paragraph, and captures the buffer whole. Every other
      paragraph and every document-level field (footnotes, header, footer, page, base font, heading look, comments)
      must come back unchanged. No paragraph's text may hold a list marker, a footnote's `[n]` or a table's pipes. The
      model must equal the capture. It found five bugs, all fixed:
      - a list or page break toggled in a table cell, which the cell's text can't show;
      - two tables inserted with nothing between them, which read back as one;
      - an empty last paragraph's style (a heading, a page break) lost on capture, and so on save;
      - a cell's own leading and trailing spaces trimmed away;
      - table ids compared by value rather than by grouping.

      Deleting the empty line between two tables no longer runs them together (#1299): a row followed by a delimiter
      row starts the next table, and the property test's deletes now reach the lines next to a table. Doing so found that
      the live model could give two tables one id: a local re-read numbered its region from its position, and an
      inserted table (numbered past the highest) left later ones out of order. Tables are now renumbered in document
      order after every local re-read.
- [x] Apply model operations and undo through one live session; keep review anchors correct across Unicode edits.
      `review_anchors_hold_across_unicode_edits_and_one_history_undoes_them` (`letters/src/live/tests.rs`, #1279) runs
      40 seeded interleavings of typing (a combining accent, a non-BMP emoji with a skin-tone modifier, CJK, Enter),
      structured commands, and undo and redo, all around a commented word and a tracked insertion. After every step
      the comment covers exactly its word, the insertion exactly its own, and the model equals the editor's reading.
      Undoing everything then restores the loaded document, because typing and commands share one history.
      `LettersCommentsSmoke.test_a_comment_stays_on_its_word_across_unicode_typed_before_it_and_undo` does the same in
      the app: it comments on a word, types `é👍中` before it through the input method, and undoes. It found one bug in
      the live model's partial re-render, fixed here. When an empty line was the last line of the span, its style was
      dropped, and the stale paragraph tags it had replaced stayed behind on its newline.
- [x] Drive editor pagination and print/export from styled paragraph/run metrics; remove byte/character-offset ambiguity.
      One layout paginates everything (#1280): `letters_core::layout::pango::Typeset`, from the document's styled runs,
      paragraph spacing and page geometry. Print Layout, Print, Print Preview, Export as PDF and the headless
      `--export-pdf` all draw one Typeset's pages (`letters/src/doc_tab.rs::typeset_for`, `PageView::write_pdf`).
      "Export as PDF with Typst" is a separately labelled alternative engine and is not the document's PDF.
      `Typeset::page_breaks` reports where each page starts as a character offset in the document's text. Line
      ranges were already character offsets, converted from Pango's byte indices in one place (`line_boxes`).
      `the_pdf_has_the_typesets_pages_with_the_text_its_breaks_say` (`letters-core/tests/pagination.rs`, PR lane)
      lays out a document whose breaks depend on run sizes and spacing, with multi-byte text before every break. The
      PDF it writes, read back with poppler, has the same page count, and each page holds exactly the text between
      its breaks.
- [x] Wire admitted review/TOC/bidi workflows or report them as unavailable until GUI and format tests pass.
      Inventory (#1281), workflow by workflow, as `docs/LETTERS-REVIEW-WORKFLOWS.md` admits them. A format test means
      our file crossing LibreOffice and back, in docx and in odt (`letters-core/tests/soffice_oracle.rs`).
      - **Comments**: wired. GUI: `LettersCommentsSmoke` (add, reply, resolve, undo; stays on its word across Unicode
        typing). Format: `comments_survive_lo_passes`.
      - **Tracked changes**: wired. GUI: `LettersTrackChangesSmoke.test_tracked_edits_are_reviewed`. Format:
        `tracked_changes_survive_lo_passes`. Documented limits: paragraph breaks and formatting changes aren't tracked.
        (The Draft editor, read-only while tracking, was retired in #1228.)
      - **Table of contents**: wired. GUI: `LettersTableOfContentsSmoke` (insert, update, undo). Format:
        `a_table_of_contents_survives_lo_passes`.
      - **Smart chips**: wired. GUI: `LettersSmartChipsSmoke`. Format: `smart_chips_survive_lo_passes`.
      - **Paragraph direction (bidi)**: unavailable, and now said so in the workflows document. No control sets a
        direction, no writer stores one, and the screen detects it per paragraph. What is promised, right-to-left
        text kept exactly, is tested: `right_to_left_text_survives_a_conversion_between_the_two_formats`. There is no
        entry point to hide.
- [~] Reconcile duplicate ADR numbers and the conflicting old/new advanced-feature scope without silently expanding scope.
      **The numbers are reconciled** (#1205). 0004 and 0006 were each two ADRs. The Decks workflows ADR keeps 0004,
      since code and readiness documents cite it as "ADR 0004", and so does the adaptive editor shell's 0006, which
      `tests/adaptive_editor_contract.py` cites by path. Tables advanced analysis became 0007 and Letters structured
      editing became 0008, the two free numbers; nothing cited either by number. Each renumbered file says so at the
      top, and `tests/test_adr_numbers.py` fails on a shared number or a heading that disagrees with its file name.
      ADR 0003 §2 now identifies its review exclusions as historical and points to the bounded current workflow
      contract; the Letters Tier 3 parity rows no longer repeat those exclusions as current. Still open (#1655):
      reconcile all advanced-feature boundaries and revision-bound capability evidence, including styles and
      encryption-adjacent work. This documentation correction does not close LET-011.

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

A strict-OOXML indent inside a table cell, once measured and unread, is
now read too (#1204): see below.

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

Table cells (#1204). rdocx exposes table paragraphs separately from the
body stream, so the scan keeps a second positional list for paragraphs
directly inside a top-level table's cells, in the order rdocx walks
tables → rows → cells → paragraphs, and trusts it on the same terms: only
when it counts exactly as many paragraphs as rdocx does. Paragraphs of a
table nested in a cell belong to neither list. Writing the guard found the
writer's half of the gap: a cell paragraph's indents and alignment were
never written at all, so they were lost on any docx save. Both directions
are covered by `strict_ooxml_indents_inside_table_cells_are_read`.

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
