#!/usr/bin/env python3
"""Judge the installed-Flatpak checks Tier C ran (vm/installed.sh, #1209).

    installed_check.py <installed.json>

Each line of installed.json is one app opened from "the file manager"
(gio open) and saved through the document portal, or the same app's file
chooser round: Save As and Open through the portal's dialogs. Lines for
one app are merged. Prints a Markdown table for the job summary and exits
1 unless every app passed every check.
"""

import json
import sys

APPS = ("letters", "tables", "decks")

# Each check, and what it means when it fails.
CHECKS = (
    ("registered", "the desktop entry is not registered for the document's MIME type"),
    ("opened", "`gio open` did not open the document in the app"),
    ("saved", "Save did not replace the document through the portal"),
    ("valid_after_save", "the saved document is not a valid archive"),
    ("no_leftovers", "the save left a temporary file beside the document"),
    ("running_after_save", "the app was not running after the save"),
    ("in_recent_files", "the opened document is not in the app's recent files"),
    ("portal_saved", "Save As through the file chooser portal did not write the named file"),
    ("portal_valid", "the document saved through the file chooser is not a valid archive"),
    ("portal_opened", "Open through the file chooser portal did not open the document"),
)


def verdicts(row: dict) -> dict:
    """Each check's result for one app's row."""
    out = {key: bool(row.get(key)) for key, _ in CHECKS}
    out["no_leftovers"] = row.get("save_leftovers") == 0
    return out


def judge(lines: list) -> tuple:
    """(markdown, failures) for the rows in `lines`."""
    rows = {}
    for line in lines:
        line = line.strip()
        if line:
            row = json.loads(line)
            rows.setdefault(row["app"], {}).update(row)
    failures = []
    md = ["## Installed Flatpak (#1209)", "", "| check | " + " | ".join(APPS) + " |", "|---|" + "---|" * len(APPS)]
    for key, meaning in CHECKS:
        cells = []
        for app in APPS:
            if app not in rows:
                cells.append("—")
                continue
            ok = verdicts(rows[app])[key]
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
        # The VM step died before any check ran: every app is unreported.
        lines = []
    md, failures = judge(lines)
    print(md)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
