#!/usr/bin/env python3
"""Structural validator for docs/PARITY.md (tuna-os/gtk-office-suite #96/#153).

The parity scorecard is the suite's public trust signal: a green row claims a
feature is proven. This validator makes that claim machine-checkable in PR CI:

  E1  A row marked green (✅) must cite evidence — not an empty cell, and
      not a bare claim with no test/instrument/path reference.
  E2  Every path-like reference inside evidence (letters-core/…, tests/gui/…,
      conformance/…, …) must actually exist in the repository.
  E3  Journey-gated features (undo/redo, multi-sheet, preferences) may only
      be fully green when the evidence includes journey-level proof — an I6
      instrument and/or a tests/gui/ path — never model-only evidence.
  E4  (with --base) Status transitions vs. the base branch's PARITY.md:
      a row may not silently regress green→non-green, and may not jump to
      green without new evidence.

Exit code is non-zero on any violation, so PR CI can gate on it. Tier 3 rows
are out of scope (not in the scorecard denominator, per PARITY.md).

Usage:
  python3 conformance/validate_parity.py                 # E1–E3 only
  python3 conformance/validate_parity.py --base BASE.md  # + E4 transitions
"""

import argparse
import re
import sys
from pathlib import Path

# Status markers, in the order they appear in a row.
MARKERS = ("✅", "❌", "⚠️", "❓")
GREEN = "✅"

# Top-level dirs whose paths are checked for existence (E2). Anything else
# that contains a slash ("Run/RunStyle", "escape/entity") is prose, not a
# path, and is ignored so we don't false-positive on normal text.
KNOWN_DIRS = (
    "letters-core", "tables-core", "decks-core",
    "tests", "conformance", "scripts", "docs", "flatpak",
    "suite-common", "suite-common-core", "suite-export",
)
PATH_RE = re.compile(
    r"(?<![A-Za-z0-9_])(" + "|".join(KNOWN_DIRS) + r")/[A-Za-z0-9_./\-]+"
)

# A green claim must reference at least one of these (E1b).
EVIDENCE_RE = re.compile(
    r"\bI[1-7]\b"                       # instrument I1–I7
    r"|(letters|tables|decks)(-core)?\b"  # crate / test-harness names
    r"|\b(suite-export|suite-common)\b"
    r"|tests?|ratchet|oracle|corpus|baseline|smoke|journey"
    r"|round-?trip|fixture|scenario|AT-SPI|a11y|flatpak"
    r"|\d+/\d+",                        # ratchet numbers: 630/652, 107/107
    re.IGNORECASE,
)

# Features that must not look fully complete without journey-level evidence
# (#96 acceptance criterion; #153).
JOURNEY_GATED = ("undo/redo", "multi-sheet", "multi sheet", "preferences")

NON_GREEN = ("❌", "⚠️", "❓")


def strip_markers(s: str) -> str:
    for m in MARKERS:
        s = s.replace(m, "")
    return s.strip()


def norm(s: str) -> str:
    """Canonical form for comparing evidence across base/head."""
    s = re.sub(r"\*\*", "", s)
    s = strip_markers(s)
    s = re.sub(r"\s+", " ", s).strip()
    return s.lower()


def parse_parity(path: Path):
    """Yield (app, tier, feature, row_text, cells) for Tier 1/2 rows."""
    rows = []
    app = None
    tier = None
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if line.startswith("## "):
            app = line[3:].strip().split(" —")[0].strip()
            tier = None
            continue
        m = re.match(r"^### Tier ([12])", line)
        if m:
            tier = int(m.group(1))
            continue
        if re.match(r"^### Tier 3", line):
            tier = None
            continue
        if not line.startswith("|") or not app or not tier:
            continue
        if re.match(r"^\|[\s:\-|]+\|?$", line):  # |---|---| separator
            continue
        cells = [c.strip() for c in line.strip("|").split("|")]
        if len(cells) < 2:
            continue
        rows.append((app, tier, cells[0], line, cells))
    return rows


def has_marker(row_text: str) -> bool:
    return any(m in row_text for m in MARKERS)


def path_tokens(evidence: str):
    for m in PATH_RE.finditer(evidence):
        tok = m.group(0)
        # strip a trailing sentence-punctuation / closing paren
        tok = re.sub(r"[.,;:)\]}>]+$", "", tok)
        yield tok


def evidence_exists(evidence: str, repo_root: Path):
    """E2: every path-like reference must exist (file or dir prefix)."""
    missing = []
    for tok in path_tokens(evidence):
        p = repo_root / tok
        if p.exists():
            continue
        # Allow partial paths that are a prefix of a real file
        # (e.g. "letters-core/tests/corpus" with files under it).
        try:
            if any(str(f).startswith(tok) for f in repo_root.rglob("*")):
                continue
        except OSError:
            pass
        missing.append(tok)
    return missing


