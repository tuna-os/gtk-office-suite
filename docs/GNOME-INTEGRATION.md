# Strategic Plan: GNOME Platform Integration & Portal Architecture

**Date**: 2026-09-03  
**Status**: Proposal / Hold-Gated Review  
**Tracking Issue**: #119  
**Maintainer**: tuna-os (strategist agent)

---

## Executive Summary

`gtk-office-suite` aims to deliver a modern, GNOME-native office experience (Letters, Tables, Decks) shipped primary via Flatpak. Fulfilling this mission requires seamless integration with host desktop services, portals, and desktop interaction standards while preserving strict sandbox isolation.

Currently, desktop integration features (XDG Desktop Portals, recent documents, drag-and-drop, and application activation) are handled heterogeneously across application crates. This document establishes a unified strategy and architectural specification for GTK4/Libadwaita platform integration across the suite.

---

## 1. Architectural Strategy & Scope

### 1.1 XDG Desktop Portals (`ashpd`)
All sandboxed document and system interactions MUST use XDG Desktop Portals via `ashpd` / standard GTK portal bridges:
- **File Chooser Portal** (`org.freedesktop.portal.FileChooser`): Standardized file open/save dialogs respecting flatpak sandbox boundaries (`suite-common/src/file_dialogs.rs`).
- **OpenURI Portal** (`org.freedesktop.portal.OpenURI`): Safe hyperlinking to external web pages and external document viewers without sandbox escapes.
- **Print Portal** (`org.freedesktop.portal.Print`): Integration with host printing subsystem for Typst/Cairo PDF generation.
- **Trash Portal** (`org.freedesktop.portal.Trash`): Safe document deletion and trashing from file management sidebars.

### 1.2 Recent Files & Session Storage
- **Freedesktop Recent Files Spec**: Interop via `GtkRecentManager` / `ashpd` document portal registration so opened and edited documents surface in GNOME Files (Nautilus) and GNOME Shell search.
- **Session Restorer Integration**: Auto-save and crash recovery snapshots must be registered with system session restoration services (`AdwApplication` state restoration).

### 1.3 Target Drag-and-Drop (DnD) Operations
Standardize GTK4 `GtkDropTarget` and `GtkDragSource` across all three app canvases:
- **Letters**: Image insertion via image file drop onto document canvas; text snippet dropping; external `.docx`/`.md` drop to open.
- **Tables**: CSV/XLSX data sheet drop to open/import; cell range drag-and-drop.
- **Decks**: Asset drop (images, SVG, multimedia) directly onto presentation slides.

---

## 2. Shared Subsystem Matrix

| Component | Target Crate | Technology | Responsibility |
|-----------|--------------|------------|----------------|
| Portal File Dialogs | `suite-common` | `ashpd` / `gtk4::FileDialog` | Asynchronous sandboxed file pickers across apps |
| Recent File Tracker | `suite-common` | `GtkRecentManager` | Synchronize opened/saved files with host recent index |
| Canvas Drop Controller | `suite-common` + App UI | `gtk4::DropTarget` | Unified MIME handling (`image/*`, `text/uri-list`, `text/plain`) |
| Application Activator | Per-app (`main.rs`) | `gio::Application` | `open` signal handling for desktop file association launches |

---

## 3. Milestones & Implementation Roadmap

1. **Phase 1: Portal Audit & `suite-common` Consolidation**
   - Audit `suite-common/src/file_dialogs.rs` to ensure full asynchronous `gtk4::FileDialog` and `ashpd` portal coverage.
2. **Phase 2: Recent Document & File Association Alignment**
   - Implement `GtkRecentManager` helper in `suite-common` for automatic recent file history updates upon file open/save.
3. **Phase 3: Native Canvas Drag-and-Drop**
   - Refactor image/file drop logic into shared event controllers for Letters, Tables, and Decks.

---
*Filed by strategist agent (ACMM L5 — hold-gated mode)*
