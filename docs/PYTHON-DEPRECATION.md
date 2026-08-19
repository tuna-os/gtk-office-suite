# Python Office Suite Deprecation and Migration Roadmap

**Date**: 2026-08-18  
**Status**: Approved / In Effect  
**Tracking Issue**: [#82](https://github.com/tuna-os/gtk-office-suite/issues/82)

---

## Executive Summary

To eliminate the dual-maintenance burden across multiple standalone repositories and differing technology stacks, the legacy Python/Meson office suite implementations are deprecated in favor of the unified, GNOME-native Rust monorepo **`gtk-office-suite`** (Letters, Tables, Decks).

| Component | Legacy Python Repository | Unified Rust Target | Status |
|-----------|--------------------------|---------------------|--------|
| **Word Processor** | `tuna-os/letters` (Python + GTK4) | `gtk-office-suite/letters` (Rust) | Deprecated |
| **Spreadsheet** | `tuna-os/tables` (Python + Webkit/JS) | `gtk-office-suite/tables` (Rust + Cairo/IronCalc) | Deprecated |
| **Presentations** | `tuna-os/decks` (Python + JS engine) | `gtk-office-suite/decks` (Rust + Cairo) | Deprecated |
| **Shared Core** | `tuna-os/suite-common` (Python) | `suite-common` / `suite-common-core` (Rust) | Deprecated |

---

## Deprecation Timeline

### Phase 1: Feature Freeze & Deprecation Notice (Q3 2026 — Current)
- **Feature Development**: Frozen on legacy Python repositories (`letters`, `tables`, `decks`, `suite-common`).
- **Support Level**: Critical security and data-loss bugfixes only.
- **Notices**: Clear deprecation banners added to READMEs and repositories pointing to `tuna-os/gtk-office-suite`.
- **Packaging**: Downstream distributions and Flatpak manifests transition to `gtk-office-suite` releases.

### Phase 2: Documentation & Tooling Consolidation (Q4 2026)
- Consolidated documentation, contributing guidelines, and design audit resources hosted centrally in `gtk-office-suite`.
- Migration helpers for user document formats, configurations, and autosave cache locations.
- Polyglot JS workarounds removed; native Cairo/Pango rendering used uniformly.

### Phase 3: Repository Archival (Q1 2027)
- Standalone Python repositories archived as read-only historical references.
- All active issue triage and feature roadmaps hosted exclusively in `gtk-office-suite`.

---

## Migration Guide for Users & Packagers

1. **Flatpak & System Packages**:
   - `org.tunaos.letters`, `org.tunaos.tables`, `org.tunaos.decks` Flatpaks are built and distributed directly from `gtk-office-suite/flatpak/`.
2. **File Formats**:
   - Native document formats (DOCX/ODT for Letters, XLSX/ODS for Tables, PPTX/ODP for Decks, Markdown) are 100% interoperable and pass automated round-trip parity tests against LibreOffice.
3. **Settings & Preferences**:
   - GSettings schemas remain compatible (`org.tunaos.letters`, `org.tunaos.tables`, `org.tunaos.decks`) so existing preferences and recent file histories seamlessly carry over.
