#!/usr/bin/env python3
"""Mutation tests for the capability ledger validator (#441).

A validator nobody tests is a validator that passes everything. Each test
here breaks the ledger in one specific way the contract says must be
rejected — a nonexistent test, a duplicated id, an omitted layer, a stale
revision, a skipped or failed result — and asserts the validator catches
exactly that. The last test asserts the honest ledger still passes, so the
rules cannot be satisfied by rejecting everything.

Dependency-free: no display, no GUI packages, no cargo.
"""

import copy
import json
import os
import sys
import tempfile
import unittest
from datetime import date, timedelta
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT / "conformance"))

import collect_test_inventory  # noqa: E402
import validate_capabilities as vc  # noqa: E402


def feature(**overrides) -> dict:
    base = {
        "id": "app.thing",
        "app": "letters",
        "title": "A thing",
        "scope": "What it does and does not cover.",
        "issue": 438,
        "status": "verified",
        "revision": "a" * 40,
        "requires": ["model", "gui"],
        "evidence": {
            "model": ["letters-core::mod::tests::name"],
            "gui": ["tests/gui/test_smoke.py::Class::test_name"],
        },
    }
    base.update(overrides)
    return base


def ledger(*features, waivers=None) -> dict:
    return {"schema": 1, "features": list(features), "waivers": waivers or []}


def messages(errors) -> str:
    return "\n".join(errors)


class StructureTests(unittest.TestCase):
    def test_the_honest_ledger_passes(self):
        self.assertEqual(vc.check_structure(ledger(feature())), [])

    def test_duplicate_feature_ids_are_rejected(self):
        errors = vc.check_structure(ledger(feature(), feature()))
        self.assertIn("duplicate feature id", messages(errors))

    def test_a_verified_claim_needs_a_test_at_every_required_layer(self):
        f = feature(requires=["model", "gui", "format"])
        errors = vc.check_structure(ledger(f))
        self.assertIn("names no format test", messages(errors))

    def test_an_empty_test_id_is_not_evidence(self):
        f = feature()
        f["evidence"]["model"] = ["  "]
        self.assertIn("empty test id", messages(vc.check_structure(ledger(f))))

    def test_unknown_status_is_rejected(self):
        self.assertIn("unknown status", messages(vc.check_structure(ledger(feature(status="done")))))

    def test_a_claim_without_scope_is_rejected(self):
        self.assertIn("no scope", messages(vc.check_structure(ledger(feature(scope="")))))

    def test_a_claim_without_an_owner_issue_is_rejected(self):
        self.assertIn("no owner issue", messages(vc.check_structure(ledger(feature(issue=None)))))

    def test_evidence_for_an_unrequired_layer_is_rejected(self):
        f = feature()
        f["evidence"]["performance"] = ["letters-core::perf::tests::x"]
        self.assertIn("which it does not require", messages(vc.check_structure(ledger(f))))

    def test_the_same_test_listed_twice_is_rejected(self):
        f = feature()
        f["evidence"]["model"] = ["letters-core::mod::tests::name"] * 2
        self.assertIn("the same test twice", messages(vc.check_structure(ledger(f))))


class RevisionTests(unittest.TestCase):
    def test_verified_without_a_revision_is_rejected(self):
        errors = vc.check_revisions(ledger(feature(revision="")), head=None)
        self.assertIn("without naming the revision", messages(errors))

    def test_stale_evidence_is_rejected(self):
        errors = vc.check_revisions(ledger(feature(revision="a" * 40)), head="b" * 40)
        self.assertIn("re-run the evidence", messages(errors))

    def test_current_evidence_passes(self):
        self.assertEqual(vc.check_revisions(ledger(feature(revision="a" * 40)), head="a" * 40), [])

    def test_an_unverified_claim_needs_no_revision(self):
        f = feature(status="implemented-unverified", revision="")
        self.assertEqual(vc.check_revisions(ledger(f), head="b" * 40), [])


