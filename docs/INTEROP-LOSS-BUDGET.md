# Document Interoperability Loss Budget

**Status**: Feature parity audit and loss documentation  
**Horizon**: Q4 2026 production release  
**Owners**: quality (verification), architect (design)  
**Last updated**: 2026-09-30

---

## Executive Summary

gtk-office-suite implements measured parity against LibreOffice formats (DOCX/ODT, XLSX/ODS, PPTX/ODP). This document audits where round-trip fidelity is incomplete, why, and how users can recover lost data.

**Philosophy**: Rather than claim full compatibility, we publish an explicit loss budget — users know exactly what they're trading off, and can make informed adoption decisions.

---

## Scope & Definitions

### Format Coverage

| Format | Status | Loss Type | Auditor |
|---|---|---|---|
| DOCX (Microsoft Word 2007+) | ⚠️ Partial | Tracked changes, embedded OLE | quality |
| ODT (OpenDocument Text) | ⚠️ Partial | Fields, master pages, form controls | quality |
| Markdown (CommonMark 0.30) | ✅ Full | N/A | quality |
| XLSX (Microsoft Excel 2007+) | ⚠️ Partial | Pivot tables, data validation, charts | quality |
| ODS (OpenDocument Spreadsheet) | ⚠️ Partial | Named ranges, pivot tables | quality |
| CSV / TSV | ✅ Full (1-way) | No styling (by design) | quality |
| PPTX (Microsoft PowerPoint 2007+) | ⚠️ Partial | Animations, OLE objects, 3D shapes | quality |
| ODP (OpenDocument Presentation) | ⚠️ Partial | Animations, custom slide layouts | quality |

### Loss Categories

- **🔴 Unsupported**: Feature not implemented; data is deleted on round-trip.
- **⚠️ Degraded**: Feature partially supported; some data converts to simpler form (e.g., tracked changes → comments).
- **✅ Supported**: Feature round-trips with full fidelity.
- **💾 Recoverable**: Data survives in alternate layer (e.g., original as backup).

---

## Letters (Word Processor) Loss Budget

### DOCX Format

