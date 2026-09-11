#!/usr/bin/env python3
"""Validator for the capability evidence ledger (#441).

`conformance/capabilities.json` says, per capability, which tests prove it
at which layer and the revision that was observed at. This checks the
claims are real:

  C1  Structure: unique ids, a known status, a scope, an owner issue, and
      an evidence entry for every layer the capability says it requires.
  C2  A `verified` capability names the revision its evidence came from,
      and (with --head) that revision must be the one being validated —
      evidence from an older revision is stale, not proof.
  C3  (with --collected) Every test id must appear in the tests CI
      actually collected. This is the rule the old scorecard could not
      enforce: it checked that a *path* existed, which a deleted or
      renamed test leaves untouched.
  C4  (with --results) A test recorded as skipped, failed or missing
      cannot support a `verified` claim. A skipped LibreOffice oracle is
      the case that motivated this: it is green in the log and proves
      nothing.
  C5  Waivers: a `deferred` capability names an issue, a reason, a scope
      and a review date, and an expired review date is a failure.
  C6  Duplicate Python test classes or functions in the GUI suite: two
      classes with one name means the second silently replaces the first
      and its tests never run (the duplicate TablesNamedRangeSmoke that
      hid a regression).

Exit code is non-zero on any violation, so CI can gate on it.

Usage:
  python3 conformance/validate_capabilities.py                  # C1, C5, C6
  python3 conformance/validate_capabilities.py --head SHA       # + C2
  python3 conformance/validate_capabilities.py \\
      --collected inventory.json --layer model --layer format   # + C3
  python3 conformance/validate_capabilities.py --results results.json  # + C4
"""

import argparse
import ast
import json
import sys
from datetime import date
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
LEDGER = REPO_ROOT / "conformance" / "capabilities.json"
GUI_TEST_DIR = REPO_ROOT / "tests" / "gui"

STATUSES = {"verified", "implemented-unverified", "failing", "deferred"}
LAYERS = ("model", "format", "bridge", "gui", "a11y", "performance")
# Outcomes that may support a claim. Everything else — skipped, ignored,
# filtered out, not run — may not (C4).
PASSING = {"passed", "ok"}


def load_ledger(path: Path) -> dict:
    with open(path) as f:
        return json.load(f)


def check_structure(ledger: dict) -> list:
    """C1: the ledger says what it must say."""
    errors = []
    seen = set()
    for feature in ledger.get("features", []):
        fid = feature.get("id")
        if not fid:
            errors.append("a feature has no id")
            continue
        if fid in seen:
            errors.append(f"{fid}: duplicate feature id")
        seen.add(fid)

        status = feature.get("status")
        if status not in STATUSES:
            errors.append(f"{fid}: unknown status {status!r} (expected one of {sorted(STATUSES)})")
        if not str(feature.get("scope", "")).strip():
            errors.append(f"{fid}: no scope — an unbounded claim cannot be proven")
        if not isinstance(feature.get("issue"), int):
            errors.append(f"{fid}: no owner issue number")

        requires = feature.get("requires", [])
        evidence = feature.get("evidence", {})
        for layer in requires:
            if layer not in LAYERS:
                errors.append(f"{fid}: unknown layer {layer!r}")
            tests = evidence.get(layer) or []
            if status == "verified" and not tests:
                errors.append(f"{fid}: claims verified but names no {layer} test")
            if any(not str(t).strip() for t in tests):
                errors.append(f"{fid}: {layer} has an empty test id")
        for layer, tests in evidence.items():
            if layer not in requires:
                errors.append(f"{fid}: evidence for {layer!r} which it does not require")
            if len(set(tests)) != len(tests):
                errors.append(f"{fid}: {layer} lists the same test twice")
    return errors


def check_revisions(ledger: dict, head: str | None) -> list:
    """C2: verified claims carry the revision they were observed at."""
    errors = []
    for feature in ledger.get("features", []):
        if feature.get("status") != "verified":
            continue
        revision = str(feature.get("revision", "")).strip()
        fid = feature.get("id")
        if not revision:
            errors.append(f"{fid}: verified without naming the revision the evidence came from")
        elif head and not head.startswith(revision) and not revision.startswith(head):
            errors.append(
                f"{fid}: evidence recorded at {revision[:12]}, validating {head[:12]} — "
                "re-run the evidence or lower the status"
            )
    return errors