class CollectedTests(unittest.TestCase):
    INVENTORY = {
        "covers": ["letters-core::", "tests/gui/"],
        "tests": ["letters-core::mod::tests::name",
                  "tests/gui/test_smoke.py::Class::test_name"],
    }

    def test_a_named_test_that_was_collected_passes(self):
        self.assertEqual(vc.check_collected(ledger(feature()), self.INVENTORY, False), [])

    def test_a_renamed_or_deleted_test_is_rejected(self):
        f = feature()
        f["evidence"]["model"] = ["letters-core::mod::tests::renamed_away"]
        errors = vc.check_collected(ledger(f), self.INVENTORY, False)
        self.assertIn("no collected test matches", messages(errors))

    def test_ids_outside_the_inventory_namespace_are_left_alone(self):
        # The Rust lane must not report pytest ids as missing.
        rust_only = {"covers": ["letters-core::"], "tests": ["letters-core::mod::tests::name"]}
        self.assertEqual(vc.check_collected(ledger(feature()), rust_only, False), [])

    def test_require_coverage_rejects_a_test_no_lane_collects(self):
        rust_only = {"covers": ["letters-core::"], "tests": ["letters-core::mod::tests::name"]}
        errors = vc.check_collected(ledger(feature()), rust_only, True)
        self.assertIn("no test lane collects", messages(errors))


class ResultTests(unittest.TestCase):
    def test_a_passing_result_supports_the_claim(self):
        results = {"letters-core::mod::tests::name": "passed",
                   "tests/gui/test_smoke.py::Class::test_name": "passed"}
        self.assertEqual(vc.check_results(ledger(feature()), results, set()), [])

    def test_a_skipped_test_cannot_support_a_verified_claim(self):
        results = {"letters-core::mod::tests::name": "skipped",
                   "tests/gui/test_smoke.py::Class::test_name": "passed"}
        errors = vc.check_results(ledger(feature()), results, set())
        self.assertIn("was skipped", messages(errors))

    def test_a_failed_test_cannot_support_a_verified_claim(self):
        results = {"letters-core::mod::tests::name": "failed",
                   "tests/gui/test_smoke.py::Class::test_name": "passed"}
        self.assertIn("was failed", messages(vc.check_results(ledger(feature()), results, set())))

    def test_a_test_that_never_ran_cannot_support_a_verified_claim(self):
        errors = vc.check_results(ledger(feature()), {}, set())
        self.assertIn("has no recorded result", messages(errors))

    def test_results_are_only_demanded_of_verified_claims(self):
        f = feature(status="failing")
        self.assertEqual(vc.check_results(ledger(f), {}, set()), [])


class WaiverTests(unittest.TestCase):
    def waiver(self, **overrides):
        base = {"id": "app.thing", "issue": 500, "reason": "not in scope yet",
                "scope": "the whole capability",
                "review_date": (date.today() + timedelta(days=30)).isoformat()}
        base.update(overrides)
        return base

    def test_a_deferred_capability_needs_a_waiver(self):
        errors = vc.check_waivers(ledger(feature(status="deferred")), date.today())
        self.assertIn("deferred without a waiver", messages(errors))

    def test_a_complete_waiver_passes(self):
        led = ledger(feature(status="deferred"), waivers=[self.waiver()])
        self.assertEqual(vc.check_waivers(led, date.today()), [])

    def test_an_expired_waiver_is_rejected(self):
        expired = self.waiver(review_date=(date.today() - timedelta(days=1)).isoformat())
        led = ledger(feature(status="deferred"), waivers=[expired])
        self.assertIn("has passed", messages(vc.check_waivers(led, date.today())))

    def test_a_waiver_missing_a_field_is_rejected(self):
        led = ledger(feature(status="deferred"), waivers=[self.waiver(reason="")])
        self.assertIn("waiver has no reason", messages(vc.check_waivers(led, date.today())))

    def test_a_waiver_for_an_unknown_capability_is_rejected(self):
        led = ledger(feature(), waivers=[self.waiver(id="app.ghost")])
        self.assertIn("not in the ledger", messages(vc.check_waivers(led, date.today())))


