# Headless Document Conversion CLI Specification (`suite-convert`)

## Overview

`suite-convert` is a headless CLI utility for batch converting ODF (`.odt`, `.ods`, `.odp`), OpenXML (`.docx`, `.xlsx`, `.pptx`), PDF, and Markdown documents without requiring a GTK display server or GUI main loop.

## Architecture

1. **`suite-common-core`**: Core parser and rendering AST engines.
2. **Headless Engine**: `suite-convert` CLI binary executing format conversion pipelines cleanly.
3. **Format Adapters**:
   - `odt` / `docx` / `markdown` text document transformers.
   - `ods` / `xlsx` spreadsheet recalculation and CSV/PDF export.
   - `odp` / `pptx` presentation slide extractors.

## Usage & Interface

```bash
suite-convert --from <format> --to <format> --input <file> --output <file>
```

## Related

Tracking Issue: #854
