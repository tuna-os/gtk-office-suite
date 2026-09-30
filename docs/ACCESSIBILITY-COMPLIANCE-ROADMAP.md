# Accessibility Compliance Roadmap

**Status**: Framework for AT-SPI, WCAG, and keyboard navigation verification  
**Horizon**: Q4 2026 production release  
**Owners**: quality (verification), architect (design), all apps  
**Last updated**: 2026-09-30

---

## Executive Summary

gtk-office-suite must meet WCAG 2.1 Level AA standards and support Linux screen-reader workflows (Orca via AT-SPI) for production adoption. This roadmap establishes the audit framework, test automation, and sign-off criteria.

Three compliance pillars:

1. **AT-SPI screen reader accessibility** — Orca navigation, content announcement, spatial awareness
2. **Visual accessibility** — high-contrast themes, focus indicators, color contrast ratios
3. **Motor accessibility** — keyboard-only operation, customizable keybindings, no time-based interactions

---

## Pillar 1: AT-SPI Screen Reader Accessibility

### Orca Navigation Journeys

Each application must support the following workflows entirely via screen reader (Orca 47+) with no visual feedback:

#### Letters: Open → Edit → Save

```
1. Launch application
   → Orca announces: "gtk-office-suite Letters, main window"
   → Tab to File menu
   → Announce available actions (Open, New, Recent files)

2. Open document
   → Announce file dialog, list recent files
   → Navigate with arrow keys to select file
   → Announce file name and size
   → Open file; Orca announces: "Document loaded, 2500 words, page 1 of 8"

3. Edit paragraph
   → Click/Tab into document body (Orca: "document, body role")
   → Type text; Orca announces each paragraph as caret moves
   → Ctrl+B to bold selection; Orca: "bold applied"
   → Announce word count change

4. Save
   → Ctrl+S; Orca: "Document saved, press Tab for more actions"
```

**Test**: `tests/gui/test_a11y_journeys.py::test_letters_open_edit_save`

**Evidence**: CI captures Orca output log + timestamp, compared against baseline narrative.

---

#### Tables: Navigate Grid → Edit Cell → Keyboard Navigation

```
1. Open spreadsheet
   → Orca announces: "Sheet 1 of 3, active cell A1"
   
2. Navigate to data
   → Arrow Down: "Row 2, cell A2: 'Product Name'"
   → Arrow Right: "Cell B2: 'Q3 Sales', number format"
   → Orca announces: row header, column header, cell value, data type

3. Edit cell
   → F2 or Enter: "In edit mode, type to replace"
   → Type new value
   → Tab: "Cell updated, moved to next cell"
   → Orca continuously announces: row/column context as grid moves

4. Apply formatting
   → Ctrl+B: "Bold applied to selection (5 cells)"
   → Shift+↑/↓: "Selection expanded to 10 rows"
```

**Test**: `tests/gui/test_a11y_journeys.py::test_tables_grid_navigation`

**Grid-specific requirements**:
- All cells announce row/column headers
- Selection spanning announced with count ("5 rows, 3 columns selected")
- Keyboard-driven selection (Shift+arrow, Ctrl+Shift+End) fully navigable
- Named ranges announced when activated

---

#### Decks: Slide Navigation → Text Editing

```
1. Open presentation
   → Orca announces: "Presentation with 10 slides, slide 1 active"
   → Tab between slide thumbnails (left panel)
   → Orca announces: "Slide 2 of 10, title: 'Quarterly Results', 2 text boxes"

2. Edit slide text
   → Tab into slide content area
   → Orca announces: "Text box, 150 words"
   → Edit mode; Orca announces character/word changes
   → Tab to next element (heading, bullet list, shape text)

3. Navigate slide outline
   → Ctrl+Home/End: announce first/last slide
   → Alt+↑/↓: move between slides with narration
```

**Test**: `tests/gui/test_a11y_journeys.py::test_decks_slide_navigation`

---

### AT-SPI Object Model Compliance

All interactive elements must expose:

| AT-SPI Property | Requirement | Evidence |
|---|---|---|
| `name` (accessible label) | Every button, menu item, text field labeled | Orca verbose output |
| `role` | Correct role for element type (button, menuitem, text, table cell, etc.) | AT-SPI inspector dump |
| `state` | Reflects enabled/disabled, selected, focused, checked | State change narration |
| `description` | Extended help for complex controls (chart axis, formula bar) | Orca description mode |
| `actions` | Exposed and annotated (e.g., "Press to activate") | Orca action menu |
| `parent/children` | Hierarchy correct for window → toolbar → buttons | Tree structure audit |

**Verification tool**: `accerciser` (GNOME accessibility inspector) confirms object tree and properties.

**Automation**: `tests/gui/test_a11y_at_spi_model.py` validates properties of 50+ key UI elements per app.

---

## Pillar 2: Visual Accessibility

### High-Contrast Theme Support

Each app must pass WCAG AA (4.5:1 contrast) on:

- **HC Black** theme (light text on dark background)
- **HC White** theme (dark text on light background)
- **Dark mode** (system default, already verified)

| Component | Letters | Tables | Decks | Target |
|---|---|---|---|---|
| Toolbar buttons | ✅ Pass AA | ✅ Pass AA | ⬜ 2026-10-15 |
| Text on canvas/grid | ✅ Pass AA | ✅ Pass AA | ⬜ 2026-10-30 |
| Focus ring / selection highlight | ✅ Visible | ✅ Visible | ⬜ 2026-10-30 |
| Status bar text | ✅ Pass AA | ✅ Pass AA | ⬜ 2026-11-15 |
| Dialog text / headings | ✅ Pass AA | ✅ Pass AA | ⬜ 2026-11-15 |

