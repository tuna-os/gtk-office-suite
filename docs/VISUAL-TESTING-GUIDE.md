# Deterministic Visual Golden Scenarios & Artifact Diffs

This document specifies the visual testing architecture, scenario matrix, pixel comparison thresholds, and CI artifact retention policies for the GTK Office Suite (**Letters**, **Tables**, **Decks**).

---

## 1. Visual Scenario Matrix

Visual regression testing evaluates applications across a structured combination of layout resolutions, color schemes, display scaling factors, and document editor states:

| Dimension | Values | Purpose |
|---|---|---|
| **Widths (px)** | `400` (Narrow), `800` (Medium), `1280` (Desktop) | Validates responsive layouts, toolbar overflow, breakpoint behavior |
| **Color Schemes** | `Light` (Adwaita), `Dark` (Adwaita-dark), `High Contrast` | Verifies contrast accessibility, color inversion, and dark mode styling |
| **Display Scale** | `1x` (Standard), `2x` (HiDPI) | Validates UI scaling, crisp text/icons, and element bounds |
| **Editor States** | `Empty`, `Populated`, `Selection`, `Dialog`, `Error` | Verifies visual presentation across core user journeys |

---

## 2. Comparison Thresholds & Diff Strategy

1. **Pixel Tolerance (`DEFAULT_PIXEL_TOLERANCE = 10`)**:
   - Allows minor per-channel RGB variations caused by font anti-aliasing across different rendering backends.
2. **Mismatch Threshold (`DEFAULT_MISMATCH_THRESHOLD = 0.005`)**:
   - Caps allowed image-wide mismatched pixels at **0.5%**. Any higher deviation triggers a visual regression failure.
3. **Visual Diff Artifacts**:
   - When a regression is detected, a visual diff image is generated highlighting mismatched pixels in bright red over a desaturated background.

---

## 3. Baseline Golden Updates & Governance

- **Explicit Update Required**: Golden images stored under `tests/gui/goldens/` are never overwritten automatically.
- **Update Command**:
  ```bash
  python3 tests/gui/visual_golden.py --update-goldens
  ```
- Any baseline golden update requires explicit code review and approval.

---

## 4. CI Artifact Retention & Debugging

In CI runs (`.github/workflows/gui-tests.yml`), test failures retain complete diagnostic packages under `tests/gui/visual_artifacts/`:
- **Baseline Golden Image**: Reference target image.
- **Actual Captured Image**: Rendered output from current test run.
- **Visual Diff Image**: Highlighted pixel discrepancy heatmap.
- **State Snapshot (`GTK_OFFICE_SNAPSHOT_PATH`)**: Document state JSON dump.
- **Application Logs**: Standard output and standard error from process run.
