# GNOME GUI Guidelines — GTK Office Suite

> Compiled from GNOME HIG v47, gnome-gui-spec audits, and AGENT-GNOME-REFERENCE.md.
> Every UI decision in this project must comply with these rules.

---

## Window Architecture

```
AdwApplicationWindow
├── AdwBreakpoint (adaptive at 600sp)
├── AdwToolbarView
│   ├── AdwHeaderBar [start: primary actions] [center: title] [end: menu]
│   ├── [content] AdwTabView / AdwOverlaySplitView / GtkStack
│   └── [bottom] AdwViewSwitcherBar / statusbar
└── AdwToastOverlay
```

**Rules:**
- Use `AdwApplicationWindow`, never raw `GtkWindow`
- Use `AdwToolbarView` as the top-level layout container
- Use `AdwHeaderBar` for window chrome — never hide it except in fullscreen
- Use `AdwBreakpoint` to collapse sidebars and toolbars below 600sp
- Toast notifications via `AdwToastOverlay` — never modal dialogs for non-critical feedback

---

## Widget Hierarchy

| Intent | Widget | Rule |
|--------|--------|------|
| Tabbed documents | `AdwTabView` + `AdwTabBar` | Never use GtkNotebook |
| Window + sidebar | `AdwOverlaySplitView` | Collapse below 600sp breakpoint |
| Preferences | `AdwPreferencesDialog` + `GSettings` | No custom preferences windows |
| Empty state | `AdwStatusPage` | Every view that can be empty must show one |
| Toast feedback | `AdwToast` + `AdwToastOverlay` | For saves, exports, errors — not modal dialogs |
| Keyboard shortcuts | `AdwShortcutsDialog` | Always accessible via Ctrl+? |
| About dialog | `AdwAboutDialog` | Include app icon, version, credits, license |
| Alert dialogs | `AdwAlertDialog` | Only for destructive/blocking confirmations |

---

## Design Tokens (GNOME HIG spacing scale)

| Token | Value | Usage |
|-------|-------|-------|
| Default row spacing | 6px | Between toolbar items, form rows |
| Default container spacing | 12px | Between sections, card padding |
| Wide spacing | 18px | Between major sections |
| Section spacing | 24px | Between content blocks |

**Rules:**
- Never use arbitrary pixel values — always choose from the token scale
- Use `.dim-label` CSS class for secondary/description text
- Use system font (Adwaita Sans) — never hardcode font families

---

## Icon Naming

| Rule | Example |
|------|---------|
| Use symbolic icons | `-symbolic` suffix: `document-open-symbolic` |
| Use GNOME icon set | Consult `/usr/share/icons/Adwaita/scalable/` |
| Fall back to generic | `insert-object-symbolic` if no specific icon exists |
| Never use emoji as icons | Use proper SVG symbolic icons |

---

## Flatpak Requirements

| Requirement | Implementation |
|-------------|---------------|
| **Runtime** | `org.gnome.Platform` 50 (stable), `org.gnome.Sdk` 50 (build) |
| **App ID** | Reverse DNS: `org.tunaos.{app}-rust` |
| **Metainfo** | Must validate with `appstreamcli validate` |
| **Desktop file** | Must include `Categories=Office;` |
| **Icons** | SVG, 128x128 minimum, installed to `/app/share/icons/hicolor/scalable/apps/` |
| **GSettings** | Schema compiled during build (`glib-compile-schemas`) |

---

## Accessibility

| Requirement | Implementation |
|-------------|---------------|
| Keyboard navigation | All features accessible via keyboard (no mouse-only actions) |
| Screen reader | Widgets must be AT-SPI bridged (GtkDrawingArea needs `accessible-role`) |
| Focus indicators | Visible focus ring on all interactive elements |
| Color independence | Never rely solely on color to convey information |

---

## What NOT to Do

- ❌ Raw `GtkWindow` — always use `AdwApplicationWindow`
- ❌ `GtkNotebook` for tabs — use `AdwTabView`
- ❌ Custom preference dialogs — use `AdwPreferencesDialog` + GSettings
- ❌ Modal dialogs for non-blocking feedback — use `AdwToast`
- ❌ Hardcoded pixel values outside the design token scale
- ❌ Custom font families — use system default
- ❌ Emoji as UI icons — use GNOME symbolic icons
- ❌ CSS hacks for layout — use proper widget hierarchy
- ❌ Blocking the main thread — use async or timeout for long operations

---

## App-Specific Patterns

### Letters
- Document tabs via `AdwTabView` with drag-to-new-window
- Toolbar: primary (B/I/U) + extended (H1-H6, alignment)
- Find overlay via `GtkRevealer` sliding down from top
- Status bar with word count, page number, zoom

### Tables
- Grid via `GtkDrawingArea` + Cairo (not GtkColumnView — needs cell-level control)
- Formula bar at top: `fx` label + `GtkEntry`
- Sheet tabs at bottom: `GtkBox` + `GtkDropDown` switcher
- Column resize: `GestureDrag` + `EventControllerMotion` for cursor

### Decks
- Slide sidebar via `AdwOverlaySplitView`
- Canvas via `GtkDrawingArea` inside `GtkScrolledWindow`
- Toolbar: formatting (B/I/U) + insert (text, shape, image) + present
- Speaker notes via `GtkExpander` below canvas

---

## Reference

- **GNOME HIG:** https://developer.gnome.org/hig/
- **gnome-gui-spec:** https://github.com/hanthor/gnome-gui-spec/blob/main/GNOME-GUI-SPEC.md
- **AGENT-GNOME-REFERENCE.md:** Project-specific reference of GNOME Rust app patterns
- **LibreOffice Impress sidebar:** 7-deck sidebar for property panels (Properties, Transition, Animation, Master Slides, Gallery, Navigator, Styles)
