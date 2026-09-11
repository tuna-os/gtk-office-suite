"""Tests for the GUI stress campaign runner's own logic (#442, #313).

The campaign orchestrates real applications, which this cannot do — but
the parts that decide what a run *means* are pure functions, and those
are exactly the parts that must not be wrong. A classifier that calls a
crash a flake, or a plan that is not reproducible from its seed, would
make every campaign after it worthless while still looking green.

Dependency-free and GUI-free on purpose: this runs in the fast PR lane.
"""

import importlib.util
import json
import os
import sys
import textwrap
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _load_stress():
    path = os.path.join(REPO_ROOT, "tests", "gui", "stress.py")
    spec = importlib.util.spec_from_file_location("gui_stress", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules["gui_stress"] = module
    spec.loader.exec_module(module)
    return module


stress = _load_stress()


class ClassificationTests(unittest.TestCase):
    def test_a_clean_run_is_a_pass(self):
        self.assertEqual(stress.classify("18 passed", [], 0, False), stress.CLASS_PASS)

    def test_glibc_heap_message_is_a_product_crash(self):
        # The #507 signature. This arrives on the app's stderr while the
        # test itself fails on a missing widget, so a classifier that
        # only looked at the assertion would file a crash as a flake.
        text = "malloc(): unaligned fastbin chunk detected"
        failures = [{"message": "AssertionError: no stats label found", "text": ""}]
        self.assertEqual(stress.classify(text, failures, 1, False), stress.CLASS_CRASH)

    def test_dead_application_is_a_product_crash(self):
        failures = [{"message": "atspi_error: The application no longer exists",
                     "text": ""}]
        self.assertEqual(stress.classify("", failures, 1, False), stress.CLASS_CRASH)

    def test_sanitizer_report_is_a_product_crash(self):
        text = "==123==ERROR: AddressSanitizer: heap-use-after-free"
        self.assertEqual(stress.classify(text, [], 1, False), stress.CLASS_CRASH)

    def test_plain_assertion_is_an_assertion_mismatch(self):
        failures = [{"message": "AssertionError: 3 != 4", "text": ""}]
        self.assertEqual(stress.classify("", failures, 1, False),
                         stress.CLASS_ASSERTION)

    def test_timeout_outranks_everything(self):
        failures = [{"message": "AssertionError", "text": ""}]
        self.assertEqual(stress.classify("malloc(): bad", failures, 1, True),
                         stress.CLASS_TIMEOUT)

    def test_missing_display_is_infrastructure_not_product(self):
        text = "Fatal server error:\nServer is already active for display 99"
        self.assertEqual(stress.classify(text, [], 1, False),
                         stress.CLASS_INFRASTRUCTURE)

    def test_collection_error_is_infrastructure(self):
        failures = [{"message": "ERROR collecting test_smoke.py", "text": ""}]
        self.assertEqual(stress.classify("", failures, 2, False),
                         stress.CLASS_INFRASTRUCTURE)

    def test_nonzero_exit_with_no_evidence_is_not_silently_a_pass(self):
        self.assertEqual(stress.classify("", [], 1, False), stress.CLASS_UNKNOWN)


class JUnitTests(unittest.TestCase):
    def setUp(self):
        import tempfile
        self.dir = tempfile.mkdtemp(prefix="stress-junit-")

    def _write(self, name, body):
        path = os.path.join(self.dir, name)
        with open(path, "w") as handle:
            handle.write(body)
        return path

    def test_failures_are_read_with_their_journey_ids(self):
        path = self._write("ok.xml", textwrap.dedent("""\
            <testsuites><testsuite name="pytest" tests="2">
              <testcase classname="test_smoke.TablesSmoke" name="test_a"/>
              <testcase classname="test_smoke.TablesSmoke" name="test_b">
                <failure message="AssertionError: boom">trace</failure>
              </testcase>
            </testsuite></testsuites>
        """))
        cases, failures = stress.parse_junit(path)
        self.assertEqual(cases, 2)
        self.assertEqual(len(failures), 1)
        self.assertEqual(failures[0]["journey"],
                         "test_smoke.TablesSmoke::test_b")

    def test_errors_count_as_failures(self):
        path = self._write("err.xml", textwrap.dedent("""\
            <testsuites><testsuite name="pytest" tests="1">
              <testcase classname="c" name="t"><error message="setup died"/></testcase>
            </testsuite></testsuites>
        """))
        _, failures = stress.parse_junit(path)
        self.assertEqual([f["kind"] for f in failures], ["error"])

    def test_a_missing_report_is_a_failure_not_a_clean_sheet(self):
        # A run that died before pytest wrote its report must not be
        # scored as zero failures.
        cases, failures = stress.parse_junit(os.path.join(self.dir, "absent.xml"))
        self.assertIsNone(cases)
        self.assertEqual(len(failures), 1)
        self.assertIn("no JUnit report", failures[0]["message"])

    def test_an_unparseable_report_is_a_failure(self):
        path = self._write("bad.xml", "<testsuite><unclosed>")
        cases, failures = stress.parse_junit(path)
        self.assertIsNone(cases)
        self.assertIn("unparseable", failures[0]["message"])


class PlanTests(unittest.TestCase):
    JOURNEYS = [f"test_smoke.py::C::test_{n}" for n in range(8)]

    def test_the_same_seed_plans_the_same_campaign(self):
        a = stress.plan(self.JOURNEYS, 3, 1234, stress.MATRICES["none"])
        b = stress.plan(self.JOURNEYS, 3, 1234, stress.MATRICES["none"])
        self.assertEqual([x.order for x in a], [y.order for y in b])

    def test_a_different_seed_plans_a_different_order(self):
        a = stress.plan(self.JOURNEYS, 3, 1, stress.MATRICES["none"])
        b = stress.plan(self.JOURNEYS, 3, 2, stress.MATRICES["none"])
        self.assertNotEqual([x.order for x in a], [y.order for y in b])

    def test_repetitions_do_not_repeat_one_order(self):
        attempts = stress.plan(self.JOURNEYS, 4, 7, stress.MATRICES["none"])
        orders = {tuple(a.order) for a in attempts}
        self.assertGreater(len(orders), 1,
                           "repeating one fixed order explores nothing")

    def test_every_attempt_runs_every_journey(self):
        for attempt in stress.plan(self.JOURNEYS, 2, 7, stress.MATRICES["display"]):
            self.assertEqual(sorted(attempt.order), sorted(self.JOURNEYS))

    def test_the_matrix_multiplies_repetitions(self):
        configs = stress.MATRICES["display"]
        attempts = stress.plan(self.JOURNEYS, 2, 7, configs)
        self.assertEqual(len(attempts), 2 * len(configs))
        self.assertEqual(len({a.index for a in attempts}), len(attempts))

    def test_display_matrix_covers_narrow_dark_and_scaled(self):
        configs = stress.MATRICES["display"]
        self.assertTrue(any(c.screen.startswith("400x") for c in configs),
                        "400px wide is where adaptive layout breaks")
        self.assertTrue(any(c.color_scheme == "prefer-dark" for c in configs))
        self.assertTrue(any(c.scale == 2 for c in configs))

    def test_config_env_sets_only_what_it_changes(self):
        self.assertNotIn("GDK_SCALE", stress.BASELINE.env())
        scaled = stress.DisplayConfig("x", scale=2).env()
        self.assertEqual(scaled["GDK_SCALE"], "2")
        self.assertEqual(scaled["GUI_TEST_COLOR_SCHEME"], "default")


class SummaryTests(unittest.TestCase):
    def _attempt(self, index, classification, failures=()):
        return stress.Attempt(index=index, repetition=index,
                              config=stress.BASELINE, order=[],
                              classification=classification,
                              failures=list(failures))

    def test_all_green_is_a_zero_rate(self):
        summary = stress.summarize([self._attempt(1, stress.CLASS_PASS),
                                    self._attempt(2, stress.CLASS_PASS)])
        self.assertEqual(summary["failed_attempts"], 0)
        self.assertEqual(summary["first_attempt_failure_rate"], 0.0)

    def test_failure_rate_counts_every_attempt_once(self):
        summary = stress.summarize([
            self._attempt(1, stress.CLASS_PASS),
            self._attempt(2, stress.CLASS_CRASH,
                          [{"journey": "test_smoke.py::C::test_x"}]),
            self._attempt(3, stress.CLASS_PASS),
            self._attempt(4, stress.CLASS_PASS),
        ])
        self.assertEqual(summary["failed_attempts"], 1)
        self.assertEqual(summary["first_attempt_failure_rate"], 0.25)
        self.assertEqual(summary["by_classification"][stress.CLASS_CRASH], 1)
        self.assertEqual(summary["failing_journeys"], ["test_smoke.py::C::test_x"])

    def test_classifications_are_reported_separately(self):
        summary = stress.summarize([self._attempt(1, stress.CLASS_CRASH),
                                    self._attempt(2, stress.CLASS_TIMEOUT),
                                    self._attempt(3, stress.CLASS_ASSERTION)])
        self.assertEqual(summary["by_classification"], {
            stress.CLASS_CRASH: 1,
            stress.CLASS_TIMEOUT: 1,
            stress.CLASS_ASSERTION: 1,
        })
        self.assertEqual(summary["failed_attempts"], 3)


class NoRetryTests(unittest.TestCase):
    """The campaign must not contain a retry. A green rerun does not
    unfail the first attempt, and "retry until green" is the exact
    failure mode the roadmap forbids."""

    def test_no_retry_option_is_offered(self):
        # Checked through the parser rather than by grepping the source,
        # so the prose is free to explain why retries are absent.
        for flag in ("--retry", "--retries", "--reruns", "--rerun-failures"):
            with self.assertRaises(SystemExit):
                stress.main([flag, "2", "--output", "/nonexistent/campaign"])

    def test_every_planned_attempt_is_distinct_evidence(self):
        attempts = stress.plan(["a::b"], 3, 5, stress.MATRICES["none"])
        self.assertEqual([a.index for a in attempts], [1, 2, 3])
        self.assertEqual([a.repetition for a in attempts], [1, 2, 3])

    def test_a_failing_attempt_still_counts_when_a_later_one_passes(self):
        summary = stress.summarize([
            stress.Attempt(1, 1, stress.BASELINE, [],
                           classification=stress.CLASS_CRASH),
            stress.Attempt(2, 2, stress.BASELINE, [],
                           classification=stress.CLASS_PASS),
        ])
        self.assertEqual(summary["failed_attempts"], 1)


class CLITests(unittest.TestCase):
    def test_an_existing_output_directory_is_refused(self):
        import tempfile
        with tempfile.TemporaryDirectory() as existing:
            with self.assertRaises(SystemExit) as caught:
                stress.main(["--output", existing])
            self.assertNotEqual(caught.exception.code, 0)

    def test_a_setup_failure_is_classified_as_infrastructure(self):
        """An attempt that never started a journey is not evidence about the
        apps. The runner's own setup failures used to score as
        "unclassified", which reads like a product problem."""
        self.assertEqual(
            stress.classify("GUI setup failed: no X display at :99 after 10s.",
                            [], 1, False),
            stress.CLASS_INFRASTRUCTURE,
        )

    def test_the_campaign_leaves_display_allocation_to_the_runner(self):
        """stress.py used to scan /tmp for a free display number and pass it
        as GUI_TEST_DISPLAY_NUM, working around a runner that would
        otherwise fail into an existing display. The runner now asks Xvfb
        to allocate one (-displayfd), which is race-free where a scan is
        not: two campaigns scanning at once pick the same number."""
        source = open(stress.__file__, encoding="utf-8").read()
        self.assertNotIn("free_display_number", source)
        self.assertNotIn("GUI_TEST_DISPLAY_NUM", source)


if __name__ == "__main__":
    unittest.main()
