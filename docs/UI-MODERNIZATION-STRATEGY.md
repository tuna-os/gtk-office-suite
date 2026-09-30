# GTK4 / Libadwaita UI Modernization Strategy

**Status**: HIG alignment and pattern standardization roadmap  
**Horizon**: Q4 2026 + post-v2.0 phases  
**Owners**: architect (design), all apps  
**Last updated**: 2026-09-30

---

## Executive Summary

gtk-office-suite targets GNOME's modern desktop user experience. This roadmap aligns Letters, Tables, and Decks with GTK4 patterns, Libadwaita components, and GNOME Human Interface Guidelines (HIG) — ensuring consistent, native, accessible UI across all three applications.

Three modernization pillars:

1. **HIG Alignment** — standard keybindings, menu structure, dialog patterns
2. **Libadwaita Components** — adaptive layouts, modern widgets, system theme integration
3. **Accessibility Integration** — accessible widget composition, semantic markup, focus management

---

## Pillar 1: GNOME HIG Alignment

### Standard Patterns

gtk-office-suite must follow GNOME HIG conventions:

| Pattern | Requirement | Letters | Tables | Decks | Target |
|---|---|---|---|---|---|
| **Main menu** | Primary, contextual, common | ✅ Implemented | ✅ Implemented | ✅ Implemented | — |
| **Toolbar** (icon + label) | Persistent, customizable tools | ✅ Partial (Labels missing on icons) | ⬜ 2026-11-30 | ⬜ 2026-11-30 |
| **Command palette** (Ctrl+?) | Discoverable command access | ✅ Implemented | ✅ Implemented | ✅ Implemented | — |
| **Keyboard shortcuts** | Standard set (Ctrl+O, Ctrl+S, etc.) | ✅ | ✅ | ⬜ 2026-10-30 |
| **Search** (Ctrl+F) | In-document search + replace | ✅ | ✅ | ⬜ Future |
| **Preferences** (Ctrl+,) | Consistent settings dialog | ✅ Basic | ✅ Basic | ⬜ 2026-11-15 |
| **About dialog** | Standard info + credits | ✅ | ✅ | ✅ | — |
| **Print dialog** | System standard print UI | ✅ | ✅ | ✅ | — |
| **File dialogs** | Portal-based open/save | ✅ | ✅ | ✅ | — |

### Keyboard Shortcut Standardization

All apps support HIG keyboard conventions:

```
File menu:
  Ctrl+N         New document
  Ctrl+O         Open document
  Ctrl+S         Save document
  Ctrl+Shift+S   Save as
  Ctrl+Q         Quit application
  Ctrl+P         Print

Edit menu:
  Ctrl+Z         Undo
  Ctrl+Shift+Z   Redo
  Ctrl+X         Cut
  Ctrl+C         Copy
  Ctrl+V         Paste
  Ctrl+A         Select all
  Ctrl+F         Find
  Ctrl+H         Find & replace

View menu:
  Ctrl+Plus      Zoom in
  Ctrl+Minus     Zoom out
  Ctrl+0         Reset zoom
  F11            Fullscreen
  Ctrl+Shift+F   Focus mode

Common formatting:
  Ctrl+B         Bold
  Ctrl+I         Italic
  Ctrl+U         Underline
```

**Test**: `tests/gui/test_keyboard_shortcuts.py` validates all shortcuts work consistently.

---

### Menu Structure Consistency

Each app exposes the same primary menu structure:

```
Primary menu (hamburger):
├── New
├── Open
├── Recent (submenu)
├── ─────────────
├── Preferences
├── Help
├── About
└── Quit

Contextual menus:
├── Cut / Copy / Paste
├── Delete
├── ─────────────
└── Format / Properties
```

---

## Pillar 2: Libadwaita Component Modernization

### Layout Patterns

Libadwaita provides adaptive layouts that scale from phone → tablet → desktop. gtk-office-suite must leverage:

#### 2a. Adaptive Window Layouts

