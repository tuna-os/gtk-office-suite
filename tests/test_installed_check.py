"""tools/render-lab/installed_check.py judges the installed-Flatpak checks
Tier C runs (#1209): every app must pass every check, and an app whose
check never reported fails rather than being skipped."""

import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "render-lab"))

import installed_check as ic  # noqa: E402


def row(app, **over):
    r = {"app": app, "default_handler": f"org.tunaos.{app}.desktop", "registered": True, "opened": True,
         "saved": True, "valid_after_save": True, "save_leftovers": 0, "in_recent_files": True,
         "running_after_save": True}
    r.update(over)
    return json.dumps(r)


def test_every_app_passing_every_check_passes():
    md, failures = ic.judge([row(a) for a in ic.APPS])
    assert failures == []
    assert "❌" not in md


def test_a_save_that_did_not_land_fails_and_says_why():
    _, failures = ic.judge([row("letters", saved=False), row("tables"), row("decks")])
    assert failures == ["letters: Save did not replace the document through the portal"]


def test_a_leftover_temporary_fails():
    _, failures = ic.judge([row("letters"), row("tables", save_leftovers=1), row("decks")])
    assert failures == ["tables: the save left a temporary file beside the document"]


def test_an_app_that_never_reported_fails():
    md, failures = ic.judge([row("letters"), row("tables")])
    assert failures == ["decks: the check did not finish"]
    assert "—" in md


def test_main_exits_nonzero_on_a_failure(tmp_path, capsys):
    p = tmp_path / "installed.json"
    p.write_text("\n".join([row("letters", opened=False), row("tables"), row("decks")]) + "\n")
    assert ic.main(["x", str(p)]) == 1
    assert "did not open the document" in capsys.readouterr().out