def gated_feature(feature: str) -> bool:
    f = re.sub(r"\s*\(.*\)$", "", feature).strip().lower()
    return any(g in f for g in JOURNEY_GATED)


def check_evidence(row, repo_root, errors, warnings):
    app, tier, feature, row_text, cells = row
    if not has_marker(row_text):
        return
    green = GREEN in row_text
    evidence = " ".join(cells[1:])
    bare = norm(evidence)

    # E1: green rows need actual evidence text.
    if green and not bare:
        errors.append(
            f"E1 {app} T{tier}: '{feature}' is marked green but the evidence "
            f"cell is empty — every green claim needs a test/instrument/path."
        )
        return

    if not green or not bare:
        return

    # E1b: evidence must reference a test/instrument/path.
    if not EVIDENCE_RE.search(bare):
        errors.append(
            f"E1 {app} T{tier}: '{feature}' is green but its evidence "
            f"('{evidence[:120]}') cites no test, instrument (I1–I7), corpus, "
            f"or file path."
        )

    # E2: referenced paths must exist.
    for tok in evidence_exists(evidence, repo_root):
        errors.append(
            f"E2 {app} T{tier}: '{feature}' cites '{tok}' which does not "
            f"exist in the repository."
        )

    # E3: journey-gated features need journey-level proof.
    if gated_feature(feature):
        journey = ("I6" in row_text) or ("tests/gui/" in evidence)
        if green and not journey:
            errors.append(
                f"E3 {app} T{tier}: '{feature}' is fully green on "
                f"model/oracle evidence only — undo/multi-sheet/preferences "
                f"must cite I6 or a tests/gui/ path before they can be marked "
                f"complete (#96)."
            )
        elif not green and journey:
            warnings.append(
                f"{app} T{tier}: '{feature}' is not green but already cites "
                f"journey evidence — consider marking it green."
            )


def build_index(rows):
    idx = {}
    for app, tier, feature, _, cells in rows:
        if not has_marker(" ".join(cells)):
            continue
        key = (app, tier, norm(feature))
        idx[key] = {
            "green": GREEN in " ".join(cells),
            "evidence": norm(" ".join(cells[1:])),
        }
    return idx


def check_transitions(base_path, head_rows, errors):
    base_idx = build_index(parse_parity(base_path))
    head_idx = build_index(head_rows)
    for key in sorted(set(base_idx) & set(head_idx)):
        b, h = base_idx[key], head_idx[key]
        feature = " | ".join(str(x) for x in key)
        if b["green"] and not h["green"]:
            if b["evidence"] == h["evidence"]:
                errors.append(
                    f"E4 {feature}: silently regressed green → non-green with "
                    f"unchanged evidence. Either fix the feature or update "
                    f"the row to explain the regression."
                )
        elif not b["green"] and h["green"]:
            if b["evidence"] == h["evidence"]:
                errors.append(
                    f"E4 {feature}: jumped to green without new evidence — a "
                    f"row can only go green when its evidence cell changes "
                    f"(add the test/instrument that proves it)."
                )


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--base", metavar="FILE", default=None,
                    help="base-branch docs/PARITY.md for transition checks (E4)")
    ap.add_argument("--repo-root", metavar="DIR", default=None,
                    help="repo root (default: parent of the script's directory)")
    ap.add_argument("--parity", metavar="FILE", default="docs/PARITY.md")
    args = ap.parse_args()

    repo_root = Path(args.repo_root) if args.repo_root else Path(__file__).resolve().parent.parent
    parity = repo_root / args.parity
    if not parity.exists():
        print(f"validate_parity: {parity} not found", file=sys.stderr)
        return 2

    rows = parse_parity(parity)
    errors, warnings = [], []
    for row in rows:
        check_evidence(row, repo_root, errors, warnings)

    if args.base:
        base = Path(args.base)
        if not base.exists():
            print(f"validate_parity: base file {base} not found", file=sys.stderr)
            return 2
        check_transitions(base, rows, errors)

    claims = [r for r in rows if has_marker(r[3])]
    green = sum(1 for r in claims if GREEN in r[3])
    file_refs = sum(len(list(path_tokens(" ".join(r[4][1:])))) for r in claims)

    for w in warnings:
        print(f"  note: {w}")
    for e in errors:
        print(f"  FAIL: {e}")

    status = "PASSED" if not errors else "FAILED"
    print(
        f"\nPARITY VALIDATION {status}: {len(claims)} claims ({green} green), "
        f"{file_refs} path references checked"
    )
    if warnings:
        print(f"  ({len(warnings)} note(s) — informational only)")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
