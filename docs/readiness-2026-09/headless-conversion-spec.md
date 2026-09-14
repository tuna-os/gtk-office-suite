# Headless Document Conversion CLI Specification (`suite-convert`)

**Status**: Draft / Strategic Proposal  
**Target Milestone**: Q4 2026 Release Gate  
**Owner**: `strategist` / GTK Office Suite Architecture Team  

---

## 1. Executive Summary

`gtk-office-suite` applications (`Letters`, `Tables`, `Decks`) feature GTK-free core crates (`letters-core`, `tables-core`, `decks-core`) and shared document processing utilities in `suite-common-core`. Currently, document conversion and ODF/PDF rendering require either running full GTK desktop binaries (with X11/Wayland display contexts) or delegating to external LibreOffice `soffice` processes.

This document specifies `suite-convert` (or `gtk-office-convert`), a dedicated, GTK-free CLI binary that leverages the core parsing, document layout, and export engines directly.

---

## 2. Strategic Value

1. **Enterprise & Server Automation**: Enables headless batch document conversion in CI/CD pipelines, containerized microservices, and Linux server environments without display server requirements.
2. **Deterministic Parity Testing**: Accelerates interop validation by executing format conversions directly against `suite-common-core` without GUI event loop overhead.
3. **Architectural Enforcement**: Reaffirms the core guideline: *No business logic in widget code*. `suite-convert` acts as a consumer of pure-Rust document models.

---

## 3. CLI Command & Interface Specification

### Syntax
```bash
suite-convert --from <FORMAT> --to <FORMAT> [OPTIONS] <INPUT_FILE> -o <OUTPUT_FILE>
suite-convert batch --to <FORMAT> --out-dir <DIRECTORY> <INPUT_FILES...>
```

### Supported Formats (Phase 1 Target)
- **Letters Core**: `.odt`, `.docx`, `.md` (CommonMark), `.txt` -> `.pdf`, `.odt`, `.html`
- **Tables Core**: `.ods`, `.xlsx`, `.csv` -> `.pdf`, `.csv`, `.ods`
- **Decks Core**: `.odp`, `.pptx` -> `.pdf`, `.odp`, `.png` (slide snapshots)

### Key Flags
- `-i, --input <PATH>`: Input document file path.
- `-o, --output <PATH>`: Output document or rendering target path.
- `-f, --from <FORMAT>`: Explicit source format override (autodetected by default).
- `-t, --to <FORMAT>`: Target export format (`pdf`, `odt`, `ods`, `odp`, `csv`, `markdown`).
- `--loss-budget <PATH>`: Optional path to an interop loss budget JSON contract. Exits non-zero if document transformation exceeds loss thresholds.
- `--verbose`: Enable detailed diagnostics log emission.

---

## 4. Integration & Crate Architecture

```
suite-common-core (GTK-free: parsing, AST, loss budgets)
  ├── letters-core (Text document engine, layout, pagination)
  ├── tables-core (Formula evaluation, spreadsheet model, CSV/ODS)
  └── decks-core (Slide structure, vector rendering AST)
        └── suite-convert (CLI binary, clap parser, headless entrypoint)
```

---

## 6. Checklist

- [ ] Specify `suite-convert` CLI interfaces and subcommands
- [ ] Define integration points with `letters-core`, `tables-core`, and `decks-core`
- [ ] Establish interop loss budget enforcement interface via `--loss-budget`

