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


class ReleaseRevision(unittest.TestCase):
    """A release may not be certified by evidence a pull request never ran.

    The LibreOffice oracle needs LibreOffice installed, so it runs on a
    schedule and on format changes — never at every revision. Nothing
    connected that to a release: a tag inherited a verdict reached on
    different code, and the ledger recorded the claim as verified without
    noting that the two revisions disagreed. #313 names this directly.
    """

    RELEASE = "b" * 40
    LANES = {"lanes": [
        {"job": "test", "runs_on_every_revision": True,
         "namespaces": ["letters-core::"]},
        {"job": "oracle", "runs_on_every_revision": False,
         "namespaces": ["letters-core::soffice_oracle::"]},
    ]}

    def _ledger(self, revision, test="letters-core::soffice_oracle::round_trip"):
        return {"features": [{
            "id": "suite.oracle", "status": "verified", "revision": revision,
            "evidence": {"format": [test]},
        }]}

    def test_evidence_recorded_at_the_release_revision_passes(self):
        errors = vc.check_release_revision(
            self._ledger(self.RELEASE), self.LANES, self.RELEASE)
        self.assertEqual(errors, [])

    def test_a_short_revision_still_matches_the_release(self):
        errors = vc.check_release_revision(
            self._ledger(self.RELEASE[:12]), self.LANES, self.RELEASE)
        self.assertEqual(errors, [])

    def test_evidence_from_another_revision_is_refused(self):
        errors = vc.check_release_revision(
            self._ledger("a" * 40), self.LANES, self.RELEASE)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("does not run on every revision", errors[0])
        self.assertIn("would certify code that lane never saw", errors[0])

    def test_evidence_with_no_revision_at_all_is_refused(self):
        errors = vc.check_release_revision(self._ledger(""), self.LANES, self.RELEASE)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("(nothing)", errors[0])

    def test_a_claim_resting_only_on_every_revision_lanes_is_not_asked(self):
        """The rule is about lanes that can be stale, not about all claims:
        requiring every claim to be re-verified at each release would make
        the gate unusable, and it would say nothing about staleness."""
        ledger = {"features": [{
            "id": "letters.thing", "status": "verified", "revision": "a" * 40,
            "evidence": {"model": ["letters-core::mod::tests::name"]},
        }], }
        ledger["features"].append(self._ledger(self.RELEASE)["features"][0])
        self.assertEqual(
            vc.check_release_revision(ledger, self.LANES, self.RELEASE), [])

    def test_a_lane_nobody_cites_makes_the_check_vacuous_and_is_refused(self):
        """Otherwise this passes by having nothing to check — the failure
        mode the rest of this validator exists to prevent."""
        ledger = {"features": [{
            "id": "letters.thing", "status": "verified", "revision": "a" * 40,
            "evidence": {"model": ["letters-core::mod::tests::name"]},
        }]}
        errors = vc.check_release_revision(ledger, self.LANES, self.RELEASE)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("no claim cites it", errors[0])

    def test_a_lane_that_does_not_declare_the_property_is_refused(self):
        """A new lane must not slip in as trustworthy by omitting the
        field: absence would otherwise read as "runs everywhere"."""
        lanes = copy.deepcopy(self.LANES)
        del lanes["lanes"][1]["runs_on_every_revision"]
        errors = vc.check_release_revision(
            self._ledger(self.RELEASE), lanes, self.RELEASE)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("does not say whether it runs on every revision", errors[0])

    def test_a_map_where_every_lane_claims_every_revision_is_refused(self):
        lanes = {"lanes": [{"job": "test", "runs_on_every_revision": True,
                            "namespaces": ["letters-core::"]}]}
        errors = vc.check_release_revision(
            self._ledger(self.RELEASE), lanes, self.RELEASE)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("would certify nothing", errors[0])


