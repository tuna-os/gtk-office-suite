# Package parts the apps don't model

Each app writes a document's whole package from its model. Without help, a save drops every part of the source package that the model doesn't hold. This page records, for each kind of such part, whether a save after an edit carries it or asks about it first (#1274). Nothing is passed through half-consistent.

**Carried** parts are safe to keep as they were, whatever the edit: nothing the app rewrites refers to them, and they refer to nothing the app rewrites. `suite_common::carry` reads them from the file the document came from, before the save replaces it. It then adds them to the bytes the writer produced, with their content types, package relationships or manifest entries, and replaces the file once (`Carried::write_with`). Every save path in all three apps goes through it (see [SAVE-PATHS.md](SAVE-PATHS.md)).

**Asked** parts are referenced by content the app rewrites (a slide, a sheet, the document body), or refer into it. Keeping them would leave a relationship pointing at a part that changed or is gone. So the save asks first: "Save Without This Content?". Cancel writes nothing, and Save Anyway writes without them.

## Decisions

| App | Family | Part | Decision | Why | Where |
|---|---|---|---|---|---|
| all | OOXML | `customXml/**` (data parts, their properties and `_rels`) | carried | its relationships stay inside `customXml/` | `suite_common_core::carry` |
| all | OOXML | `docProps/thumbnail.*` | carried | the preview image, related only from `_rels/.rels` | `suite_common_core::carry` |
| all | OOXML | `docProps/custom.xml` | carried | custom document properties, related only from `_rels/.rels` | `suite_common_core::carry` |
| all | ODF | `Thumbnails/**` | carried | the preview image, listed only in the manifest | `suite_common_core::carry` |
| all | ODF | `settings.xml` | carried when the writer wrote none | view settings; nothing refers to them | `suite_common_core::carry` |
| letters | OOXML | `word/vbaProject*` | asked: Macros | the body's part relationships name it | `letters_core::loss` |
| letters | OOXML | `word/embeddings/**` | asked: Embedded objects | referenced from the body | `letters_core::loss` |
| letters | OOXML | `word/diagrams/**` | asked: SmartArt graphics | referenced from the body | `letters_core::loss` |
| letters | OOXML | `word/charts/**` | asked: Charts | referenced from the body | `letters_core::loss` |
| letters | ODF | `Basic/**`, `Scripts/**` | asked: Macros | bound to the document's events | `letters_core::loss` |
| letters | ODF | `Object N/**`, `ObjectReplacements/**` | asked: Embedded objects | `draw:object` frames in the body | `letters_core::loss` |
| tables | both | macros, pivot tables, slicers, threaded comments, formatted tables, sparklines, external links, data connections, embedded objects, pictures, validation rules xlsx can't express | asked | referenced from sheets or the workbook | `tables_core::io::loss` |
| decks | both | comments, audio and video, embedded objects, SmartArt, emphasis and motion-path animations, pictures missing from the file | asked | referenced from slides | `decks_core::loss` |

The rest of a package is what the writers regenerate from the model: document, styles, theme, numbering, workbook and sheets, slides, layouts and masters, `docProps/core.xml` and `app.xml`. Those parts are rewritten, not lost; what the model doesn't hold of them is the format question, not this one.

## Tests

- `suite-common-core/src/carry.rs`: carrying OOXML and ODF parts with their types, relationships and manifest entries; nothing crossing families or going into a plain-text save; the single atomic replace, which leaves the target untouched while the writer runs and leaves no staging file behind; `problems` finding a dangling relationship, an override for a missing part and a part with no type.
- `letters-core/tests/package_consistency.rs`, `decks-core/tests/package_consistency.rs`, `tables-core/tests/package_consistency.rs`: each writer's own packages have no `problems`.
- `letters-core/src/loss.rs`: the parts Letters asks about, and none for a file it wrote.
- GUI journeys in `tests/gui/test_smoke.py`. Each opens a package with a custom XML part and a thumbnail, edits it, saves with Ctrl+S, and checks that the parts are there byte for byte and that the package's types, relationships and parts agree:
  - `LettersOpaquePartsSmoke`
  - `TablesOpaquePartsSmoke`
  - `DecksOpaquePartsSmoke`
- `LettersUnreadContentSmoke`: a docx with a macro project asks first; Cancel keeps the file's bytes; Save Anyway drops the macro and still carries the safe parts.
