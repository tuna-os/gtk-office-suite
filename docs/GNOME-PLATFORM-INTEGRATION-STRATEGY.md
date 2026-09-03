# GNOME Platform Integration Strategy

This document defines the platform integration architecture and requirements for the GTK Office Suite (Letters, Tables, Decks) on GNOME desktop environments, Wayland display servers, and Flatpak sandbox runtimes.

## 1. Overview & Architecture

To deliver a seamless, native GNOME desktop experience, application components must integrate directly with core GNOME platform APIs and XDG Desktop Portals.

The primary scope of integration covers:
- **XDG Desktop Portals**: FileChooser, Print, Notification, and Appearance (Dark/Light mode) portals.
- **Recent Document Management**: Standardized document registration via `GtkRecentManager` and portal-based document exposure.
- **Drag-and-Drop (DND)**: Standardized URI and selection data exchange across application windows and shell components (Nautilus, Decks slides, Letters embedding).
- **Application Identification & Desktop Entry**: Consistent `app-id` matching (`org.gnome.Letters`, `org.gnome.Tables`, `org.gnome.Decks`) across D-Bus names and desktop entries.

---

## 2. XDG Desktop Portal Integration

In sandboxed Flatpak installations, direct host filesystem or printer hardware access is restricted. Applications must delegate host interactions to `ashpd` / XDG portals:

1. **File Chooser Portal (`org.freedesktop.portal.FileChooser`)**:
   - Asynchronous native dialogs for Open and Save operations.
   - Preserves sandboxed file access rights via document portal paths.

2. **Print Portal (`org.freedesktop.portal.Print`)**:
   - PDF rasterization and spooling delegated to host CUPS print subsystems via portal calls.

3. **Settings & Appearance (`org.freedesktop.portal.Settings`)**:
   - Synchronize light/dark style schemes with `color-scheme` portal keys (matching Libadwaita `AdwStyleManager`).

---

## 3. Recent Document Management

Each document lifecycle event (Open, Save As, Export) must register metadata with the desktop environment:

- **Metadata Fields**: Canonical URI, MIME type (`application/x-gtk-office-*`), display name, and last-accessed timestamp.
- **Privacy & Storage**: Respect system privacy settings (`org.gnome.desktop.privacy remember-recent-files`). Clean stale entries upon file deletion or failure to resolve path.

---

## 4. Drag-and-Drop (DND) Interoperability

Standardize GDK drag-and-drop targets across all suite applications:

- **`text/uri-list`**: Import images, graphics, and external documents dropped onto canvas/editor views.
- **`application/x-gtk-office-clip`**: Internal rich-text and object transfer between Letters, Tables, and Decks.
- **Visual Feedback**: Drag highlights, drop previews, and drop target validation matching GNOME Human Interface Guidelines (HIG).

---

## 5. Verification & Compliance Gates

Before release tagging, integration points must be validated against the following criteria:

- [ ] All file dialog operations function inside a non-privileged Flatpak sandbox without host filesystem overrides (`--filesystem=host` prohibited).
- [ ] System theme transitions (Light/Dark mode) propagate instantly to active windows.
- [ ] Opening external documents updates the recent documents list and GNOME Shell launch menus.
