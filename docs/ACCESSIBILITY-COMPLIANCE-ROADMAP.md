# Accessibility Compliance & AT-SPI Verification Roadmap

**Date**: 2026-09-02  
**Status**: Draft / Strategic Review  
**Target Release**: Q4 2026  

---

## Executive Summary

To ensure `gtk-office-suite` (Letters, Tables, Decks) is fully accessible to visually impaired users and eligible for Linux desktop deployment in public sector and enterprise environments, this document defines the accessibility audit framework, AT-SPI tree layout requirements, and automated Orca screen reader verification gates.

---

## Current Accessibility Status

| Component | AT-SPI Accessible Exposure | Screen Coordinate Translation | Screen Reader Navigation |
|-----------|---------------------------|--------------------------------|--------------------------|
| **Letters** | Native GTK4 text view accessible | Full | Active |
| **Tables** | Grid cell virtual accessible nodes exposed | Partial (origin at 0,0) | Pending verification |
| **Decks** | Slide canvas object virtual accessible nodes exposed | Partial (origin at 0,0) | Pending verification |
| **Suite Shell** | Command palette & header controls accessible | Full | Active |

---

## Verification Gates & Protocol

### 1. Spatial Extents & Coordinate Translation
- **Requirement**: Virtual accessible nodes for grid cells (`Tables`) and canvas shapes (`Decks`) must translate widget-relative bounds to exact screen coordinates via `get_extents` AT-SPI implementation.
- **Verification**: `tests/gui/test_a11y_extents.py` verifying coordinate accuracy under Xvfb and Wayland.

### 2. Orca Navigation & Speech Feedback Matrix
- **Requirement**: Arrow key navigation through Tables cells and Decks slides must generate predictable live speech output in Orca.
- **Verification**: Integration probe with `pyatspi` validating speech events and accessible parent/child linkages.

### 3. Contrast & High-Contrast Theme Compliance
- **Requirement**: Palette colors and canvas grid renderers must dynamically adapt to `HighContrast` GTK settings without contrast degradation.

---

## Implementation Phases

### Phase 1: Spatial Extents Resolution (Q3 2026)
- Glue widget-relative-to-screen matrix math in `GridArea` and `CanvasArea` accessible implementations.

### Phase 2: Automated AT-SPI CI Gate (Q4 2026)
- Add `at-spi2-core` bus setup to CI runner and integrate `pyatspi` structural assertion checks into `scripts/release_gate.py`.

---
