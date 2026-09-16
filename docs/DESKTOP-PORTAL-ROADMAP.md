# Linux Desktop Integration & XDG Desktop Portal Roadmap

This document outlines the strategic roadmap for GTK Office Suite integration with Linux desktop standards, XDG Desktop Portals (`xdg-desktop-portal`), sandboxed file handles, and GVfs network storage (Nextcloud, WebDAV, SMB, NFS).

---

## 1. Executive Summary & Core Objectives

GTK Office Suite (Letters, Tables, Decks) is packaged and distributed primarily via Flatpak. For enterprise deployments and Linux workstation daily use, sandboxing must guarantee document security without breaking common workflows such as opening files from network mounts, preserving recent document handles across sandbox restarts, printing via system portals, and respecting system-wide appearance (dark mode / accent colors).

### Strategic Goals
- **Zero-Breakage Flatpak Sandboxing**: Full reliance on `org.freedesktop.portal.FileChooser` and `org.freedesktop.portal.Documents` for file access.
- **GVfs & Remote Mount Reliability**: Transactional atomic save semantics across high-latency or non-POSIX virtual filesystems (Nextcloud, SMB, SSHFS).
- **System Portal Parity**: Seamless integration with Print, Background, Secret Service (for document password keyrings), and Appearance portals.

---

## 2. Integration Pillars

### Pillar 1: Document Portal & FileChooser Sandboxing

| Feature | Target Behavior | Implementation Gate |
|---------|-----------------|---------------------|
| **FileChooser Portal** | Use `GtkFileChooserNative` / `libadwaita` portal choosers to avoid requiring direct host filesystem access permissions (`--filesystem=host`). | Q4 2026 |
| **Document Portal References** | Register opened documents with `org.freedesktop.portal.Documents` to maintain persistent, permissioned document URIs across application restarts. | Q4 2026 |
| **Recent Files Portal** | Synchronize document history with GNOME Desktop recent files via GTK recent manager and `org.freedesktop.portal.Recent`. | Q1 2027 |

### Pillar 2: Remote Mounts & Atomic Save Guarding (GVfs)

| Challenge | Mitigation Strategy | Status |
|-----------|---------------------|--------|
| **Non-POSIX POSIX locks** | Fallback to lock-free atomic staging files when saving to WebDAV/SMB mounts where POSIX `fcntl` locking is unsupported. | Staged |
| **Temporary Symlink Interop** | Perform safe temporary-to-target replacement without relying on symlinks or cross-device hard links on remote shares. | Staged |
| **Network Interruption Recovery** | Enforce client-side transactional sidecar checkpoints before attempting remote socket commits. | Planned |

### Pillar 3: Desktop Environment Capabilities

1. **System Appearance**: Dynamic listener for `org.freedesktop.portal.Settings` to update light/dark mode and accent colors dynamically without restarting GTK apps.
2. **Print Portal**: Delegate rendering to `org.freedesktop.portal.Print` for sandbox-safe native PDF printing.
3. **Secret Service Integration**: Store document protection passwords and signing keys safely using standard Secret Service portal bindings.

---

## 3. Milestones & Release Gating

### Q4 2026 Milestone
- [ ] Implement `GtkFileChooserNative` portal integration verification across Letters, Tables, and Decks.
- [ ] Add integration unit tests verifying non-POSIX atomic save behavior under mock GVfs mount paths.
- [ ] Validate dark mode & high-contrast portal signal transitions in AT-SPI GUI stress harness.

### Q1 2027 Milestone
- [ ] Full `org.freedesktop.portal.Documents` persistent handle management for recent documents list.
- [ ] Enterprise remote file sync fault injection suite (simulating network timeouts during save transactions).

---

## 4. Architectural Guidelines

1. **No Hardcoded Path Assumptions**: Never assume direct file paths (`/home/...`) exist inside Flatpak sandboxes. Always accept `file://` URIs and `fd://` handles.
2. **Isolated Persistence Adapter**: Keep portal communications inside `suite-common` GTK helpers while storing canonical document URIs in `suite-common-core`.
3. **Deterministic Testing**: Mock portal D-Bus calls in `tests/gui/` using synthetic D-Bus session buses.
