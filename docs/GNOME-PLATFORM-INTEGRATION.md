# GNOME Platform Integration Strategy

**Status**: Desktop portals, recent files, drag-and-drop integration plan  
**Horizon**: Q4 2026 production release  
**Owners**: architect (design), all apps  
**Last updated**: 2026-09-30

---

## Executive Summary

gtk-office-suite must integrate seamlessly with GNOME platform affordances — leveraging desktop portals for document access, exposing recent files for quick-open workflows, and supporting drag-and-drop between applications and the file manager. This roadmap establishes the integration patterns and verification gates.

Three integration pillars:

1. **Desktop Portals** — Flatpak portals for file access, printing, permissions
2. **Recent Files Integration** — GTK recent-files protocol for file history + quick-open
3. **Drag-and-Drop** — native D&D from file manager into canvas/grid/slide

---

## Pillar 1: Desktop Portals

### Flatpak Portals Overview

gtk-office-suite runs as Flatpak. File access requires portals:

- **OpenURI** — open URLs in web browser
- **OpenFile** — browse file system (sandboxed access)
- **SaveFile** — save documents with file dialog
- **Print** — access system printer
- **Screenshot** — capture window for sharing

### Implementation Status

| Portal | Purpose | Status | Target |
|---|---|---|---|
| `xdg-open` (OpenURI) | Open hyperlinks in default browser | ✅ Working | — |
| `org.freedesktop.portal.OpenFile` | File picker for Open dialog | ✅ Working | — |
| `org.freedesktop.portal.SaveFile` | Save dialog | ✅ Working | — |
| `org.freedesktop.portal.Print` | Print dialog integration | ✅ Working | — |
| `org.freedesktop.portal.Screenshot` | Share screenshot via portal | ⬜ Planned | 2026-11-30 |
| `org.freedesktop.portal.FileChooser` (v3 only) | Advanced filters | ⬜ Future | Post-v2.0 |

**Verification**: Open → Save → Print workflow in Flatpak container; verify no "permission denied" errors in portal logs.

---

### Permission Scopes (Flatpak Manifest)

Current `flatpak/org.tunaos.letters.json` declares:

```json
{
  "context": {
    "filesystems": [
      "home",
      "xdg-documents",
      "xdg-desktop"
    ]
  },
  "modules": [
    {
      "name": "org-tunaos-letters",
      "config-opts": ["--libdir=lib"]
    }
  ]
}
```

**Audit checklist**:
- [ ] No overprivileged access (`host` filesystem)
- [ ] Only declared filesystems used
- [ ] Portal access logged during UI tests
- [ ] Error handling for permission denial

---

## Pillar 2: Recent Files Integration

### GTK Recent Files Protocol

Integrate with GNOME's recent-files system (`~/.local/share/recently-used.xbel`):

- When user opens/saves a document → write to recent-files DB
- Recent files appear in system file dialogs + file manager
- Recent files accessible via `Ctrl+Alt+O` (Open Recent) in suite

### Implementation Checklist

| Component | Letters | Tables | Decks | Target |
|---|---|---|---|---|
| Save file → add to recent | ✅ Implemented | ✅ Implemented | ⬜ 2026-10-30 |
| Recent files in File menu | ✅ Working | ✅ Working | ⬜ 2026-10-30 |
| Recent files in file picker | ✅ via GTK | ✅ via GTK | ✅ via GTK |
| Thumbnail preview in recent | ⬜ Custom renderer | ⬜ Custom renderer | ⬜ Future |
| Remove from recent (right-click) | ✅ | ✅ | ⬜ 2026-11-30 |

**Implementation detail**: GTK `GtkRecentManager` handles protocol automatically; app calls:

```rust
let manager = gtk::RecentManager::default();
manager.add_item(&file_uri); // On open/save
```

**Test**: `tests/gui/test_recent_files.py`
- Open document → verify entry in `recently-used.xbel`
- Restart app → verify Recent menu repopulated
- File picker shows recent files in sidebar

---

## Pillar 3: Drag-and-Drop

### D&D from File Manager → App Canvas

Users should be able to drag a .docx/.xlsx/.odp file from GNOME Files onto the app canvas to open it (or onto a specific document area to embed).

#### Letters: Drag .docx/.md/.odt onto canvas

**Behavior**:
- If document open: ask user "Open in new window or replace current?"
- If empty document: open file in current window
- If external media (image): insert at cursor

