"""tools/render-lab/upgrade_check.py judges the prior-release upgrade checks
Tier C runs (#1209): every app must pass every check, and an app whose
check never reported fails rather than being skipped."""

import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "render-lab"))

import upgrade_check as uc  # noqa: E402


def row(app, **over):
    r = {"app": app, "prior_version": "2.1.0", "candidate_version": "2.2.0", "prior_started": True,
         "candidate_started": True, "settings_kept": True, "checkpoint_written": True,
         "checkpoint_recovered": True, "running": True}
    r.update(over)
    return json.dumps(r)


def test_every_app_passing_every_check_passes():
    md, failures = uc.judge([row(a) for a in uc.APPS])
    assert failures == []
    assert "❌" not in md
    assert "2.1.0 → 2.2.0" in md


def test_lost_settings_fail_and_say_why():
    _, failures = uc.judge([row("letters", settings_kept=False), row("tables"), row("decks")])
    assert failures == ["letters: settings changed in the prior release did not survive the upgrade"]


def test_a_checkpoint_never_written_fails_rather_than_passing_untested():
    _, failures = uc.judge([row("letters"), row("tables", checkpoint_written=False, checkpoint_recovered=False),
                            row("decks")])
    assert "tables: the prior release wrote no recovery checkpoint, so recovery was not tested" in failures


def test_a_checkpoint_not_taken_back_fails():
    _, failures = uc.judge([row("letters"), row("tables"), row("decks", checkpoint_recovered=False)])
    assert failures == ["decks: the candidate did not take back the prior release's checkpoint"]


def test_an_app_that_never_reported_fails():
    _, failures = uc.judge([row("letters"), row("tables")])
    assert failures == ["decks: the check did not finish"]


def test_no_report_at_all_fails_every_app(tmp_path):
    assert uc.main(["upgrade_check.py", str(tmp_path / "missing.json")]) == 1
