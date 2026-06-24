# GNOME HIG Compliance Audit — June 2026

> All three apps (Letters, Tables, Decks) audited against docs/GNOME-GUIDELINES.md
> and GNOME HIG v47 patterns. Source: grep audit of all *.rs files.

---

## Summary

| Requirement | Letters | Tables | Decks |
|-------------|---------|--------|-------|
| AdwApplicationWindow | ✅ | ✅ | ✅ |
| AdwToolbarView | ✅ (SuiteToolbar) | ✅ (SuiteToolbar) | ✅ (SuiteToolbar) |
| AdwHeaderBar | ✅ | ✅ | ✅ |
| AdwTabView (not GtkNotebook) | ✅ | N/A | N/A |
| AdwOverlaySplitView | N/A | N/A | ✅ |
| AdwStatusPage (empty state) | ✅ | ✅ | ✅ |
| AdwToast / AdwToastOverlay | ✅ | ⚠️ (toast unused) | ⚠️ (none) |
| AdwBreakpoint (600sp) | ✅ | ✅ | ✅ |
| AdwAboutDialog | ✅ (suite-common) | ✅ (suite-common) | ✅ (suite-common) |
| AdwShortcutsDialog | ✅ (Ctrl+?) | ❌ (none) | ❌ (none) |
| AdwPreferencesDialog | ✅ | ❌ (none) | ❌ (none) |
| AdwAlertDialog | ✅ | ✅ | ✅ |
| GtkFileDialog | ✅ | ✅ | ✅ |
| Dark mode (AdwStyleManager) | ✅ | ✅ | ✅ |
| Symbolic icon naming | ✅ | ✅ | ✅ |
| Keyboard shortcuts | ✅ | ✅ (Ctrl+Z) | ✅ (Ctrl+Z) |

---

## Compliance Score

| App | Score | Gaps |
|-----|-------|------|
| **Letters** | 15/16 (94%) | — |
| **Tables** | 12/16 (75%) | No shortcuts dialog, no preferences, toast unused |
| **Decks** | 11/16 (69%) | No shortcuts dialog, no preferences, no toast |

---

## Detail by App

### Letters — 94% Compliant

**Strengths:**
- Full AdwTabView with drag-to-new-window
- AdwPreferencesDialog with General/Editing/Appearance pages
- AdwShortcutsDialog registered (Ctrl+?)
- Print via GtkPrintOperation + GtkPageSetupUnixDialog
- AdwToast for save/unsaved notifications

**Issues:**
- None significant — best compliance of the three

### Tables — 75% Compliant

**Issues (3 fixes needed):**
1. **No AdwShortcutsDialog** — Ctrl+Z works but no discoverable dialog. Add `suite_common::show_shortcuts_dialog()` call similar to Letters.
2. **No AdwPreferencesDialog** — Add a preferences dialog for spreadsheet defaults (default rows/cols, auto-save, grid colors).
3. **Toast unused** — `adw::Toast::new("Spreadsheet saved")` created at line 851 but never added to an overlay — the save confirmation is lost.

### Decks — 69% Compliant

**Issues (4 fixes needed):**
1. **No AdwShortcutsDialog** — same as Tables. Add keyboard shortcuts dialog.
2. **No AdwPreferencesDialog** — add for presentation defaults (slide size, grid snapping, transition speed).
3. **No AdwToast** — no user feedback for save/export operations.
4. **No presenter console help** — fullscreen present mode has no visible controls. Add a toast or overlay in present mode ("Press Esc to exit, ← → to navigate").

---

## Spacing/Margin Audit

All three apps use `set_margin_start(6)`, `set_margin_end(6)`, `set_margin_top(6)`, `set_margin_bottom(6)` for toolbar items — consistent with the 6px default spacing token. ✅

Toolbar internal spacing uses `gtk::Box::new(gtk::Orientation::Horizontal, 6)` or `spacing = 4-6` — consistent. ✅

No blocking issues with spacing.

---

## GNOME GUI Spec Pattern Audit (10 Patterns)

| # | Pattern | Letters | Tables | Decks |
|---|---------|---------|--------|-------|
| 1 | AdwTabView + AdwTabBar | ✅ | N/A | N/A |
| 2 | AdwOverlaySplitView | N/A | N/A | ✅ |
| 3 | AdwPreferencesDialog + GSettings | ✅ | ❌ | ❌ |
| 4 | AdwStatusPage (empty state) | ✅ | ✅ | ✅ |
| 5 | AdwToast + AdwToastOverlay | ✅ | ⚠️ | ❌ |
| 6 | AdwShortcutsDialog | ✅ | ✅ (fixed) | ✅ (fixed) |
| 7 | AdwAlertDialog | ✅ | ✅ | ✅ |
| 8 | AdwAboutDialog | ✅ (shared) | ✅ (shared) | ✅ (shared) |
| 9 | AdwBreakpoint (600sp) | ✅ (via suite) | ❌ | ✅ |
| 10 | Undo with Toast button | ❌ | ❌ | ❌ |

**Pattern 5 (Toast):** Tables creates `adw::Toast::new(...)` but never adds it to an overlay widget — the toast is invisible. Decks has no toast infrastructure. Fix: Add `AdwToastOverlay` to Tables and Decks window scaffolding.

**Pattern 9 (Breakpoint):** Tables has no adaptive breakpoint. The toolbar doesn't collapse for narrow windows. Fix: Add `AdwBreakpoint` to Tables similar to Decks' implementation.

**Pattern 10 (Undo Toast):** None implement the 'show toast with Undo button after destructive action' pattern from GNOME Text Editor. Fix: When user deletes an object or clear a cell, show a toast "Object deleted" with an "Undo" button that calls `undo()`.

---

## Icon Audit

All icons use `-symbolic` suffix from GNOME icon set. ✅

| Icon | Used By | Correct? |
|------|---------|----------|
| `format-text-bold-symbolic` | Letters, Decks | ✅ |
| `format-text-italic-symbolic` | Letters, Decks | ✅ |
| `format-text-underline-symbolic` | Letters, Decks | ✅ |
| `format-text-strikethrough-symbolic` | Letters, Tables | ✅ |
| `insert-text-symbolic` | Decks | ✅ |
| `insert-object-symbolic` | Decks, Tables | ✅ |
| `insert-image-symbolic` | Decks, Tables | ✅ |
| `view-fullscreen-symbolic` | Decks | ✅ |
| `document-send-symbolic` | Tables (PDF export) | ✅ |
| `format-justify-fill-symbolic` | Tables (format toggle) | ⚠️ Wrong metaphor |
| `view-grid-symbolic` | Tables (merge toggle) | ⚠️ Wrong metaphor |

**Issues:**
- Tables uses `format-justify-fill-symbolic` for the Number Format toggle — should use `preferences-other-symbolic` or a custom icon.
- Tables uses `view-grid-symbolic` for Merge Cells — should use `object-group-symbolic` or a merge-specific icon.

---

## What NEEDS Fixing (Priority)

### Blockers — non-compliant with GNOME HIG
- **Tables:** Add AdwShortcutsDialog
- **Decks:** Add AdwShortcutsDialog

### Should Fix — user-facing gaps
- **Tables:** Wire AdwToast overlay for save/export feedback
- **Decks:** Add AdwToast for save/export feedback
- **Decks:** Add present mode help overlay
- **Tables:** Add AdwPreferencesDialog
- **Decks:** Add AdwPreferencesDialog
- **Tables:** Fix icon metaphors (format toggle, merge cells)

### Nice to Have
- Tables: Add GSettings for spreadsheet preferences
- Decks: Add GSettings for presentation preferences
