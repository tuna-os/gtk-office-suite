# Internationalization (i18n), Localization (l10n), and Translation Strategy

This document establishes the architecture, operational workflows, text-shaping requirements, and quality standards for internationalization (i18n) and localization (l10n) across the GTK Office Suite (Letters, Tables, Decks, and shared crates).

---

## 1. Vision & Architectural Scope

To serve as a daily-driver office suite for the global Linux desktop community, GTK Office Suite must deliver first-class support for diverse languages, scripts, reading directions, and cultural formatting conventions.

The scope of internationalization encompasses five core pillars:
1. **Unified Gettext Infrastructure**: Standardized string extraction, runtime translation binding, and locale domain management via `gettext-rs` and `suite-common`.
2. **Complex Text Shaping & Bi-directional (RTL) Layout**: Correct Pango/Cairo layout handling for right-to-left scripts (Arabic, Hebrew, Persian) and complex scripts (Indic, CJK) across document canvases, table grids, and presentation slides.
3. **Locale-Aware Number, Currency, and Date Formatting**: Culturally correct rendering and parsing of numeric values, dates, percentages, and currencies in document text and spreadsheet calculation engines.
4. **Automated CI Validation & Translation Workflows**: Automated POT catalog synchronization, string freeze milestones, and translation completeness ratchets in CI.
5. **Upstream Translation Platform & Flatpak Packaging**: Seamless integration with translation management platforms (Weblate / Damned Lies) and standardized Flatpak locale bundle distribution.

---

## 2. Gettext Infrastructure & Runtime Architecture

### 2.1 Domain & Runtime Binding
The suite uses the unified gettext domain `gtk-office-suite`. Runtime localization is initialized during application startup in `suite-common`:

- **Initialization**: `suite_common::init_i18n()` sets up locale discovery and binds the text domain to the application's locale installation path (`/app/share/locale` in Flatpak, `/usr/share/locale` in standard host packages).
- **String Wrapping Macro**: All user-visible strings across UI templates, dialogs, error messages, menus, and command palettes must be wrapped with `suite_common::i18n("...")` or `suite_common::i18n_f("...", &[...])` for format strings with positional placeholders.
- **Pluralization**: Plural forms must use `suite_common::ngettext("singular", "plural", n)` to support language-specific plural rules.

### 2.2 Extraction & POT Maintenance
- **POT Catalog**: `po/gtk-office-suite.pot` serves as the authoritative translation template.
- **Extraction Tooling**: `scripts/update-pot.sh` extracts translatable strings from all Rust source files (`src/**/*.rs`), UI XML blueprints (`data/ui/**/*.blp` / `data/ui/**/*.ui`), and GSettings schemas.
- **Source Inventory (`po/POTFILES`)**: All source files containing translatable strings must be registered in `po/POTFILES`. CI verifies that no translatable source file is omitted.
- **Active Locales (`po/LINGUAS`)**: Target languages are tracked in `po/LINGUAS`.

---

## 3. Complex Text Shaping & Bi-Directional (RTL) Layout

### 3.1 Pango & HarfBuzz Integration
- **Text Shaping**: All rendering pipelines (Letters document views, Tables cell displays, Decks text boxes) rely on Pango and HarfBuzz for contextual glyph substitution, cursive script joining (Arabic), and ligature resolution.
- **Font Fallback Cascade**: Text rendering configurations specify robust font fallback cascades (e.g., Cantarell -> Noto Sans CJK -> Noto Sans Arabic -> Noto Sans Devanagari) to eliminate missing glyph artifacts (tofu).

### 3.2 UI & Canvas Bi-Directional Mirroring
- **Widget Hierarchy**: GTK4 widgets automatically respect the active locale's text direction (`gtk::TextDirection::Rtl`). UI margins, toolbars, and split views flip layout horizontally.
- **Spreadsheet Grid (Tables)**: When in RTL mode, column ordering in the sheet header and canvas rendering begins at column A on the right edge, advancing leftwards. Formula bar alignment and row headers adjust accordingly.
- **Canvas Objects & Rulers (Letters & Decks)**: Horizontal rulers and coordinate axes support RTL origin anchoring. Text box alignment inside presentation shapes respects natural paragraph direction (bidi algorithm).

---

## 4. Locale-Aware Formatting & Spreadsheet Parsing

### 4.1 Number & Currency Formatting
- **Shared Formatting Engine**: `suite_common::format::NumberFormat` provides locale-aware formatting for decimals, thousands grouping, scientific notation, and currencies based on the user's `LC_NUMERIC` and `LC_MONETARY` settings.
- **Parsing vs. Formula Grammar**:
  - In formula inputs (`IronCalc` engine), the canonical formula syntax preserves standard OpenFormula separators internally, while UI display and cell editing render locale-appropriate decimal points (dot vs. comma) and argument separators (comma vs. semicolon).

### 4.2 Date & Time Formatting
- **Standardized Calendars**: Date formatting leverages `glib::DateTime::format` and locale `LC_TIME` definitions to render long, short, and ISO date representations in document fields and table cells.

---

## 5. Translation Lifecycle & CI/CD Governance

### 5.1 CI Quality Gates
1. **POT Freshness Gate**: A CI job executes `scripts/update-pot.sh` and verifies that no unstaged changes exist in `po/gtk-office-suite.pot`. If strings were added/modified in Rust or UI files without updating the POT file, CI fails.
2. **PO Syntax & Validity Check**: `msgfmt --check` validates all `.po` files listed in `po/LINGUAS` for format errors and invalid format specifiers.
3. **POTFILES Completeness Gate**: Validates that all `.rs` and `.ui` files containing `i18n()` calls are present in `po/POTFILES`.

### 5.2 Release Milestones & String Freeze
- **String Freeze Window**: Major feature releases declare a 2-week string freeze prior to release tagging, preventing UI string churn while translation teams complete localization.
- **Coverage Ratchet**: Release builds enforce a minimum translation threshold (e.g., >= 80% translated strings) for inclusion in `po/LINGUAS` production bundles.

---

## 6. Implementation Milestones

| Milestone | Target Horizon | Deliverables |
|-----------|----------------|--------------|
| **Phase 1: Baseline Extraction & CI Gates** | Near-term (Q3 2026) | Complete `po/POTFILES` audit across all crates; enforce POT freshness check in CI; standardize `suite_common::i18n` macro usage. |
| **Phase 2: Locale Formatting & Tables RTL** | Mid-term (Q4 2026) | Implement locale-aware decimal/grouping separators in `suite-common::format`; add RTL coordinate transformation in Tables canvas. |
| **Phase 3: Translation Platform Sync & Tooling** | Mid-term (Q4 2026) | Set up automated Weblate / upstream synchronization; add translation status badges to release documentation. |
| **Phase 4: Font Fallback & Complex Scripts** | Long-term (Q1 2027) | Comprehensive CJK/Indic/Arabic typography testing suite; HarfBuzz font shaping regression benchmarks in `suite-export`. |
