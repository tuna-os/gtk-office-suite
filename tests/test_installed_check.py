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


def portal(app, **over):
    """The app's file chooser round, a line of its own."""
    r = {"app": app, "portal_saved": True, "portal_valid": True, "portal_opened": True}
    r.update(over)
    return json.dumps(r)


def drop(app, **over):
    """The app's drag-and-drop round, a line of its own."""
    r = {"app": app, "dropped": True}
    r.update(over)
    return json.dumps(r)


def both(app, **over):
    """All of an app's lines; `over` lands on whichever line has the key."""
    gio = {k: v for k, v in over.items() if not k.startswith("portal_") and k != "dropped"}
    chooser = {k: v for k, v in over.items() if k.startswith("portal_")}
    dragged = {k: v for k, v in over.items() if k == "dropped"}
    return [row(app, **gio), portal(app, **chooser), drop(app, **dragged)]


def lines(*per_app):
    return [line for app in per_app for line in app]


def test_every_app_passing_every_check_passes():
    md, failures = ic.judge(lines(*(both(a) for a in ic.APPS)))
    assert failures == []
    assert "❌" not in md


def test_a_save_that_did_not_land_fails_and_says_why():
    _, failures = ic.judge(lines(both("letters", saved=False), both("tables"), both("decks")))
    assert failures == ["letters: Save did not replace the document through the portal"]


def test_a_leftover_temporary_fails():
    _, failures = ic.judge(lines(both("letters"), both("tables", save_leftovers=1), both("decks")))
    assert failures == ["tables: the save left a temporary file beside the document"]


def test_an_app_that_never_reported_fails():
    md, failures = ic.judge(lines(both("letters"), both("tables")))
    assert failures == ["decks: the check did not finish"]
    assert "—" in md


def test_main_exits_nonzero_on_a_failure(tmp_path, capsys):
    p = tmp_path / "installed.json"
    p.write_text("\n".join([row("letters", opened=False), row("tables"), row("decks")]) + "\n")
    assert ic.main(["x", str(p)]) == 1
    assert "did not open the document" in capsys.readouterr().out


def test_a_missing_results_file_fails_every_app(tmp_path, capsys):
    # The VM step can die before writing anything; that is a failure of
    # every check, not a crash that a pipe into the job summary hides.
    assert ic.main(["x", str(tmp_path / "installed.json")]) == 1
    out = capsys.readouterr().out
    assert all(f"{app}: the check did not finish" in out for app in ic.APPS)


def test_a_file_chooser_save_that_did_not_land_fails_and_says_why():
    _, failures = ic.judge(lines(both("letters"), both("tables"), both("decks", portal_saved=False, portal_valid=False)))
    assert failures == [
        "decks: Save As through the file chooser portal did not write the named file",
        "decks: the document saved through the file chooser is not a valid archive",
    ]


def test_an_app_without_its_file_chooser_round_fails():
    _, failures = ic.judge(lines(both("letters"), both("tables"), [row("decks")]))
    assert "decks: Open through the file chooser portal did not open the document" in failures


def test_a_drop_that_opened_nothing_fails():
    md, failures = ic.judge(lines(both("letters"), both("tables", dropped=False), both("decks")))
    assert failures == ["tables: a document dragged from Files onto the app did not open"]
    assert "| dropped | ✅ | ❌ | ✅ |" in md
