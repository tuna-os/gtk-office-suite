# Enterprise Accessibility & AT-SPI Standardized Roadmap

> **Status**: Strategic Specification & Implementation Plan  
> **Related Issue**: [#768](https://github.com/tuna-os/gtk-office-suite/issues/768)  
> **Target Alignment**: Q4 2026 / Q1 2027 Enterprise Fleet Readiness (Section 508 / EN 301 549 Compliance)

---

## 1. Overview & Objective

To enable deployment of **GTK Office Suite** (Letters, Tables, Decks) in public-sector, educational, and corporate Linux workstation environments, the applications must strictly adhere to accessibility compliance frameworks, specifically **Section 508 (US)** and **EN 301 549 (EU)**.

While basic virtual AT-SPI accessibility nodes have been introduced (e.g. Tables grid cell accessible naming and Decks slide object nodes), several critical gaps prevent screen-reader navigation and assistive technology (AT) tools from functioning correctly.

This roadmap outlines the technical architecture, milestone goals, and verification requirements to achieve full screen-reader parity across all three office applications.

---

## 2. Core Gap Analysis & Technical Drivers

### Gap A: Screen Extent & Coordinate Translation
- **Current Behavior**: Virtual AT-SPI accessible objects for canvas shapes in Decks and grid cells in Tables report static `(0, 0)` bounding extents or lack dynamic widget-to-screen coordinate translation.
- **Impact**: Screen magnifiers (e.g. Orca Zoom), visual focus indicators, and screen-reader highlight overlays cannot locate the active selection on screen.
- **Remediation**: Implement `AtkComponent` / `GtkAccessible` dynamic bounding box translation via `PageContainer` and `GridArea` screen bounds mapping.

### Gap B: Structural Document Role Annotations in Letters
- **Current Behavior**: Letters exposes standard rich text buffer regions, but lacks granular AT-SPI document structure roles (`heading`, `list`, `table`, `footnote`).
- **Impact**: Screen readers cannot announce structural headings or navigate document sections by keyboard shortcuts (e.g., Orca `H` for next heading).
- **Remediation**: Map custom paragraph attributes and style definitions to GTK4 `GtkAccessibleRole` annotations in `letters-core` buffer bridges.

### Gap C: Automated Orca & AT-SPI CI Testing
- **Current Behavior**: AT-SPI assertions rely on basic Dogtail queries without testing full screen-reader speech output or focus movement cycles.
- **Impact**: Visual changes or custom widget refactoring can break screen-reader accessibility undetected.
- **Remediation**: Integrate headless AT-SPI event listening and Orca speech buffer capture in `tests/gui/` test suites.

---

## 3. Execution Phases

### Phase 1: Dynamic Screen Extent & Focus Bounds (Q4 2026)
1. Wire `get_extents` and `get_position` handlers on `CanvasArea` (Decks) and `GridArea` (Tables) custom GObject accessible nodes.
2. Translate widget-relative cell coordinates to absolute screen coordinates using `gtk_widget_compute_bounds`.
3. Add AT-SPI smoke tests asserting valid non-zero bounding boxes for active grid cells and selected slide shapes.

### Phase 2: Structural Document Role Mapping (Q4 2026 / Q1 2027)
1. Extend `letters-core` buffer bridge to emit structural AT-SPI role metadata (`HEADING`, `LIST_ITEM`, `TABLE_CELL`).
2. Add structured navigation actions in `suite-common` to support direct AT-SPI landmark jumping.
3. Validate heading and list hierarchy announcement via `dogtail` inspection script.

### Phase 3: High-Contrast & Accessibility Audit (Q1 2027)
1. Enforce high-contrast dark/light theme variant contrast compliance (WCAG 2.1 AA 4.5:1 ratio for text and control borders).
2. Establish continuous AT-SPI regression testing in CI (`gui-tests.yml`).
