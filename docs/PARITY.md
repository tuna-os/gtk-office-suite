# Parity Plan — tiers, features, and how each is proven

The definition of done for every feature is a **test that fails without it**,
in the cheapest instrument that can catch it. "100% parity" for this suite
means: every Tier 1 and Tier 2 feature below has its linked test green, and
the corpus/oracle numbers that cover it are at their ratcheted maximums.
Tier 3 items each need an explicit decision to enter scope.

## The instruments (in cost order)

| # | Instrument | Where | Catches |
|---|---|---|---|
| I1 | Model unit tests | `*-core` crates, `cargo test` | logic, invariants, edge cases |
| I2 | Round-trip ratchets | corpus harnesses (CommonMark, DOCX/XLSX/PPTX fixtures) | format fidelity, regressions |
| I3 | LO-authored parity corpus | `lo_parity.rs` (soffice writes, we read) | reading real-world files |
| I4 | soffice oracle | `soffice_oracle.rs` per core crate — 65 tests (Letters 25, Tables 20, Decks 20 — the coverage target in TESTING.md): we write → LO reads/rewrites → we re-read, asserting attributes not just text | writing real-world files |
| I5 | Buffer/bridge round-trips | Xvfb `cargo test -p <app> bridge` | model ⇄ widget translation |
| I6 | AT-SPI smoke tests | `tests/gui/test_smoke.py` (17) — incl. per-cell/per-object virtual a11y nodes | app-level behavior, input |
| I7 | VLM visual audit | scheduled, non-gating | rendering/HIG regressions |

Rule of thumb: every feature needs I1; anything that persists needs I2–I4;
anything interactive needs I5 or I6. Cross-app clipboard: fragment matrix I1 + per-app GDK glue I6 (copy/paste round trips in Letters and Tables).

**The Render column** says whether the feature *looks* right, next to the
file-level claim in Status. ✅ names render-lab fixtures that are green in
every tier measured (A and B, and C, the shipped Flatpak, once it has run) of [`tools/render-lab/baseline.json`](../tools/render-lab/baseline.json)
(see [RENDER-PARITY-ROADMAP.md](RENDER-PARITY-ROADMAP.md)); 🟠 names a fixture
that is amber, with the reason; 🟡 file-only is a visual feature proven only
at the file level, waiting for a fixture; — is a feature with nothing of its
own to draw. `conformance/validate_parity.py` (E5) fails a ✅ whose fixtures
are not all green, and any fixture name the baseline does not have.

---

## Letters (word processor)

### Tier 1 — Core (daily-driver writing)

| Feature | Status | Proven by | Render |
|---|---|---|---|
| Styled runs (b/i/u/s, highlight, inline code) | ✅ | I1 model, I2 docx 17/17, I3 109/109, I4, I5 | ✅ letters/char-emphasis, letters/highlight |
| Headings 1–6 | ✅ | I1, I2, I3, I5 | ✅ letters/headings |
| Paragraph alignment | ✅ | I1–I5 | ✅ letters/alignment |
| Bullet/numbered lists (flat) | ✅ | I1–I3 + I5: markers render as the buffer representation and capture back to ListKind (bridge round-trip green) | ✅ letters/bullet-list, letters/numbered-list, letters/nested-list |
| Hyperlinks | ✅ | I2, I5 (dynamic link:<url> tags) | ✅ letters/hyperlink |
| Code blocks | ✅ | I1, I2 (CommonMark fenced 24/29), I3 | ✅ letters/code-block |
| Markdown save/load with formatting | ✅ | I2 CommonMark ratchet **630/652 — target met** (raw HTML preserved verbatim; remaining 22 are escape/entity/autolink edge cases) | — |
| DOCX save/load | ✅ | I2, I3, I4 | — |
| Undo/redo | ✅ (buffer-level) | **move to model ops + I1**; I6 journey: tests/gui/test_letters.py (undo/redo typed text) | — |
| Find & replace | ✅ UI | **extract to core + I1**; I6 | — |
| Word count | ✅ | I6 smoke (live) | — |
| Spell check | ✅ | dictionaries bundled in Flatpak; squiggle visible over AT-SPI attrs | — |
| Print / PDF export | ✅ in-process Typst | I1 suite-export tests (valid PDF, error surfacing) | — (export lab, docs/EXPORT-PARITY-SPEC.md) |
| Inline images | ✅ | I1+I2 byte-identical docx round-trip; I5 buffer paintable round-trip | ✅ letters/image |