| Scenario | Layout | Status | Target |
|---|---|---|---|
| Mobile (< 600px) | Single-column, sidebar collapses | ⬜ Future | Post-v2.0 |
| Tablet (600–900px) | Two-column, sidebar/inspector | ⬜ Future | Post-v2.0 |
| Desktop (> 900px) | Full sidebar + canvas + inspector | ✅ Letters | ✅ 2026-10-30 |

**Implementation**: Use `AdwWindow` + `AdwNavigationView` (Libadwaita 1.4+) for responsive stacks.

#### 2b. Sidebar Standardization

Each app has a consistent sidebar pattern:

**Letters sidebar**:
```
├── 📄 Pages (collapsible)
│  ├── Page 1
│  ├── Page 2
│  └── Page 3 (active)
├── 🔍 Find & Replace
└── 📋 Properties (font, paragraph)
```

**Tables sidebar**:
```
├── 📊 Sheets (tab-like)
├── 📐 Format Inspector
└── 🔧 Cell properties
```

**Decks sidebar**:
```
├── 🎞️ Slide sorter (thumbnails)
├── 📐 Layout/theme selector
└── 📋 Notes area
```

All sidebars use `AdwFlap` (collapsible, adaptive) instead of fixed `GtkBox`.

---

### Modern Widgets

Replace legacy GTK3 patterns with Libadwaita equivalents:

| Old Pattern | Libadwaita Replacement | Status | Target |
|---|---|---|---|
| `GtkHeaderBar` | `AdwHeaderBar` | ✅ | — |
| `GtkListBox` | `AdwListBox` + `AdwPreferencesPage` | ✅ Partial | 2026-11-30 |
| `GtkComboBoxText` | `AdwComboRow` | ⬜ | 2026-11-30 |
| `GtkSpinButton` | `AdwSpinRow` or numeric entry | ⬜ | 2026-11-30 |
| `GtkColorButton` | `AdwColorButton` (Libadwaita 1.4) | ⬜ | Post-v2.0 |
| `GtkFileChooserDialog` | Portal-based (already done) | ✅ | — |

**Rationale**: Libadwaita widgets are theme-aware, accessible, and responsive.

---

### Libadwaita Theme Integration

All apps respond to system theme + accent color settings:

| Setting | Implementation | Status |
|---|---|---|
| **Dark mode** | Automatic via `GtkSettings:gtk-application-prefer-dark-theme` | ✅ Working |
| **System accent color** | GNOME accent color preference + CSS custom properties | ⬜ 2026-11-15 |
| **High-contrast mode** | Libadwaita HC theme (`adwaita-hc`, `adwaita-hc-dark`) | ⬜ 2026-11-15 |
| **Reduced motion** | Respect `gtk-enable-animations` for accessibility | ✅ Partial |

**Implementation detail**: Set accent color via CSS:

```css
/* letters.css */
:root {
  --app-accent-color: @accent_color;
}

.document-area {
  background-color: @accent_color;
}
```

---

## Pillar 3: Accessibility Integration

### Semantic Markup

All UI elements must expose correct accessibility roles:

| Widget | Accessible Role | Status |
|---|---|---|
| Document canvas | Document, scrollable | ✅ Letters |
| Grid / table | Table / grid | ✅ Tables |
| Toolbar buttons | Button | ✅ |
| Status bar | Status bar | ⬜ 2026-11-30 |
| Sidebar toggle | Toggle button | ✅ |
| Color picker | Spin button (or combobox) | ⬜ Future |

**Test**: `tests/gui/test_a11y_at_spi_model.py` validates AT-SPI roles.

---

### Focus Management

Keyboard navigation must be predictable:

- **Tab order**: Logical left-to-right, top-to-bottom through all controls
- **Focus wrapping**: Tab at end of toolbar → document; Tab in document → back to toolbar
- **Focus indicators**: Visible 2px outline on all focused elements
- **Escape key**: Closes dialog, cancels operation, or unfocuses search

**Test**: `tests/gui/test_keyboard_navigation.py` verifies Tab order + Escape behavior.

---

### Color Contrast

