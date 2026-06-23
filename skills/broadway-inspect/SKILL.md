# Skill: GTK App Inspection via Broadway (`gtk4-broadwayd`)

This skill explains how to run, inspect, and debug GNOME/GTK4 applications inside a web browser using the Broadway HTML5 backend.

## Overview

Broadway is the GTK backend that allows GTK applications to render as HTML5/Canvas pages, enabling remote access and automated browser-based interface inspections without requiring a local X11/Wayland display server.

---

## 1. Start the Broadway Daemon

First, launch the Broadway daemon (`gtk4-broadwayd`) on the host or inside the container to listen on a specific port and display slot:

```bash
# Syntax: gtk4-broadwayd --port <PORT> :<DISPLAY_ID>
gtk4-broadwayd --port 8085 :5
```

- `--port 8085` specifies the HTTP port to serve the interface on.
- `:5` specifies the Broadway display ID (corresponds to socket/display number).

---

## 2. Run the GTK Application

Run your GTK/Libadwaita application with environment variables directing it to use the Broadway backend:

```bash
# Inside your development container or environment:
env DBUS_SESSION_BUS_ADDRESS="" \
    GSETTINGS_SCHEMA_DIR=/path/to/flatpak/gschemas \
    GDK_BACKEND=broadway \
    BROADWAY_DISPLAY=:5 \
    /path/to/compiled/binary
```

### Key Environment Variables:
- `GDK_BACKEND=broadway`: Tells GTK to use the Broadway rendering backend instead of X11 or Wayland.
- `BROADWAY_DISPLAY=:5`: Matches the display ID configured on the `gtk4-broadwayd` daemon.
- `DBUS_SESSION_BUS_ADDRESS=""`: Optional. Prevents DBus-related session errors when running in isolated containers.
- `GSETTINGS_SCHEMA_DIR`: Points to compiled GSettings schemas required by the application.

---

## 3. Access and Inspect via Web Browser

1. Open your web browser and navigate to:
   `http://localhost:8085/`
2. You will see the GTK application rendered inside the browser window.

### Interaction Guidelines:
- **Keyboard Input:** Standard GTK keyboard shortcuts (e.g., `<Control>n` for new, `<Control>q` for quit) work directly when the Broadway canvas is focused.
- **Tab Navigation & Indexing:** Use the `Tab` and `Shift+Tab` keys to cycle through active widgets, and `Space`/`Enter` to click them if cursor targeting is offset.
- **Layout Debugging:** Check the browser console or inspect elements to check sizes.

---

## 4. Managing leftover processes

When launching or restarting apps repeatedly, multiple instances may remain active on the display slot. Clean up using:

```bash
pkill -9 -f <app-name>
```
