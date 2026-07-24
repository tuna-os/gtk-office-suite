#!/usr/bin/env python3
"""Conformance scorecard — one table with every parity number.

Runs the corpus harnesses, reads the ratchet baselines, counts the
feature-status marks in docs/PARITY.md, and writes a markdown table to
stdout and (in CI) to $GITHUB_STEP_SUMMARY. Exits non-zero if any corpus
run regresses (the ratchet asserts do the real gating; this is reporting).

Usage: python3 conformance/scorecard.py [--no-run]
  --no-run  skip cargo invocations; report baselines + PARITY.md only
"""

import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def read_baseline(rel):
    try:
        with open(os.path.join(ROOT, rel)) as f:
            return int(f.read().strip())
    except OSError:
        return None


def run_corpus(package, test, pattern):
    """Run one corpus test, return (passed, total) parsed from output."""
    proc = subprocess.run(
        ["cargo", "test", "-p", package, "--test", test, "--", "--nocapture"],
        capture_output=True, text=True, cwd=ROOT,
    )
    out = proc.stdout + proc.stderr
    m = re.search(pattern, out)
    ok = proc.returncode == 0
    if m:
        return int(m.group(1)), int(m.group(2)), ok
    return None, None, ok


def parity_marks():
    """Count checked/total tier-1+2 feature rows per app in docs/PARITY.md."""
    with open(os.path.join(ROOT, "docs/PARITY.md")) as f:
        text = f.read()
    apps = {}
    current_app = None
    tier3 = False
    for line in text.splitlines():
        h = re.match(r"^## (Letters|Tables|Decks)", line)
        if h:
            current_app = h.group(1)
            apps[current_app] = [0, 0]
            tier3 = False
            continue
        if re.match(r"^### Tier 3", line):
            tier3 = True
            continue
        if re.match(r"^### Tier [12]", line):
            tier3 = False
            continue
        if current_app and not tier3 and line.startswith("|") and ("✅" in line or "❌" in line):
            apps[current_app][1] += 1
            # a row counts as green only if it has no ❌ (partials stay red)
            if "❌" not in line:
                apps[current_app][0] += 1
    return apps


def main():
    no_run = "--no-run" in sys.argv
    rows = []
    failed = False

    cm_base = read_baseline("letters-core/tests/corpus/roundtrip-baseline.txt")
    lo_base = read_baseline("letters-core/tests/corpus/lo-parity-baseline.txt")

    if no_run:
        rows.append(("CommonMark round-trip", f"baseline {cm_base}/652"))
        rows.append(("LO-authored parity (Letters)", f"baseline {lo_base}/104"))
    else:
        p, t, ok = run_corpus("letters-core", "markdown_corpus",
                              r"round-trip idempotence: (\d+)/(\d+)")
        failed |= not ok
        rows.append(("CommonMark round-trip", f"{p}/{t} (baseline {cm_base})"))
        p, t, ok = run_corpus("letters-core", "lo_parity",
                              r"LibreOffice-authored parity: (\d+)/(\d+)")
        failed |= not ok
        rows.append(("LO-authored parity (Letters)", f"{p}/{t} (baseline {lo_base})"))
        dk_base = read_baseline("decks-core/tests/corpus/lo-parity-baseline.txt")
        p, t, ok = run_corpus("decks-core", "lo_parity",
                              r"Decks LO-authored parity: (\d+)/(\d+)")
        failed |= not ok
        rows.append(("LO-authored parity (Decks)", f"{p}/{t} (baseline {dk_base})"))
        of_base = read_baseline("tables-core/tests/corpus/openformula-baseline.txt")
        p, t, ok = run_corpus("tables-core", "openformula",
                              r"OpenFormula conformance: (\d+)/(\d+)")
        failed |= not ok
        rows.append(("OpenFormula conformance (Tables)", f"{p}/{t} (baseline {of_base})"))

    for app, (done, total) in parity_marks().items():
        rows.append((f"{app} tier-1/2 features green", f"{done}/{total}"))

    lines = ["## Conformance scorecard", "", "| Metric | Value |", "|---|---|"]
    lines += [f"| {name} | {value} |" for name, value in rows]
    report = "\n".join(lines)

    print(report)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a") as f:
            f.write(report + "\n")

    # The step summary is only visible in that one Actions run's UI and
    # isn't downloadable/diffable across runs — also write a file (#108's
    # "capability-matrix artifacts are published") so CI can upload it
    # the same way junit-results already is.
    with open(os.path.join(ROOT, "conformance", "scorecard-latest.md"), "w") as f:
        f.write(report + "\n")

    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