class LaneExcludes(unittest.TestCase):
    """A lane may declare a namespace it does not run.

    nextest's JUnit names the crate, so a `letters-core::` inventory
    swallows `letters-core::soffice_oracle::...` too. On a pull request,
    where LibreOffice is not installed, those tests do not even skip — they
    return early and report *ok*. So without this the lane offers a pass
    from a run with no LibreOffice in it as evidence for a claim only the
    nightly oracle can support.
    """

    EXCLUDES = ("letters-core::soffice_oracle::",)
    INVENTORY = {"tests": ["letters-core::mod::tests::name"],
                 "covers": ["letters-core::"]}

    def _ledger(self):
        return {"features": [{
            "id": "suite.oracle", "status": "verified", "revision": "a" * 40,
            "evidence": {"format": ["letters-core::soffice_oracle::round_trip"]},
        }]}

    def test_an_excluded_test_is_not_judged_missing(self):
        self.assertEqual(
            vc.check_collected(self._ledger(), self.INVENTORY, False, self.EXCLUDES), [])

    def test_without_the_exclusion_it_is_judged_missing(self):
        errors = vc.check_collected(self._ledger(), self.INVENTORY, False)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("no collected test matches", errors[0])

    def test_an_excluded_pass_is_not_offered_as_this_lanes_evidence(self):
        """The dangerous direction: the lane reports a pass for a test that
        returned early, and the ledger would accept it."""
        results = {"letters-core::soffice_oracle::round_trip": "passed"}
        self.assertEqual(
            vc.check_results(self._ledger(), results, set(), ["letters-core::"],
                             self.EXCLUDES), [])
        # And it is still missing as far as this lane's inventory goes, so
        # nothing here silently counts it.
        self.assertEqual(
            vc.check_collected(self._ledger(), self.INVENTORY, False, self.EXCLUDES), [])

    def test_an_excluded_test_that_skipped_is_not_held_against_the_claim(self):
        results = {"letters-core::soffice_oracle::round_trip": "skipped"}
        self.assertEqual(
            vc.check_results(self._ledger(), results, set(), ["letters-core::"],
                             self.EXCLUDES), [])

    def test_without_the_exclusion_a_skip_sinks_the_claim(self):
        results = {"letters-core::soffice_oracle::round_trip": "skipped"}
        errors = vc.check_results(self._ledger(), results, set(), ["letters-core::"])
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("was skipped", errors[0])


class TheRealReleaseRule(unittest.TestCase):
    """The committed map and ledger have to work with the committed rule,
    or the check passes in the tests and fails in CI."""

    def _lanes(self):
        return json.loads((REPO_ROOT / "conformance/lanes.json").read_text())

    def test_every_lane_says_whether_it_runs_on_every_revision(self):
        for lane in self._lanes()["lanes"]:
            self.assertIsInstance(
                lane.get("runs_on_every_revision"), bool,
                f"lane {lane['job']!r} does not declare runs_on_every_revision")

    def test_at_least_one_lane_cannot_certify_a_release_on_its_own(self):
        """If every lane ran on every revision the rule would be vacuous —
        and the oracle lane genuinely does not."""
        stale = [lane for lane in self._lanes()["lanes"]
                 if not lane["runs_on_every_revision"]]
        self.assertTrue(stale, "no lane is marked as not running on every revision")

    def test_the_real_ledger_cites_every_such_lane(self):
        ledger = json.loads((REPO_ROOT / "conformance/capabilities.json").read_text())
        lanes = self._lanes()
        cited = {
            lane["job"]
            for lane in lanes["lanes"]
            if not lane["runs_on_every_revision"]
            for feature in ledger["features"]
            for tests in (feature.get("evidence") or {}).values()
            for test in tests
            if test.startswith(tuple(lane["namespaces"]))
        }
        uncited = [lane["job"] for lane in lanes["lanes"]
                   if not lane["runs_on_every_revision"] and lane["job"] not in cited]
        self.assertEqual(uncited, [], "a lane the release rule watches certifies nothing")

    def test_the_real_ledger_passes_at_the_revision_it_records(self):
        ledger = json.loads((REPO_ROOT / "conformance/capabilities.json").read_text())
        lanes = self._lanes()
        stale = [lane for lane in lanes["lanes"] if not lane["runs_on_every_revision"]]
        namespaces = tuple(n for lane in stale for n in lane["namespaces"])
        recorded = {
            feature["revision"]
            for feature in ledger["features"]
            for tests in (feature.get("evidence") or {}).values()
            for test in tests
            if test.startswith(namespaces)
        }
        self.assertEqual(len(recorded), 1, f"expected one oracle revision, got {recorded}")
        self.assertEqual(
            vc.check_release_revision(ledger, lanes, recorded.pop()), [])


if __name__ == "__main__":
    unittest.main()