**Implementation**:
```rust
// In window.rs drop_target setup
let drop_target = gtk::DropTarget::new(
    gtk::SelectionDataType::URIs,
    gdk::DragAction::COPY,
);
drop_target.connect_drop(|_dt, value, _x, _y| {
    if let Ok(uris) = value.get::<Vec<String>>() {
        for uri in uris {
            window.open_file(&uri);
        }
    }
    true
});
page_edit.add_controller(&drop_target);
```

**Test**: `tests/gui/test_drag_drop_documents.py`
- Drag .docx file from file manager onto Letters canvas
- Verify file opens in new window

---

#### Tables: Drag .xlsx/.ods onto grid

**Behavior**:
- If workbook open: ask "Open in new window or replace?"
- If external data (CSV): import as new sheet

**Implementation**: Similar to Letters; handle via `DropTarget` on `grid_area`.

---

#### Decks: Drag .pptx/.odp + image/video files

**Behavior**:
- Drag presentation file → open in new window
- Drag image (.png/.jpg) → insert as shape on current slide
- Drag video → embed as media placeholder

---

### D&D to File Manager (Export)

Allow users to drag documents from app to file manager to save:

- Drag a table selection (or worksheet) to Files → export as .csv/.ods
- Drag slide → export as .png (image)
- Drag document → save to destination folder

**Status**: Low priority (post-v2.0 polish); requires export-on-demand pipeline.

---

## Integration Verification Matrix

### End-to-End Workflow Tests

| Workflow | Scope | Status | Target |
|---|---|---|---|
| Open → File portal → recent files visible | Flatpak container | ✅ | — |
| Save → GTK recent-files added | All platforms | ✅ | — |
| Recent files menu + File > Open Recent | UI | ✅ | — |
| Drag .docx from Files to Letters | D&D | ⬜ 2026-10-30 | 2026-10-30 |
| Drag image to Tables cell | D&D | ⬜ 2026-11-15 | 2026-11-15 |
| Drag .pptx from Files to Decks | D&D | ⬜ 2026-11-15 | 2026-11-15 |
| Export + drag table to Files | D&D export | ⬜ Post-v2.0 | — |

### Portal Permission Audit

```bash
# In CI: run app in Flatpak, capture portal access logs
flatpak run \
  --no-documents \
  --socket=x11 \
  org.tunaos.letters \
  &
sleep 2
# Verify no permission denials in systemd journal
journalctl -u flatpak -- | grep -i "permission\|denied" || echo "✅ No permission errors"
```

---

## GNOME Platform Checklist

### Pre-Release (v2.2.0)

- [ ] All apps run unmodified in Flatpak sandbox (no host filesystem access)
- [ ] Portals (OpenFile, SaveFile, Print) working in CI
- [ ] Recent files populated after open/save cycle
- [ ] D&D document open from file manager working (Letters, Tables, Decks)
- [ ] D&D media insert (image to Tables, image/video to Decks) working
- [ ] No unhandled portal permission errors in logs

### Documentation

- [ ] Manifest permissions documented in flatpak/README.md
- [ ] D&D limitations (supported formats) noted in release notes
- [ ] Portal debugging guide in docs/GNOME-PLATFORM-DEBUGGING.md

---

## Known Limitations & Future Work

### Supported in v2.2.0

- File portals for open/save/print
- Recent files integration
- D&D document open from file manager
- D&D media insert (image/video)

### Deferred (post-v2.0)

- D&D export (save table selection to file manager)
- Contextual menus (right-click paste, cut)
- Thumbnail preview in recent-files sidebar
- Search integration (GNOME Search)
- Keyboard shortcuts in action group (for accelerator customization)

### Out of Scope

- WebDAV / cloud storage portals (enterprise feature, post-v2.0)
- FUSE-based file access (complexity outweighs benefit)
- Custom D&D types between suite apps (low priority)

---

## References

- GNOME Desktop Portals: https://flatpak.readthedocs.io/en/latest/portal-docs.html
- GTK4 Drag & Drop: https://developer.gnome.org/gtk4/stable/section-Drag-Drop.html
- GTK Recent Files: https://developer.gnome.org/gtk4/stable/GtkRecentManager.html
- Flatpak Security Model: https://docs.flatpak.org/en/latest/sandbox-permissions.html
