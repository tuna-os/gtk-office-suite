# ADR 0012: Shared document primitives, application-neutral clipboard boundary, and sibling-core dependency rule

Date: 2026-10-25 · Status: **accepted** · Relates to: [ADR-0001](0001-core-shell-split.md), [ADR 0011](0011-one-op-history.md), [RFC-0001](../rfc/0001-crdt-collaboration.md), Issue [#284](https://github.com/tuna-os/gtk-office-suite/issues/284)

## Context

The core/shell split (ADR-0001) organizes each application into two layers: a GTK-free `*-core` crate owning models, formats, and algorithms, and a shell binary crate handling GTK4/libadwaita views and signal wiring. ADR-0001 specifies that all three core crates (`letters-core`, `tables-core`, `decks-core`) are peer persistence adapters on top of shared primitives in `suite-common-core`.

However, an architecture finding (#284) revealed an interface coupling violation:
- `tables-core/src/fragment.rs` re-exports `letters_core::fragment::{Fragment, GridCell, MIME}`.
- `decks-core` imports `letters_core::model::{Run, RunStyle, Paragraph}` throughout its model, ODP parser/writer, notes, shape XML, and fragment code.
- `decks-core/src/fragment.rs` re-exports the Letters clipboard `Fragment` contract.
- Both `tables-core/Cargo.toml` and `decks-core/Cargo.toml` declare direct dependencies on `letters-core`.

This creates dependency edges:
```
tables-core  ──►  letters-core
decks-core   ──►  letters-core
```
Even though Letters is a sibling application, not a shared foundation. As a consequence:
1. **Unnecessary dependency baggage:** Tables and Decks inherit the Letters format stack (`rdocx`, DOCX/ODT/Markdown parsers, and ZIP/XML dependencies) solely to obtain generic rich-text runs and clipboard data structures.
2. **Coupled maintenance:** Changes to the Letters document model or its Writer-specific formatting structures risk breaking or forcing rebuilds in Tables and Decks.
3. **Publication barrier:** `tables-core` is marked as a published crate in `docs/ARCHITECTURE.md`, yet it cannot be cleanly published or embedded without pulling in an unpublished word processor crate (`letters-core`).
4. **Collaboration contract ambiguity:** RFC-0001 and ADR 0011 establish unified op and history representations for collaboration across all three apps. Rich-text runs and paragraphs shared between Decks and Letters form part of the collaboration contract and must have neutral, canonical ownership.

## Decision

### 1. Sibling-Core Independence Invariant

**Sibling application core crates must never depend on one another.**

```
                      ┌──────────────────────┐
                      │  suite-common-core   │
                      │  (pure Rust, shared) │
                      └──────────▲───────────┘
                                 │
            ┌────────────────────┼────────────────────┐
            │                    │                    │
  ┌─────────┴────────┐ ┌─────────┴────────┐ ┌─────────┴────────┐
  │   letters-core   │ │   tables-core    │ │    decks-core    │
  │   (Letters doc)  │ │   (Sheet model)  │ │   (Slide model)  │
  └─────────▲────────┘ └─────────▲────────┘ └─────────▲────────┘
            │                    │                    │
  ┌─────────┴────────┐ ┌─────────┴────────┐ ┌─────────┴────────┐
  │     letters      │ │      tables      │ │      decks       │
  │   (GTK4 shell)   │ │   (GTK4 shell)   │ │   (GTK4 shell)   │
  └──────────────────┘ └──────────────────┘ └──────────────────┘
```

The valid dependency graph is strictly hierarchical:
- `suite-common-core` depends only on generic external utility crates.
- `letters-core`, `tables-core`, and `decks-core` may depend on `suite-common-core` (and `suite-export`).
- No core crate may depend on a sibling core crate (`tables-core ↛ letters-core`, `decks-core ↛ letters-core`, `letters-core ↛ tables-core`, etc.).
- Shell binaries depend on their corresponding core crate and `suite-common`.

This invariant is codified and checked by automated tests in `tests/test_core_shell_split.py`.

### 2. Canonical Primitive Location

Application-neutral rich-text primitives (`Run`, `RunStyle`, `Paragraph`, `ParaStyle`), clipboard data structures (`Fragment`, `GridCell`), and MIME definitions belong in **`suite-common-core`** (under a new module `suite_common_core::document`).

A dedicated `suite-document-model` crate was considered but rejected: `suite-common-core` already holds shared undo commands (`ops.rs`), number formats (`format.rs`), style cascades (`style.rs`), property pools (`props.rs`), and unit metrics (`units.rs`). Adding document primitives to `suite-common-core` avoids crate proliferation and keeps shared types within the established, published GTK-free crate.

### 3. Field-Level Ownership and Classification

The candidate types currently located in `letters-core` combine genuinely neutral formatting concepts with word-processing-specific fidelity fields. Moving these types wholesale would leak Letters layout and pagination logic into `suite-common-core`.

Every field is therefore explicitly categorized into one of three tiers:
- **Tier 1 (Neutral Shared Contract):** Lives canonically in `suite-common-core::document`. Used by Letters, Decks, and Tables.
- **Tier 2 (Letters Application Extension):** Belongs to `letters-core`. Preserved in `letters-core::model` or serialized via backwards-compatible extension mechanisms.
- **Tier 3 (Application Boundary Conversion):** Conversion functions that translate between application-specific representations and neutral wire DTOs.

#### Type: `Run` and `RunStyle`

| Struct | Field | Type | Tier | Classification & Destination |
|---|---|---|---|---|
| `Run` | `text` | `String` | Tier 1 | Shared neutral text content (`suite-common-core`) |
| `Run` | `style` | `RunStyle` | Tier 1 | Shared neutral style descriptor (`suite-common-core`) |
| `RunStyle` | `bold` | `bool` | Tier 1 | Shared character styling (`suite-common-core`) |
| `RunStyle` | `regular` | `bool` | Tier 1 | Shared override for bold heading contexts |
| `RunStyle` | `italic` | `bool` | Tier 1 | Shared character styling (`suite-common-core`) |
| `RunStyle` | `caps` | `bool` | Tier 1 | Shared character capitalization (`suite-common-core`) |
| `RunStyle` | `underline` | `bool` | Tier 1 | Shared character styling (`suite-common-core`) |
| `RunStyle` | `strikethrough` | `bool` | Tier 1 | Shared character styling (`suite-common-core`) |
| `RunStyle` | `highlight` | `bool` | Tier 1 | Shared text highlight flag (`suite-common-core`) |
| `RunStyle` | `code` | `bool` | Tier 1 | Shared inline monospace code span (`suite-common-core`) |
| `RunStyle` | `link` | `Option<String>` | Tier 1 | Shared hyperlink target URI (`suite-common-core`) |
| `RunStyle` | `font_family` | `Option<String>` | Tier 1 | Shared typeface family name (`suite-common-core`) |
| `RunStyle` | `font_size_hp` | `Option<u16>` | Tier 1 | Shared font size in half-points (24 = 12pt) |
| `RunStyle` | `color` | `Option<String>` | Tier 1 | Shared hex color (RRGGBB) (`suite-common-core`) |
| `RunStyle` | `vert_align` | `Option<VertAlign>` | Tier 1 | Shared sub/superscript (`suite-common-core`) |
| `RunStyle` | `html` | `bool` | Tier 2 | Letters Markdown raw HTML pass-through |
| `RunStyle` | `letter_spacing_tw`| `Option<i32>` | Tier 2 | Letters typography spacing (twips) |
| `RunStyle` | `position_hp` | `Option<i32>` | Tier 2 | Letters vertical baseline shift (half-points) |
| `RunStyle` | `footnote` | `Option<usize>` | Tier 2 | Letters footnote document index |
| `RunStyle` | `chip` | `Option<Chip>` | Tier 2 | Letters smart chip object |
| `RunStyle` | `revision` | `Option<Revision>` | Tier 2 | Letters tracked changes metadata |
| `RunStyle` | `comments` | `Vec<u32>` | Tier 2 | Letters document comment anchor IDs |
| `RunStyle` | `image` | `Option<String>` | Tier 2 | Letters inline image URI / alt text |
| `RunStyle` | `image_extent_emu` | `Option<(u64,u64)>`| Tier 2 | Letters image EMU dimensions |
| `RunStyle` | `image_sides_emu` | `Option<(u64,u64)>`| Tier 2 | Letters image margin margins |
| `RunStyle` | `image_anchor` | `Option<ImageAnchor>` | Tier 2 | Letters floating object page anchor |
| `RunStyle` | `text_box` | `Option<Box<TextBox>>`| Tier 2 | Letters floating text box frame |

#### Type: `Paragraph` and `ParaStyle`

| Struct | Field | Type | Tier | Classification & Destination |
|---|---|---|---|---|
| `Paragraph` | `runs` | `Vec<Run>` | Tier 1 | Shared run list (`suite-common-core`) |
| `Paragraph` | `style` | `ParaStyle` | Tier 1 | Shared paragraph style descriptor (`suite-common-core`) |
| `ParaStyle` | `alignment` | `Alignment` | Tier 1 | Shared alignment: Left, Center, Right, Justify |
| `ParaStyle` | `list` | `ListKind` | Tier 1 | Shared list category: None, Bullet, Numbered |
| `ParaStyle` | `list_level` | `u8` | Tier 1 | Shared list indentation nesting level (0..=8) |
| `ParaStyle` | `line_spacing` | `f32` | Tier 1 | Shared proportional line height factor (1.0..=2.0) |
| `ParaStyle` | `space_before_pt`| `f64` | Tier 1 | Shared vertical standoff before paragraph (pt) |
| `ParaStyle` | `space_after_pt` | `f64` | Tier 1 | Shared vertical standoff after paragraph (pt) |
| `ParaStyle` | `heading` | `Option<u8>` | Tier 1 | Shared heading level (1..=6) |
| `ParaStyle` | `named_style` | `Option<String>` | Tier 1 | Shared style reference (e.g. "Title", "Subtitle") |
| `ParaStyle` | `list_start` | `Option<u32>` | Tier 2 | Letters numbered list sequence override |
| `ParaStyle` | `list_label` | `Option<ListLabel>` | Tier 2 | Letters numbered prefix/suffix formatting |
| `ParaStyle` | `list_hanging_pt`| `Option<f64>` | Tier 2 | Letters list marker hanging indent |
| `ParaStyle` | `line_height` | `Option<LineHeight>`| Tier 2 | Letters exact/minimum line height constraints |
| `ParaStyle` | `left_indent_pt` | `f64` | Tier 2 | Letters left margin indent |
| `ParaStyle` | `right_indent_pt`| `f64` | Tier 2 | Letters right margin indent |
| `ParaStyle` | `first_line_indent_pt`| `f64` | Tier 2 | Letters first line paragraph indent |
| `ParaStyle` | `tab_stops_pt` | `Vec<TabStop>` | Tier 2 | Letters tab stop positions, alignments & leaders |
| `ParaStyle` | `code_block` | `Option<String>` | Tier 2 | Letters Markdown code block fence language |
| `ParaStyle` | `block_quote` | `bool` | Tier 2 | Letters block quote nesting |
| `ParaStyle` | `html_block` | `bool` | Tier 2 | Letters Markdown raw HTML block line |
| `ParaStyle` | `page_break_before`| `bool` | Tier 2 | Letters pagination hard page break |
| `ParaStyle` | `own_look` | `bool` | Tier 2 | Letters style inheritance bypass flag |
| `ParaStyle` | `table_cell` | `Option<TableCell>` | Tier 2 | Letters tabular document structure marker |
| `ParaStyle` | `keep_with_next` | `bool` | Tier 2 | Letters page layout flow constraint |
| `ParaStyle` | `keep_lines` | `bool` | Tier 2 | Letters page layout keep-together constraint |
| `ParaStyle` | `widow_control` | `Option<bool>` | Tier 2 | Letters orphan/widow line split control |
| `ParaStyle` | `shading` | `Option<String>` | Tier 2 | Letters background fill hex color |
| `ParaStyle` | `borders` | `ParaBorders` | Tier 2 | Letters paragraph outline rules |
| `ParaStyle` | `toc` | `Option<u8>` | Tier 2 | Letters table of contents generated entry level |

#### Type: `Fragment`, `GridCell`, and Clipboard Constants

| Struct/Constant | Item | Tier | Classification & Destination |
|---|---|---|---|
| `MIME` | `"application/x-tunaos-suite+json"` | Tier 1 | Shared suite-wide clipboard MIME type identifier |
| `Fragment` | `Text(Vec<Paragraph>)` | Tier 1 | Shared styled-text clipboard payload |
| `Fragment` | `Grid(Vec<Vec<GridCell>>)` | Tier 1 | Shared spreadsheet grid clipboard payload |
| `GridCell` | `value: String` | Tier 1 | Displayed cell string representation |
| `GridCell` | `formula: Option<String>` | Tier 1 | Uncomputed formula without leading '=' |
| `GridCell` | `num_format: Option<String>`| Tier 1 | XLSX-style number formatting code |
| Method | `to_json()`, `from_json()` | Tier 1 | Fallible serialization to canonical suite JSON |
| Method | `to_plain()` | Tier 1 | Neutral TSV / newline text export fallback |
| Method | `to_html()` | Tier 1 | Cross-suite basic HTML export fallback |
| Method | `grid_to_paragraphs()` | Tier 3 | **Letters-only conversion**: constructs `TableCell` document nodes; placed in `letters-core::fragment` |

### 4. Wire and Serialization Compatibility

The shared clipboard contract MIME is `application/x-tunaos-suite+json`. To guarantee round-trip fidelity and backward compatibility:

1. **Serialization Stability:** All shared fields use serde with `#[serde(default)]` and `#[serde(skip_serializing_if = "...")]` to omit empty, zero, or default values.
2. **Downward and Upward Compatibility:** When an older application or a non-Letters application deserializes a `Fragment::Text` payload containing Letters-specific extension fields, serde ignores unrecognized fields without error.
3. **Loss Reporting:** When pasting between applications with differing capability subsets (e.g. Decks pasting a rich-text fragment containing footnotes or smart chips), the receiving shell sanitizes unsupported features to plain text and logs the transformation (via `decks_core::fragment::Pasted.flattened`).

### 5. Phased Cutover Plan

To avoid risky monolithic code refactors across all three applications simultaneously, the extraction is executed in ordered, independent slices:

1. **Slice 1 (ADR 0012 & Ratchet Gate — Issue #284 / #1674):**
   - Document this ADR and establish the architecture boundary.
   - Update `docs/ARCHITECTURE.md`.
   - Add the sibling-core independence check to `tests/test_core_shell_split.py`, recording the two legacy edges (`tables-core -> letters-core`, `decks-core -> letters-core`) as ratcheted migration debt to prevent new violations.

2. **Slice 2 (Canonical Primitive Definitions — Issue #1675):**
   - Implement `suite_common_core::document` defining the Tier 1 canonical types (`Run`, `RunStyle`, `Paragraph`, `ParaStyle`, `Alignment`, `ListKind`).
   - Add unit tests verifying serialization compatibility, defaults, and plain-text conversion.
   - Update `letters-core` to consume or re-export the canonical types.

3. **Slice 3 (Suite Clipboard DTOs — Issue #1661):**
   - Relocate `Fragment`, `GridCell`, and `MIME` to `suite_common_core::document::fragment`.
   - Retain `grid_to_paragraphs` in `letters-core::fragment` as an application adapter.
   - Migrate `tables-core/src/fragment.rs` to import from `suite_common_core`.
   - Remove `letters-core` from `tables-core/Cargo.toml`.

4. **Slice 4 (Decks Core Migration — Issue #1662):**
   - Migrate `decks-core` call sites (`parse.rs`, `write.rs`, `shape_xml.rs`, `controller.rs`, `odp.rs`, `text_body.rs`) to consume `Run` and `RunStyle` from `suite_common_core::document`.
   - Remove `letters-core` from `decks-core/Cargo.toml`.

5. **Slice 5 (Ratchet Closure):**
   - Clear `KNOWN_SIBLING_EDGES` in `tests/test_core_shell_split.py`.
   - All three cores are completely independent peers.

## Consequences

### Positive
- `tables-core` and `decks-core` can build, test, and be published without pulling in `letters-core`, `rdocx`, or word-processor XML parsing stacks.
- Rebuilding `letters-core` no longer invalidates compilation artifacts for Tables and Decks.
- The CRDT collaboration op and history layer (RFC-0001, ADR 0011) has an unambiguous, shared foundational type set for rich text across applications.
- Automated tests prevent accidental introduction of circular or sibling crate dependencies.

### Negative / Trade-offs
- Letters-specific fields on runs (e.g. chips, revisions, text-boxes) must be modeled as application-layer extensions rather than direct fields on the shared base `RunStyle`.
- Migration requires coordinated updates across existing tests and call sites in `letters-core` and `decks-core`.
