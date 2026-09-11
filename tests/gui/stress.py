#!/usr/bin/env python3
"""Seeded, replayable GUI journey campaigns (#442).

Repetition is an instrument of its own: a journey that passes once has
been observed once, and the crash campaign's question is how often it
passes. This runs the existing AT-SPI journeys over and over, in a
recorded order, across a bounded display matrix, and writes down enough
to replay exactly what happened — the revision, the binaries' checksums,
the seed, the order, every attempt's verdict, and the artifacts of the
failures.

Deliberately absent: retries. A second run that passes does not unfail
the first, so every attempt is recorded and any first-attempt failure
fails the campaign. Diagnostic reruns are a separate invocation with its
own output directory.

Usage:
    tests/gui/stress.py --repeat 20 --seed 20260907 --output /tmp/campaign
    tests/gui/stress.py --repeat 1 --matrix display --output /tmp/matrix
    tests/gui/stress.py --repeat 5 --select Tables --output /tmp/tables

Replay a recorded campaign by passing the same --seed, --repeat, --matrix
and --select: the order of each attempt is derived from them alone.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import shutil
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field, asdict

GUI_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(os.path.dirname(GUI_DIR))
RUNNER = os.path.join(GUI_DIR, "run_gui_tests.sh")
APPS = ("letters", "tables", "decks")


# ── display matrix ───────────────────────────────────────────────────
#
# Bounded on purpose: every config multiplies the campaign's wall time,
# and a matrix nobody can afford to run is not coverage. These are the
# axes the harness can actually set today — screen geometry, scale
# factor and the light/dark preference. High contrast is NOT covered
# here: it needs a theme the container does not ship, and claiming the
# axis with an env var that changes nothing would be worse than leaving
# it visibly undone (see docs/readiness-2026-09/crash-stress.md).


@dataclass(frozen=True)
class DisplayConfig:
    name: str
    screen: str = "1920x1080"
    scale: int = 1
    color_scheme: str = "default"

    def env(self) -> dict:
        env = {
            "GUI_TEST_SCREEN_SIZE": self.screen,
            "GUI_TEST_COLOR_SCHEME": self.color_scheme,
        }
        if self.scale != 1:
            env["GDK_SCALE"] = str(self.scale)
        return env


BASELINE = DisplayConfig("baseline")

# Narrow widths are the interesting ones: a 400px-wide window is where
# libadwaita's adaptive breakpoints and our own minimum sizes collide.
DISPLAY_MATRIX = (
    BASELINE,
    DisplayConfig("narrow-400", screen="400x800"),
    DisplayConfig("medium-800", screen="800x600"),
    DisplayConfig("wide-1280", screen="1280x1024"),
    DisplayConfig("baseline-dark", color_scheme="prefer-dark"),
    DisplayConfig("narrow-400-dark", screen="400x800", color_scheme="prefer-dark"),
    DisplayConfig("baseline-scale2", scale=2),
    DisplayConfig("wide-1280-scale2", screen="1280x1024", scale=2),
)

MATRICES = {
    "none": (BASELINE,),
    "display": DISPLAY_MATRIX,
}


# ── failure classification ───────────────────────────────────────────
#
# The campaign's useful output is not a pass count but a breakdown: a
# product crash and a flaky wait are both red, and treating them alike
# is how a crash ends up being "fixed" with a longer sleep.

CLASS_PASS = "pass"
CLASS_CRASH = "product-crash"
CLASS_ASSERTION = "assertion-mismatch"
CLASS_TIMEOUT = "timeout"
CLASS_INFRASTRUCTURE = "infrastructure"
CLASS_UNKNOWN = "unclassified"

# Markers of the application dying, as opposed to a test disagreeing
# with it. Anything here means a product crash even if the test framework
# reported the failure as an ordinary assertion, because that is what the
# assertion is about.
CRASH_MARKERS = (
    "malloc(): ",
    "free(): ",
    "double free",
    "corrupted size vs. prev_size",
    "Segmentation fault",
    "SIGSEGV",
    "SIGABRT",
    "AddressSanitizer",
    "the application no longer exists",
    "crashed",
    "died",
)

# Setup that never got as far as running a journey. These are real
# failures of the campaign, but they are not evidence about the apps.
INFRASTRUCTURE_MARKERS = (
    "Fatal server error",
    "Cannot open display",
    "error: could not compile",
    "No such file or directory: '/workspace",
    "ERROR collecting",
    "INTERNALERROR",
    "ModuleNotFoundError",
    "dogtail requires",
)


def classify(text: str, junit_failures: list, exit_code: int, timed_out: bool) -> str:
    """Classify one attempt from what it printed and what JUnit recorded."""
    if timed_out:
        return CLASS_TIMEOUT
    haystack = "\n".join([text] + [f.get("message", "") + f.get("text", "")
                                   for f in junit_failures])
    lowered = haystack.lower()
    if any(marker.lower() in lowered for marker in CRASH_MARKERS):
        return CLASS_CRASH
    if any(marker.lower() in lowered for marker in INFRASTRUCTURE_MARKERS):
        return CLASS_INFRASTRUCTURE
    if junit_failures:
        return CLASS_ASSERTION
    if exit_code == 0:
        return CLASS_PASS
    return CLASS_UNKNOWN


def parse_junit(path: str) -> tuple:
    """Return (cases, failures) from a pytest JUnit XML file.

    A campaign must not be silently scored on a file that was never
    written, so a missing or unparseable report is reported as such
    instead of as zero failures.
    """
    if not os.path.exists(path):
        return None, [{"message": f"no JUnit report at {path}", "text": ""}]
    try:
        root = ET.parse(path).getroot()
    except ET.ParseError as exc:
        return None, [{"message": f"unparseable JUnit report: {exc}", "text": ""}]
    cases, failures = 0, []
    for case in root.iter("testcase"):
        cases += 1
        for kind in ("failure", "error"):
            for node in case.findall(kind):
                failures.append({
                    "journey": f"{case.get('classname', '')}::{case.get('name', '')}",
                    "kind": kind,
                    "message": node.get("message", ""),
                    "text": (node.text or "")[-2000:],
                })
    return cases, failures


# ── environment ──────────────────────────────────────────────────────


def free_display_number(start: int = 90, limit: int = 200) -> int:
    """First X display number with no lock file and no socket.

    Attempts run one after another, but a killed predecessor can leave
    :99 locked — and the runner's Xvfb then fails into whatever display
    that lock belongs to. Picking a free number per attempt keeps one
    abandoned campaign from quietly invalidating the next.
    """
    for num in range(start, limit):
        if os.path.exists(f"/tmp/.X{num}-lock"):
            continue
        if os.path.exists(f"/tmp/.X11-unix/X{num}"):
            continue
        return num
    raise RuntimeError("no free X display number between "
                       f"{start} and {limit}")


def binary_digests() -> dict:
    """sha256 of the app binaries under test, so a manifest names them.

    "It passed on my machine" is not evidence unless the binary is
    identified; a rebuild between attempts would otherwise be invisible.
    """
    target = os.environ.get("CARGO_TARGET_DIR") or os.path.join(REPO_ROOT, "target")
    digests = {}
    for app in APPS:
        path = os.path.join(target, "debug", app)
        if not os.path.exists(path):
            digests[app] = None
            continue
        digest = hashlib.sha256()
        with open(path, "rb") as handle:
            for chunk in iter(lambda: handle.read(1 << 20), b""):
                digest.update(chunk)
        digests[app] = digest.hexdigest()
    return digests


def revision() -> dict:
    def git(*args):
        try:
            return subprocess.run(("git", *args), cwd=REPO_ROOT, check=True,
                                  capture_output=True, text=True).stdout.strip()
        except (subprocess.CalledProcessError, FileNotFoundError):
            return None
    return {
        "commit": git("rev-parse", "HEAD"),
        # A campaign run on uncommitted work is still useful, but it is
        # not replayable from the commit alone and must say so.
        "dirty": bool(git("status", "--porcelain")),
    }


# ── collection and ordering ──────────────────────────────────────────


def collect(selection: list, select: str | None, python_bin: str | None) -> list:
    """Ask pytest which journeys the selection names, in file order."""
    args = [RUNNER, *selection]
    if select:
        args += ["-k", select]
    # -q twice: the runner appends its own -v, and pytest's verbosity is
    # (-v count) minus (-q count), so one -q only gets back to the tree
    # listing while two gives the node ids.
    args += ["--collect-only", "-q", "-q"]
    env = dict(os.environ)
    env["GUI_TEST_DISPLAY_NUM"] = str(free_display_number())
    if python_bin:
        env["GUI_TEST_PYTHON"] = python_bin
    with tempfile.NamedTemporaryFile("w+", suffix=".txt") as sink:
        subprocess.run(args, cwd=GUI_DIR, env=env, stdout=sink, stderr=subprocess.STDOUT)
        sink.seek(0)
        text = sink.read()
    ids = [line.strip() for line in text.splitlines()
           if "::" in line and not line.startswith(("=", "-", " "))]
    if not ids:
        raise RuntimeError(f"collected no journeys\n{text}")
    return ids


def plan(journeys: list, repeat: int, seed: int, configs: tuple) -> list:
    """Attempts in the order they will run, derived only from the inputs.

    Each attempt shuffles the journeys with a per-attempt generator, so
    a campaign explores orderings (a journey that only fails after
    another one has run is a real class of bug) while still replaying
    identically from the same seed.
    """
    attempts = []
    for index in range(repeat):
        for config in configs:
            order = list(journeys)
            random.Random(f"{seed}:{index}:{config.name}").shuffle(order)
            attempts.append(Attempt(index=len(attempts) + 1, repetition=index + 1,
                                    config=config, order=order))
    return attempts


@dataclass
class Attempt:
    index: int
    repetition: int
    config: DisplayConfig
    order: list
    classification: str = ""
    exit_code: int | None = None
    duration_s: float = 0.0
    cases: int | None = None
    failures: list = field(default_factory=list)
    log: str = ""
    artifacts: str = ""

    def record(self) -> dict:
        data = asdict(self)
        data["config"] = self.config.name
        data["config_env"] = self.config.env()
        return data


# ── running ──────────────────────────────────────────────────────────


def run_attempt(attempt: Attempt, output: str, timeout: int,
                python_bin: str | None, video: str) -> Attempt:
    attempt_dir = os.path.join(output, f"attempt-{attempt.index:03d}")
    os.makedirs(attempt_dir, exist_ok=True)
    junit = os.path.join(attempt_dir, "junit.xml")
    log_path = os.path.join(attempt_dir, "run.log")

    env = dict(os.environ)
    env.update(attempt.config.env())
    env["GUI_TEST_DISPLAY_NUM"] = str(free_display_number())
    env["GUI_TEST_VIDEO"] = video
    env["GUI_TEST_VIDEO_DIR"] = os.path.join(attempt_dir, "videos")
    if python_bin:
        env["GUI_TEST_PYTHON"] = python_bin

    args = [RUNNER, *attempt.order, f"--junitxml={junit}"]
    started = time.monotonic()
    timed_out = False
    # Straight to a file, never through a pipe. The runner backgrounds an
    # X server and a window manager that inherit its stdout, so a pipe
    # stays open after pytest exits and reading it to EOF would hang the
    # campaign forever on the first attempt.
    with open(log_path, "w") as sink:
        try:
            proc = subprocess.run(args, cwd=GUI_DIR, env=env, timeout=timeout,
                                  stdout=sink, stderr=subprocess.STDOUT)
            attempt.exit_code = proc.returncode
        except subprocess.TimeoutExpired:
            timed_out = True
            attempt.exit_code = None
            sink.write(f"\n[campaign] killed after {timeout}s\n")
    attempt.duration_s = round(time.monotonic() - started, 2)
    with open(log_path, errors="replace") as handle:
        text = handle.read()
    attempt.log = os.path.relpath(log_path, output)

    attempt.cases, attempt.failures = parse_junit(junit)
    attempt.classification = classify(text, attempt.failures,
                                     attempt.exit_code or 0, timed_out)

    # Move, never copy: the harness writes failure artifacts to one fixed
    # directory, so leaving them there would let the next attempt
    # overwrite the evidence of this one.
    produced = os.path.join(GUI_DIR, "failure_artifacts")
    if os.path.isdir(produced) and os.listdir(produced):
        kept = os.path.join(attempt_dir, "failure_artifacts")
        if os.path.isdir(kept):
            shutil.rmtree(kept)
        shutil.move(produced, kept)
        attempt.artifacts = os.path.relpath(kept, output)
    return attempt


def write_manifest(path: str, manifest: dict) -> None:
    # Rewritten after every attempt: a campaign that is killed halfway
    # still has a readable record of what it observed.
    with open(path, "w") as handle:
        json.dump(manifest, handle, indent=2)
        handle.write("\n")


def summarize(attempts: list) -> dict:
    counts = {}
    for attempt in attempts:
        counts[attempt.classification] = counts.get(attempt.classification, 0) + 1
    failed = [a for a in attempts if a.classification != CLASS_PASS]
    return {
        "attempts": len(attempts),
        "failed_attempts": len(failed),
        # The headline number the roadmap asks for: of the attempts that
        # ran, how many were red the first (and only) time they ran.
        "first_attempt_failure_rate": (round(len(failed) / len(attempts), 4)
                                       if attempts else None),
        "by_classification": counts,
        "failing_journeys": sorted({f.get("journey", "?") for a in failed
                                    for f in a.failures}),
    }


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("selection", nargs="*", default=["test_smoke.py"],
                        help="journey files to draw from (default: test_smoke.py)")
    parser.add_argument("--repeat", type=int, default=20,
                        help="repetitions of the whole selection (default: 20)")
    parser.add_argument("--seed", type=int, default=None,
                        help="ordering seed; default is derived from the clock "
                             "and recorded in the manifest so it can be replayed")
    parser.add_argument("--select", default=None,
                        help="pytest -k expression narrowing the journeys")
    parser.add_argument("--matrix", choices=sorted(MATRICES), default="none",
                        help="display configurations to run each repetition on")
    parser.add_argument("--output", required=True,
                        help="directory to write the manifest and artifacts to; "
                             "must not already exist")
    parser.add_argument("--timeout", type=int, default=1800,
                        help="seconds allowed per attempt (default: 1800)")
    parser.add_argument("--video", choices=("off", "failures", "all"),
                        default="failures",
                        help="record journeys (default: failures)")
    parser.add_argument("--python", default=os.environ.get("GUI_TEST_PYTHON"),
                        help="interpreter dogtail is installed for")
    args = parser.parse_args(argv)

    if os.path.exists(args.output):
        parser.error(f"{args.output} already exists; campaigns never overwrite "
                     "their own evidence")
    os.makedirs(args.output)

    seed = args.seed if args.seed is not None else int(time.time())
    configs = MATRICES[args.matrix]
    journeys = collect(args.selection or ["test_smoke.py"], args.select, args.python)
    attempts = plan(journeys, args.repeat, seed, configs)

    manifest_path = os.path.join(args.output, "manifest.json")
    manifest = {
        "schema": 1,
        "seed": seed,
        "repeat": args.repeat,
        "matrix": args.matrix,
        "select": args.select,
        "selection": args.selection,
        "journeys": journeys,
        "revision": revision(),
        "binaries": binary_digests(),
        "environment": {
            "python": sys.version.split()[0],
            "interpreter": args.python or "/usr/bin/python3",
            "cargo_target_dir": os.environ.get("CARGO_TARGET_DIR"),
        },
        "started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "attempts": [],
    }
    write_manifest(manifest_path, manifest)

    print(f"campaign: {len(attempts)} attempt(s) over {len(journeys)} journey(s), "
          f"seed {seed}, matrix {args.matrix}")
    for attempt in attempts:
        print(f"  attempt {attempt.index}/{len(attempts)} "
              f"[{attempt.config.name}] ...", flush=True)
        run_attempt(attempt, args.output, args.timeout, args.python, args.video)
        print(f"    {attempt.classification} in {attempt.duration_s}s "
              f"({len(attempt.failures)} failure(s))", flush=True)
        manifest["attempts"].append(attempt.record())
        manifest["summary"] = summarize(attempts[:attempt.index])
        write_manifest(manifest_path, manifest)

    manifest["finished_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    manifest["summary"] = summarize(attempts)
    write_manifest(manifest_path, manifest)

    summary = manifest["summary"]
    print(json.dumps(summary, indent=2))
    print(f"manifest: {manifest_path}")
    return 1 if summary["failed_attempts"] else 0


if __name__ == "__main__":
    sys.exit(main())
