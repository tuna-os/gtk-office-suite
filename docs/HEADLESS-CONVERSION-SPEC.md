# Headless Document Conversion CLI (`suite-convert`) Specification

**Target Milestone**: Q4 2026 (P1)  
**Tracking Issue**: #579  
**Status**: Specification Draft  

---

## 1. Overview

`suite-convert` is a dedicated, headless command-line tool designed for batch document conversion, format validation, and server/pipeline automation. Built directly on `suite-common-core`, `letters-core`, `tables-core`, and `decks-core`, `suite-convert` operates without initializing GTK4 or libadwaita UI components, offering sub-second execution speeds and minimal memory overhead.

This binary serves as a lightweight alternative to `soffice --headless` for Linux desktop scripting, CI/CD document validation pipelines, and enterprise automation workflows.

---

## 2. Supported Formats & Conversion Matrix

| Document Category | Input Formats | Output Formats | Loss Budget Policy |
|-------------------|---------------|----------------|-------------------|
| **Text Documents** | `.md` (CommonMark), `.odt` (ODF 1.3), `.docx` | `.odt`, `.docx`, `.md`, `.pdf`, `.html` | Full style fidelity reported via loss inspector |
| **Spreadsheets** | `.csv`, `.ods` (ODF 1.3) | `.ods`, `.csv`, `.pdf`, `.html` | Formulas preserved via `IronCalc` evaluation engine |
| **Presentations** | `.odp` (ODF 1.3), `.pptx` | `.odp`, `.pptx`, `.pdf` | Slide layout and vector elements rendered losslessly |

---

## 3. Command Line Interface (CLI)

```bash
suite-convert [OPTIONS] <INPUT_FILE>
```

### Options

- `-f, --from <FORMAT>`: Explicitly set input format (`md`, `odt`, `docx`, `csv`, `ods`, `odp`, `pptx`). Inferred from file extension if omitted.
- `-t, --to <FORMAT>`: Explicitly set output target format (`pdf`, `odt`, `docx`, `html`, `csv`, `ods`, `odp`).
- `-o, --output <PATH>`: Output file path. Use `-` for stdout.
- `--batch <PATTERN>`: Convert multiple files matching glob pattern.
- `--report-loss <json|text>`: Output structural and styling loss analysis to stderr or JSON file.
- `--fail-on-loss <LEVEL>`: Exit with code 3 if format loss exceeds severity (`none`, `minor`, `major`).
- `-v, --verbose`: Enable detailed diagnostic logging.
- `-h, --help`: Print help information.
- `-V, --version`: Print version information.

---

## 4. Architectural Design

```
+-------------------------------------------------------------------+
|                        suite-convert CLI                          |
+-------------------------------------------------------------------+
                                  |
         +------------------------+------------------------+
         |                        |                        |
         v                        v                        v
+------------------+    +-------------------+    +------------------+
|   letters-core   |    |    tables-core    |    |    decks-core    |
| (ODT/DOCX/MD)    |    | (ODS/CSV/Formula) |    |   (ODP/PPTX)     |
+------------------+    +-------------------+    +------------------+
         |                        |                        |
         +------------------------+------------------------+
                                  |
                                  v
+-------------------------------------------------------------------+
|                           suite-export                            |
|             (PDF / HTML5 / Vector Rendering Engine)               |
+-------------------------------------------------------------------+
```

1. **GTK-Free Execution**: Zero GTK/X11/Wayland dependencies. Executes in pure terminal environments and lightweight OCI containers.
2. **Core Reusability**: Shared parsing and document models from core crates guarantee 100% parity between GUI app saving and CLI conversion.
3. **Deterministic Output**: Reproducible PDF and document generation with strict seed control for CI pipelines.

---

## 5. Exit Code Semantics

| Exit Code | Name | Description |
|-----------|------|-------------|
| `0` | `SUCCESS` | Document converted successfully within loss budget tolerances. |
| `1` | `USAGE_ERROR` | Invalid flags, incompatible format combinations, or missing arguments. |
| `2` | `CONVERSION_ERROR` | File parsing failed, corrupted input document, or IO write failure. |
| `3` | `LOSS_BUDGET_EXCEEDED` | Structural or styling loss exceeded `--fail-on-loss` threshold. |

---

## 6. Enterprise & Packaging Integration

- **Flatpak Integration**: Bundled as `/app/bin/suite-convert` inside the `org.gnome.OfficeSuite` Flatpak manifest.
- **Export Sandbox Portal**: Compatible with `org.freedesktop.portal.documents` when invoked under sandboxed environments.
