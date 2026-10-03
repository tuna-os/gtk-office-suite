#!/usr/bin/env python3
"""Judge the prior-release upgrade checks Tier C ran (vm/upgrade.sh, #1209).

    upgrade_check.py <upgrade.json>

Each line of upgrade.json is one app: the prior release installed and used
(two settings changed, a document edited, the app killed with its recovery
checkpoint on disk), then the candidate installed over it and started.
Prints a Markdown table for the job summary and exits 1 unless every app
passed every check.
"""

import json
import sys

APPS = ("letters", "tables", "decks")

# Each check, and what it means when it fails.
CHECKS = (
    ("prior_started", "the prior release did not start"),
    ("checkpoint_written", "the prior release wrote no recovery checkpoint, so recovery was not tested"),
    ("settings_kept", "settings changed in the prior release did not survive the upgrade"),
    ("candidate_started", "the candidate did not start over the prior release's data"),
    ("checkpoint_recovered", "the candidate did not take back the prior release's checkpoint"),
    ("running", "the candidate was not running after recovery"),
)


def judge(lines: list) -> tuple:
    """(markdown, failures) for the rows in `lines`."""
    rows = {}
    for line in lines:
        line = line.strip()
        if line:
            row = json.loads(line)
            rows[row["app"]] = row
    failures = []
    md = ["## Upgrade from the prior release (#1209)", "",
          "| check | " + " | ".join(APPS) + " |", "|---|" + "---|" * len(APPS)]
    versions = [f"{rows[a].get('prior_version') or '?'} → {rows[a].get('candidate_version') or '?'}"
                if a in rows else "—" for a in APPS]
    md.append("| version | " + " | ".join(versions) + " |")
    for key, meaning in CHECKS:
        cells = []
        for app in APPS:
            if app not in rows:
                cells.append("—")
                continue
            ok = bool(rows[app].get(key))
            cells.append("✅" if ok else "❌")
            if not ok:
                failures.append(f"{app}: {meaning}")
        md.append(f"| {key} | " + " | ".join(cells) + " |")
    for app in APPS:
        if app not in rows:
            failures.append(f"{app}: the check did not finish")
    if failures:
        md += ["", "**Failing:**", ""] + [f"- {f}" for f in failures]
    return "\n".join(md) + "\n", failures


def main(argv: list) -> int:
    try:
        with open(argv[1]) as f:
            lines = f.readlines()
    except FileNotFoundError:
        # The VM step died before any upgrade ran: every app is unreported.
        lines = []
    md, failures = judge(lines)
    print(md)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
