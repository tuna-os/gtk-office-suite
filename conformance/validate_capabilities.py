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
  C7  (with --release-critical, used by release-revision.yml) A capability
      marked `"release_critical": true` must be verified, or deferred under
      a waiver that passes C5; and with --results every one of its tests in
      the namespaces the report covers must have passed. A release whose
      release-critical test was skipped, failed or never ran is refused
      (#1207). `release_critical` must be a boolean wherever it appears.
      With --every-release-critical-result the report has to hold *every*
      release-critical test: one it does not cover is missing, not another
      lane's business (SH-001). And with --lanes, every release-critical
      test has to sit in a lane that runs on the release candidate
      (`runs_on_candidate`), or nothing at the candidate could prove it.
  C8  (with --lanes) Each evidence layer has its own namespaces
      (lanes.json `layers`): a core-crate test cannot be filed as `gui`
      evidence, nor a journey as `model`, and the GUI-facing layers may not
      share a namespace with the core layers (SH-001).
  C9  (with --lane and --collected) A lane's inventory is a record of one
      run at one revision. It must contain tests in the lane's namespaces,
      every collected test needs an outcome (unless the lane declares
      change-based selection and the inventory lists it as deselected), and
      with --candidate it must have been recorded at that revision. Missing,
      empty and stale inventories are errors, not successful empty sets.

Exit code is non-zero on any violation, so CI can gate on it.

Usage:
  python3 conformance/validate_capabilities.py                  # C1, C5, C6
  python3 conformance/validate_capabilities.py --head SHA       # + C2
  python3 conformance/validate_capabilities.py \\
      --collected inventory.json --layer model --layer format   # + C3
  python3 conformance/validate_capabilities.py --results results.json  # + C4
  python3 conformance/validate_capabilities.py --release-critical \\
      --collected inventory.json --results results.json              # + C7
  python3 conformance/validate_capabilities.py --lanes conformance/lanes.json  # + C3b, C7, C8
  python3 conformance/validate_capabilities.py --lanes conformance/lanes.json \\
      --lane test --collected inventory.json --results results.json \\
      --candidate SHA                                                  # + C9
"""

import argparse
import ast
import json
import re
import subprocess
import sys
from datetime import date
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
LEDGER = REPO_ROOT / "conformance" / "capabilities.json"
GUI_TEST_DIR = REPO_ROOT / "tests" / "gui"

STATUSES = {"verified", "implemented-unverified", "failing", "deferred"}
# `persistence` is the save transaction itself (atomic replace, recovery
# checkpoints): kept apart from `format` so a green round trip cannot stand
# in for an unproven save (#1207).
LAYERS = ("model", "format", "persistence", "bridge", "gui", "a11y", "performance")
# C8: evidence for the layers a user sees (an AT-SPI journey) may never share
# a namespace with the layers that run without a window, or a passing unit
# test would satisfy a journey requirement.
UI_LAYERS = ("gui", "a11y")
CORE_LAYERS = ("model", "format", "persistence", "bridge")
# Tests of the save transaction itself — the atomic replace, the autosave
# snapshot and its recovery. Filed under `model` or `format` they would let a
# green round trip stand in for an unproven save, which is why `persistence`
# exists (#1207); the check keeps them there.
SAVE_DURABILITY_RE = re.compile(r"::(atomic_save|autosave|save_transaction)s?::")
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

        if "release_critical" in feature and not isinstance(feature["release_critical"], bool):
            errors.append(f"{fid}: release_critical must be true or false")

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
            if layer in ("model", "format"):
                for test in tests:
                    if SAVE_DURABILITY_RE.search(str(test)):
                        errors.append(
                            f"{fid}: {test} is save-durability evidence and belongs to "
                            f"`persistence`, not `{layer}`"
                        )
            if layer not in requires:
                errors.append(f"{fid}: evidence for {layer!r} which it does not require")
            if len(set(tests)) != len(tests):
                errors.append(f"{fid}: {layer} lists the same test twice")
    return errors


def _same_revision(recorded: str, other: str) -> bool:
    """Whether two revisions name the same commit, allowing short forms."""
    if not recorded or not other:
        return False
    return other.startswith(recorded) or recorded.startswith(other)


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
        elif head and not _same_revision(revision, head):
            errors.append(
                f"{fid}: evidence recorded at {revision[:12]}, validating {head[:12]} — "
                "re-run the evidence or lower the status"
            )
    return errors


def lane_row(lanes: dict, job: str) -> dict:
    """The lane map's row for `job`, or an empty one."""
    for lane in (lanes or {}).get("lanes", []):
        if lane.get("job") == job:
            return lane
    return {}


def in_lane(row: dict, test: str) -> bool:
    """Whether `test` is in the lane's namespaces and not in its excludes."""
    excludes = tuple(row.get("excludes", []))
    return (test.startswith(tuple(row.get("namespaces", [])))
            and not (excludes and test.startswith(excludes)))


def lane_scope(lanes: dict, jobs) -> tuple:
    """The namespaces the named lanes collect, and the ones they exclude.

    A lane speaks for exactly its own namespaces: a crate-level `covers` in
    an inventory does not let the oracle's report pass judgement on the
    model tests in the same crate, and a test in the lane's namespace that
    the run did not collect is the lane's to answer for, whatever the
    inventory says it covers.
    """
    rows = [lane_row(lanes, job) for job in jobs]
    namespaces = tuple(n for row in rows for n in row.get("namespaces", []))
    excludes = tuple(n for row in rows for n in row.get("excludes", []))
    return namespaces, excludes


def check_collected(ledger: dict, inventory: dict, require_coverage: bool,
                    excluded=(), scope=None) -> list:
    """C3: every referenced test exists in what CI collected.

    An inventory declares the id namespaces it covers, so the job that ran
    the Rust tests can check the Rust ids without pretending to know
    anything about the pytest ones. `require_coverage` is for the job that
    holds every inventory: there, an id nobody covers is itself a finding —
    a claim resting on a test no lane runs.

    `scope` is the namespaces of the lane(s) being validated (lane_scope).
    With it, a cited test in those namespaces must have been collected
    whatever the inventory claims to cover — a run whose inventory quietly
    omits a file the lane is declared to collect would otherwise vouch for
    it by silence — and a test outside them is another lane's to judge.
    """
    errors = []
    collected = set(inventory.get("tests", []))
    covers = tuple(inventory.get("covers", []))
    excluded = tuple(excluded)
    for feature in ledger.get("features", []):
        for layer, tests in (feature.get("evidence") or {}).items():
            for test in tests:
                # A namespace this lane declares it does not run is not
                # this lane's to judge, even though a crate-level `covers`
                # swallows it (see lanes.json's excludes). Without this the
                # oracle targets' early return — which reports *ok*, not
                # skipped, when LibreOffice is absent — would count as
                # evidence here.
                if excluded and test.startswith(excluded):
                    continue
                if scope is not None:
                    if not test.startswith(tuple(scope)):
                        continue
                    if test not in collected:
                        errors.append(
                            f"{feature.get('id')}: {layer} names {test!r}, which is in this "
                            "lane's namespaces but its run did not collect it (renamed, "
                            "deleted, or the lane stopped collecting it)"
                        )
                    continue
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


def cited_tests(ledger: dict):
    """(feature, layer, test) for every test the ledger cites."""
    for feature in ledger.get("features", []):
        for layer, tests in (feature.get("evidence") or {}).items():
            for test in tests:
                yield feature, layer, test


def check_layer_namespaces(ledger: dict, lanes: dict) -> list:
    """C8: each layer is satisfied only by tests of that layer's kind.

    A layer is a label on the claim; without a namespace behind it a
    `letters-core::` unit test filed under `gui` satisfied a journey
    requirement, and every lane accepted it because the id was collected
    and passed. The lane map's `layers` names the id prefixes each layer
    may cite, and the GUI-facing layers may not share one with the core
    layers — otherwise the separation is a matter of which prefix happened
    to be listed first.
    """
    layers = (lanes or {}).get("layers")
    if not isinstance(layers, dict) or not layers:
        return ["lane map declares no per-layer namespaces (conformance/lanes.json "
                "`layers`), so a core test could stand in for a journey"]
    errors = []
    for layer in LAYERS:
        prefixes = layers.get(layer)
        if (not isinstance(prefixes, list) or not prefixes
                or any(not str(p).strip() for p in prefixes)):
            errors.append(f"layer {layer!r} declares no namespaces in the lane map, so "
                          "nothing could tell its evidence from another layer's")
    for layer in layers:
        if layer not in LAYERS:
            errors.append(f"lane map declares namespaces for unknown layer {layer!r}")
    for ui in UI_LAYERS:
        for core in CORE_LAYERS:
            for a in layers.get(ui) or []:
                for b in layers.get(core) or []:
                    if a.startswith(b) or b.startswith(a):
                        errors.append(
                            f"layer {ui!r} shares namespace {a!r} with {core!r} ({b!r}) — "
                            f"a {core} test could satisfy a {ui} requirement"
                        )
    if errors:
        return errors
    for feature, layer, test in cited_tests(ledger):
        prefixes = tuple(layers.get(layer) or ())
        if prefixes and not test.startswith(prefixes):
            errors.append(
                f"{feature.get('id')}: {test!r} is filed as {layer} evidence but is not in "
                f"that layer's namespaces {list(prefixes)} — a test of another layer "
                "cannot satisfy it"
            )
    return errors


def check_release_critical_lanes(ledger: dict, lanes: dict) -> list:
    """C7, statically: release-critical evidence runs on the candidate.

    `release-revision.yml` re-runs the lanes marked `runs_on_candidate` at
    the revision being released. A release-critical capability citing a
    test in any other lane — the scheduled oracle, or a validator test no
    release job runs — can only be "proven" by a result from some other
    revision, so it is refused here, on every pull request, rather than at
    tag time.
    """
    rows = (lanes or {}).get("lanes", [])
    undeclared = [row.get("job") for row in rows
                  if not isinstance(row.get("runs_on_candidate"), bool)]
    if undeclared:
        return [
            f"lane {job!r} does not say whether it runs on the release candidate "
            "(conformance/lanes.json: runs_on_candidate)"
            for job in undeclared
        ]
    candidate = [row for row in rows if row["runs_on_candidate"]]
    if not candidate:
        return ["no lane runs on the release candidate, so nothing release-critical "
                "could be proven at the revision being released"]
    errors = []
    for feature in ledger.get("features", []):
        if feature.get("release_critical") is not True or feature.get("status") == "deferred":
            continue
        for layer, tests in (feature.get("evidence") or {}).items():
            for test in tests:
                if not any(in_lane(row, test) for row in candidate):
                    errors.append(
                        f"{feature.get('id')}: release-critical, but {layer} test {test!r} "
                        "is in no lane that runs on the release candidate — no result at "
                        "the released revision could prove it"
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


def check_lane_inventory(lanes: dict, jobs, inventory: dict, results: dict | None,
                         candidate: str | None) -> list:
    """C9: a lane's inventory is a record of one complete run at one revision.

    Everything else here judges cited tests against an inventory, and holds
    vacuously when the inventory is empty: a lane whose collector found
    nothing, or whose run died before writing outcomes, offered "no failing
    evidence" and passed. So:

    - the run must have collected tests in the lane's namespaces;
    - with results, every collected test needs an outcome — a test that was
      collected and never reported did not run, whatever the step's exit
      code says. The one exception is a lane that declares change-based
      selection (`selective`), whose inventory names the tests it
      deliberately left out (`deselected`); those are reported, never
      counted as passing;
    - with a candidate revision, the inventory must say it was recorded at
      that revision. An older nightly's inventory is evidence about older
      code, not about the candidate.
    """
    jobs = list(jobs)
    rows = [lane_row(lanes, job) for job in jobs]
    label = "+".join(jobs)

    def in_scope(test):
        return any(in_lane(row, test) for row in rows)

    errors = []
    collected = sorted(t for t in inventory.get("tests", []) if in_scope(t))
    if not collected:
        errors.append(
            f"lane {label!r} collected no tests in its namespaces — an empty inventory "
            "is a missing result, not a pass"
        )
    deselected = set(inventory.get("deselected", []))
    if deselected:
        if not all(row.get("selective") is True for row in rows):
            errors.append(
                f"lane {label!r} lists {len(deselected)} deselected tests but does not "
                "declare change-based selection (conformance/lanes.json: selective), so "
                "an unrun test cannot be excused"
            )
        stray = sorted(deselected - set(inventory.get("tests", [])))
        if stray:
            errors.append(
                f"lane {label!r} deselected {len(stray)} tests it never collected, e.g. "
                f"{stray[0]!r} — a selection has to be taken from the collection"
            )
    if results is not None and collected:
        reported = [t for t in collected if t in results]
        if not reported:
            errors.append(
                f"lane {label!r} recorded no outcome for any of its {len(collected)} "
                "collected tests — a run that reported nothing is missing, not passing"
            )
        else:
            contradicted = sorted(deselected & set(results))
            if contradicted:
                errors.append(
                    f"lane {label!r} reported outcomes for {len(contradicted)} tests its "
                    f"selection says it left out, e.g. {contradicted[0]!r} — the selection "
                    "listing is not the one that ran"
                )
            unreported = [t for t in collected if t not in results and t not in deselected]
            if unreported:
                errors.append(
                    f"lane {label!r}: {len(unreported)} collected tests have no recorded "
                    f"outcome (the run stopped early or never ran them), e.g. "
                    f"{', '.join(repr(t) for t in unreported[:3])}"
                )
    if candidate is not None:
        errors += check_inventory_revision(inventory, candidate, f"lane {label!r}")
    return errors


def check_inventory_revision(inventory: dict, candidate: str, label: str) -> list:
    """An inventory certifies only the revision it was recorded at."""
    recorded = str(inventory.get("revision", "")).strip()
    if not recorded:
        return [f"{label}: the inventory does not say which revision it was recorded "
                f"at, so it cannot certify {candidate[:12]}"]
    if not _same_revision(recorded, candidate):
        return [f"{label}: the inventory was recorded at {recorded[:12]}, validating "
                f"{candidate[:12]} — results from another revision are stale, not "
                "evidence for this one"]
    return []


def deselected_citations(ledger: dict, inventory: dict) -> list:
    """Cited tests the lane's selection deliberately did not run here."""
    deselected = set(inventory.get("deselected", []))
    return sorted({(feature.get("id"), test)
                   for feature, _, test in cited_tests(ledger) if test in deselected})


# What a release commit may change on top of the revision its scheduled
# evidence was recorded at (#313 with autorelease.yml). A commit cannot
# contain its own SHA, so a ledger that records "the oracle passed at X" is
# necessarily committed at some Y != X. When X and Y differ only in the
# ledger itself and the AppStream release entries, the code the lane ran is
# the code being released, and Y may be certified by evidence recorded at X.
RELEASE_METADATA = (
    re.compile(r"conformance/capabilities\.json"),
    re.compile(r"flatpak/[^/]+\.metainfo\.xml"),
)


def only_release_metadata(paths) -> bool:
    """Whether every changed path is release metadata (none at all counts)."""
    return all(any(p.fullmatch(path) for p in RELEASE_METADATA) for path in paths)


def git_changed_paths(recorded: str, revision: str, cwd=None):
    """Files that differ between two revisions, or None when git can't say."""
    try:
        out = subprocess.run(
            ["git", "diff", "--name-only", "--no-renames", recorded, revision, "--"],
            capture_output=True, text=True, check=True, cwd=cwd,
        ).stdout
    except (OSError, subprocess.CalledProcessError):
        return None
    return [line for line in out.splitlines() if line]


def _metadata_only(changed_paths, recorded, revision, memo) -> bool:
    if changed_paths is None or not recorded:
        return False
    if recorded not in memo:
        paths = changed_paths(recorded, revision)
        memo[recorded] = paths is not None and only_release_metadata(paths)
    return memo[recorded]


def check_release_revision(ledger: dict, lanes: dict, revision: str,
                           changed_paths=None) -> list:
    """A release may not be certified by evidence a pull request never ran.

    The LibreOffice oracle and the parity corpus need LibreOffice
    installed, so they run nightly against whatever `main` was at 05:00,
    plus on pull requests that touch format or model code — neither of
    which is a run at the revision being released. Nothing connected that to a release: a tag cut
    at noon inherited a verdict reached on different code, and the ledger
    recorded the claim as verified without saying the two revisions
    disagreed. #313 asks for exactly this: "nightly-only execution must not
    certify a different release revision".

    So for every claim resting on a lane the map says does not run on
    every revision, the recorded revision has to be the revision being
    released. The fix when this fires is to run that lane at the
    release revision (nightly.yml accepts workflow_dispatch) and record
    what it found — not to lower the bar.

    The one exception is a release commit that changes nothing but release
    metadata (RELEASE_METADATA) on top of the recorded revision: the ledger
    cannot hold its own commit's SHA, so recording the lane's run and
    tagging always takes a second commit. *changed_paths(recorded,
    revision)* lists the files that differ, or returns None when that is
    unknown (and the evidence is then refused as before).

    A lane like that which no claim cites is also an error. Otherwise this
    check passes by having nothing to check, which is the failure mode the
    rest of this file exists to prevent.
    """
    rows = (lanes or {}).get("lanes", [])
    undeclared = [
        lane.get("job") for lane in rows
        if not isinstance(lane.get("runs_on_every_revision"), bool)
    ]
    if undeclared:
        return [
            f"lane {job!r} does not say whether it runs on every revision, so a "
            "release cannot tell whether its evidence is current "
            "(conformance/lanes.json: runs_on_every_revision)"
            for job in undeclared
        ]
    scheduled = [lane for lane in rows if not lane["runs_on_every_revision"]]
    if not scheduled:
        return [
            "every lane claims to run on every revision, so a release-revision check "
            "would certify nothing — declare the lanes that do not "
            "(conformance/lanes.json)"
        ]

    errors = []
    cited_lanes = set()
    memo = {}
    for lane in scheduled:
        namespaces = tuple(lane.get("namespaces", []))
        job = lane.get("job")
        if not namespaces:
            errors.append(
                f"lane {job!r} does not run on every revision but declares no namespaces"
            )
            continue
        for feature in ledger.get("features", []):
            tests = [
                test
                for tests in (feature.get("evidence") or {}).values()
                for test in tests
                if test.startswith(namespaces)
            ]
            if not tests:
                continue
            cited_lanes.add(job)
            recorded = str(feature.get("revision", "")).strip()
            if not _same_revision(recorded, revision) and not _metadata_only(
                    changed_paths, recorded, revision, memo):
                errors.append(
                    f"{feature.get('id')}: rests on the {job!r} lane, which does not run "
                    f"on every revision, and its evidence "
                    f"was recorded at {recorded[:12] or '(nothing)'} — releasing "
                    f"{revision[:12]} would certify code that lane never saw. Run it at "
                    f"the release revision and record what it found."
                )
    for lane in scheduled:
        if lane.get("job") not in cited_lanes and lane.get("namespaces"):
            errors.append(
                f"lane {lane.get('job')!r} does not run on every revision and no claim "
                "cites it, "
                "so this check has nothing to verify — either cite its evidence or stop "
                "declaring the lane"
            )
    return errors


def check_results(ledger: dict, results: dict, layers: set, covers=(),
                  excluded=(), scope=None, deselected=()) -> list:
    """C4: a skipped or failing test is not evidence.

    `covers` scopes the check to the namespaces the report speaks for, the
    same way check_collected does: a Rust report saying nothing about a
    pytest id means "not mine", not "never ran". `scope` (lane_scope) is
    stricter and replaces it: inside the lane's namespaces a missing result
    is an error whatever the report claims to cover. `deselected` are the
    tests a selective lane's inventory says it deliberately did not run;
    they are reported by the caller, and are never a pass.
    """
    errors = []
    covers = tuple(covers)
    excluded = tuple(excluded)
    deselected = set(deselected)
    for feature in ledger.get("features", []):
        if feature.get("status") != "verified":
            continue
        for layer, tests in (feature.get("evidence") or {}).items():
            if layers and layer not in layers:
                continue
            for test in tests:
                if scope is not None:
                    if not test.startswith(tuple(scope)):
                        continue
                elif covers and not test.startswith(covers):
                    continue
                if excluded and test.startswith(excluded):
                    continue
                if test in deselected and test not in results:
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


def check_release_critical(ledger: dict, results: dict | None, covers=(), excluded=(),
                           scope=None, complete=False) -> list:
    """C7: nothing release-critical ships unproven.

    A release-critical capability is verified, or deferred under a waiver
    (whose fields and date C5 checks). With *results*, every test it cites
    in the namespaces the report covers must have passed: a skipped test is
    a refused release, not a footnote.

    `complete` is for the job that runs every release-critical lane at the
    candidate (release-revision.yml): there no test is another lane's
    business. A test the report lacks never ran, and one in a namespace
    this run excludes reported an outcome that is not evidence — the
    oracle targets "pass" by returning early without LibreOffice.
    """
    errors = []
    critical = [f for f in ledger.get("features", []) if f.get("release_critical") is True]
    if not critical:
        errors.append("no capability is marked release_critical, so the release gate would "
                      "certify anything; mark the ones a release must not ship without")
    waived = {w.get("id") for w in ledger.get("waivers", [])}
    for feature in critical:
        fid, status = feature.get("id"), feature.get("status")
        if status == "deferred" and fid in waived:
            continue
        if status != "verified":
            errors.append(f"{fid}: release-critical but {status} — verify it, or defer it "
                          "under a dated waiver, before releasing")
            continue
        if results is None:
            continue
        if complete:
            for layer, tests in (feature.get("evidence") or {}).items():
                for test in tests:
                    if excluded and test.startswith(tuple(excluded)):
                        errors.append(
                            f"release-critical {fid}: {test} is in a namespace this run "
                            "excludes, so its outcome here is not evidence"
                        )
                        continue
                    outcome = results.get(test)
                    if outcome is None:
                        errors.append(f"release-critical {fid}: {test} has no recorded result "
                                      "at this revision — it never ran")
                    elif str(outcome).lower() not in PASSING:
                        errors.append(f"release-critical {fid}: {test} was {outcome}")
            continue
        sub = {"schema": 1, "features": [feature], "waivers": []}
        errors += [f"release-critical {e}"
                   for e in check_results(sub, results, set(), covers, excluded, scope)]
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


def load_report(path: Path, loader, errors: list):
    """Load a lane's report, or record that the lane never wrote it.

    A report that is not there is a run that did not finish, so it is a
    finding rather than a traceback — and certainly not an empty set.
    """
    try:
        return loader(path)
    except FileNotFoundError:
        errors.append(f"{path}: no such report — a lane that did not write its results "
                      "has not passed")
    except (json.JSONDecodeError, UnicodeDecodeError) as e:
        errors.append(f"{path}: unreadable report ({e})")
    return None


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
                             "be in a namespace some CI lane collects, in its layer's "
                             "namespaces, and release-critical ones in a candidate lane")
    parser.add_argument("--lane", action="append", default=[],
                        help="with --lanes and --collected: judge exactly this job's "
                             "namespaces, assert its declaration matches what it collected, "
                             "and require a complete inventory (repeatable)")
    parser.add_argument("--candidate",
                        help="with --collected: the revision this run is evidence for; an "
                             "inventory recorded at another revision, or at none, is stale")
    parser.add_argument("--release-critical", action="store_true",
                        help="C7: refuse unless every release-critical capability is verified "
                             "(or waived) and, with --results, every one of its tests passed")
    parser.add_argument("--every-release-critical-result", action="store_true",
                        help="with --release-critical and --results: every release-critical "
                             "test must have passed in this report; none is another lane's")
    parser.add_argument("--release-revision",
                        help="with --lanes: refuse to certify this revision with claims "
                             "whose evidence comes from a lane that does not run on "
                             "pull requests unless it was recorded at this revision")
    args = parser.parse_args(argv)
    if args.lane and (args.lanes is None or args.collected is None):
        parser.error("--lane needs --lanes and --collected: a lane judged without its map "
                     "or its inventory proves nothing")
    if args.candidate and args.collected is None:
        parser.error("--candidate needs --collected: it is the inventory's revision that "
                     "is checked")
    if args.every_release_critical_result and not (args.release_critical and args.results):
        parser.error("--every-release-critical-result needs --release-critical and --results")

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
        errors += check_layer_namespaces(ledger, lanes)
        errors += check_release_critical_lanes(ledger, lanes)
    if args.release_revision:
        if lanes is None:
            parser.error("--release-revision needs --lanes to know which lanes are scheduled")
        errors += check_release_revision(ledger, lanes, args.release_revision,
                                         changed_paths=git_changed_paths)
    scope, excluded = lane_scope(lanes, args.lane) if args.lane else (None, ())
    inventory = None
    if args.collected:
        inventory = load_report(args.collected, load_inventory, errors)
    results = None
    if args.results:
        results = load_report(args.results, lambda p: json.loads(Path(p).read_text()), errors)
    covers = inventory.get("covers", []) if inventory else []
    deselected = inventory.get("deselected", []) if inventory else []
    if inventory is not None:
        errors += check_collected(ledger, inventory, args.require_coverage, excluded, scope)
        for job in args.lane:
            errors += check_lane_declaration(lanes, job, inventory)
        if args.lane:
            errors += check_lane_inventory(lanes, args.lane, inventory, results,
                                           args.candidate)
        elif args.candidate:
            errors += check_inventory_revision(inventory, args.candidate, "inventory")
    if results is not None:
        errors += check_results(ledger, results, layers, covers, excluded, scope, deselected)
    if args.release_critical:
        errors += check_release_critical(ledger, results, covers, excluded, scope,
                                         complete=args.every_release_critical_result)

    if inventory is not None:
        unrun = deselected_citations(ledger, inventory)
        if unrun:
            print(f"Not selected on this revision ({len(unrun)} cited tests; the lane's "
                  "change-based selection left them out, so this run is not evidence "
                  "for them):")
            for fid, test in unrun:
                print(f"  - {fid}: {test}")
    if errors:
        print("CAPABILITY LEDGER VALIDATION FAILED")
        for e in errors:
            print(f"  - {e}")
        return 1

    features = ledger.get("features", [])
    verified = [f for f in features if f.get("status") == "verified"]
    layer_scope = f" ({', '.join(sorted(layers))} layers)" if layers else ""
    checked = "structure"
    if args.head:
        checked += ", revisions"
    if lanes is not None:
        checked += ", lane coverage, layer namespaces, candidate lanes"
    if args.release_revision:
        checked += f", release revision {args.release_revision[:12]}"
    if args.collected:
        checked += ", collected tests"
    if args.lane:
        checked += f", {'+'.join(args.lane)} lane inventory"
    if args.candidate:
        checked += f" at {args.candidate[:12]}"
    if args.results:
        checked += ", results"
    if args.release_critical:
        critical = sum(1 for f in features if f.get("release_critical") is True)
        checked += f", {critical} release-critical"
        if args.every_release_critical_result:
            checked += " (every result required)"
    print(f"CAPABILITY LEDGER OK: {len(verified)}/{len(features)} verified; "
          f"checked {checked}{layer_scope}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
