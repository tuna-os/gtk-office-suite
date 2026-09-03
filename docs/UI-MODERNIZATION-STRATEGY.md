# GTK4 / Libadwaita UI Modernization & HIG Alignment Strategy

## Overview

This roadmap defines the architectural guidelines and step-by-step migration targets to bring `gtk-office-suite` applications (Letters, Tables, Decks) into compliance with current GNOME Human Interface Guidelines (HIG) and GTK4 / Libadwaita patterns.

## Strategic Mandates

1. **Native Libadwaita Widget Adoption**:
   - Migrate top-level windows to `AdwApplicationWindow` and `AdwHeaderBar`.
   - Utilize `AdwPreferencesWindow`, `AdwPreferencesGroup`, and `AdwActionRow` for application preferences.
   - Replace legacy dialogs with `AdwMessageDialog` and modern sheet overlays.

2. **Adaptive Layout & Responsive Scaling**:
   - Enforce breakpoint navigation (`AdwBreakpoint`, `AdwViewStack`, `AdwFlap`) to accommodate mobile, tablet, and desktop display factors.
   - Ensure dynamic UI math and scale-invariant layout structures across all custom widgets.

3. **Accessibility & Screen Reader Integration**:
   - Ensure all custom GTK4 drawing areas and widgets export proper AT-SPI tree nodes and accessible roles (`GtkAccessible`).
   - Validate keyboard navigation order and focus rings across modal sheets and custom canvas views.

4. **System Color Scheme & Styling Consistency**:
   - Respond dynamically to `AdwStyleManager` dark/light appearance changes.
   - Utilize standard GNOME named colors and CSS variables for high-contrast accessibility compliance.

## Target Milestones

- **Phase 1 (Q4 2026)**: Foundation & Preference Dialog Standardization (`AdwPreferencesWindow` across Letters and Tables).
- **Phase 2 (Q1 2027)**: Responsive Navigation Breakpoints (`AdwBreakpoint` & `AdwViewStack` integration).
- **Phase 3 (Q2 2027)**: Custom Canvas AT-SPI Role & Focus Synchronization.
