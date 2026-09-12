# RFC 0002: Headless CLI Document Conversion Binary (`suite-convert`)

**Status**: Draft  
**Author**: Strategist Agent  
**Date**: 2026-09-12  
**Tracking Issue**: #654 (Related: #579)

---

## Context & Motivation

GTK Office Suite currently ships three core desktop productivity applications: **Letters** (word processor), **Tables** (spreadsheet), and **Decks** (presentations). While primary interactive workflows run inside Flatpak desktop environments using GTK4 and libadwaita, enterprise fleet deployments, CI document validation pipelines, and automated server workflows require headless document conversion capability.

Currently, document loading and export logic are housed within core engine crates (`letters-core`, `tables-core`, `decks-core`, and `suite-common-core`). However, batch conversion requires launching desktop binaries or running custom script wrappers. 

`suite-convert` provides a native, headless Rust CLI binary that exposes format conversion (e.g. ODT/DOCX/Markdown to PDF/SVG/ODT) without requiring an active X11/Wayland display server or GTK widget tree initialization.

---

## Architectural Requirements & Constraints

1. **GTK-Free Engine Direct Invocations**: `suite-convert` must depend directly on `letters-core`, `tables-core`, `decks-core`, and `suite-common-core`. It MUST NOT link against GTK4, libadwaita, or UI display looper code.
2. **Deterministic Output & Loss Budgets**: File transformations must enforce loss-budget policies, reporting skipped elements, visual layout fidelity warnings, or unhandled XML nodes.
3. **Headless Execution**: Conversion runs synchronously or in batch mode from command-line environments, containerized CI runners, and automated document pipelines.
4. **Standardized Exit Codes & Structured Diagnostics**: Failures and conversion warnings must be reported via standard error outputs and optional JSON diagnostic logs.

---

## Proposed CLI Interface Design

```bash
# Basic conversion
suite-convert --input document.odt --output document.pdf

# Explicit format specification and page range selection
suite-convert --input presentation.odp --format pdf --pages 1-5 --output presentation_preview.pdf

# Batch directory conversion with loss-budget verification
suite-convert --batch ./docs/ --output-dir ./dist/pdf/ --format pdf --strict-loss-budget
```

### Command-Line Arguments Spec

| Argument | Short | Type | Description |
|----------|-------|------|-------------|
| `--input` | `-i` | Path | Input document path |
| `--output` | `-o` | Path | Output document path |
| `--format` | `-f` | String | Target export format (`pdf`, `svg`, `odt`, `commonmark`, `png`) |
| `--batch` | `-b` | Path | Directory path for batch conversion mode |
| `--output-dir` | `-d` | Path | Output directory for batch mode conversions |
| `--pages` | `-p` | String | Page / slide range selection (e.g. `1-3,5`) |
| `--strict-loss-budget` | | Flag | Fail execution (exit code 2) if format fidelity loss exceeds threshold |
| `--json-report` | | Path | Path to write structured execution summary & warning report |

---

## Crate Layout & Integration

```
suite-convert/
├── Cargo.toml
└── src/
    ├── main.rs            # CLI entry point, argument parsing (clap)
    ├── converter.rs       # Format routing & engine invocation
    ├── loss_budget.rs     # Loss budget validation & fidelity checks
    └── report.rs          # JSON report builder
```

---

## Considered Alternatives

1. **Wrapping Desktop App Executables in Headless Flag (`letters --headless`)**:
   - *Rejected*: Desktop binaries pull in GTK4/libadwaita links, requiring display initialization checks or headless virtual framebuffers (`Xvfb`).
2. **Python Automation Script Wrapper**:
   - *Rejected*: Increases dependency footprint and conflicts with the pure-Rust architecture commitment.

---

## Success Criteria & Milestones

- [ ] Milestone 1: Create `suite-convert` crate skeleton in workspace and wire CLI parser.
- [ ] Milestone 2: Implement ODT -> PDF and CommonMark -> ODT conversion pipeline using `letters-core`.
- [ ] Milestone 3: Implement ODP -> PDF export pipeline using `decks-core`.
- [ ] Milestone 4: Add `--strict-loss-budget` reporting and CI verification suite.
