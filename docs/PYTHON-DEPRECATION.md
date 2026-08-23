# Python Office Suite Deprecation and Migration Roadmap

**Date**: 2026-08-18  
**Last verified**: 2026-08-22
**Status**: Approved / In Effect  
**Tracking Issues**: [#82](https://github.com/tuna-os/gtk-office-suite/issues/82), [#263](https://github.com/tuna-os/gtk-office-suite/issues/263)

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

## Retirement Gate

Deprecation, repository archival, and distribution migration are separate
outcomes. A component is retired only when every user-facing gate below has an
owner and linked evidence. Archiving a repository does not by itself prove that
existing users receive the Rust application.

| Gate | Completion evidence |
|------|---------------------|
| Canonical identity | The app ID and canonical source repository agree across manifests, store listings, install docs, and user-facing redirects. |
| Distribution cutover | The currently served Flatpak/package is built from this repository, with the build or registry digest linked. |
| Legacy publisher stopped | Legacy publish credentials and scheduled/tag-triggered publishing are disabled or demonstrably unable to replace the canonical build. |
| Upgrade verified | An install-over-upgrade test confirms settings, recent files, autosave state, and representative documents survive the transition. |
| Rollback defined | An owner, rollback trigger, and recovery path are recorded for a failed cutover. |
| Contributor routing | Legacy issue and contribution surfaces point to the active tracker and do not advertise obsolete package identities. |

### Component Ledger

Status values are **Unverified**, **In progress**, or **Verified**. “Verified”
requires an evidence link, not only a repository or README state.

| Component | Canonical app ID / target | Repository state | Distribution cutover | Upgrade proof | Owner | Target |
|-----------|---------------------------|------------------|----------------------|---------------|-------|--------|
| Letters | `org.tunaos.letters` | Legacy repo archived early; archived README still conflicts with the v2.0 identity | Unverified | Unverified | Unassigned | 2026-09-15 |
| Tables | `org.tunaos.tables` | Unverified | Unverified | Unverified | Unassigned | 2026-10-15 |
| Decks | `org.tunaos.decks` | Unverified | Unverified | Unverified | Unassigned | 2026-10-15 |
| Shared core | Workspace `suite-common` / `suite-common-core` | Unverified | N/A | N/A | Unassigned | 2026-12-15 |

Letters is the pilot because its legacy repository is already read-only while
its archived README still describes the Rust successor as
`org.tunaos.letters-rust`. Since v2.0, the canonical Rust manifest uses
`org.tunaos.letters`. Close [#263](https://github.com/tuna-os/gtk-office-suite/issues/263)
only after the ledger links the served-build evidence and an upgrade result.

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

### Phase 3: Repository Archival (through Q1 2027)
- Archive each standalone Python repository after its retirement gate is
  verified. Letters was archived ahead of this gate and remains an incomplete
  migration until its distribution and upgrade evidence are recorded.
- All active issue triage and feature roadmaps hosted exclusively in `gtk-office-suite`.

---

## Migration Guide for Users & Packagers

1. **Flatpak & System Packages**:
   - `org.tunaos.letters`, `org.tunaos.tables`, `org.tunaos.decks` Flatpaks are built and distributed directly from `gtk-office-suite/flatpak/`.
2. **File Formats**:
   - Native document formats (DOCX/ODT for Letters, XLSX/ODS for Tables, PPTX/ODP for Decks, Markdown) are 100% interoperable and pass automated round-trip parity tests against LibreOffice.
3. **Settings & Preferences**:
   - GSettings schemas remain compatible (`org.tunaos.letters`, `org.tunaos.tables`, `org.tunaos.decks`) so existing preferences and recent file histories seamlessly carry over.
