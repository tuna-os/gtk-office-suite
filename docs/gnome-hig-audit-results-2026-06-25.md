# Comprehensive GNOME HIG Audit — Results

Date: 2026-06-25
Method: 12 parallel sub-agent audits across 5 Rust crates (suite-common, letters, decks, tables)

## Summary

| Category | Verdict | Critical | Major | Minor | Clean |
|----------|---------|----------|-------|-------|-------|
| **Text** | ✅ All fixed | 0 | 0 | 0 | Yes |
| **Buttons** | ✅ All icons in Adwaita | 0 | 0 | 0 | Yes |
| **Layout** | ❌ 1 critical violation | 1 | 1 | 0 | No |
| **Icons** | ✅ All valid Adwaita | 0 | 0 | 0 | Yes |
| **Shortcuts** | ✅ All standard | 0 | 0 | 0 | Yes |
| **Accessibility** | ✅ Tooltips present | 0 | 0 | 0 | Yes |
| **Colors** | ❌ 7 violations | 1 | 6 | 0 | No |
| **Dialogs** | ⚠️ 1 minor | 0 | 0 | 1 | No |
| **Responsive** | ⚠️ Missing breakpoints | 0 | 1 | 0 | No |
| **CSS classes** | ✅ All valid Adwaita | 0 | 0 | 0 | Yes |
| **GSettings** | ⚠️ Minor issues | 0 | 0 | 2 | No |
| **i18n** | ❌ No i18n infrastructure | 1 | 0 | 0 | No |

## Critical Violations

### 1. Layout: 6 stacked bars in Letters
**File**: `letters/src/window.rs`
**Finding**: Letters adds 4 separate top bars (tab_bar, ruler, style_dropdown, find_revealer) on top of HeaderBar + SuiteToolbar = **6 stacked bars**. GNOME HIG recommends unified HeaderBar.
**Fix**: Consolidate toolbars. Move ruler into the document chrome, move style dropdown into toolbar, integrate find/revealer into header bar.

### 2. Colors: Tables grid dark mode disabled
**File**: `tables/src/grid_render.rs:48`
**Finding**: `let is_dark = false; // detect from theme` — dark mode detection hardcoded to false.
**Fix**: Use `AdwStyleManager::is_dark()` or CSS variable detection.

### 3. i18n: No internationalization infrastructure
**Finding**: Zero translatable strings (`gettext`/`i18n`), no `.po` files, no locale directories.
**Fix**: Add `gettext-rs` crate, wrap all user-visible strings in `fltr!()` or `gettext()`.

## Major Violations

### Colors
- **Ruler** (`letters/src/ruler.rs`): All colors hardcoded gray. Invisible in dark mode.
- **PageContainer** (`letters/src/page_container.rs`): 14 hardcoded Cairo colors. Desktop background should be adaptive.
- **Print preview** (`letters/src/print_preview.rs`): Same hardcoded colors as PageContainer.
- **Decks canvas** (`decks/src/canvas.rs`): Selection blue hardcoded `(0.0, 0.5, 1.0)` instead of `@theme_selected_bg_color`.
- **Styles** (`letters/src/styles.rs`): Hex colors `#666666, #333333, #F0F0F0` bypass theme system.

### Layout
- **Decks**: Extra toolbar added by `build_decks_toolbar()` while SuiteToolbar is invisible — empty container still occupies vertical space.

### Responsive
- **No breakpoints**: None of the 3 apps use `AdwBreakpoint` for responsive layouts.

## Minor Violations

- **Dialogs**: "Headers and footers" dialog passes `None` as parent window.
- **GSettings**: Schema key naming inconsistencies, missing range constraints.

## Fixes Already Applied (during this session)

- 17 text fixes (sentence case in menus, tooltips, prefs)
- Dark mode toggle removed from HeaderBar
- 5 invalid icon names replaced with real Adwaita icons
- Textview transparent background CSS fix

## Next Steps

1. Fix Tables grid dark mode detection
2. Fix ruler colors for dark mode
3. Fix PageContainer/print preview colors
4. Consolidate stacked bars in Letters
5. Add i18n infrastructure
6. Add AdwBreakpoint for responsive layout