**Test**: `tests/gui/test_high_contrast_themes.py` captures screenshots on each theme + measures text/background color contrast programmatically.

### Focus Indicators

Every keyboard-navigable element must show a visible focus ring:

- Ring width: ≥2px
- Contrast against background: ≥3:1 (not AA, but sufficient for visibility)
- Color: High-contrast outline (yellow on dark, dark on light in HC themes)

**Test**: `tests/gui/test_focus_indicators.py` verifies every Tab-able control shows focus ring in screenshots.

### Color Usage

No information conveyed by color alone (e.g., "error messages in red" without also saying "Error: ...").

| Case | Requirement | Evidence |
|---|---|---|
| Validation errors | Text label + icon | Dialog shows "Required field" + ⚠️ |
| Cell highlighting (Tables) | Color + border + screen-reader announcement | Selection announced in Orca |
| Syntax highlighting (text) | Color + font weight / style | Bold for keywords in addition to color |

---

## Pillar 3: Motor Accessibility

### Keyboard Navigation

**All primary workflows must complete keyboard-only** (no mouse required).

| Workflow | Letters | Tables | Decks | Status |
|---|---|---|---|---|
| Open file → Save | Tab + Enter, Ctrl+O/S | ✅ | ✅ | ✅ |
| Edit document (type, format, undo) | ✅ | ✅ (grid with arrow keys) | ⬜ |
| Apply styles (Ctrl+B, Ctrl+I, etc.) | ✅ | ✅ | ✅ |
| Navigate UI (menu, toolbars, dialogs) | ✅ Tab order | ✅ Tab order | ⬜ Nov 2026 |
| Export / print | ✅ | ✅ | ⬜ |

**Test order**:
1. `tests/gui/test_keyboard_navigation.py` — tab order, focus trapping, escape-to-close
2. Per-app: `test_letters_keyboard_editing.py`, `test_tables_keyboard_grid.py`, `test_decks_keyboard_nav.py`

### No Time-Based Interactions

- All animated transitions > 500ms have pause/skip option
- Autosave notification does not force dismiss
- Dialogs remain on screen until explicitly closed (not auto-dismissed after N seconds)

---

## Compliance Checklist

### Pre-Release (each app at feature freeze)

- [ ] Orca screen reader journey test passes (CI green)
- [ ] AT-SPI object model audit passes (accerciser dump validated)
- [ ] High-contrast theme screenshots pass AA contrast check
- [ ] Focus indicators visible on all TAB-able elements
- [ ] Keyboard-only workflow test passes
- [ ] No time-based interaction blocking (manual review)
- [ ] Color-alone information rule checked (manual review)

### Release Approval

Architect + quality sign-off:

```
[Checklist all items above]

Quality: Accessibility compliance verified for v2.2.0
Architect: Design review confirms no new contrast/focus regressions
Date: [ISO 8601]
```

---

## Test Automation

### CI Test Suite

```bash
# Runs weekly + on every PR touching UI
pytest tests/gui/test_a11y_*.py \
  --capture=no \
  --verbose \
  --junit-xml=a11y-report.xml
```

### Baseline Data

- `tests/gui/a11y_baselines/` — canonical Orca narratives (text files, manually maintained)
- `tests/gui/screenshots/a11y_hc_*.png` — reference high-contrast theme screenshots
- `tests/gui/at_spi_model_baseline.json` — reference AT-SPI object tree

### Manual Audit Schedule

Monthly during Q4:

| Month | App | Auditor | Scope |
|---|---|---|---|
| Oct | Letters | quality | Orca + HC themes |
| Nov | Tables | quality | Orca grid + high-contrast |
| Dec | Decks | quality | Orca + focus indicators |

---

## Known Limitations & Scope

### Out of Scope (documented, not bugs)

- **Braille display support** — requires hardware testing; future roadmap
- **Voice control (Hey Google / Alexa)** — not applicable to desktop office suite
- **Magnification** — uses system magnifier (Zoom extension)
- **Custom keybinding UI** — keyboard remapping deferred to post-v2.0

### Acceptable Loss

- Complex embedded objects (OLE, external links) may not announce fully in Orca
- Presentation slide animations may not be fully navigable (static playback only)
- Charts in Tables announced as "image with alternative text" (not data table structure)

Document these in release notes.

---

## Success Metrics

By **2026-12-31**:

- ✅ 3/3 apps pass Orca screen-reader journey tests
- ✅ 3/3 apps support WCAG AA contrast on HC Black + HC White themes
- ✅ 3/3 apps navigable keyboard-only for primary workflows
- ✅ AT-SPI object model validated on all 50+ key controls
- ✅ Zero blockers in release notes (known limitations documented)

**Release definition**: "Users with visual impairments can use gtk-office-suite with Orca screen reader; users requiring high-contrast or keyboard-only interaction can operate the suite without workarounds."

---

## References

- WCAG 2.1 Level AA: https://www.w3.org/WAI/WCAG21/quickref/
- AT-SPI specification: https://wiki.gnome.org/Accessibility
- Orca user guide: https://help.gnome.org/users/orca/stable/
- GNOME Accessibility Guide: https://developer.gnome.org/gtk4/stable/gtk-migrating-accessibility.html
