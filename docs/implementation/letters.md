# Letters implementation specification

Status: **remaining work, pending unless a package cites observed completion.**
Baseline: `6dacab4d` (PR #1550), inspected 2026-10-13 on branch
`work/implementation-specifications`. This is an execution specification, not a
release certificate. Shared gates are in [shared.md](shared.md) as
SH-001…SH-008; this file owns only `letters-core` / `letters` work and cites
shared IDs instead of duplicating them.

Authoritative starting points: `docs/readiness-2026-09/letters-fidelity.md`
(5/6, one `[~]` scope row), `docs/readiness-2026-09/README.md`,
`ROADMAP.md`, `docs/LETTERS-FIELD-MAP.md`, `docs/LETTERS-REVIEW-WORKFLOWS.md`,
`docs/RENDER-PARITY-ROADMAP.md`, ADRs 0003/0008/0010/0011, `docs/PARITY.md`,
`docs/DESIGN-UI.md`, `docs/ACCESSIBILITY.md`.

## Retained architecture — do not regress

- One edit authority: `letters/src/live.rs::LiveModel` owns `Document`
  (`letters-core/src/model.rs`) and `edit::History` (`letters-core/src/edit.rs`).
  `GtkTextBuffer` is a projection written by `project()`; `foreign_edits`
  counts any other writer. `bridge::document_of`, save (`letters/src/window/saving.rs`
  via `letters-core/src/save.rs::write` / `write_with_opaque` and
  `letters-core/src/session.rs::DocumentSession::save_to`), page layout
  (`letters/src/doc_tab.rs::typeset_for`), `PageView::set_typeset` /
  `update_document`, printing and headless PDF all read the model.
  `capture_from_buffer` is test-only.
- One layout: `letters-core/src/layout/mod.rs` produces `RenderTree`;
  `letters-core/src/layout/pango.rs::Typeset` shapes/draws it (Pango/Cairo).
  Screen, thumbnails (`letters/src/thumbnails.rs`), print
  (`letters/src/printing.rs`), preview and ordinary PDF (`PageView::write_pdf`)
  derive from the same revision and `Typeset`. Typst (`letters/src/engine.rs`)
  is a separately labelled alternative conversion, never WYSIWYG evidence.
- Model projections, not full semantics: `letters-core/src/docx.rs` (rdocx
  0.14.0 pinned) plus `docx_fields.rs`, `docx_anchors.rs`, `docx_chips.rs`,
  `docx_comments.rs`, `docx_revisions.rs`, `docx_toc.rs`; separate
  `letters-core/src/odt.rs` reader/writer. `Document` is flat paragraphs with
  `table_cell` membership plus `table_columns` / `table_rows` / `table_spans` /
  `table_fills`, one `PageGeometry`, plain-text `header`/`footer` plus
  `header_pictures` / `footer_pictures` and alignment, heading looks,
  `comments`, `track` revision marks, `toc` entries, chips, footnotes.
  `ParaStyle.named_style` is a reference, not a reusable style-definition graph.
- Structured commands are model ops through `letters-core/src/structured.rs::StructuredEditor`
  and `letters/src/page_edit.rs` / `actions.rs` / `insert.rs` / `dialogs.rs`;
  never string-prefix rewrites or a second document copy. No resurrected legacy
  shadow buffer, no second layout engine, no widget-owned business logic.
- Safety already landed: `save_page_to_path` commits through `save_to` before
  clearing dirty state; `save_asking_about_loss` asks before known destructive
  saves; `letters-core/src/loss.rs` finite scan + `suite_common::carry::capture`
  opaque parts; recovery via `DocumentSession` + `suite-common-core/autosave.rs`.
  Packages below extend these boundaries; they do not rebuild them.

## Completeness map (readiness → package)

| Readiness / issue source | State at baseline | Package |
|---|---|---|
| letters-fidelity gate 1: data safety + import provenance; binary budget #1545/#1546; hostile media/XML; source deletion/replacement; Save As across formats; remote-upload failure; crash recovery; loss detector exhaustiveness | Open; PR #1546 OPEN (approved review citing green CI, not merged); opaque/loss read source path at save time | LET-001, LET-002; SH-002, SH-003, SH-007 |
| Gate 2: canonical document/commands/history; `foreign_edits` zero; import-context + save-revision ownership; buffer retirement #1202 stage 4+ | Open; #1202 OPEN (stage 3 merged, buffer still holds caret/selection; find/spelling read buffer) | LET-003 |
| Gate 3: screen/export agreement; pagination beyond `pagination.rs` text check; zooms/scales; missing-font reporting; Typst separation | Open; `the_pdf_has_the_typesets_pages_with_the_text_its_breaks_say` only | LET-004 |
| Gate 4: paragraph/page typography; per-paragraph controls; mixed sections; bidi direction unavailable; CJK/combining/RTL | Open; global widow/orphan + one `PageGeometry` only; `base_direction` helper only | LET-005, LET-005S, LET-006 |
| Gate 5: styles/tables/images as editable content; nested tables flatten; padding #1533 OPEN/DIRTY; borders/pagination; anchors/wrapping/alt text | Open; widths/heights/spans/fills landed; padding unmerged | LET-007, LET-008 |
| Gate 6: GNOME keyboard + a11y beyond `LettersKeyboardOnlySmoke` / `LettersAccessibleGeometrySmoke`; empty `AccessibleTextImpl::attributes`; per-char bounds; Orca/IME | Open; `performance-accessibility.md` manual-Orca row `[ ]` | LET-009; SH-006 |
| Gate 7: release evidence on actual documents; 30-doc `baseline-real.json` Tier A 4/25/1, Tier B 3/27/0 stored verdicts; #1369 red + 25 amber open incl. #1456; forms/long/multilingual; Tier C gating #1531 idea; publisher PDFs #1534 idea | Open | LET-010; SH-004, SH-005, SH-008 |
| letters-fidelity `[~]` ADR scope: numbers reconciled (#1205, `tests/test_adr_numbers.py`), ADR 0003 §2 vs shipped review/ADR 0008 scope still open | Open | LET-011 |
| Review workflows: comments/track/TOC/chips wired with limits (para breaks + formatting changes untracked); bidi unavailable; `LETTERS-REVIEW-WORKFLOWS.md` contract | Partially admitted; needs extension packages | LET-006 (bidi), LET-012 |
| Window God-file `letters/src/window.rs` 1134/1300 ceiling, #168 P0 | Debt open | LET-013 |
| Export parity #1127 nested-list, #1115 super-subscript amber; render #904 table amber (accepted Tesseract artifact, blocked) | Open | LET-004, LET-007 |
| Capability ledger, CI required-check (`test` only), soak wiring, release exact-revision evidence, RustSec #1549, legacy Letters #263 | Shared | SH-001, SH-004, SH-007, SH-008 |
| Deferred: collaboration spike only, WASM, enterprise policy, telemetry, encryption/signatures, `suite-convert` #579, translations | Parked by #1190/registry | Non-goals below; README deferred table |

Unsupported semantics vs visual proof: a passing `docx.rs` / `odt.rs` /
`soffice_oracle.rs` round trip proves the admitted model survived; it does not
prove an unmodelled semantic was preserved, nor that pixels match. Every
visible change needs a foreign-writer render-lab fixture plus inspected
side-by-side images (SH-005); every preserved semantic needs a Writer rewrite
assertion; unsupported content needs WarnOnLoss or refusal (LET-001/LET-002).

---

## LET-001 — Import provenance and save-loss honesty

**Goal:** unknown or unmodelled source content is preserved safely, explicitly
disclosed before overwrite, or refused — never treated as kept because
DOCX/ODT can represent it.

**Current evidence:** `window/saving.rs::save_page_to_path` (lines 25-70)
captures opaque parts from `td.file` clone at save time then
`carried.write_with(path, |p| bridge::save_buffer_to_file(...))` plus
`locations::commit_save(path)` inside `save_to`. `save_asking_about_loss`
builds `compatibility_report(doc, format)` from the model plus
`loss::content_a_save_drops(source)` read from disk at dialog time.
`loss.rs::content_a_save_drops` scans macros, embedded objects, charts/SmartArt,
nested tables; `save.rs::compatibility_report` describes the model only.
Both read the source path at save time, not an immutable import context owned
by `DocumentSession`. `session.rs` owns identity/recovery, not content or
provenance. PR #1546 (OPEN, `sec/fix-docx-binary-budget`, approved review)
moves `budget.read_entry()` before the XML/RELS filter + hostile-package
regression; do not assume it present on this branch (parent note: protection
candidate is on separate branch PR #1552; docs branch is `6dacab4d`).

**Target files/symbols:** `letters-core/src/session.rs::DocumentSession`,
`letters-core/src/save.rs::{format_for_path,write,write_with_opaque,compatibility_report}`,
`letters-core/src/loss.rs::content_a_save_drops`, `letters-core/src/docx.rs::read_with_report`,
`letters-core/src/odt.rs::read_with_report`, `letters/src/window/saving.rs::{save_page_to_path,save_asking_about_loss,save_with_prompt}`,
`letters/src/bridge.rs::save_buffer_to_file`, `suite-common-core/src/interop.rs::{OpaquePackage,CompatibilityReport}`,
`suite-common-core/src/zip_guard.rs`, `suite-common-core/src/carry.rs`.

**Dependencies:** SH-002 (conflict precondition), SH-003 (recovery generation),
SH-007 (ZIP/parser bounds).

**Steps in order:**
1. Bind carried parts + loss findings to the imported revision: capture
   `OpaquePackage` + `CompatibilityReport` in `read_with_report` at open,
   store alongside `DocumentSession` (explicit owner: session holds
   import-context; `LiveModel` holds content), reuse at save/dialog instead of
   re-reading the live source path.
2. Define and test source deletion/replacement between open and save: missing
   source ⇒ carried parts unavailable ⇒ dialog states original unrecoverable
   from source, offers Save-a-Copy / Save-Anyway / Cancel; replacement with
   different bytes ⇒ refuse silent carry, re-derive or warn. Never carry bytes
   from a path that no longer resolves to the imported revision.
3. Exercise Save As across formats (docx↔odt/md/txt/html): model report +
   source report recomputed for the *target* format; Cancel writes nothing and
   leaves tab unsaved + dirty + checkpoint intact; Save-Anyway records
   per-target confirmation (`LOSS_CONFIRMED_KEY` scope stays per-path).
4. Keep remote-upload failure inside the transaction: `commit_save` failure ⇒
   `save_to` Err, identity/checkpoint/dirty unchanged, visible error with
   accessible focus return.
5. Close the DOCX binary budget gap per #1545: apply per-entry + total
   decompression budgets to every package member during preflight (read PR
   #1546 final diff first; do not duplicate if merged — rebase and verify).
   Prove hostile media/XML rejection without losing the open document: open
   doc stays editable, error names the member/limit, no partial file left.
6. Extend `loss.rs` only for observed corpus misses: each new detector needs a
   minimized fixture + oracle or hostile test; document the scan as finite,
   not exhaustive. Do not claim “no-loss” beyond the enumerated scan +
   carried-part list.

**Edge/error/data-loss cases:** cancel at each dialog; failed write at each
`atomic_save::fault` boundary; kill mid-save (existing `KilledMidSaveSmoke`);
renamed/missing original; unsaved doc Save As; Unicode/long filenames;
read-only destination; concurrent external replacement; oversized optional part
skipped vs fatal per `hostile_packages.rs` contract.

**Test plan (existing paths; proposed IDs marked NEW):**
existing `letters-core/tests/hostile_packages.rs`, `malformed_inputs.rs`,
`package_consistency.rs`, `soffice_oracle.rs`, `tests/gui/test_smoke.py::LettersSaveFailureSmoke`,
`LettersLossWarningCancelSmoke`, `LettersOpaquePartsSmoke`,
`LettersUnreadContentSmoke`, `LettersKilledMidSaveSmoke`,
`LettersRenamedOriginalSmoke`, `LettersOpenPathsSmoke`.
NEW: `source_replaced_between_open_and_save_refuses_silent_carry`,
`save_as_across_formats_recomputes_both_reports`,
`remote_commit_failure_keeps_identity_and_checkpoint`,
`a_docx_whose_binary_member_expands_past_the_budget_is_refused` (take from
PR #1546 if merged), GUI `LettersSourceReplacedSmoke` (NEW), extend
`LettersLossWarningCancelSmoke` for Save As variant (NEW case).

**GUI/render/interop:** dialogs keep Cancel default, destructive styling,
per-target confirmation; loss bullets via `dropped_features`; recent-files only
on success; no second serializer.

**CI verification:** `cargo test -p letters-core`, GUI `Letters*Save*` /
`*Loss*` / `*Opaque*` / `*Unread*` journeys in parallel CI; require
`REQUIRE_SOFFICE=1` oracle lane where touched; no `SUITE_GTK_TESTS=skip` as proof.

**Acceptance:** original bytes + dirty/recovery state survive
cancellation/failure at every boundary; successful reopen matches every
admitted semantic field + approved loss report; hostile input never takes down
the open document; binary-budget regression green on candidate.

**Non-goals:** exhaustive loss oracle; versioned conflict merge (SH-002);
encryption/signatures; macro execution.

**Safe PR boundary:** one PR for import-context binding + source
replacement/Save-As/remote cases; separate PR for binary-budget merge/verify
if PR #1546 unmerged; separate PR per new loss detector.

---

## LET-002 — Opaque preservation round-trip discipline

**Goal:** safe unmodelled parts (custom XML, thumbnail, ODF settings) survive
unrelated edits byte-identical; unsafe parts are never silently kept or
dropped.

**Evidence:** `interoperability.md` 9/9 historical; `OpaquePackage::capture` /
`append_to` single-write ordering (write-once after writer replaces file);
`write_with_opaque` in `docx.rs`/`odt.rs`.

**Targets:** same save/opaque symbols as LET-001 plus
`letters-core/tests/package_consistency.rs`.

**Steps:** enumerate admitted safe-part list per format; assert byte-identity
after unrelated model edit + Ctrl+S; assert `ooxml_package_problems` empty;
assert unsafe partial pass-through refused; keep single-write ordering (no
moment on disk with edit-but-no-parts).

**Tests:** existing `docx_and_odt_packages_are_consistent`,
`LettersOpaquePartsSmoke`, `LettersUnreadContentSmoke`. NEW:
`opaque_parts_survive_unrelated_table_edit_byte_identical` per format.

**Acceptance:** edit → save → reopen keeps admitted semantics + opaque bytes;
unsafe content always routes to LET-001 dialog.

---

## LET-003 — One content authority to buffer retirement

**Goal:** `LiveModel` the only content authority; `DocumentSession` owns
identity/recovery + import context (LET-001); projection buffer retired to a
caret/selection/accessibility view with zero `foreign_edits` for all user paths.

**Evidence:** #1202 OPEN; stage 3 merged (edits in `page_edit.rs`,
`actions.rs`, `insert.rs`, `dialogs.rs` are model ops with unit + GUI proof);
still writing/reading buffer directly: find/replace highlights, spelling
highlights, fallback paths for buffer-without-model, `SpellChecker::replace_last`
dead code, `page_view.rs` / find / spelling reads of buffer text, caret/selection
offsets via buffer.

**Targets:** `letters/src/live.rs::{LiveModel,of,project,edit_with,restyle,snapshot,sequence_offset}`,
`letters/src/bridge.rs::{render_to_buffer,render_document_paragraphs,document_of,selection_fragment,save_buffer_to_file,load_document}`,
`letters/src/page_view.rs::{set_typeset,update_document,buffer_offset_at,caret_rect,AccessibleTextImpl}`,
`letters/src/page_edit.rs::{typed_ops,delete_ops,make_editable}`,
`letters/src/dialogs.rs::{replace_ranges,FindState,header_footer_of}`,
`letters/src/spell.rs::{SpellChecker,SpellCheckHandle}`, `letters/src/insert.rs`,
`letters/src/doc_tab.rs::{typeset_with_starts,copy_selection}`,
`letters-core/src/edit.rs::{Op,History,locate,sequence_text}`, `letters/src/live/tests.rs`.

**Dependencies:** LET-001 (save reads model, not buffer); SH-003 (recovery from model).

**Steps:**
1. Move find/replace + spelling highlights to presentation-only tags already
   excluded (`PRESENTATION_TAGS`); prove model bytes unchanged across
   highlight/show/hide cycles.
2. Move find match enumeration, spelling word iteration and suggestion replace
   onto `LiveModel::snapshot().0` sequence offsets; buffer offsets used only
   for caret placement via `sequence_offset` mapping. Delete or wire
   `replace_last` dead code (remove if unwired).
3. Move `PageView` text reading (`AccessibleTextImpl`, hit-test, caret) to the
   typeset document revision; keep buffer only as IME/caret host until GTK
   selection contract allows removal. Each step must hold `foreign_edits == 0`
   for typing, IME, formatting, paste, find/replace, review and structural
   commands.
4. Expand seeded tests to new model fields (LET-007/LET-008) and mixed
   tables/objects: one `History` restores edited + untouched content, review
   anchors, selection. Table/structure ops stay `SetParagraphs`-family block
   ops, never text-marker hacks.
5. Remove buffer-as-authority comments in `doc_tab.rs` / `page_view.rs` /
   `window.rs` as each read-side adapter dies; do not reintroduce a competing
   model. Shrink `bridge.rs` to projection + clipboard fragment only.

**Edge cases:** IME commit at mark edges; comment/track anchors across Unicode
(combining, non-BMP, CJK, Enter); empty-last-paragraph style; table-cell vs
neighbour joins (`EditError::TableStructure`); pending-format caret moves;
multi-paragraph/merged-cell edits.

**Tests:** existing `live/tests.rs::{unrelated_edits_and_commands_leave_every_other_field_alone,review_anchors_hold_across_unicode_edits_and_one_history_undoes_them}`,
`page_edit.rs` widget tests, `LettersCommentsSmoke` unicode case,
`LettersModelUndoSmoke`, `LettersReplaceAllSmoke`, `LettersCutPasteSmoke`,
`LettersPasteOverSelectionSmoke`, `LettersClipboardSmoke`, stateful
`letters-core/tests/stateful.rs` + `property.rs`. NEW: `find_and_spell_never_touch_the_model`
(NEW), `no_buffer_without_model_fallback_writes_content` (NEW),
per-new-field seeded cases (NEW).

**Acceptance:** zero `foreign_edits` across the full command matrix; undoing
everything restores the loaded document; buffer retirement simplifies adapters
(measured `bridge.rs` / `window.rs` shrinkage, not just moved lines);
#1202 exit (one editing surface, journeys green) or explicit remaining stage
with owner.

**Non-goals:** second editor surface; CRDT replication (deferred).

---

## LET-004 — Single-revision screen/export agreement

**Goal:** ordinary screen, thumbnails, print, preview, headless PDF agree on
page count, breaks, object bounds and text for the same document revision.

**Evidence:** `pagination.rs::the_pdf_has_the_typesets_pages_with_the_text_its_breaks_say`
checks count + per-page text only; render fixtures green except #904 table
amber (accepted Tesseract misread, blocked), export #1127 nested-list amber,
#1115 super-subscript amber; Typst path still present as alternative.

**Targets:** `letters-core/src/layout/{mod.rs,pango.rs}`,
`letters/src/doc_tab.rs::typeset_for`, `letters/src/page_view.rs::write_pdf`,
`letters/src/printing.rs`, `letters/src/thumbnails.rs`, `letters/src/export.rs`,
`tools/render-lab/{fixtures.py,edit_journeys.py,run.sh,run-export.sh}`,
`tools/render-lab/baseline.json`, `baseline-export.json`,
`letters-core/tests/pagination.rs`, `layout/tests.rs`.

**Steps:**
1. Pin revision plumbing: every ordinary path takes `(Document, LayoutOptions)`
   from one `LiveModel::snapshot` + `layout_options(container)`; record
   revision in snapshot metadata used by thumbnails/print/PDF/headless.
2. Extend pagination tests to image/footnote/table-heavy pages; compare every
   page after edits, at multiple zooms/scales, with explicit missing-font
   reporting (log substituting family per page; fail only where fixture
   requires the missing face).
3. Export ambers #1127 (nested-list) and #1115 (super-subscript): done. No
   geometry miss existed; word recall counted OCR misreads of LibreOffice's
   own render, which the scorer now leaves out where we don't match them,
   checked against the reference's text layer (`compare.mark_printed`). No
   threshold was tuned.
4. Keep Typst clearly separate in UI/docs (`engine.rs` labelled alternative);
   never let Typst pagination satisfy a WYSIWYG claim. Delete or repair
   `visual_golden.py` advertising per render roadmap (coordinate with SH-005;
   parent owns cross-app decision, Letters only removes its stale references).
5. Extend the existing `letters/edited-journey` fixture (owned by `tools/render-lab/edit_journeys.py::JOURNEYS["letters"]`, `LettersEditRenderJourney`, docx) with a new table-cell/list case rather than creating a duplicate journey entry: extend the journey's typed content, keep one manifest entry, and ratchet the same pair.

**Tests:** existing pagination + layout tests, `LettersPrintLayoutEditingSmoke`,
`LettersPageThumbnailsSmoke`, render/export CI jobs. NEW: `pdf_matches_typeset_on_table_heavy_pages`
(NEW), `zoom_and_scale_do_not_move_breaks` (NEW), per-fixture export greens for
#1127/#1115 (CI-closed, not hand-closed).

**Acceptance:** matching page count/breaks/object bounds/text screen-vs-PDF on
the heavy pages; independently reviewed reference differences recorded;
self-agreement alone never accepted (requires LibreOffice comparison per
SH-005).

---

## LET-005 — Paragraph typography (no section model change)

**Goal:** imported paragraph/run typography survives and lays out as Writer does for the admitted single-section document; anything unmodelled is loss-reported, not silently kept. Sections are LET-005S, not this package.

**Evidence:** layout already handles spacing collapse, keep-with-next, global orphans/widows (`LayoutOptions.orphans/widows = 2`), indents, tab stops, columns (page-wide + section-read), page breaks (three placements), lists, headers/footers with `{page}`/`{total}`. Missing: per-paragraph keep/widow/orphan overrides, exact vs proportional line-spacing gaps, tab leaders, numbering edge cases, font fallback policy, CJK/combining/RTL cursor parity.

**Targets:** `letters-core/src/model.rs::{ParaStyle,BaseFont,RunStyle}`, `letters-core/src/layout/mod.rs::{LayoutOptions,relayout,spacing,flow}`, `letters-core/src/layout/pango.rs::{PangoShaper,Typeset,line_boxes}`, `letters-core/src/lists.rs`, `letters-core/src/docx.rs` (indents, spacing, numbering), `letters-core/src/odt.rs` (margins, tab-stop children), `letters/src/ruler.rs`, `letters/src/actions.rs::cycle_line_spacing_in`, `letters-core/tests/soffice_oracle.rs`, `lo_parity.rs`, render fixtures `letters/*spacing*`, `*indent*`, `*list*`, `*columns*`.

**State contract (existing types only, no new model in this package):** paragraph state is the existing `ParaStyle` fields (`space_before/after_pt`, `left/right/first_line_indent_pt`, `tab_stops_pt`, `line_spacing`, `keep_with_next`, `page_break_before`, list fields) plus run state in `RunStyle`; document defaults are `BaseFont` + `LayoutOptions::{font_family,font_size_pt,orphans,widows}`. Mutation boundary: `Op::SetParaStyle` / `Mark` / block `SetParagraphs`; one control change is one history transaction with exact inverse; undo restores the prior style struct verbatim.

**Edge-case matrix (each needs model + undo + DOCX/ODT + Writer-rewrite + fixture, or a loss ID):** style inheritance (direct vs named-style effective value); font fallback chain + missing-font report per page; exact (points) vs proportional (multiple) line spacing; before/after collapse at page/column top (larger wins mid-page, zero at top except document top); negative first-line (hanging) vs indent; tab stops with leaders vs default tabs; numbering restart vs continued (`list_start`, `list_label`); `keep_with_next` + heading-keep chaining into orphan/widow split; footnote reference on a paginating line (`split`/`carry`); CJK shaping + combining-mark cursor steps + RTL run caret via `Typeset::caret`.

**Steps:**
1. Implement the matrix rows in corpus priority (forms/questionnaires first): reader maps the OOXML/ODF value into the named existing field; writer emits it; layout consumes it; control edits it through one op.
2. Add per-paragraph keep/widow/orphan overrides as new `ParaStyle` fields only with the full four-layer coverage above; otherwise record as unsupported with loss IDs and keep the global default.
3. Cover CJK, combining sequences and RTL cursor movement/selection through the shared shaper; never shape a paragraph outside `layout::paragraph_request` / `Typeset`.

**Tests:** existing oracle indent/spacing/columns/break/tab-stop/numbering tests; NEW per added field/row: model + undo + both writers + oracle + fixture (proposed names, NEW).

**Acceptance:** each shipped matrix row round-trips through Writer both ways and renders within fixture budget; each deferred row explicitly listed as unsupported with loss ID.

## LET-005S — Mixed-section geometry model (proposed names marked)

**Goal:** represent multi-section documents (different page size/margins/columns/headers per part) without corrupting single-section files. This is a model migration, not a layout tweak.

**Evidence:** `Document.page: Option<PageGeometry>` is document-wide; ODT section-columns are read as page-wide fallback with stated precedence; docx `sectPr` read from one section properties element. Mixed-geometry files currently collapse to one geometry + loss note.

**Proposed state contract (proposed names; existing `PageGeometry` unchanged):** proposed `Document.sections: Vec<DocSection>` (NEW name), where proposed `DocSection { id: u32, range: ParagraphRange, geometry: PageGeometry, header: Option<String>, footer: Option<String> }` (NEW names); `range` owns contiguous paragraph indexes, ranges are non-overlapping and cover the body in order; paragraph index 0 always has a section. Inheritance: a paragraph uses its owning section's geometry/header/footer; `Document.page` remains the default for documents with empty `sections` (backwards compatibility: empty means one implicit section). Overrides: no per-paragraph geometry override in this package — paragraph fields affect spacing/indents only. Stable IDs: section `id` never reused after delete; undo restores the section vector verbatim.

**Migration order:** 1. add `sections` with serde defaults (empty = legacy) + legacy snapshot acceptance; 2. docx multi-`sectPr` + ODT section-styles readers populate it; 3. both writers emit it; 4. layout paginates per-section geometry (new section starts a new page/column context); 5. Page Setup dialog edits the caret's section via one `SetSections` op (proposed op name, NEW); 6. recovery/clipboard/a11y carry the vector. Single-section files must byte-compare before/after except the added empty vector.

**Edge-case matrix:** section break at paragraph boundary vs mid-paragraph run (mid-paragraph splits the paragraph); last-section properties stored at docx body end vs per-`sectPr`; unequal column counts/gaps per section; per-section header/footer vs continued; table spanning a section break (break before the table, never mid-row); undo of section insert/delete restores ranges + IDs; Save As to Markdown/text drops sections with one loss dialog (LET-001).

**Tests (existing + NEW):** `page_geometry_survives`, `table_column_widths_survive_writer_both_ways`, `a_page_break_survives_a_conversion_between_the_two_formats`. NEW: `multi_section_geometry_survives_writer_both_ways` (NEW), `section_delete_restores_ranges_and_ids` (NEW), `table_never_splits_across_section_break` (NEW), GUI `LettersSectionSetupSmoke` (NEW).

**Acceptance:** two-section Writer files (portrait-to-landscape, 1-to-2 columns, changed margins/headers) survive both formats both ways with correct pagination; single-section corpus byte-stable; SH-003 recovery carries the vector.

**Status (PR #1580):** landed. `letters-core/src/sections.rs` holds the model (`Document.sections`, `DocSection`, `ParagraphRange`, `Op::SetSections`; edits follow sections and undo restores them verbatim); layout lays each section on its own pages; Page Setup edits the caret's section, Insert/Remove Section Break are `app.insert-section-break`/`app.remove-section-break`; DOCX writes and reads a `w:sectPr` per section, ODT a master page per section (Writer's `text:section` columns map to the section). Verified by name: `multi_section_geometry_survives_writer_both_ways` (nightly oracle), `section_delete_restores_ranges_and_ids`, `table_never_splits_across_section_break`, `recovery_snapshot_carries_the_section_vector`, `a_copy_carries_its_section_breaks_into_a_paste`, `LettersSectionSetupSmoke`, render fixture `letters/mixed-sections` A/B green. Unmodelled: continuous/odd/even breaks (next-page), unequal columns, page-number restarts — each a loss finding (`section-kinds`, `section-unequal-columns`, `page-number-restart`); first/left page headers and per-section header pictures remain document-level.

---

## LET-006 — Bidi paragraph direction (currently unavailable)

**Goal:** either ship explicit direction or keep the honest “unavailable”
contract — no half-wired control.

**Evidence:** `LETTERS-REVIEW-WORKFLOWS.md` §bidi: text kept exactly
(`right_to_left_text_survives_a_conversion_between_the_two_formats`), Pango/GTK
per-paragraph detection, `base_direction` helper only; no model field, no
writer/reader (`w:bidi`, `style:writing-mode`), no control, caret/selection
follow detection.

**Targets:** `letters-core/src/model.rs::ParaStyle` (new field if admitted),
`letters-core/src/docx.rs`, `letters-core/src/odt.rs`, layout/shaper direction,
`letters/src/actions.rs` + dialogs control, `page_view.rs` caret/selection,
`soffice_oracle.rs`, GUI journey.

**Steps if admitted:** model field + both writers/readers + control + GUI
journey + oracle both directions + cursor/selection tests (RTL, mixed, neutral
fallback). If not admitted in this milestone: keep unavailable wording in UI
docs + loss table; keep text-exactness test; do not add a control that writes
nothing.

**Acceptance:** direction edits are one undo step, survive Save/reopen + Writer
both ways, and move caret/selection correctly; or the unavailable contract
remains with no writer/control stub.

---

## LET-007 — Styles, tables and lists as editable content

**Goal:** reusable style inheritance + table/list geometry survive editing and
Writer round-trips; nested tables never silently flatten.

**Evidence:** `named_style` reference only; table widths/heights/spans/fills
landed; PR #1533 (OPEN, DIRTY, stacked on merged-cells) adds
`Document.table_padding` / `CellPadding` + docx `w:tblCellMar` (+ first-cell
`w:tcMar`) + odt first-cell `fo:padding` + layout padding — treat as
in-progress awaiting CI, not verified. Nested tables flatten with warning
(`a_nested_table_lo_wrote_keeps_its_text` keeps text only). #904 table amber is
a recorded OCR artifact, not a geometry pass.

**Targets:** `letters-core/src/model.rs::{ParaStyle.named_style,heading,ListLabel,NumberFormat,TableCell,table_columns,table_rows,table_spans,table_fills}`,
`letters-core/src/docx.rs` (numbering, indents in cells, `tblCellMar`, fills,
widths/heights), `letters-core/src/odt.rs` (auto-style dedup key incl. tab-stop
children, fills, padding), `letters-core/src/layout/mod.rs::{layout_table,column_widths,close_tall}`,
`letters-core/src/lists.rs::{ordinals,marker_for}`, `letters-core/src/structured.rs`,
`letters/src/actions.rs` (list/indent/outdent/restart), `letters/src/ruler.rs`.

**Steps:**
1. Preserve reusable style definitions + inheritance (not just effective
   appearance): import effective appearance today; add definition graph only
   with model + command/undo + DOCX/ODT + screen/export coverage per capability
   or honest loss disposition. Reconcile with LET-011 scope decision first.
2. Land padding correctly: rebase PR #1533 onto candidate, resolve DIRTY state,
   run `cargo test -p letters-core --features render`, oracle
   `table_cell_padding_survives_writer_both_ways`, field-map update, render-real
   dispatch; merge only when CI green on the candidate revision.
3. Extend table semantics as corpus requires: measured padding (step 2),
   borders, pagination (row split vs keep), multi-paragraph/merged-cell edits
   round-tripping without losing neighbours.
4. Nested tables: keep flatten-with-warning until a real tree model ships; never
   claim structural fidelity. Any nested-table model change is its own package
   with grid-stability proof (`Document::locate` panic class from #532 history).
5. Lists: restarts, labels (`prefix`/`format`/`suffix`), levels, `clear`/`bar`/
   `num` exclusion already handled — extend only for observed Writer misses,
   each with oracle both directions.

**Tests:** existing `soffice_oracle.rs` table/indent/numbering suites,
`layout/tests.rs` table geometry, `live/tests.rs` table cases, field-map
`tests/test_letters_field_map.py`. NEW per added semantic: model + undo +
both writers + Writer rewrite + fixture.

**State contract (existing types):** table identity is `TableCell.table` with `(row, col)` grid positions; geometry is `table_columns` / `table_rows` / `table_spans` / `table_fills` (+ `table_padding` from PR #1533 once merged); style state is `ParaStyle.list*` + `named_style` + heading. Mutation boundary: structural changes are `Op::SetParagraphs` block ops with table renumbering in document order; undo restores the paragraph vector + side maps verbatim.

**Edge-case matrix:** adjacent tables with no blank paragraph stay two tables; delete of the separator paragraph re-splits them; multi-paragraph and merged-cell edits keep neighbours; covered grid positions keep empty paragraphs; column-count change falls back to equal columns; nested table keeps text with warning (no structural claim); list `clear`/`bar`/`num` `w:tab` values never become stops; strict-OOXML `w:start`/`w:end` indents read positionally only on exact paragraph-count match.

**Acceptance:** edits in multi-paragraph/merged cells round-trip without losing
neighbours; each added capability has the four-layer coverage or a loss ID;
nested-table limitation stays user-visible.

---

## LET-008 — Images, footnotes, headers/footers, fields

**Goal:** placed content persists with anchor/size/alt text and renders where
Writer puts it; floating layout never marketed as Pages-like free canvas.

**Evidence:** inline images + header/footer pictures + alignment landed (#1522);
footnote bodies cross both ways (#839 follow-up); footer text-box text kept
(#1532); `ImageAnchor` (frame/offset/align/behind) exists; `docx_anchors.rs`
placements; ODT graphic styles; `{page}`/`{total}` fields via `docx_fields.rs`;
TOC field/index mapped.

**Targets:** `model.rs::{RunStyle.image,image_extent_emu,image_anchor,footnote,footnotes,header,footer,header_pictures,footer_pictures}`,
`docx.rs` + `docx_anchors.rs` + `docx_fields.rs`, `odt.rs` graphic/note styles,
`layout/...` image/note/header/footer/TOC placement, `toc.rs`, `chips.rs`,
`insert.rs::insert_image/insert_footnote`, `dialogs.rs` header/footer dialog.

**Steps:** test anchor/size/alt text, wrapping expectations, persistence across
save/recovery; footnote split-across-pages behavior (`split`/`carry` in layout
flow); header/footer first-page/odd-even limits stated honestly; text-box
content beyond kept-text declared; floating coordinates alone never claimed as
page-layout support.

**Tests:** existing image/footnote/header-footer/TOC oracle + round-trip tests.
NEW: `floating_anchor_survives_writer_both_ways` (NEW),
`footnote_split_keeps_text_and_numbering` (NEW),
`image_alt_text_survives_save_and_recovery` (NEW).

**State contract (existing types):** image state is `RunStyle.image` + `image_extent_emu` + `image_anchor` (frame/offset/align/behind) with alt text as run text; notes are `RunStyle.footnote` indexes into `Document.footnotes`; furniture is `header`/`footer` + `header_pictures`/`footer_pictures` + alignments; fields are `{page}`/`{total}` runs via `docx_fields.rs`. Mutation boundary: insert footnote/image/link/chip is one model op (one undo step); anchor moves with its text.

**Edge-case matrix:** missing image file renders placeholder + preflight warning, never blocks open/save; floating anchor frames (Page/Margin/Text) with behind/in-front; footnote body excluded from body stream with citation marker stripped; footnote split across pages carries numbering; text-box text beyond kept-text declared unsupported; Save As to Markdown/text warns per LET-001.

**Acceptance:** admitted image/note/furniture cases survive save/recovery +
Writer both ways and render in-budget; unadmitted placements have loss IDs.

---

## LET-009 — Native editing, keyboard and accessibility

**Goal:** every admitted editing/review/insert/find/recover task completes
without a pointer and is announced correctly; geometry follows actual layout.

**Evidence:** `LettersKeyboardOnlySmoke` (edit/undo/save only),
`LettersAccessibleGeometrySmoke` (text-box bounds at zooms/scroll),
`LettersScreenReaderTraversalSmoke` (walk without crash);
`AccessibleTextImpl::attributes` / `default_attributes` return empty vecs;
per-character bounds unavailable until GTK 4.16 build; manual Orca row `[ ]`
in `performance-accessibility.md`; `ACCESSIBILITY.md` Letters section.

**Targets:** `letters/src/page_view.rs::AccessibleTextImpl`,
`letters/src/page_edit.rs`, `actions.rs`, `dialogs.rs` (find/replace),
`review_ui.rs`, `comments_ui.rs`, `toc_ui.rs`, `chips_ui.rs`, `link_ui.rs`,
`insert.rs`, `outline.rs`, `window.rs` focus/shortcut wiring,
`tests/gui/test_smoke.py` Letters a11y/keyboard classes.

**Steps:**
1. Extend keyboard journeys: navigate headings/tables, select + format,
   insert link/image, review comments, find/replace, recover — all without
   pointer. Keep focus + selection continuity across zoom/reflow.
2. Expose formatting, heading/table semantics through AT-SPI (attributes beyond
   empty lists); per-character bounds only after GTK 4.16 dependency admitted —
   until then state the magnifier limitation honestly.
3. Do not fabricate Orca evidence from role lists (SH-006): record observer,
   candidate, GNOME/Wayland versions, settings, spoken output, deviations.

**Tests:** extend `LettersKeyboardOnlySmoke`, `LettersAccessibleGeometrySmoke`;
NEW `LettersHeadingsTablesKeyboardSmoke` (NEW), `LettersReviewKeyboardSmoke`
(NEW), `LettersFindReplaceKeyboardSmoke` (NEW). Deterministic AT-SPI remains
gating; VLM advisory only.

**Acceptance:** pointer-free completion of each admitted task; refusal reasons
announced; focus reachable; geometry matches layout; human Orca sign-off
recorded per SH-006 (external prerequisite, not code).

---

## LET-010 — Real-document and long-document release evidence

**Goal:** the advertised “usable for” set is no broader than passing journeys
on real files.

**Evidence:** `tools/render-lab/baseline-real.json` 30 Letters docs (Tier A
4 green / 25 amber / 1 red #1369; Tier B 3/27/0) — stored ratchet verdicts, not
a rerun. Open real issues incl. #1456 (SMA template: 98% words but amber —
text presence ≠ convincing page), #1369 red expression-of-interest form,
questionnaires, salary/annex/statistical/form corpus. Manifest: ≤8-page GOV.UK
DOCX/ODT, hashes/licences recorded; not broad long/multilingual sample. Tier A
= GTK render-node; Tier B = Broadway browser screenshot, not shipped
GNOME/Wayland. #1531 (Tier C gating proposal) and #1534 (publisher/Office PDFs
second reference) are ideas, not installed oracles.

**Targets:** `tools/render-lab/{fixtures.py,real_corpus/manifest.json,baseline-real.json,compare.py,run.sh}`,
`tools/render-lab/vm/` + `installed_check.py`, `.github/workflows/render-parity.yml`,
`render-real.yml`, `release-revision.yml`, per-issue side-by-side reports.

**Steps:**
1. Reduce each meaningful amber/red to an authored licensed regression; retain
   the full document as integration case. Require open → edit → undo/redo →
   save-a-copy → reopen → export/print with semantic + visual assertions.
2. Add long reports, forms, letters/CVs, mixed-language docs beyond the
   ≤8-page GOV.UK sample; record hashes/licences/provenance.
3. Gate shipped Flatpak in GNOME (Tier C) per SH-005/SH-008; publish exact
   revision, fonts, references, budgets, missing cases, residual losses.
4. Never close a real-doc issue by title or word-count alone (#1456 pattern);
   CI green on tiers A+B (+ export where applicable) with inspected images
   closes it.

**Admission state contract:** each admitted file records provenance + licence + hash (`real_corpus/manifest.json`), admitted semantics (which LET packages it exercises), and expected tiers (A/B/export). Reduction rule: fix the reduced single-feature fixture first, then re-run the full file; never delete the real file to turn amber green.

**Edge-case matrix:** forms with padded cells (LET-007) vs letters/CVs (LET-005) vs long reports (LET-004 pagination) vs mixed-language docs (LET-005/LET-006 shaping); edited-copy journey on at least one file per class; word-count-only greens rejected (#1456 pattern: 98% words but amber layout).

**Acceptance:** “usable for” ⊆ passing journeys; no blanket Word/Pages
replacement claim on the 30-doc baseline; ratchet never moves away from green.

---

## LET-011 — Scope reconciliation (ADR 0003 vs shipped review)

**Goal:** close the letters-fidelity `[~]` row without silently expanding or
deleting scope.

**Evidence:** ADR numbers reconciled (#1205, `tests/test_adr_numbers.py`);
ADR 0003 §2 said footnotes-only (comments/track out) but `LETTERS-REVIEW-WORKFLOWS.md`,
ADR 0008, and shipped comments/track/TOC/chips paths admit more. README parent
rule: older closures are history, not present truth.

**Targets:** `docs/adr/0003-later-tier-scope.md`, `docs/adr/0008-letters-structured-editing.md`,
`docs/LETTERS-REVIEW-WORKFLOWS.md`, `docs/readiness-2026-09/letters-fidelity.md`,
`docs/PARITY.md` Letters Tier 3 rows, `conformance/capabilities.json` Letters entries.

**Steps:** for each Tier-3 item (comments, track, TOC, chips, footnotes,
bidi, styles, columns, encryption-adjacent): record admitted vs deferred with
evidence IDs or explicit revised-scope decision + owner; migrate PARITY +
capabilities ledger to match; keep the `[~]` tick only when every row resolves.

**Acceptance:** no advertised feature relies on an older exclusion; no deferred
item erased without a decision record.

---

## LET-012 — Review, TOC, chips and fields hardening

**Goal:** admitted workflows stay correct across edits, undo and Writer passes.

**Evidence:** wired per #1281 with GUI + oracle coverage; documented limits:
paragraph breaks + formatting changes untracked; tracked deletion across break
keeps break; orphan comments saved as empty-range at doc start; TOC levels 1-3,
empty headings omitted, `toc::settle` two-pass page numbers; chips as content
controls; `{page}`/`{total}` fields.

**Targets:** `letters-core/src/{track.rs,track/tests.rs,comments.rs,comments/tests.rs,toc.rs,toc/tests.rs,chips.rs,review.rs,docx_revisions.rs,docx_comments.rs,docx_toc.rs,docx_chips.rs,docx_fields.rs,docx_anchors.rs}`,
`letters/src/{review_ui.rs,comments_ui.rs,toc_ui.rs,chips_ui.rs}`,
`letters-core/tests/soffice_oracle.rs` review/TOC/chips cases.

**Steps:** extend seeded unicode + structural coverage to new fields; prove
orphan/deleted-insertion (`Revision::under`) accept/reject both directions;
TOC update after heading moves with settled numbers; chips persistence as text
+ link where controls unsupported (warn per LET-001).

**Tests:** existing `tracked_changes_survive_lo_passes`,
`comments_survive_lo_passes`, `a_table_of_contents_survives_lo_passes`,
`smart_chips_survive_lo_passes`, GUI `LettersTrackChangesSmoke`,
`LettersCommentsSmoke`, `LettersTableOfContentsSmoke`, `LettersSmartChipsSmoke`.
NEW cases only for newly admitted field behavior.

**Acceptance:** one undo step per review action; anchors hold across unicode +
structural edits; Writer passes preserve admitted semantics.

---

## LET-013 — Window decomposition without behavior drift

**Goal:** pay down the P0 God-file debt while keeping every Letters journey green.

**Evidence:** `letters/src/window.rs` 1134 lines vs 1300 ceiling
(`scripts/release_gate.py` + `tests/test_roadmap_figures.py` enforce); saving
already split to `letters/src/window/saving.rs`; #168 tracks decomposition.

**Targets:** `letters/src/window.rs`, `letters/src/window/saving.rs`,
`letters/src/doc_tab.rs`, `letters/src/dialogs.rs`, size-gate scripts.

**Steps:** extract one bounded owner per PR (tabs/session wiring, save/close
guards, recovery, palette/actions registration); keep `LiveModel` +
`DocumentSession` authority intact; never lower ceilings as a substitute for
ownership; update measured tables.

**Acceptance:** line count moves toward decomposition with zero journey
regressions; no behavior change bundled with a move.

---

## LET-014 — Performance budgets for Letters paths

**Goal:** large-document editing, layout and save/open stay within published budgets.

**Evidence:** `letters-core/tests/performance_budgets.rs` (20/500/5000-para
docs, DOCX/ODT save+open, Markdown, 200-char typing, 7 samples p50/p95, PR
lane) + `peak_memory.rs`; prior find: large DOCX save 30 s via rdocx clones
(127 MB vs 7 MB model); `layout::PageStack` prefix-sum fix for 500-page frames.

**Targets:** `letters-core/tests/performance_budgets.rs`, `peak_memory.rs`,
`letters-core/src/layout/mod.rs::PageStack`, `letters-core/src/docx.rs` write
path, `letters/src/page_view.rs` frame path.

**Steps:** keep budgets scaled small→large with recorded runtime; gate measured
regressions; fix by reducing clones/reflows/visible-page work, never by
loosening budgets without evidence.

**Acceptance:** PR-lane budgets green on candidate; large-doc save/open/typing
+ 500-page frame within budget; memory peaks recorded.

---

## Human and infrastructure blockers (not code tasks)

| Blocker | Owner / evidence needed |
|---|---|
| LibreOffice oracle availability (`soffice`, Writer conversion filters incl. strict/transitional DOCX, fonts Liberation/Carlito/Caladea/DejaVu/Noto) | Infra: `REQUIRE_SOFFICE=1` lane + nightly; `soffice --version` alone insufficient |
| Render Tier C VM (`/dev/kvm`, GNOME Wayland/Mutter, Flatpak bundle install) + proposed Tier-C gating #1531 | Infra + maintainer decision; Tier B never substitutes for Tier C (SH-005) |
| Second reference publisher/Office PDFs #1534 | Idea only; needs licensed Office install + revision-matched PDFs before use as oracle |
| Manual Orca + IME journeys on GNOME/Wayland; narrow/scale/dark/high-contrast; external-display observation | Human observer per SH-006; agents must not fabricate |
| Branch required-check configuration (only `test` required at 2026-10-13 inspection) | Repository admin per SH-001 |
| RustSec #1549 (Typst → `serde_yaml` → `unsafe-libyaml`) policy | Maintainer + SH-007 migration/waiver; feature PRs must not suppress it |
| Exact-revision candidate freeze, architectures, reproducible/offline builds, Flathub submission, legacy Letters #263 cutover | Maintainer + SH-008; meta-tracker #1190 ends at v3.0 |
| Multi-window same-document races, soak streaks (20 consecutive + matrix), coredump collection in CI | SH-002/SH-004; release-soak.yml exists — bind candidate, do not duplicate |

## Non-goals for Letters packages

Collaboration beyond the Loro spike, WASM extensions, enterprise dconf policy,
telemetry, encryption/signatures, headless `suite-convert` (tracked #579,
deferred by #1190), actual translations, macro execution, proprietary-format
parity, free-canvas page layout (Pages-like placement reuses Decks object model
later per DESIGN-UI), embedding LibreOffice engine, adopting rdocx/Typst layout
as the WYSIWYG authority, and any second document/layout authority.

## How to use this file

Implement one LET package per bounded PR per README protocol: reproduce on the
real path, retain a minimized fixture on failure, implement GTK-free logic in
`letters-core`, wire GTK only, keep one operation one history transaction,
migrate all consumers (load/save, clipboard `letters-core/src/fragment.rs`,
recovery, screen/export, thumbnails, a11y), add foreign-writer fixtures +
Writer assertions + inspected render images where visible, and return exact
test IDs, revision, results/skips and artifact URLs. Cross-app blockers cite
SH IDs; do not re-specify them here.
