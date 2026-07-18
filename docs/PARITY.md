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

---

## Letters (word processor)

### Tier 1 — Core (daily-driver writing)

| Feature | Status | Proven by |
|---|---|---|
| Styled runs (b/i/u/s, highlight, inline code) | ✅ | I1 model, I2 docx 17/17, I3 109/109, I4, I5 |
| Headings 1–6 | ✅ | I1, I2, I3, I5 |
| Paragraph alignment | ✅ | I1–I5 |
| Bullet/numbered lists (flat) | ✅ | I1–I3 + I5: markers render as the buffer representation and capture back to ListKind (bridge round-trip green) |
| Hyperlinks | ✅ | I2, I5 (dynamic link:<url> tags) |
| Code blocks | ✅ | I1, I2 (CommonMark fenced 24/29), I3 |
| Markdown save/load with formatting | ✅ | I2 CommonMark ratchet **630/652 — target met** (raw HTML preserved verbatim; remaining 22 are escape/entity/autolink edge cases) |
| DOCX save/load | ✅ | I2, I3, I4 |
| Undo/redo | ✅ (buffer-level) | **move to model ops + I1**; I6 smoke |
| Find & replace | ✅ UI | **extract to core + I1**; I6 |
| Word count | ✅ | I6 smoke (live) |
| Spell check | ✅ | dictionaries bundled in Flatpak; squiggle visible over AT-SPI attrs |
| Print / PDF export | ✅ in-process Typst | I1 suite-export tests (valid PDF, error surfacing) |
| Inline images | ✅ | I1+I2 byte-identical docx round-trip; I5 buffer paintable round-trip |

### Tier 2 — Nice-to-have (rounds out the product)

| Feature | How to test |
|---|---|
| Tables in documents (cell-tagged model) | ✅ I1+I2 round-trip, I3 structural (table-2x2 asserts coordinates). Interleaved position + UI editing remain |
| Named paragraph styles (Title, Subtitle, Quote) | ✅ I1+I2 round-trip |
| Font size / color per run | ✅ I1 + I3 scenarios |
| Superscript / subscript | ✅ I1 + I3 (incl. LO w:position encoding) |
| Headers & footers with fields ({page}) | ✅ I2 round-trip (Document.header/footer) |
| Page setup | breaks ✅ I2 round-trip; size/margins ✅ I2 (PageGeometry in docx sectPr + odt page-layout) + I3 oracle: geometry survives LO odt→docx pass |
| Font family round-trip | ✅ I2 (RunStyle.font_family; docx rFonts + odt fo:font-family) |
| Block quotes | ✅ I1 + I3 (BlockQuotation style) + markdown quote round-trip |
| Line spacing round-trip | ✅ I2 both formats (odt fo:line-height %, docx w:spacing auto rule via rdocx line_spacing_multiple) + I3 oracle through LO in both |
| ODT read/write | ✅ I2 10-test round-trip (paras, h1–6, b/i/u/s, highlight, size, color, links, alignment, lists, page breaks, header/footer) + I3 oracle 7 tests: LO opens ours, we open LO's, bold survives LO odt→docx pass |

### Tier 3 — Advanced (each needs an explicit scope decision)

| Feature | Test approach if adopted |
|---|---|
| Track changes | I1 revision model; I2 docx `w:ins`/`w:del` fixtures; I3 LO-authored tracked docs |
| Comments | I2 docx comments part round-trip |
| Footnotes/endnotes | I2 + I3 (rdocx has a footnotes module already) |
| Multi-column sections | I1 layout; I7 visual |
| Bidi/RTL editing | I3 already covers RTL text survival; editing needs I6 caret tests |
| Table of contents generation | I1: TOC derived from heading model |

## Tables (spreadsheet)

### Tier 1 — Core

| Feature | Status | Proven by |
|---|---|---|
| Cell editing + formula evaluation | ✅ IronCalc | I1 engine tests; I6 smoke (extend: type into cell) |
| OpenFormula function coverage | ✅ 107/107 | I2 ratchet (IronCalc upstream-main patch until next release) |
| XLSX round-trip | ✅ | I1 io tests, I4 Calc oracle |
| ODS / CSV / TSV import | ✅ | I1; add I3-style: LO-authored ods/xlsx read |
| Number formats (currency, %, date) | ✅ | I1 format.rs + I2 xlsx format codes |
| Undo/redo | ✅ | I1 (12 tests) |
| Multi-sheet | ✅ | I1 + I4: names survive xlsx→Calc→xlsx |
| Sort, cell borders, merge, validation | ✅ model | I1 + I4: merges/frozen panes/column widths persist to xlsx and survive Calc |

### Tier 2 — Nice-to-have

| Feature | How to test |
|---|---|
| Formulas surviving save | ✅ I2+I4: written as formulas with cached results; Calc evaluates ours |
| Charts (bar/line/pie exist as Cairo) | I1 chart-model; I4: chart part present in xlsx Calc opens |
| Conditional formatting | I1 rule engine; I2 round-trip |
| Freeze panes / autofill / named ranges | I1 each; freeze survives xlsx (I2) |
| Cross-sheet references | I1 IronCalc already supports; add coverage |

### Tier 3 — Advanced

Pivot tables (I1 aggregation model + I2), array formulas (IronCalc
roadmap-dependent), external file references (decision: likely never),
1M-row performance (criterion bench with budget gate in CI).

## Decks (presentations)

### Tier 1 — Core

| Feature | Status | Proven by |
|---|---|---|
| Slide CRUD + object model | ✅ | I1 (10 tests) |
| Text boxes, rects, circles, images | ✅ | I1 round-trip |
| PPTX save/load | ✅ | I1, I4 Impress oracle |
| Speaker notes | ✅ | I1, LO round-trip (notesSlide parts read+written) |
| Undo/redo | ✅ | I1 (9 tests) |
| Present mode + transitions | ✅ | I6 smoke: enter/exit presenting; I7 visual |
| **LO-authored parity corpus for Decks** | ✅ 9/9 | decks-core/tests/lo_parity.rs (pptx through-the-oracle, ratcheted) |

### Tier 2 — Nice-to-have

| Feature | How to test |
|---|---|
| Styled text inside text boxes (runs, not plain) | ✅ | model+pptx (shared Run/RunStyle) + Pango canvas rendering |
| Master slides applied on render | I1 resolution logic; I7 |
| ODP read/write | I2 fixtures + I4 (LO native) |
| Slide reorder / duplicate | I1 + I6 |
| Image fit/crop modes | I1 geometry |

### Tier 3 — Advanced

Animations beyond transitions (I7-heavy), embedded audio/video (decision
needed), Impress-template import (I3-style corpus).

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
