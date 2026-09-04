# Headless Documents CLI and Offline Batch Conversion Strategy

> **Status**: Strategic Proposal | **Owner**: strategist agent  
> **Target Horizon**: Q4 2026 / Q1 2027

---

## 1. Executive Summary

While `gtk-office-suite` (Letters, Tables, Decks) is designed primarily as a native GTK4/Libadwaita GUI application suite for Linux desktop users, headless server side processing, automated batch document conversion (e.g., ODT/DOCX -> PDF/HTML, OpenFormula evaluation, slide export), and CI document verification present major opportunities for ecosystem adoption.

This document outlines the strategic roadmap for decoupling document processing capabilities from GTK visual dependencies into a dedicated headless CLI binary suite (`gtk-office-cli` / `suite-convert`).

---

## 2. Strategic Rationale

1. **Ecosystem & Cloud-Native Adoption**: Enterprise and server-side Linux environments require command-line tools to transform, inspect, and render office documents without running a display server (X11/Wayland) or initializing GTK widget trees.
2. **CI/CD Preview & Verification Pipelines**: Automated pipelines in GitHub Actions and GitLab CI need lightweight tools to validate document interop, check layout parity, and export PDF/HTML artifacts automatically upon commit.
3. **Decoupled Architecture**: Elevating `suite-common-core`, `letters-core`, `tables-core`, and `decks-core` into headless-first Rust libraries ensures clean separation between GTK representation logic and underlying core document engines.

---

## 3. Architecture & Binary Targets

### 3.1 Crate & Component Decoupling

```
┌─────────────────────────────────────────────────────────┐
│              Headless CLI (`suite-convert`)            │
└────────────────────────────┬────────────────────────────┘
                             │
       ┌─────────────────────┼─────────────────────┐
       ▼                     ▼                     ▼
┌──────────────┐      ┌──────────────┐      ┌──────────────┐
│ letters-core │      │ tables-core  │      │  decks-core  │
└──────────────┘      └──────────────┘      └──────────────┘
       │                     │                     │
       └─────────────────────┼─────────────────────┘
                             ▼
                   ┌───────────────────┐
                   │ suite-common-core │
                   └───────────────────┘
```

### 3.2 CLI Capabilities Matrix

| Command | Purpose | Input Formats | Target Outputs | Headless Requirements |
|---------|---------|---------------|----------------|-----------------------|
| `suite-convert letters` | Batch document conversion | `.odt`, `.md`, `.docx` | `.pdf`, `.html`, `.txt` | Cairo PDF surface / Typst backend |
| `suite-convert tables` | Spreadsheet formula eval & dump | `.ods`, `.xlsx`, `.csv` | `.csv`, `.json`, `.pdf` | Pure Rust IronCalc / OpenFormula engine |
| `suite-convert decks` | Slide deck render & export | `.odp`, `.pptx` | `.pdf`, `.png` | Offscreen Cairo surface rendering |

---

## 4. Implementation Phases

### Phase 1: Core Engine Separation (Q4 2026)
- Verify zero GTK/GDK dependency imports in `*-core` packages (`letters-core`, `tables-core`, `decks-core`, `suite-common-core`).
- Standardize document loading, AST representation, and export traits.

### Phase 2: Headless CLI Executable (Q1 2027)
- Introduce `crates/gtk-office-cli` binary.
- Implement subcommands for conversion, document metadata extraction, and formula evaluation.

### Phase 3: Packaging & CI Integration (Q1 2027)
- Distribute `gtk-office-cli` alongside Flatpak applications and standalone static binaries.
- Provide GitHub Action (`tuna-os/setup-gtk-office-cli`) for headless document transformation pipelines.

---

## 5. Success Metrics

- **Zero GUI Initialization**: Executables process documents without `$DISPLAY` or Wayland socket requirements.
- **Conversion Speed**: Batch conversion benchmark < 100ms per standard 10-page document.
- **Memory Footprint**: Heap allocation ceiling < 32MB per conversion process.

---
*Maintained by the strategist agent (tuna-os hive)*
