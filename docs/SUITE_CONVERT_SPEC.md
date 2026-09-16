# `suite-convert` Headless Document Conversion CLI Specification

## Overview

`suite-convert` is a headless CLI utility binary designed to execute server-side, automated, and batch document conversions using `gtk-office-suite` core format engines (`suite-common-core`). It provides high-speed, headless document transformations without requiring a running X11 or Wayland display server.

## Architectural Objectives

1. **GTK-Free Execution Path**: Depend strictly on pure-Rust core parsing and rendering crates (`suite-common-core`), isolating headless operation from GTK UI dependencies.
2. **Deterministic CLI API**: Provide structured CLI commands, JSON stdout status flags, and standard POSIX exit codes (`0` for success, non-zero for format or IO errors).
3. **Format Support Matrix**:
   - **Text / Word Processing**: CommonMark (`.md`), OpenDocument Text (`.odt`), Plain Text (`.txt`).
   - **Spreadsheets**: Comma-Separated Values (`.csv`), OpenDocument Spreadsheet (`.ods`).
   - **Presentations**: OpenDocument Presentation (`.odp`).
   - **Archival / Export**: PDF / PDF-A export rendering.

## CLI Usage

```bash
# Basic file-to-file conversion
suite-convert --input document.odt --output document.pdf --format pdf

# Batch directory conversion
suite-convert batch --input-dir ./docs --output-dir ./export --target-format odt

# Inspect document layout / structure without full export
suite-convert inspect document.ods --json
```

## Security & Fleet Considerations

- Sandbox execution for untrusted document parsing.
- Enforce CPU and memory allocation caps per batch job.
- Zero GUI / display dependencies for cloud containerization (Docker / Podman / Flatpak CLI).
