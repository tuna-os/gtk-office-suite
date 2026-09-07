#!/usr/bin/env python3
"""Repeat real GUI journeys in replayable order; any failed attempt fails the run.

This runner stresses existing journeys. Stateful random editing and I/O fault
injection are separate planned instruments; see docs/readiness-2026-09/.
"""

import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import random
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]


def scenarios(apps, repeats, matrix, seed):
    displays = [(1280, "Adwaita", 1)]
    if matrix == "display":
        displays = list(itertools.product(
            (400, 800, 1280), ("Adwaita", "Adwaita:dark", "HighContrast"), (1, 2)
        ))
    cases = [dict(app=app, repeat=repeat, width=width, theme=theme, scale=scale)
             for app in apps for repeat in range(repeats)
             for width, theme, scale in displays]
    random.Random(seed).shuffle(cases)
    return cases


def run_process(command, env, log_path, timeout):
    """Own the whole launched session, including Xvfb and application children."""
    with log_path.open("w") as log:
        process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=log,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        try:
            return process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            return 124
        finally:
            # Reap any descendants even if the runner exited before them.
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                pass
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()


def run_cases(cases, output, seed, timeout, execute=run_process):
    results = []
    for index, case in enumerate(cases):
        attempt = output / f"{index:04d}-{case['app']}"
        attempt.mkdir(parents=True)
        env = dict(os.environ, GUI_TEST_REUSE_DISPLAY="0",
                   GUI_TEST_WIDTH=str(case["width"]), GUI_TEST_THEME=case["theme"],
                   GUI_TEST_SCALE=str(case["scale"]),
                   GUI_TEST_ARTIFACT_DIR=str(attempt / "artifacts"))
        command = [str(ROOT / "tests/gui/run_gui_tests.sh"), "test_smoke.py",
                   "-k", case["app"].capitalize(),
                   f"--junitxml={attempt / 'junit.xml'}"]
        started = time.monotonic()
        try:
            code = execute(command, env, attempt / "runner.log", timeout)
        except OSError as error:
            (attempt / "runner.log").write_text(str(error))
            code = 127
        result = dict(case, seed=seed, command=command, exit_code=code,
                      seconds=round(time.monotonic() - started, 3))
        (attempt / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        results.append(result)
        # Persist after each attempt so interruption cannot erase prior failures.
        (output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        print(f"{index + 1}/{len(cases)} {case}: exit {code}", flush=True)
    return results


def positive_int(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", choices=("all", "letters", "tables", "decks"), default="all")
    parser.add_argument("--repeat", type=positive_int, default=3)
    parser.add_argument("--matrix", choices=("baseline", "display"), default="baseline")
    parser.add_argument("--seed", type=int, default=20260907)
    parser.add_argument("--timeout", type=positive_int, default=600)
    parser.add_argument("--output", type=Path, required=True,
                        help="new directory; existing evidence is never overwritten")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    status = subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True)
    apps = [args.app] if args.app != "all" else ["letters", "tables", "decks"]
    binaries = {}
    for app in apps:
        binary = ROOT / "target/debug" / app
        binaries[app] = hashlib.sha256(binary.read_bytes()).hexdigest() if binary.is_file() else None
    (output / "run.json").write_text(json.dumps(
        dict(revision=revision, worktree_status=status, binaries=binaries,
             seed=args.seed, matrix=args.matrix, repeats=args.repeat), indent=2) + "\n")
    results = run_cases(scenarios(apps, args.repeat, args.matrix, args.seed),
                        output, args.seed, args.timeout)
    return int(any(result["exit_code"] != 0 for result in results))


if __name__ == "__main__":
    raise SystemExit(main())
