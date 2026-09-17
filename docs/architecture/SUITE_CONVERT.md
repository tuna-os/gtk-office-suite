# RFC 0003: Headless CLI Document Conversion Binary (`suite-convert`)

**Author**: Strategist Agent  
**Status**: Draft  
**Target Milestone**: Q4 2026  
**Tracking Issue**: #794 (Related: #579)

---

## 1. Executive Summary

This specification defines the architecture, CLI flags, IPC interfaces, and error contracts for `suite-convert`, a standalone headless binary for document conversion within the `gtk-office-suite` ecosystem.

`suite-convert` leverages `suite-common-core` and app core engines (`letters-core`, `tables-core`, `decks-core`) without initializing GTK4 or display server context, providing container-native document conversion for server and CLI pipelines.

---

## 2. CLI Interface & Flag Specification

```text
suite-convert [OPTIONS] --to <FORMAT> <INPUT_FILE> [<OUTPUT_FILE>]
```

### Options

- `--to <FORMAT>`: Target output format (`pdf`, `odt`, `ods`, `odp`, `md`, `html`, `txt`). Required.
- `--from <FORMAT>`: Override input format detection (auto-detected by MIME/extension by default).
- `--export-options <JSON_KEY_VALUE>`: Pass format-specific rendering options (e.g., `'{"pdf_version": "PDF/A-1b", "embed_fonts": true}'`).
- `-o, --output <PATH>`: Explicit output file path (defaults to stdout if `-` specified or `<INPUT_STEM>.<FORMAT>`).
- `-v, --verbose`: Enable verbose operational logging to stderr.
- `-q, --quiet`: Suppress all non-error output.

---

## 3. Supported Conversion Matrix

| Input Format | Core Library | Supported Target Formats |
|---|---|---|
| OpenDocument Text (`.odt`) | `letters-core` | PDF, CommonMark (`.md`), Plain Text (`.txt`), HTML |
| CommonMark (`.md`) | `letters-core` | ODT, PDF, HTML, Plain Text |
| OpenDocument Spreadsheet (`.ods`) | `tables-core` | PDF, CSV, TSV, HTML |
| OpenDocument Presentation (`.odp`) | `decks-core` | PDF, PNG (slide rendering), SVG |

---

## 4. Process Model & Exit Codes

`suite-convert` executes strictly headless and exits with standard process status codes:

- `0`: Conversion successful.
- `1`: Invalid CLI parameters or unsupported format pairing.
- `2`: Document parsing/decoding error (corrupt document structure).
- `3`: Conversion engine failure or rendering layout error.
- `4`: I/O error (file missing, permission denied, output directory unwritable).

---

## 5. Security & Isolation

- **Headless Guarantee**: Never links against GTK/Wayland/X11 display context; safe for minimal CI runner environments.
- **Resource Ceilings**: Strict timeout bounds and allocation limits on document parsing loops.
