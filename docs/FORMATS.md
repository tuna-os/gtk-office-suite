# File formats

What each app opens and saves. Each app's core crate declares this list once
(`letters_core::save::FORMATS`, `tables_core::io::FORMATS`,
`decks_core::FORMATS`). The Open dialog's filter is built from it, and a test
in each crate fails when these tables or the app's desktop entry
(`flatpak/org.tunaos.<app>.desktop`, whose `MimeType` is what a file manager
offers the app for) say something else.

A format an app opens but does not save is opened read-only in effect: saving
asks for a new name in a format the app writes, and the original file is never
overwritten with another format's bytes. How much of each format survives a
round trip is in [INTEROP-EVIDENCE.md](INTEROP-EVIDENCE.md) and
[PARITY.md](PARITY.md).

## Letters

| Format | Extensions | MIME type | Opens | Saves |
|---|---|---|---|---|
| OpenDocument Text | `.odt` | `application/vnd.oasis.opendocument.text` | yes | yes |
| Word document | `.docx` | `application/vnd.openxmlformats-officedocument.wordprocessingml.document` | yes | yes |
| Markdown | `.md`, `.markdown` | `text/markdown` | yes | yes |
| HTML | `.html`, `.htm` | `text/html` | yes | yes |
| Plain text | `.txt`, `.text` | `text/plain` | yes | yes |

A file with any other extension opens as Markdown. A save to any other
extension is refused, naming the ones Letters can write.

## Tables

| Format | Extensions | MIME type | Opens | Saves |
|---|---|---|---|---|
| Excel workbook | `.xlsx` | `application/vnd.openxmlformats-officedocument.spreadsheetml.sheet` | yes | yes |
| Excel macro-enabled workbook | `.xlsm` | `application/vnd.ms-excel.sheet.macroEnabled.12` | yes | no |
| Excel 97–2003 workbook | `.xls` | `application/vnd.ms-excel` | yes | no |
| OpenDocument Spreadsheet | `.ods` | `application/vnd.oasis.opendocument.spreadsheet` | yes | no |
| Comma-separated values | `.csv` | `text/csv` | yes | no |
| Tab-separated values | `.tsv` | `text/tab-separated-values` | yes | no |

Any other extension, `.xlsb` included, is refused with a reason. `.xlsm` is
not saved because the writer produces a plain workbook and would drop the
macros.

## Decks

| Format | Extensions | MIME type | Opens | Saves |
|---|---|---|---|---|
| PowerPoint presentation | `.pptx` | `application/vnd.openxmlformats-officedocument.presentationml.presentation` | yes | yes |
| OpenDocument Presentation | `.odp` | `application/vnd.oasis.opendocument.presentation` | yes | yes |

A save to any other extension is refused.