| Feature | Status | Loss Type | Recovery | Evidence |
|---|---|---|---|---|
| **Basic formatting** (bold, italic, underline, color) | ✅ Supported | None | — | `tests/corpus_debug.rs::test_docx_text_style` |
| **Paragraph styles** (Heading, Body Text, etc.) | ✅ Supported | None | — | `tests/corpus_debug.rs::test_docx_styles` |
| **Lists** (ordered, unordered, nested) | ✅ Supported | None | — | `tests/corpus_debug.rs::test_docx_lists` |
| **Tables** (basic rows/columns) | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::letters_table` |
| **Hyperlinks** | ✅ Supported | None | — | `tests/corpus_debug.rs::test_docx_links` |
| **Footnotes/endnotes** | ⚠️ Partial | Converted to inline text | User must re-insert | `tests/corpus_debug.rs::test_docx_notes` |
| **Track changes** (insertions, deletions) | 🔴 Unsupported | Converted to inline comments | Reapply in LibreOffice | Issue #447 |
| **Comments** (review mode) | ⚠️ Partial | Converted to inline text blocks | Re-enter in LibreOffice | Issue #448 |
| **Embedded images** | ✅ Supported | None (for common formats) | — | `tests/snapshot_fidelity.rs::letters_images` |
| **Embedded objects** (OLE: charts, spreadsheets) | 🔴 Unsupported | Deleted; placeholder left | Re-embed in LibreOffice | Issue #449 |
| **Headers/footers** | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::letters_headers_footers` |
| **Page breaks** | ✅ Supported | None | — | `tests/corpus_debug.rs::test_docx_pagination` |
| **Sections** (different headers per section) | ⚠️ Partial | Collapsed to single section | Rebuild in LibreOffice | Issue #450 |
| **Fields** (date, page #, TOC) | 🔴 Unsupported | Static snapshot of field value | Manual update or rebuild | Issue #451 |
| **Complex nested tables** | ⚠️ Partial | Flattened; merged cells lose context | Rebuild cell merges | `tests/corpus_debug.rs::test_docx_table_merge` |

### ODT Format

| Feature | Status | Loss Type | Recovery |
|---|---|---|---|
| **Basic text + formatting** | ✅ Supported | None | — |
| **Styles** (paragraph, character, list) | ✅ Supported | None | — |
| **Tables** | ✅ Supported | None | — |
| **Hyperlinks** | ✅ Supported | None | — |
| **Images** | ✅ Supported | None | — |
| **Fields** (date, page #) | 🔴 Unsupported | Static snapshot | Manual update |
| **Master pages** | 🔴 Unsupported | Removed; default page style used | Rebuild layout |
| **Form controls** (text box, checkbox) | 🔴 Unsupported | Deleted; text content preserved | Re-insert controls |
| **Change tracking** | ⚠️ Partial | Converted to comments | Reapply |

### Markdown (CommonMark)

| Feature | Status | Loss Type |
|---|---|---|
| **Paragraphs, headings, lists** | ✅ Supported | None |
| **Bold, italic, code** | ✅ Supported | None |
| **Links, images** | ✅ Supported | None |
| **Blockquotes, code blocks** | ✅ Supported | None |
| **HTML passthrough** | ⚠️ Partial | Sanitized; scripts removed | By design (security) |
| **Tables** (GFM extension) | ✅ Supported | None |

---

## Tables (Spreadsheet) Loss Budget

### XLSX Format

| Feature | Status | Loss Type | Recovery | Evidence |
|---|---|---|---|---|
| **Cell values** (text, number, date) | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::tables_values` |
| **Number formats** | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::tables_number_formats` |
| **Basic formatting** (bold, color, fill) | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::tables_formatting` |
| **Formulas** (arithmetic, SUM, IF, VLOOKUP) | ✅ Supported (OpenFormula subset) | None | — | `tests/openformula.rs` (107/107 passing) |
| **Conditional formatting** (color scales, data bars) | ⚠️ Partial | Converted to static cell formatting | Re-apply rules in Calc | Issue #452 |
| **Data validation** (drop-down, range checks) | 🔴 Unsupported | Removed; user can enter invalid data | Re-add validation | Issue #453 |
| **Charts** (bar, line, scatter, pie) | 🔴 Unsupported | Deleted; data table preserved | Re-create chart | Issue #454 |
| **Pivot tables** | 🔴 Unsupported | Converted to static data snapshot | Rebuild pivot | Issue #455 |
| **Merged cells** | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::tables_merged` |
| **Frozen rows/columns** | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::tables_freeze` |
| **Multiple sheets** | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::tables_sheets` |
| **Named ranges** | ⚠️ Partial | Names preserved but formula references may break | Update formula references | Issue #456 |
| **VBA macros** | 🔴 Unsupported | Deleted; no execution | Rewrite in LibreOffice / Python | By design (security) |
| **Embedded objects** (charts, shapes, images) | ⚠️ Partial | Images preserved; OLE objects deleted | Re-embed shapes | Issue #457 |

### ODS Format

| Feature | Status | Loss Type | Recovery |
|---|---|---|---|
| **Cell values + formatting** | ✅ Supported | None | — |
| **Formulas** (OpenFormula) | ✅ Supported | None | — |
| **Sheets, freezing, merges** | ✅ Supported | None | — |
| **Named ranges** | ⚠️ Partial | Names preserved; scope may differ | Verify and update |
| **Pivot tables** | 🔴 Unsupported | Converted to static data | Rebuild |
| **Database ranges** | 🔴 Unsupported | Removed | Re-define |
| **Conditional formatting** | ⚠️ Partial | Simplified to static formatting | Re-apply |
| **Charts** | 🔴 Unsupported | Deleted; data preserved | Recreate |

### CSV / TSV

| Feature | Status | Loss Type |
|---|---|---|
| **Raw cell values** | ✅ Supported | None |
| **Formatting** (colors, bold) | 🔴 Unsupported (by design) | N/A |
| **Formulas** | 🔴 Unsupported (by design) | N/A |
| **Multiple sheets** | 🔴 Unsupported (1 sheet per file) | N/A |

---

## Decks (Presentation) Loss Budget

### PPTX Format

| Feature | Status | Loss Type | Recovery | Evidence |
|---|---|---|---|---|
| **Slides** (content, layout) | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::decks_slides` |
| **Text boxes** | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::decks_text` |
| **Shapes** (rectangles, circles, lines) | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::decks_shapes` |
| **Images** | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::decks_images` |
| **Bullet lists** | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::decks_bullets` |
| **Text formatting** (bold, color, font size) | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::decks_text_style` |
| **Master slides** | ⚠️ Partial | Master layout used but custom CSS ignored | Re-style in Impress | Issue #458 |
| **Animations** (entrance, emphasis, exit) | 🔴 Unsupported | Deleted; no playback on export | Rebuild animations | Issue #459 |
| **Transitions** (slide dissolves, wipes) | 🔴 Unsupported | Removed; static playback | Re-add transitions | Issue #460 |
| **OLE objects** (embedded video, spreadsheet) | 🔴 Unsupported | Deleted; placeholder left | Re-embed | Issue #461 |
| **3D shapes** | 🔴 Unsupported | Converted to 2D image | Rebuild with 3D tool |
| **Notes pages** | ⚠️ Partial | Notes preserved but formatting simplified | Re-apply styling |
| **Speaker notes** | ✅ Supported | None | — | `tests/snapshot_fidelity.rs::decks_notes` |

### ODP Format

| Feature | Status | Loss Type | Recovery |
|---|---|---|---|
| **Slides, text, shapes, images** | ✅ Supported | None | — |
| **Bullet lists, formatting** | ✅ Supported | None | — |
| **Master slides** | ⚠️ Partial | Layout used; custom styling lost | Re-style |
| **Animations** | 🔴 Unsupported | Deleted | Rebuild |
| **Transitions** | 🔴 Unsupported | Removed | Re-add |
| **Draw shapes** (advanced connectors) | ⚠️ Partial | Simplified to basic shapes | Rebuild |

---

## Unsupported Feature Inspector UI

When saving a document containing unsupported features, users see:

```
⚠️ This document contains features that cannot be saved

The following features will be removed or degraded:

• Tracked changes (5 edits)
  → Will be converted to comments
  [Learn more]

• Embedded OLE objects (2)
  → Will be removed
  [Learn more]

Do you want to:
  [Save anyway]  [View in LibreOffice]  [Cancel]
```

**Implementation**: `suite-common/src/unsupported_feature_inspector.rs`

### Feature Registry

```rust
pub enum UnsupportedFeature {
    TrackedChanges(usize),
    EmbeddedOLE(usize),
    ChartObject(usize),
    Animation(usize),
    VBAMacro(usize),
    FormControl(usize),
    // ...
}

impl UnsupportedFeature {
    pub fn reason(&self) -> &str;
    pub fn recovery(&self) -> &str;
}
```

**Test**: `tests/gui/test_unsupported_feature_inspector.py`

---

## Conformance Corpus & Fidelity Metrics

### Corpus Structure

```
interop/
├── corpus.json          # Index of test files + expected loss
├── packages/
│   ├── docx/            # 50 representative .docx files
│   ├── odt/             # 30 representative .odt files
│   ├── xlsx/            # 40 representative .xlsx files
│   ├── ods/             # 30 representative .ods files
│   ├── pptx/            # 20 representative .pptx files
│   └── odp/             # 20 representative .odp files
└── results/
    ├── letters_docx_fidelity.json
    ├── tables_xlsx_fidelity.json
    ├── decks_pptx_fidelity.json
    └── ...
```

### Fidelity Scoring

Each file round-tripped through gtk-office-suite → LibreOffice. Score computed:

```
fidelity = (features_preserved / features_present) * 100
```

**Target for v2.2.0**:
- Letters DOCX: ≥85% fidelity on conformance corpus
- Tables XLSX: ≥80% fidelity on conformance corpus
- Decks PPTX: ≥75% fidelity on conformance corpus

**Test**: `tests/parity_conformance.rs` runs on every commit; dashboard at `ci.example.com/interop-dashboard`.

---

## Release Sign-Off

### Quality Checklist

- [ ] Conformance corpus fidelity scores ≥target (checked in CI)
- [ ] Unsupported feature inspector UI functional
- [ ] Loss budget document published in docs/
- [ ] Release notes enumerate unsupported features per format
- [ ] User guide documents recovery paths (link to this file)

### Architect Review

- [ ] Loss budget philosophically acceptable (quality trade-offs justified)
- [ ] No regressions in supported features
- [ ] Inspector messaging clear to users

---

## Success Criteria

By **2026-12-31**:

- ✅ Conformance corpus passing at target fidelity
- ✅ Unsupported feature inspector shipped and tested
- ✅ Loss budget document published + linked from README
- ✅ Release notes clearly state: "Measured parity — see INTEROP-LOSS-BUDGET.md for known limitations"
- ✅ Zero data loss during round-trip on supported features

**User-facing success**: "I understand exactly what LibreOffice features gtk-office-suite cannot preserve, and I can make an informed choice about which application to use for my workflow."

---

## References

- PARITY.md — detailed conformance matrix
- RENDER-PARITY-ROADMAP.md — visual rendering fidelity roadmap
- docs/readiness-2026-09/interoperability.md — Q4 readiness audit
