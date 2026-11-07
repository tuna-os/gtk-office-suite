#!/usr/bin/env python3
"""Prepare a release commit on top of a revision whose evidence passed.

autorelease.yml runs the lanes that do not run on every revision (the
LibreOffice oracle, the collaboration journeys) at main's head X. When they
pass, this script writes the two things a release commit adds on top of X:

- the ledger's record that those lanes passed at X: every claim citing a
  scheduled lane (conformance/lanes.json, runs_on_every_revision: false)
  gets `revision: X`;
- an AppStream <release> for the new version in each app's metainfo, plus
  one for any earlier tag the file does not list yet. Release commits are
  tagged, not merged back to main, so main's metainfo would otherwise
  forget every automatic release before the latest.

Nothing else may change: validate_capabilities.py --release-revision
certifies the tagged commit with evidence recorded at X only when the two
differ in release metadata alone (RELEASE_METADATA there).

Usage:
  cut_release.py --version 2.2.0 --revision <X> [--date YYYY-MM-DD]
  cut_release.py --next-version {major,minor,patch}   # prints the version
"""

import argparse
import json
import re
import subprocess
import sys
from datetime import date, datetime, timezone
from pathlib import Path
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / "conformance" / "capabilities.json"
LANES = ROOT / "conformance" / "lanes.json"
APPS = ("letters", "tables", "decks")
TAG_RE = re.compile(r"v(\d+)\.(\d+)(?:\.(\d+))?")
# Subjects that say nothing to someone reading the release notes.
QUIET = re.compile(
    r"^(chore|ci|build|docs|test|tests|refactor|style|render-lab|conformance)\b"
    r"|^\w+\((deps|test|tests|ci|render-lab|conformance)\)|^Merge branch ", re.I)
MAX_NOTES = 12


def git(*args) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True,
                          text=True, check=True).stdout


def release_tags():
    """[(version tuple, tag)] for every v* tag, newest version first."""
    tags = []
    for tag in git("tag", "--list", "v*").split():
        m = TAG_RE.fullmatch(tag)
        if m:
            tags.append(((int(m[1]), int(m[2]), int(m[3] or 0)), tag))
    return sorted(tags, reverse=True)


def version_key(version: str):
    parts = [int(p) for p in re.findall(r"\d+", version)[:3]]
    return tuple(parts + [0] * (3 - len(parts)))


def next_version(bump: str) -> str:
    tags = release_tags()
    major, minor, patch = tags[0][0] if tags else (0, 0, 0)
    if bump == "major":
        return f"{major + 1}.0.0"
    if bump == "minor":
        return f"{major}.{minor + 1}.0"
    return f"{major}.{minor}.{patch + 1}"


def scheduled_claims(ledger: dict, lanes: dict) -> list:
    namespaces = tuple(
        ns for lane in lanes["lanes"] if lane.get("runs_on_every_revision") is False
        for ns in lane.get("namespaces", []))
    return [
        feature["id"] for feature in ledger["features"]
        if any(test.startswith(namespaces)
               for tests in (feature.get("evidence") or {}).values() for test in tests)
    ]


def record_revision(text: str, ids, revision: str) -> str:
    """Set each named claim's `revision`, leaving the file's layout alone."""
    for fid in ids:
        start = text.index(f'"id": {json.dumps(fid)}')
        end = text.find('"id": ', start + 1)
        end = len(text) if end < 0 else end
        block, n = re.subn(r'"revision": "[^"]*"', f'"revision": "{revision}"',
                           text[start:end], count=1)
        if n != 1:
            raise SystemExit(f"{fid}: no revision field to record {revision} in")
        text = text[:start] + block + text[end:]
    return text


def release_notes(since: str | None, revision: str) -> list:
    """Pull request titles merged since the last release, newest first."""
    span = f"{since}..{revision}" if since else revision
    notes = []
    for entry in git("log", "--first-parent", "--format=%s%x1f%b%x1e", span).split("\x1e"):
        if not entry.strip():
            continue
        subject, _, body = entry.strip().partition("\x1f")
        if subject.startswith("Merge pull request"):
            subject = next((line.strip() for line in body.splitlines() if line.strip()), "")
        subject = re.sub(r"\s*\(#\d+\)$", "", subject).strip()
        if subject and not QUIET.match(subject) and subject not in notes:
            notes.append(subject)
    return notes


def release_entry(version: str, when: str, notes, indent: str) -> str:
    inner = indent + "  "
    lines = [f'{indent}<release version="{version}" date="{when}">',
             f"{inner}<description>"]
    if notes:
        lines.append(f"{inner}  <p>Changes in this release:</p>")
        lines.append(f"{inner}  <ul>")
        lines += [f"{inner}    <li>{escape(n)}</li>" for n in notes[:MAX_NOTES]]
        if len(notes) > MAX_NOTES:
            lines.append(f"{inner}    <li>{len(notes) - MAX_NOTES} further changes</li>")
        lines.append(f"{inner}  </ul>")
    else:
        lines.append(f"{inner}  <p>Maintenance release.</p>")
    lines += [f"{inner}</description>", f"{indent}</release>"]
    return "\n".join(lines) + "\n"


def add_releases(text: str, entries) -> str:
    """Insert the (version, date, notes) entries newer than any the file
    lists, newest first. Older ones were written by hand on main."""
    m = re.search(r"^([ \t]*)<releases>\n", text, re.M)
    if not m:
        raise SystemExit("metainfo has no <releases> element")
    indent = m.group(1) + "  "
    listed = [version_key(v) for v in re.findall(r'<release version="([^"]+)"', text)]
    newest = max(listed, default=(0, 0, 0))
    block = "".join(release_entry(v, d, n, indent)
                    for v, d, n in entries if version_key(v) > newest)
    return text[:m.end()] + block + text[m.end():]


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--next-version", choices=("major", "minor", "patch"))
    parser.add_argument("--version")
    parser.add_argument("--revision")
    parser.add_argument("--date", default=datetime.now(timezone.utc).date().isoformat())
    args = parser.parse_args(argv)

    if args.next_version:
        print(next_version(args.next_version))
        return 0
    if not (args.version and args.revision):
        parser.error("--version and --revision are required")
    date.fromisoformat(args.date)
    revision = git("rev-parse", "--verify", f"{args.revision}^{{commit}}").strip()

    ledger_text = LEDGER.read_text()
    ids = scheduled_claims(json.loads(ledger_text), json.loads(LANES.read_text()))
    if not ids:
        raise SystemExit("no claim cites a scheduled lane: nothing to record")
    LEDGER.write_text(record_revision(ledger_text, ids, revision))

    tags = release_tags()
    previous = tags[0][1] if tags else None
    entries = [(args.version, args.date, release_notes(previous, revision))]
    # Earlier automatic releases, whose tagged commits never reached main.
    for _, tag in tags:
        when = git("log", "-1", "--format=%cs", tag).strip()
        entries.append((tag[1:], when, []))
    for app in APPS:
        path = ROOT / "flatpak" / f"org.tunaos.{app}.metainfo.xml"
        path.write_text(add_releases(path.read_text(), entries))

    print(f"recorded {revision[:12]} for {len(ids)} claim(s): {', '.join(ids)}")
    print(f"metainfo: release {args.version} ({args.date}), "
          f"{len(entries[0][2])} change(s) since {previous or 'the first commit'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