class DuplicateTestDetectionTests(unittest.TestCase):
    """C6 — the duplicate class that hid a crash reproduction (#507)."""

    def parse(self, source: str):
        import ast
        return vc.duplicate_definitions(ast.parse(source), "test_x.py")

    def test_duplicate_classes_are_rejected(self):
        errors = self.parse("class A:\n    pass\n\n\nclass A:\n    pass\n")
        self.assertIn("A defined twice", messages(errors))

    def test_duplicate_methods_within_a_class_are_rejected(self):
        errors = self.parse("class A:\n    def test_x(self):\n        pass\n"
                            "    def test_x(self):\n        pass\n")
        self.assertIn("A.test_x defined twice", messages(errors))

    def test_distinct_names_pass(self):
        self.assertEqual(self.parse("class A:\n    def test_x(self):\n        pass\n"
                                    "\n\nclass B:\n    def test_x(self):\n        pass\n"), [])


class JUnitParsingTests(unittest.TestCase):
    REPORT = """<?xml version="1.0"?>
    <testsuites name="nextest-run">
      <testsuite name="letters-core" tests="3">
        <testcase name="structured::tests::a" classname="letters-core"/>
        <testcase name="odt::tests::b" classname="letters-core"><skipped/></testcase>
        <testcase name="docx_round_trip" classname="letters-core::docx"><failure message="x"/></testcase>
      </testsuite>
      <testsuite name="letters" tests="1">
        <testcase name="bridge::tests::c" classname="letters"/>
      </testsuite>
    </testsuites>"""

    def test_outcomes_are_read_per_test(self):
        outcomes, _ = collect_test_inventory.parse_junit(self.REPORT)
        self.assertEqual(outcomes["letters-core::structured::tests::a"], "passed")
        self.assertEqual(outcomes["letters-core::odt::tests::b"], "skipped")
        self.assertEqual(outcomes["letters-core::docx::docx_round_trip"], "failed")
        self.assertEqual(outcomes["letters::bridge::tests::c"], "passed")

    def test_covered_namespaces_come_from_the_report(self):
        # A Rust report must not claim to speak for pytest ids, or a
        # renamed journey would pass unnoticed in the Rust lane.
        _, covers = collect_test_inventory.parse_junit(self.REPORT)
        self.assertEqual(covers, ["letters-core::", "letters::"])
        self.assertFalse("tests/gui/x.py::A::b".startswith(tuple(covers)))


class InventoryParsingTests(unittest.TestCase):
    def test_cargo_listing_is_parsed_and_prefixed(self):
        text = ("structured::tests::insert_table: test\n"
                "docx::tests::normalize: test\n"
                "some noise line\n"
                "benches::throughput: benchmark\n")
        ids = collect_test_inventory.parse_cargo(text, "letters-core")
        self.assertEqual(ids, ["letters-core::structured::tests::insert_table",
                               "letters-core::docx::tests::normalize",
                               "letters-core::benches::throughput"])

    def test_pytest_listing_is_parsed_and_made_repo_relative(self):
        text = ("test_smoke.py::LettersSmoke::test_launch_shows_window\n"
                "test_smoke.py::TablesSmoke::test_launch_shows_window\n"
                "\n46 tests collected in 0.23s\n")
        ids = collect_test_inventory.parse_pytest(text, "tests/gui")
        self.assertEqual(ids, [
            "tests/gui/test_smoke.py::LettersSmoke::test_launch_shows_window",
            "tests/gui/test_smoke.py::TablesSmoke::test_launch_shows_window",
        ])

    def test_an_already_relative_pytest_path_is_not_doubled(self):
        ids = collect_test_inventory.parse_pytest(
            "tests/gui/test_smoke.py::A::test_b\n", "tests/gui")
        self.assertEqual(ids, ["tests/gui/test_smoke.py::A::test_b"])


class RealLedgerTests(unittest.TestCase):
    """The ledger in the repository must satisfy its own validator."""

    def test_the_committed_ledger_is_structurally_valid(self):
        led = vc.load_ledger(vc.LEDGER)
        self.assertEqual(vc.check_structure(led), [])
        self.assertEqual(vc.check_revisions(led, None), [])
        self.assertEqual(vc.check_waivers(led, date.today()), [])

    def test_the_gui_suite_has_no_duplicate_definitions(self):
        self.assertEqual(vc.check_duplicate_tests(vc.GUI_TEST_DIR), [])

    def test_a_mutated_copy_of_the_real_ledger_is_rejected(self):
        # Guards against the validator passing the real file by accident:
        # break one entry and it must complain about that entry.
        led = copy.deepcopy(vc.load_ledger(vc.LEDGER))
        self.assertTrue(led["features"], "the ledger should not be empty")
        target = led["features"][0]
        target["evidence"][target["requires"][0]] = []
        errors = vc.check_structure(led)
        self.assertIn(target["id"], messages(errors))

    def test_main_exits_zero_on_the_committed_ledger(self):
        self.assertEqual(vc.main([]), 0)

    def test_main_exits_nonzero_on_a_broken_ledger(self):
        led = copy.deepcopy(vc.load_ledger(vc.LEDGER))
        led["features"][0]["status"] = "totally-done"
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "capabilities.json")
            with open(path, "w") as f:
                json.dump(led, f)
            self.assertEqual(vc.main(["--ledger", path]), 1)