def check_collected(ledger: dict, inventory: dict, require_coverage: bool) -> list:
    """C3: every referenced test exists in what CI collected.

    An inventory declares the id namespaces it covers, so the job that ran
    the Rust tests can check the Rust ids without pretending to know
    anything about the pytest ones. `require_coverage` is for the job that
    holds every inventory: there, an id nobody covers is itself a finding —
    a claim resting on a test no lane runs.
    """
    errors = []
    collected = set(inventory.get("tests", []))
    covers = tuple(inventory.get("covers", []))
    for feature in ledger.get("features", []):
        for layer, tests in (feature.get("evidence") or {}).items():
            for test in tests:
                covered = not covers or test.startswith(covers)
                if not covered:
                    if require_coverage:
                        errors.append(
                            f"{feature.get('id')}: {layer} names {test!r}, which no test lane "
                            "collects — nothing would notice if it disappeared"
                        )
                    continue
                if test not in collected:
                    errors.append(
                        f"{feature.get('id')}: {layer} names {test!r}, which no collected "
                        "test matches (renamed, deleted, or never collected)"
                    )
    return errors


def check_lane_coverage(ledger: dict, lanes: dict) -> list:
    """C3b: every cited test falls in a namespace some CI lane collects.

    C3 compares cited tests against what a lane collected, but a lane only
    speaks for the namespaces it declares — so an id in a namespace *no*
    lane covers was skipped by every lane and accepted. Demonstrated on a
    real ledger: a claim citing
    `tests/nonexistent/test_phantom.py::PhantomSuite::test_nothing_runs_this`
    passed the GUI lane with "CAPABILITY LEDGER OK".

    `--require-coverage` catches it, but only when run against every
    lane's inventory at once — the job its documentation describes, which
    does not exist. This asks the same question of a committed map
    instead, so it needs no test run and can gate every pull request.
    """
    covered = tuple(
        namespace
        for lane in lanes.get("lanes", [])
        for namespace in lane.get("namespaces", [])
    )
    if not covered:
        return ["lane map declares no namespaces, so it can vouch for nothing"]
    errors = []
    for feature in ledger.get("features", []):
        for layer, tests in (feature.get("evidence") or {}).items():
            for test in tests:
                if not test.startswith(covered):
                    errors.append(
                        f"{feature.get('id')}: {layer} names {test!r}, which is in no "
                        "namespace any CI lane collects — nothing would notice if it "
                        "disappeared (see conformance/lanes.json)"
                    )
    return errors


def check_lane_declaration(lanes: dict, job: str, inventory: dict) -> list:
    """The lane map has to match what the lane really collected.

    Without this the map is a promise nothing keeps: a row could claim a
    job covers a namespace it stopped collecting, and C3b above would go
    on trusting it.
    """
    rows = [lane for lane in lanes.get("lanes", []) if lane.get("job") == job]
    if not rows:
        return [f"lane map has no row for job {job!r}, so its coverage is undeclared"]
    actual = tuple(inventory.get("covers", []))
    errors = []
    for row in rows:
        for namespace in row.get("namespaces", []):
            if not any(a.startswith(namespace) or namespace.startswith(a) for a in actual):
                errors.append(
                    f"lane {job!r} is declared to collect {namespace!r}, but this run "
                    f"collected {list(actual)!r} — the lane map is out of date"
                )
    return errors


def check_results(ledger: dict, results: dict, layers: set, covers=()) -> list:
    """C4: a skipped or failing test is not evidence.

    `covers` scopes the check to the namespaces the report speaks for, the
    same way check_collected does: a Rust report saying nothing about a
    pytest id means "not mine", not "never ran".
    """
    errors = []
    covers = tuple(covers)
    for feature in ledger.get("features", []):
        if feature.get("status") != "verified":
            continue
        for layer, tests in (feature.get("evidence") or {}).items():
            if layers and layer not in layers:
                continue
            for test in tests:
                if covers and not test.startswith(covers):
                    continue
                outcome = results.get(test)
                if outcome is None:
                    errors.append(f"{feature.get('id')}: {test} has no recorded result")
                elif str(outcome).lower() not in PASSING:
                    errors.append(
                        f"{feature.get('id')}: {test} was {outcome} — "
                        "that cannot support a verified claim"
                    )
    return errors


def check_waivers(ledger: dict, today: date) -> list:
    """C5: deferring something is a dated, owned decision."""
    errors = []
    waivers = {w.get("id"): w for w in ledger.get("waivers", [])}
    for feature in ledger.get("features", []):
        if feature.get("status") != "deferred":
            continue
        fid = feature.get("id")
        waiver = waivers.get(fid)
        if not waiver:
            errors.append(f"{fid}: deferred without a waiver")
            continue
        for field in ("issue", "reason", "scope", "review_date"):
            if not str(waiver.get(field, "")).strip():
                errors.append(f"{fid}: waiver has no {field}")
        review = str(waiver.get("review_date", "")).strip()
        if review:
            try:
                if date.fromisoformat(review) < today:
                    errors.append(f"{fid}: waiver review date {review} has passed")
            except ValueError:
                errors.append(f"{fid}: waiver review date {review!r} is not an ISO date")
    for wid in waivers:
        if wid not in {f.get("id") for f in ledger.get("features", [])}:
            errors.append(f"{wid}: waiver for a capability that is not in the ledger")
    return errors


