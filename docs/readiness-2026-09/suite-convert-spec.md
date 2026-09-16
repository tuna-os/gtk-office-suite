# Headless CLI Document Conversion Specification (`suite-convert`)

Related issue: [#741](https://github.com/tuna-os/gtk-office-suite/issues/741)

## Overview

`suite-convert` is a headless command-line utility for GTK Office Suite that performs high-throughput batch document conversion and rendering without requiring an X11/Wayland display server or GTK initialization.

By interfacing directly with `letters-core`, `tables-core`, `decks-core`, and `suite-common-core`, `suite-convert` guarantees strict separation of GUI presentation from document processing logic.

---

## Command Line Interface Syntax

```sh
suite-convert [OPTIONS] --input <FILE> --output <FILE> [--format <FORMAT>]
```

### Supported Conversion Modes

| Input Format | Target Output Formats | Processing Engine |
|---|---|---|
| OpenDocument Text (`.odt`) | PDF, CommonMark (`.md`), HTML | `letters-core` |
| CommonMark (`.md`) | ODT, PDF, HTML | `letters-core` |
| Spreadsheet (`.ods`, `.xlsx`) | CSV, PDF, HTML | `tables-core` |
| Presentation (`.odp`, `.pptx`) | PDF, SVG | `decks-core` |

---

## Technical Architecture & Constraints

1. **GTK Independence**: Must compile and execute without linking against GTK4 or `libadwaita` libraries.
2. **Error Isolation**: Format parsing or rendering failures report structured diagnostics to `stderr` with standardized exit codes (0 = success, 1 = input failure, 2 = conversion error).
3. **Atomic Writes**: Persistent output writes follow `suite-common-core` durability semantics (write-to-temp and atomic rename).
4. **Performance Target**: Batch conversion throughput target < 150ms per standard document page.
