# Skill: GTK App Inspection via Broadway (`gtk4-broadwayd`)

This skill explains how to run, inspect, and debug GNOME/GTK4 applications inside a web browser using the Broadway HTML5 backend. It also covers how to use Broadway as a bridge for LLM agents to build and test GTK4 applications dynamically.

## Architecture

### Traditional Broadway (GTK3)
GTK3 Broadway streams simple image frames to the browser — each frame is a static image rendered server-side and sent over WebSockets.

### GTK4 Broadway — Render Node Tree
GTK4's Broadway backend (documented by Alexander Larsson) shifts away from image streaming to rendering a **serialized Render Node Tree** sent over WebSockets. The browser decodes this tree and translates it into highly optimized HTML/CSS DOM nodes — using real DOM elements for layout, textures, borders, and gradients rather than a flat canvas. This means:

- **Structural transparency**: the browser's DOM contains real nodes with bounding boxes, classes, labels, and nesting — not just pixels
- **Selective updates**: only changed nodes are transmitted, making updates near-instant
- **LLM-friendly**: agents can scrape the DOM tree to understand UI state without OCR or vision models

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
- **DOM Inspection:** Open DevTools (F12) and inspect the DOM — GTK4 Broadway renders real HTML nodes, not a flat canvas. You can see widget labels, bounding boxes, CSS classes, and nesting.

---

## 4. Managing leftover processes

When launching or restarting apps repeatedly, multiple instances may remain active on the display slot. Clean up using:

```bash
pkill -9 -f <app-name>
```

---

## 5. LLM Agent Framework: Building & Testing GTK Apps via Broadway

By leveraging the Render Node Tree architecture, LLM agents can build, test, and adapt GTK4 GUI applications dynamically — without relying on raw pixels (hard for LLMs) or code compilation loops (slow). The agent manipulates the system at the **Render Tree / UI state level** in real-time.

### 5.1 Agent Architecture Loop

```
 +-----------------+       IPC Commands        +---------------------+
 |                 | ----------------------> |                     |
 |    LLM Agent    |                         |  GTK4 App Instance  |
 |  (Orchestrator) | <---------------------- |  (Grid/Layout App)  |
 |                 |    DOM State / Events   +---------------------+
 +-----------------+                                    |
          ^                                             | GTK4 Broadway Backend
          |                                             v
          |          Scraped DOM Node Tree     +---------------------+
          +---------------------------------- |   Headless Chrome   |
                                              |  (Playwright/Selenium)|
                                              +---------------------+
```

**Loop steps:**
1. The LLM agent issues a high-level command (e.g. "Insert a GtkButton at position 0,1")
2. The agent framework sends an IPC command to the running GTK4 process (Unix socket, gRPC, or stdin pipe)
3. The GTK4 app modifies its live widget tree — adding, removing, or reconfiguring widgets
4. Broadway streams only the changed render nodes to the headless browser
5. The headless browser updates its DOM; the LLM scrapes the DOM to verify the change
6. The LLM can also simulate user interaction (clicks, typing) via Playwright

### 5.2 Implementation Steps

#### Step A: Build a Dynamic GTK4 App with IPC

Instead of forcing the LLM to write boilerplate C or Python from scratch, write a generic GTK4 program that initializes a `Gtk.Grid`. The application exposes an IPC layer (Unix socket, gRPC, or stdin pipe) that allows external commands to insert, remove, or modify widgets dynamically.

**Python example (PyGObject):**
```python
import gi
gi.require_version('Gtk', '4.0')
from gi.repository import Gtk

class AgentGridApp(Gtk.Application):
    def do_activate(self):
        self.win = Gtk.ApplicationWindow(application=self)
        self.grid = Gtk.Grid()
        self.win.set_child(self.grid)
        self.win.present()
        # Start async thread to listen for LLM agent commands...
```

**Rust example (gtk4-rs, for this project):**
```rust
use gtk4::prelude::*;
// The Letters, Tables, or Decks applications are already running.
// Add an IPC listener that accepts widget manipulation commands
// and modifies the live widget tree directly.
```

#### Step B: Deploy via Broadway

```bash
# Start the broadway display daemon
gtk4-broadwayd --port 8085 :5

# Run the app targeting that display
GDK_BACKEND=broadway BROADWAY_DISPLAY=:5 cargo run --bin letters
```

#### Step C: Connect a Headless Browser Agent