class LaneCoverage(unittest.TestCase):
    """C3b: a cited test in a namespace no lane collects is a finding.

    The hole this closes, demonstrated on the real ledger before the
    check existed: a claim citing
    `tests/nonexistent/test_phantom.py::PhantomSuite::test_nothing_runs_this`
    passed the GUI lane with "CAPABILITY LEDGER OK", because the GUI
    inventory covers `tests/gui/` and an id outside a lane's namespace is
    skipped rather than judged. Every lane skipped it.
    """

    LANES = {"lanes": [
        {"job": "test", "namespaces": ["letters-core::"]},
        {"job": "smoke", "namespaces": ["tests/gui/"]},
    ]}

    def ledger(self, test):
        return {"features": [{"id": "f", "evidence": {"gui": [test]}}]}

    def test_a_test_in_a_covered_namespace_passes(self):
        errors = vc.check_lane_coverage(
            self.ledger("tests/gui/test_smoke.py::A::test_b"), self.LANES)
        self.assertEqual(errors, [])

    def test_a_test_no_lane_collects_is_rejected(self):
        errors = vc.check_lane_coverage(
            self.ledger("tests/nonexistent/test_phantom.py::A::test_b"), self.LANES)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("no namespace any CI lane collects", errors[0])

    def test_a_map_that_declares_nothing_vouches_for_nothing(self):
        errors = vc.check_lane_coverage(
            self.ledger("tests/gui/test_smoke.py::A::test_b"), {"lanes": []})
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("can vouch for nothing", errors[0])


class LaneDeclaration(unittest.TestCase):
    """The map has to match what the lane really collected, or it is a
    promise nothing keeps: a row could claim a namespace the job stopped
    collecting and the coverage check would go on trusting it."""

    LANES = {"lanes": [{"job": "smoke", "namespaces": ["tests/gui/"]}]}

    def test_a_declaration_the_run_backs_up_passes(self):
        errors = vc.check_lane_declaration(
            self.LANES, "smoke", {"covers": ["tests/gui/test_smoke.py::"]})
        self.assertEqual(errors, [])

    def test_a_declaration_the_run_does_not_back_up_is_rejected(self):
        errors = vc.check_lane_declaration(
            self.LANES, "smoke", {"covers": ["letters-core::"]})
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("the lane map is out of date", errors[0])

    def test_a_job_with_no_row_is_rejected(self):
        errors = vc.check_lane_declaration(
            self.LANES, "brand-new-lane", {"covers": ["tests/gui/test_smoke.py::"]})
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("undeclared", errors[0])


class TheRealLaneMap(unittest.TestCase):
    """The committed map has to actually cover the committed ledger —
    otherwise the check passes in the tests and fails in CI."""

    def test_every_claim_in_the_real_ledger_is_in_a_declared_namespace(self):
        ledger = json.loads((REPO_ROOT / "conformance/capabilities.json").read_text())
        lanes = json.loads((REPO_ROOT / "conformance/lanes.json").read_text())
        self.assertEqual(vc.check_lane_coverage(ledger, lanes), [])

    def test_no_lane_declares_a_bare_directory_under_tests(self):
        """`tests/` would vouch for every path beneath it, including one
        nothing collects — which is the hole, not the fix."""
        lanes = json.loads((REPO_ROOT / "conformance/lanes.json").read_text())
        for lane in lanes["lanes"]:
            for namespace in lane["namespaces"]:
                self.assertNotEqual(namespace, "tests/",
                                    f"lane {lane['job']!r} declares the whole tests/ tree")


if __name__ == "__main__":
    unittest.main()
