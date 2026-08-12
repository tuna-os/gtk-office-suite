#!/usr/bin/env python3
"""Regression checks for the cross-suite adaptive shell contract."""
from pathlib import Path

ROOT = Path(__file__).parents[1]
COMMON = (ROOT / "suite-common" / "src" / "lib.rs").read_text()


def test_shared_breakpoints_cover_audit_viewports():
    assert "NARROW_WIDTH_SP: f64 = 500.0" in COMMON
    assert "MEDIUM_WIDTH_SP: f64 = 800.0" in COMMON
    assert "WIDE_AUDIT_WIDTH_SP: f64 = 1280.0" in COMMON


def test_shared_toolbar_enforces_touch_target():
    assert "TOUCH_TARGET_SP: i32 = 44" in COMMON
    assert "apply_touch_target(&b)" in COMMON


def test_adaptive_design_records_theme_and_keyboard_policy():
    adr = (ROOT / "docs" / "adr" / "0006-adaptive-editor-shell.md").read_text()
    for required in ("Pango", "high-contrast", "command palette", "44sp"):
        assert required in adr


def test_shared_theme_policy_is_available_to_custom_renderers():
    assert "pub fn accent_rgb" in COMMON
    assert "pub fn canvas_foreground" in COMMON
    grid = (ROOT / "tables" / "src" / "grid_render.rs").read_text()
    assert "suite_common::accent_rgb" in (ROOT / "tables" / "src" / "window.rs").read_text()
    assert "draw_pango_text" in grid
