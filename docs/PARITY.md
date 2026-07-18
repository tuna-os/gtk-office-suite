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
| I4 | soffice oracle | `soffice_oracle.rs` (we write, soffice reads) | writing real-world files |
| I5 | Buffer/bridge round-trips | Xvfb `cargo test -p <app> bridge` | model ⇄ widget translation |
| I6 | AT-SPI smoke tests | `tests/gui/test_smoke.py` | app-level behavior, input |
| I7 | VLM visual audit | scheduled, non-gating | rendering/HIG regressions |

Rule of thumb: every feature needs I1; anything that persists needs I2–I4;
anything interactive needs I5 or I6.

---

## Letters (word processor)

### Tier 1 — Core (daily-driver writing)

| Feature | Status | Proven by |
|---|---|---|
| Styled runs (b/i/u/s, highlight, inline code) | ✅ | I1 model, I2 docx 12/12, I3 104/104, I4, I5 |
| Headings 1–6 | ✅ | I1, I2, I3, I5 |
| Paragraph alignment | ✅ | I1–I5 |
| Bullet/numbered lists (flat) | ✅ model/docx, ❌ bridge | I1–I3; **red I5** (buffer keeps literal "- ") |
| Hyperlinks | ✅ | I2, I5 (dynamic link:<url> tags) |
| Code blocks | ✅ | I1, I2 (CommonMark fenced 24/29), I3 |
| Markdown save/load with formatting | ✅ | I2 CommonMark ratchet 594/652 → target 630+ |
| DOCX save/load | ✅ | I2, I3, I4 |
| Undo/redo | ✅ (buffer-level) | **move to model ops + I1**; I6 smoke |
| Find & replace | ✅ UI | **extract to core + I1**; I6 |
| Word count | ✅ | I6 smoke (live) |
| Spell check | ❌ dictionaries missing | I6: misspelling gets a squiggle (needs Flatpak dicts, task #8) |
| Print / PDF export | ✅ Typst CLI | I1 typst-source assertion + PDF-validity check (phase 4) |
| Inline images | ❌ (markdown text only) | red I2: image survives docx round-trip; I3 scenario `<img>` |

### Tier 2 — Nice-to-have (rounds out the product)

| Feature | How to test |
|---|---|
| Tables in documents (real model, not flattening) | I1 table model; I2 fixture round-trip; I3 scenarios (structure, not just text — replace today's flatten-checks); I4 |
| Named paragraph styles (Title, Quote…) | I1 style registry; I3: LO styles map to ours and back |
| Font size / family / color per run | I1 model fields; I3 (`<font>`, css sizes); I4 |
| Superscript / subscript | I1 + I3 (`vert_align` getter already exists in rdocx) |
| Headers & footers with fields ({page}) | I2 docx sectPr round-trip; I7 visual |
| Page setup (size, margins, breaks) | I1 layout math; I2 page-break-before round-trip (regressed — red test exists in commit log) |
| Block quotes | I1 ParaStyle kind; I3 scenario upgrade from text-only |
| Line spacing round-trip | I2 (model has it; docx mapping missing — red) |
| ODT read/write | new I2 fixture corpus + I3/I4 via LO (its native format — oracle is authoritative) |

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
| OpenFormula function coverage | partial (83 fns) | **new I2 ratchet**: table-driven suite keyed to OpenFormula "Small" then "Medium" group; scorecard = implemented/total |
| XLSX round-trip | ✅ | I1 io tests, I4 Calc oracle |
| ODS / CSV / TSV import | ✅ | I1; add I3-style: LO-authored ods/xlsx read |
| Number formats (currency, %, date) | ✅ core | I1 format.rs; **red I2**: formats survive xlsx round-trip (currently values only) |
| Undo/redo | ✅ | I1 (12 tests) |
| Multi-sheet | ✅ model | I1; **red I4**: sheet names/count survive in Calc |
| Sort, cell borders, merge, validation | ✅ model | I1; add I4 for visual attrs surviving |

### Tier 2 — Nice-to-have

| Feature | How to test |
|---|---|
| Formulas surviving save (not just values) | red I2/I4: `=SUM(A1:B2)` reopens as a formula in Calc |
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
| **LO-authored parity corpus for Decks** | ✅ 8/8 | decks-core/tests/lo_parity.rs (pptx through-the-oracle, ratcheted) |

### Tier 2 — Nice-to-have

| Feature | How to test |
|---|---|
| Styled text inside text boxes (runs, not plain) | I1: reuse letters-core RunStyle in decks-core; I2 pptx round-trip |
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
