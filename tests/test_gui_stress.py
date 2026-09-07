"""Exercise stress orchestration without a display or GTK installation."""

import importlib.util
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("gui_stress", ROOT / "tests/gui/stress.py")
stress = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stress)


def test_seed_replays_order_and_matrix_covers_all_axes():
    cases = stress.scenarios(["letters", "tables", "decks"], 2, "display", 7)
    assert cases == stress.scenarios(["letters", "tables", "decks"], 2, "display", 7)
    assert cases != stress.scenarios(["letters", "tables", "decks"], 2, "display", 8)
    assert len(cases) == 108
    assert len({tuple(case.items()) for case in cases}) == 108


def test_failure_is_retained_when_later_attempt_passes(tmp_path):
    codes = iter([1, 0])

    def execute(command, env, log, timeout):
        assert env["GUI_TEST_REUSE_DISPLAY"] == "0"
        assert "--junitxml=" in command[-1]
        assert Path(env["GUI_TEST_ARTIFACT_DIR"]).parent == log.parent
        return next(codes)

    results = stress.run_cases(stress.scenarios(["tables"], 2, "baseline", 1),
                               tmp_path, 1, 10, execute)
    assert [r["exit_code"] for r in results] == [1, 0]
    assert json.loads((tmp_path / "results.json").read_text()) == results
    assert len(list(tmp_path.glob("*/result.json"))) == 2


def test_missing_runner_is_a_failure_with_evidence(tmp_path):
    def execute(*args):
        raise FileNotFoundError("missing runner")

    results = stress.run_cases(stress.scenarios(["letters"], 1, "baseline", 1),
                               tmp_path, 1, 10, execute)
    assert results[0]["exit_code"] == 127
    assert "missing runner" in (tmp_path / "0000-letters/runner.log").read_text()


def test_timeout_terminates_owned_process(tmp_path):
    import os
    code = stress.run_process(
        [sys.executable, "-c", "import time; time.sleep(30)"],
        os.environ.copy(), tmp_path / "timeout.log", 0.1,
    )
    assert code == 124