### Tier 2 — Nice-to-have (rounds out the product)

| Feature | Status | Proven by | Render |
|---|---|---|---|
| Tables in documents (cell-tagged model) | ✅ | I1+I2 round-trip, I3 structural (table-2x2 asserts coordinates). Interleaved position + UI editing remain | 🟠 letters/table (accepted metric artifact) |
| Named paragraph styles (Title, Subtitle, Quote) | ✅ | I1+I2 round-trip | ✅ letters/named-styles, letters/block-quote |
| Font size / color per run | ✅ | I1 + I3 scenarios | ✅ letters/font-sizes, letters/text-color |
| Superscript / subscript | ✅ | I1 + I3 (incl. LO w:position encoding) | ✅ letters/super-subscript |
| Headers & footers with fields ({page}) | ✅ | I2 round-trip (Document.header/footer) | ✅ letters/header-footer, letters/page-numbers |
| Page setup | ✅ | breaks I2 round-trip; size/margins I2 docx_page_geometry_round_trips (letters-core/tests/docx.rs) and odt page_geometry_survives (PageGeometry in docx sectPr + odt page-layout) + I4 page_geometry_survives_lo_conversion (letters-core/tests/soffice_oracle.rs) | ✅ letters/page-break, letters/page-margins, letters/landscape |
| Font family round-trip | ✅ | I2 (RunStyle.font_family; docx rFonts + odt fo:font-family) | ✅ letters/font-families |
| Block quotes | ✅ | I1 + I3 (BlockQuotation style) + markdown quote round-trip | ✅ letters/block-quote |
| Line spacing round-trip | ✅ | I2 both formats (odt fo:line-height %, docx w:spacing auto rule via rdocx line_spacing_multiple) + I3 oracle through LO in both | ✅ letters/line-spacing |
| ODT read/write | ✅ | I2 10-test round-trip (paras, h1–6, b/i/u/s, highlight, size, color, links, alignment, lists, page breaks, header/footer) + I3 oracle 7 tests: LO opens ours, we open LO's, bold survives LO odt→docx pass | — |

### Tier 3 — Advanced (each needs an explicit scope decision)

| Feature | Test approach if adopted |
|---|---|
| Track changes | I1 revision model; I2 docx `w:ins`/`w:del` fixtures; I3 LO-authored tracked docs |
| Comments | I2 docx comments part round-trip |
| Footnotes | ✅ | I2 round-trip + I4 (survives Writer rewrite); insert action in Letters. Endnotes/comments/track changes out per ADR 0003 §2 |
| Multi-column sections | I1 layout; I7 visual |
| Bidi/RTL editing | I3 already covers RTL text survival; editing needs I6 caret tests |
| Table of contents generation | I1: TOC derived from heading model |

## Tables (spreadsheet)

### Tier 1 — Core

| Feature | Status | Proven by | Render |
|---|---|---|---|
| Cell editing + formula evaluation | ✅ IronCalc | I1 engine tests; I6 smoke (extend: type into cell) | ✅ tables/values |
| OpenFormula function coverage | ✅ 107/107 | I2 ratchet (IronCalc upstream-main patch until next release) | — |
| XLSX round-trip | ✅ | I1 io tests, I4 Calc oracle | — |
| ODS / CSV / TSV import | ✅ | I1; add I3-style: LO-authored ods/xlsx read | — |
| Number formats (currency, %, date) | ✅ | I1 format.rs + I2 xlsx format codes + I6 Format Cells sheet; values render formatted on canvas and in a11y cells | 🟠 tables/number-formats (accepted metric artifact) |
| Undo/redo | ✅ | I1 (12 tests) + I6 journey: tests/gui/test_tables.py::test_undo_removes_cell_value, tests/gui/test_smoke.py::TablesUndoSaveReopenSmoke | — |
| Multi-sheet | ✅ | I1 + I4: names survive xlsx→Calc→xlsx; I6 journey: tests/gui/test_tables.py (add-sheet tabs) | — |
| Sort, cell borders, merge, validation | ✅ model | I1 + I4: merges/frozen panes/column widths persist to xlsx and survive Calc | ✅ tables/borders, tables/merged |

### Tier 2 — Nice-to-have