Use Playwright (preferred) or Selenium to spin up a headless browser:

```python
from playwright.sync_api import sync_playwright

with sync_playwright() as p:
    browser = p.chromium.launch(headless=True)
    page = browser.new_page()
    page.goto("http://127.0.0.1:8085")
    # Scrape DOM to understand UI state
    dom_text = page.content()
    # Find specific widgets by label or bounding box
    button = page.locator("text=Submit")
    button.click()
```

### 5.3 How the LLM Operates

**Semantic Observation (Instead of Vision Models):** Vision-language models struggle with pixel-perfect layouts. Instead, the agent parses Broadway's DOM structure — reading labels, bounding rectangles, and tree nesting to mathematically understand if a grid layout is correct or overlapping.

**Rapid Multi-turn Execution:** The LLM issues a high-level command (e.g. *"Add a text field at grid coordinates 0,1 and a Submit button at 1,1"*). The agent framework converts this into an IPC call to the running GTK4 app. Broadway streams only changed nodes, so the browser updates nearly instantly.

**Live DOM Verification:** The LLM scrapes the updated DOM tree to verify the widget was placed correctly — checking the node's position, label text, and parent container.

**Automated Interaction Testing:** The agent instructs Playwright to simulate clicks and keystrokes on DOM elements. The LLM can observe if app state shifts according to requirements.

### 5.4 Why Broadway Is Ideal for LLM Agents

1. **No Compiling Needed** — The host GTK4 application stays alive; the LLM sends instructions to modify the live widget tree
2. **State Transparency** — Layout structure is presented in HTML/XML-like node trees that LLMs excel at processing
3. **Selective Updates** — Only changed render nodes are transmitted, enabling real-time feedback loops
4. **Bypasses Vision Models** — No OCR or screen parsing needed; read the DOM directly
5. **Language Agnostic** — The GTK4 app can be in Rust (like Letters/Tables/Decks), Python, or C; Broadway presents the same HTML DOM to the agent

### 5.5 Applying to the tuna-os Office Suite

All build and run commands execute inside the **finupdate toolbox** (Fedora 43 with GTK4 dev libs). The toolbox name is configurable via the `toolbox` variable in the justfile.

**Quick start:**
```bash
# Build + run + inspect with Playwright (full workflow)
just verify-letters

# Or step by step:
just letters-broadway    # Start Broadway + run Letters
just letters-inspect     # Broadway + Playwright DOM inspector
just letters-dom         # Quick curl DOM scrape
just letters-verify      # Basic DOM text check
just kill-all            # Stop everything
```

**Manual toolbox invocation:**
```bash
# Start Broadway daemon on host
gtk4-broadwayd --port 8085 :5

# Build inside toolbox
toolbox run --container finupdate cargo build --manifest-path $PWD/Cargo.toml --workspace

# Run Letters inside toolbox with Broadway backend
toolbox run --container finupdate \
    env DBUS_SESSION_BUS_ADDRESS="" \
    GSETTINGS_SCHEMA_DIR=$PWD/flatpak \
    GDK_BACKEND=broadway \
    BROADWAY_DISPLAY=:5 \
    $PWD/target/debug/letters
```

3. **Inspect DOM:** Navigate to `http://localhost:8085`, open DevTools (F12). You'll see real `<div>`, `<input>`, and `<label>` nodes for toolbar buttons, the editor area, and status bar.

4. **Playwright DOM inspection (full verification):**
   ```bash
   # Full Playwright-based inspection with DOM tree + widget verification
   just letters-inspect
   just tables-inspect
   just decks-inspect

   # Or run directly:
   python3 skills/broadway-inspect/broadway_inspect.py letters
   ```

5. **DOM scraping for LLM agents:**
   ```bash
   # Quick DOM check from CLI
   curl -s http://localhost:8085/ | grep -o "Letters\|Bold\|Italic\|Normal\|Heading" | sort -u
   ```

5. **Agent use cases:** An LLM agent can:
   - Verify the PageContainer widget renders a white page on gray background by checking bounding boxes
   - Confirm the ruler shows tick marks by inspecting DOM text labels
   - Simulate typing in the editor via Playwright and verify spell-check underlines appear as CSS-styled nodes
   - Trigger style dropdown selection via IPC and verify the DOM reflects the new TextTag

6. **CI integration:** Broadway allows automated UI testing in CI without a display server — the headless browser + Broadway daemon + compiled app run entirely in a container.
