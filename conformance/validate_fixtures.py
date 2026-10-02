#!/usr/bin/env python3
"""Every committed fixture says where it came from and what it proves (#1275).

Each fixture set (a directory of committed test inputs) has a
`fixtures.json` beside its files. Per file it records the authoring app
and version, the format, the feature IDs it proves, the semantics the
round trip checks, the losses it is permitted (each naming its issue) and
the tests that use it. Files that sit in a fixture directory without
being fixtures (a README, a ratchet baseline) are listed under
`not_fixtures` with the reason.

Feature IDs are `<app>/<slug>`, with the slug taken from a PARITY.md
feature row ("ODS / CSV / TSV import" → `tables/ods-csv-tsv-import`) or a
render fixture PARITY.md cites (`letters/hyperlink`). So a fixture names
features the same way the parity table and the interop evidence table do.

Fails on:
  F1  a fixture directory with no manifest;
  F2  a file in the directory with no entry, or an entry with no file;
  F3  an entry missing a required field, or with an empty one;
  F4  an unknown feature ID;
  F5  a permitted loss with no issue number;
  F6  a `used_by` path that does not exist or never names the fixture.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "conformance"))

import validate_parity  # noqa: E402

# Directories whose committed files are test inputs. A tracked directory
# named `fixtures` or `corpus` is found automatically; these are the ones
# that must exist, so deleting one is noticed too.
FIXTURE_SETS = [
    "tests/gui/fixtures",
    "tests/fixtures",
    "tables-core/tests/fixtures",
    "tables-core/tests/corpus",
    "letters-core/tests/corpus",
    "decks-core/tests/corpus",
]
REQUIRED = ("author", "format", "features", "semantics", "permitted_losses", "used_by")
APP_RE = re.compile(r"^(letters|tables|decks)\b", re.I)


def slug(text: str) -> str:
    text = re.sub(r"\*\*", "", text).lower()
    return re.sub(r"[^a-z0-9]+", "-", text).strip("-")


def known_features(parity: Path) -> set[str]:
    ids: set[str] = set()
    for app, _tier, feature, _line, _cells in validate_parity.parse_parity(parity):
        m = APP_RE.match(app)
        if not m or feature == "Feature":
            continue
        ids.add(f"{m.group(1).lower()}/{slug(feature)}")
    for m2 in validate_parity.FIXTURE_RE.finditer(parity.read_text()):
        ids.add(m2.group(0))
    return ids


def tracked_dirs(root: Path) -> set[str]:
    try:
        out = subprocess.run(["git", "ls-files"], cwd=root, capture_output=True, text=True, check=True).stdout
    except (OSError, subprocess.CalledProcessError):
        return set()
    dirs = set()
    for path in out.splitlines():
        parent = str(Path(path).parent)
        if Path(parent).name in ("fixtures", "corpus") and not parent.startswith("tools/"):
            dirs.add(parent)
    return dirs


def check_set(root: Path, rel: str, features: set[str], errors: list[str]) -> None:
    folder = root / rel
    manifest_path = folder / "fixtures.json"
    if not manifest_path.exists():
        errors.append(f"F1 {rel}: no fixtures.json")
        return
    manifest = json.loads(manifest_path.read_text())
    entries = manifest.get("fixtures", {})
    ignored = manifest.get("not_fixtures", {})
    present = {p.name for p in folder.iterdir() if p.is_file() and p.name != "fixtures.json"
               and not p.name.startswith(".")}
    for name in sorted(present - set(entries) - set(ignored)):
        errors.append(f"F2 {rel}/{name}: no entry in fixtures.json")
    for name in sorted((set(entries) | set(ignored)) - present):
        errors.append(f"F2 {rel}/{name}: listed in fixtures.json but not present")
    for name, reason in ignored.items():
        if not str(reason).strip():
            errors.append(f"F3 {rel}/{name}: not_fixtures entry needs a reason")
    for name, entry in entries.items():
        where = f"{rel}/{name}"
        for key in REQUIRED:
            if key not in entry:
                errors.append(f"F3 {where}: missing {key}")
        author = entry.get("author") or {}
        if not str(author.get("app") or "").strip() or "version" not in author:
            errors.append(f"F3 {where}: author needs an app and a version (null when it has none)")
        if not str(entry.get("format") or "").strip():
            errors.append(f"F3 {where}: empty format")
        for key in ("features", "semantics", "used_by"):
            if key in entry and not entry[key]:
                errors.append(f"F3 {where}: empty {key}")
        for feature in entry.get("features", []):
            if feature not in features:
                errors.append(f"F4 {where}: unknown feature ID {feature!r}")
        for loss in entry.get("permitted_losses", []):
            if not isinstance(loss.get("issue"), int) or not str(loss.get("what") or "").strip():
                errors.append(f"F5 {where}: permitted loss needs `what` and an issue number: {loss}")
        for user in entry.get("used_by", []):
            path = root / user
            if not path.is_file():
                errors.append(f"F6 {where}: used_by {user} does not exist")
            elif name not in path.read_text(errors="replace"):
                errors.append(f"F6 {where}: {user} never names {name}")


def validate(root: Path = ROOT, sets: list[str] | None = None) -> list[str]:
    errors: list[str] = []
    features = known_features(root / "docs" / "PARITY.md")
    wanted = set(sets if sets is not None else FIXTURE_SETS) | (tracked_dirs(root) if sets is None else set())
    for rel in sorted(wanted):
        if not (root / rel).is_dir():
            errors.append(f"F1 {rel}: fixture directory is missing")
            continue
        check_set(root, rel, features, errors)
    return errors


def main() -> int:
    errors = validate()
    for e in errors:
        print(e)
    print(f"FIXTURE MANIFESTS {'FAIL' if errors else 'PASS'}: {len(errors)} problem(s)")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
