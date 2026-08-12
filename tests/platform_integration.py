#!/usr/bin/env python3
"""Static contract checks for the shared GNOME platform integration."""
from pathlib import Path

ROOT = Path(__file__).parents[1]


def test_desktop_entries_declare_open_formats():
    expected = {
        "letters": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "tables": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "decks": "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    }
    for app, mime in expected.items():
        desktop = (ROOT / "flatpak" / f"org.tunaos.{app}.desktop").read_text()
        assert "MimeType=" in desktop
        assert mime in desktop


def test_each_app_has_recent_files_schema_and_runtime_policy():
    for app in ("letters", "tables", "decks"):
        schema = (ROOT / "flatpak" / f"org.tunaos.{app}.gschema.xml").read_text()
        assert 'key name="recent-files" type="as"' in schema
    helper = (ROOT / "suite-common" / "src" / "recent.rs").read_text()
    assert "MAX_RECENT: usize = 10" in helper
    assert "retain_existing" in helper
    assert "pub fn clear" in helper