def check_duplicate_tests(test_dir: Path) -> list:
    """C6: a duplicate class or function name silently replaces the first."""
    errors = []
    if not test_dir.is_dir():
        return errors
    for path in sorted(test_dir.glob("test_*.py")):
        try:
            tree = ast.parse(path.read_text(), filename=str(path))
        except SyntaxError as e:
            errors.append(f"{path.relative_to(REPO_ROOT)}: cannot parse ({e})")
            continue
        errors.extend(duplicate_definitions(tree, path.relative_to(REPO_ROOT)))
    return errors


def duplicate_definitions(tree: ast.AST, label) -> list:
    """Duplicate top-level classes, and duplicate methods within a class."""
    errors = []
    seen = {}
    for node in tree.body:
        if isinstance(node, (ast.ClassDef, ast.FunctionDef)):
            if node.name in seen:
                errors.append(
                    f"{label}: {node.name} defined twice (lines {seen[node.name]} and "
                    f"{node.lineno}) — the second replaces the first and its tests never run"
                )
            seen[node.name] = node.lineno
        if isinstance(node, ast.ClassDef):
            methods = {}
            for item in node.body:
                if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    if item.name in methods:
                        errors.append(
                            f"{label}: {node.name}.{item.name} defined twice "
                            f"(lines {methods[item.name]} and {item.lineno})"
                        )
                    methods[item.name] = item.lineno
    return errors


def load_inventory(path: Path) -> dict:
    with open(path) as f:
        data = json.load(f)
    if isinstance(data, list):        # a bare list covers everything it lists
        return {"covers": [], "tests": data}
    return data


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--ledger", type=Path, default=LEDGER)
    parser.add_argument("--head", help="revision being validated (enables the staleness check)")
    parser.add_argument("--collected", type=Path,
                        help="JSON inventory of test ids CI collected")
    parser.add_argument("--results", type=Path,
                        help="JSON mapping of test id to outcome")
    parser.add_argument("--layer", action="append", default=[],
                        help="restrict the --results check to this layer (repeatable)")
    parser.add_argument("--require-coverage", action="store_true",
                        help="with --collected: fail on any test id no lane collects")
    parser.add_argument("--gui-test-dir", type=Path, default=GUI_TEST_DIR)
    parser.add_argument("--lanes", type=Path,
                        help="lane map (conformance/lanes.json): every cited test must "
                             "be in a namespace some CI lane collects")
    parser.add_argument("--lane",
                        help="with --lanes and --collected: also assert this job's "
                             "declared namespaces match what it actually collected")
    args = parser.parse_args(argv)

    ledger = load_ledger(args.ledger)
    layers = set(args.layer)

    errors = []
    errors += check_structure(ledger)
    errors += check_revisions(ledger, args.head)
    errors += check_waivers(ledger, date.today())
    errors += check_duplicate_tests(args.gui_test_dir)
    lanes = json.loads(args.lanes.read_text()) if args.lanes else None
    if lanes is not None:
        errors += check_lane_coverage(ledger, lanes)
    if args.collected:
        inventory = load_inventory(args.collected)
        errors += check_collected(ledger, inventory, args.require_coverage)
        if lanes is not None and args.lane:
            errors += check_lane_declaration(lanes, args.lane, inventory)
    if args.results:
        with open(args.results) as f:
            results = json.load(f)
        covers = load_inventory(args.collected).get("covers", []) if args.collected else []
        errors += check_results(ledger, results, layers, covers)

    if errors:
        print("CAPABILITY LEDGER VALIDATION FAILED")
        for e in errors:
            print(f"  - {e}")
        return 1

    features = ledger.get("features", [])
    verified = [f for f in features if f.get("status") == "verified"]
    scope = f" ({', '.join(sorted(layers))} layers)" if layers else ""
    checked = "structure"
    if args.head:
        checked += ", revisions"
    if lanes is not None:
        checked += ", lane coverage"
    if args.collected:
        checked += ", collected tests"
    if args.results:
        checked += ", results"
    print(f"CAPABILITY LEDGER OK: {len(verified)}/{len(features)} verified; "
          f"checked {checked}{scope}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