All text must meet WCAG AA (4.5:1 for normal text, 3:1 for large text):

| Component | Light Theme | Dark Theme | HC Black | HC White |
|---|---|---|---|---|
| Body text | ✅ 7.5:1 | ✅ 8:1 | ✅ 11:1 | ✅ 12:1 |
| Toolbar text | ✅ 5.5:1 | ✅ 6:1 | ✅ 14:1 | ✅ 13:1 |
| Status bar | ⬜ Pending | ⬜ Pending | ⬜ Pending | ⬜ Pending |

**Test**: `tests/gui/test_high_contrast_themes.py` measures contrast ratios automatically.

---

## Implementation Roadmap

### Phase 1 (October 2026): Foundation

- [ ] Replace all `GtkHeaderBar` with `AdwHeaderBar`
- [ ] Replace `GtkBox` layouts with `AdwFlap` (sidebars)
- [ ] Implement system accent color support (CSS variables)
- [ ] Add toolbar button labels (icons + text)
- [ ] Standardize Ctrl+? command palette across all apps

**Deliverable**: v2.0.1 build passes HIG alignment checklist for at least one app (Letters or Tables).

---

### Phase 2 (November 2026): Widget Migration

- [ ] Replace `GtkComboBoxText` → `AdwComboRow` in all preference dialogs
- [ ] Replace `GtkSpinButton` → `AdwSpinRow` in numeric settings
- [ ] Migrate `GtkListBox` → `AdwListBox` in preferences
- [ ] Implement high-contrast theme support (auto-detection)
- [ ] Keyboard shortcut consistency audit across all apps

**Deliverable**: v2.1.0 ships with modernized preferences UI + HC theme support.

---

### Phase 3 (December 2026): Polish & Release

- [ ] Finalize focus order + keyboard navigation
- [ ] Accessibility audit (AT-SPI, contrast, keyboard)
- [ ] Responsive layout testing (resize window at various breakpoints)
- [ ] Release notes + documentation

**Deliverable**: v2.2.0 production release with full HIG compliance + Libadwaita modernization.

---

### Phase 4 (Post-v2.0): Future Enhancement

Deferred but planned:

- Mobile-responsive layouts (Libadwaita 1.4+ adaptive)
- Custom color picker widget (Libadwaita 1.4+)
- Floating toolbars / context-sensitive panels
- Dark/light theme toggle in preferences UI
- Extension / plugin system UI

---

## Testing Strategy

### UI Compliance Tests

```bash
# HIG alignment checklist
pytest tests/gui/test_hig_alignment.py

# Keyboard shortcut validation
pytest tests/gui/test_keyboard_shortcuts.py

# Accessibility audits
pytest tests/gui/test_a11y_*.py

# Theme response
pytest tests/gui/test_theme_response.py
```

### Manual Inspection

**Monthly during Q4**:

1. Open each app on GNOME 47
2. Verify toolbar labels visible
3. Test dark mode + HC Black/White themes
4. Test keyboard navigation (Tab through all controls)
5. Screenshot for baseline comparison

---

## Success Criteria

By **2026-12-31**:

- ✅ All three apps use `AdwHeaderBar`, `AdwFlap`, `AdwListBox`
- ✅ HIG keyboard shortcuts standardized across suite
- ✅ System accent color + theme response working
- ✅ High-contrast theme support (WCAG AA) on all apps
- ✅ Focus indicators + keyboard-only navigation fully functional
- ✅ Zero accessibility regressions vs. v2.0.0

**User-facing success**: "gtk-office-suite feels native to GNOME — consistent keybindings, theme support, and accessibility across all three apps."

---

## References

- GNOME Human Interface Guidelines: https://developer.gnome.org/hig/
- Libadwaita documentation: https://gnome.pages.gitlab.gnome.org/libadwaita/
- GTK4 migration guide: https://developer.gnome.org/gtk4/stable/gtk-migrating-3to4.html
- WCAG 2.1 Level AA: https://www.w3.org/WAI/WCAG21/quickref/