| Feature | Status | Proven by | Render |
|---|---|---|---|
| Formulas surviving save | ✅ | I2+I4: written as formulas with cached results; Calc evaluates ours | — |
| Charts persisted (bar/line/pie) | ✅ | I2 round-trip (write + own reader) + I4 (survives Calc rewrite); Insert into Sheet dialog | 🟠 tables/chart-pie (amber in the shipped Flatpak, Tier C; green in A/B); the other four green in every tier: tables/chart, tables/chart-line, tables/chart-area, tables/chart-scatter |
| Conditional formatting (cell-value rules) | ✅ | I1 rule matching + I2 round-trip + I4 (survives Calc rewrite); rendered on canvas, dialog | ✅ tables/conditional |
| Freeze panes / autofill / named ranges |  | I1 each; freeze survives xlsx (I2) | ✅ tables/frozen |
| Cross-sheet references |  | I1 IronCalc already supports; add coverage | — |

### Tier 3 — Advanced

Pivot tables (I1 aggregation model + I2), array formulas (IronCalc
roadmap-dependent), external file references (decision: likely never),
1M-row performance (criterion bench with budget gate in CI).

## Decks (presentations)

### Tier 1 — Core

| Feature | Status | Proven by | Render |
|---|---|---|---|
| Slide CRUD + object model | ✅ | I1 (10 tests) | — |
| Text boxes, rects, circles, images | ✅ | I1 round-trip | ✅ decks/shapes, decks/image |
| PPTX save/load | ✅ | I1, I4 Impress oracle | — |
| Speaker notes | ✅ | I1, LO round-trip (notesSlide parts read+written) | — |
| Undo/redo | ✅ | I1 (9 tests) + I6 journey: tests/gui/test_decks.py::test_undo_removes_added_slide | — |
| Present mode + transitions | ✅ | I6 smoke: enter/exit presenting; I7 visual | — |
| **LO-authored parity corpus for Decks** | ✅ 9/9 | decks-core/tests/lo_parity.rs (pptx through-the-oracle, ratcheted) | — |

### Tier 2 — Nice-to-have

| Feature | Status | Proven by | Render |
|---|---|---|---|
| Styled text inside text boxes (runs, not plain) | ✅ | I3 LO-authored styled-runs (decks-core/tests/lo_parity.rs) + I4 soffice oracle bold_run_survives_impress_rewrite; model+pptx (shared Run/RunStyle) | ✅ decks/text-styles, decks/bullets |
| Master slides applied on render | ✅ | I1 placeholder-skip parser + I4 (Impress-authored pptx maps slides→masters); master background inherited on canvas | ✅ decks/background, decks/title-layout |
| ODP read/write | ✅ | decks-core/src/odp.rs: I2 round-trips (text, runs, geometry, notes, background) + 7 I4 oracle tests (Impress rewrite both ways; reads Impress custom-shape output) | — |
| Slide reorder / duplicate |  | I1 + I6 | — |
| Image fit/crop modes |  | I1 geometry | 🟡 file-only |

### Tier 3 — Advanced

Presenter state (current/next/notes/timer/display target) is covered by the
GTK-free contract in `decks-core/src/presenter.rs`; the presenter window and
external-display journey remain release work per
[`docs/adr/0004-decks-advanced-workflows.md`](adr/0004-decks-advanced-workflows.md).
Animations beyond admitted transitions, embedded audio/video, comments, and
Impress-template import are explicitly scoped by that ADR and require corpus
fixtures before they can become parity claims.

---

## The 100%-parity definition, operationally

1. **Every row above links to a test** (FEATURES.md per app mirrors these
   tables with links; scorecard.py counts them — phase M).
2. **Ratchet targets**: CommonMark ≥ 630/652 · LO-Letters ≥ 104 and growing
   ~10/week · LO-Decks corpus exists and ≥ 90% · OpenFormula Small group
   100%, Medium ≥ 80% · all oracles green in every CI run.
3. **Tier discipline**: Tier 1 red = release blocker. Tier 2 red = tracked,
   scheduled. Tier 3 = not red, not in the scorecard denominator, until a
   recorded decision (ADR) admits it.
4. **Sequencing next**: Letters bridge gaps (links/alignment/lists → I5),
   Decks parity corpus (the pattern is proven, ~1 session), OpenFormula
   ratchet for Tables, then images (the largest Tier 1 hole).
