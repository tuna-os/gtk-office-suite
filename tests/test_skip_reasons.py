"""The skip collector has to find the skips a JUnit report cannot show.

`collect_skip_reasons.py` exists because the obvious implementation is
vacuous: nextest omits `#[ignore]` tests from JUnit entirely, so a
JUnit-based collector reports zero skips on a workspace with seven ignored
tests. Measured on this repository: `tests="848" skipped="0"`.

So the collector reads the test *lister* for which tests are ignored and
the source for why, and each side is what keeps the other honest. Every
test below drives one of those failure modes deliberately -- a bare
`#[ignore]`, a reason with no test, a test with no reason, an empty
listing -- because a checker that cannot be made to fail is not checking.

The real tree is also checked, at the end: whatever the collector claims
about fixtures, it has to hold for this workspace's own ignored tests.
"""

import json
import os
import subprocess
import sys
import tempfile
import textwrap
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO_ROOT, "conformance"))

import collect_skip_reasons as collector  # noqa: E402


def listing(*suites) -> dict:
    """A nextest JSON listing over the given (binary_id, package, dir, kind, name, tests).

    Shaped like the real thing, including the fields the collector uses to
    decide where to look for a reason. Built here rather than recorded as a
    fixture so a test can say exactly what it is varying.
    """
    out = {"rust-suites": {}}
    for binary_id, package, pkg_dir, kind, binary_name, tests in suites:
        out["rust-suites"][binary_id] = {
            "package-name": package,
            "package-id": f"path+file://{pkg_dir}#0.1.0",
            "binary-id": binary_id,
            "binary-name": binary_name,
            "kind": kind,
            "testcases": {name: {"kind": "test", "ignored": ignored}
                          for name, ignored in tests},
        }
    return out


class Fixture:
    """A throwaway crate with one integration target."""

    def __init__(self, body: str, crate: str = "demo", target: str = "cases"):
        self.dir = tempfile.TemporaryDirectory()
        self.crate_dir = os.path.join(self.dir.name, crate)
        os.makedirs(os.path.join(self.crate_dir, "tests"))
        self.target = target
        self.path = os.path.join(self.crate_dir, "tests", f"{target}.rs")
        with open(self.path, "w", encoding="utf-8") as handle:
            handle.write(textwrap.dedent(body))
        self.crate = crate

    def listing(self, tests):
        return listing((f"{self.crate}::{self.target}", self.crate, self.crate_dir,
                        "test", self.target, tests))

    def collect(self, tests):
        return collector.collect(self.listing(tests), [self.dir.name], [])

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.dir.cleanup()


class IgnoredTestsNeedReasons(unittest.TestCase):
    def test_an_explained_ignore_is_reported_not_rejected(self):
        with Fixture('''
            #[test]
            #[ignore = "campaign-scale; run from the nightly stress workflow"]
            fn seed_campaign() {}
        ''') as fixture:
            report, errors = fixture.collect([("seed_campaign", True)])
        self.assertEqual(errors, [])
        self.assertEqual(report["count"], 1)
        row = report["skips"][0]
        self.assertEqual(row["id"], "demo::cases::seed_campaign")
        self.assertEqual(row["kind"], "ignored")
        self.assertEqual(row["reason"],
                         "campaign-scale; run from the nightly stress workflow")

    def test_a_bare_ignore_is_rejected(self):
        """The failure this whole script exists for: ignored, unexplained, invisible."""
        with Fixture('''
            #[test]
            #[ignore]
            fn dump_failures() {}
        ''') as fixture:
            report, errors = fixture.collect([("dump_failures", True)])
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("dump_failures", errors[0])
        self.assertIn("no reason", errors[0])
        self.assertEqual(report["count"], 0)

    def test_an_empty_reason_string_is_no_better_than_a_bare_ignore(self):
        with Fixture('''
            #[test]
            #[ignore = ""]
            fn shrug() {}
        ''') as fixture:
            _, errors = fixture.collect([("shrug", True)])
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("no reason", errors[0])

    def test_a_non_ignored_test_is_not_in_the_report(self):
        with Fixture('''
            #[test]
            fn runs_every_time() {}
        ''') as fixture:
            report, errors = fixture.collect([("runs_every_time", False)])
        self.assertEqual(errors, [])
        self.assertEqual(report["count"], 0)

    def test_attributes_between_the_ignore_and_the_fn_do_not_hide_it(self):
        """Attribute order is the author's choice; the scan has to follow it."""
        with Fixture('''
            #[ignore = "needs a display"]
            #[test]
            #[cfg(unix)]
            // a comment in the middle, too
            fn widget_opens() {}
        ''') as fixture:
            report, errors = fixture.collect([("widget_opens", True)])
        self.assertEqual(errors, [])
        self.assertEqual(report["skips"][0]["reason"], "needs a display")


class StaleReasonsAreRejected(unittest.TestCase):
    def test_a_reason_for_a_test_the_listing_never_reports_is_an_error(self):
        """A renamed or deleted test leaves a reason that reads as current."""
        with Fixture('''
            #[test]
            #[ignore = "campaign-scale; run from the nightly stress workflow"]
            fn seed_campaign_renamed_away() {}
        ''') as fixture:
            _, errors = fixture.collect([("seed_campaign", True)])
        self.assertTrue(any("no listed test matches" in e for e in errors), errors)

    def test_an_ignored_test_whose_attribute_cannot_be_found_is_an_error(self):
        """Ignored by the listing, absent from the source the listing points at."""
        with Fixture('''
            #[test]
            fn something_else() {}
        ''') as fixture:
            _, errors = fixture.collect([("invisible", True)])
        self.assertTrue(any("no #[ignore] attribute for it was found" in e for e in errors),
                        errors)


class TheListingMustCoverTheWorkspace(unittest.TestCase):
    def test_an_empty_listing_over_a_tree_with_ignores_is_rejected(self):
        """Otherwise every check above passes by having nothing to check.

        This is the shape of bug the capability ledger kept producing: a
        rule that holds trivially because its input was empty. A listing of
        the wrong workspace -- or one taken without --run-ignored all --
        must not read as a clean sheet.
        """
        with Fixture('''
            #[test]
            #[ignore = "campaign-scale"]
            fn seed_campaign() {}
        ''') as fixture:
            _, errors = collector.collect({"rust-suites": {}}, [fixture.dir.name], [])
        self.assertTrue(any("does not cover this workspace" in e for e in errors), errors)

    def test_an_empty_listing_over_a_tree_with_no_ignores_is_fine(self):
        """The guard must not fire on a workspace that genuinely skips nothing."""
        with Fixture('''
            #[test]
            fn runs_every_time() {}
        ''') as fixture:
            report, errors = collector.collect({"rust-suites": {}}, [fixture.dir.name], [])
        self.assertEqual(errors, [])
        self.assertEqual(report["count"], 0)


class OneCrateCannotLendAnotherItsReason(unittest.TestCase):
    def test_a_same_named_test_in_another_target_is_not_a_match(self):
        """`seed_campaign` exists in two targets per crate in the real tree.

        If the scan matched on the function name alone, an explained
        `seed_campaign` in one target would cover a bare one in another --
        exactly the mistake that makes a checker report success it did not
        verify.
        """
        with tempfile.TemporaryDirectory() as root:
            crate = os.path.join(root, "demo")
            os.makedirs(os.path.join(crate, "tests"))
            with open(os.path.join(crate, "tests", "explained.rs"), "w") as handle:
                handle.write('#[test]\n#[ignore = "campaign-scale"]\nfn seed_campaign() {}\n')
            with open(os.path.join(crate, "tests", "bare.rs"), "w") as handle:
                handle.write("#[test]\n#[ignore]\nfn seed_campaign() {}\n")
            spec = listing(
                ("demo::explained", "demo", crate, "test", "explained",
                 [("seed_campaign", True)]),
                ("demo::bare", "demo", crate, "test", "bare", [("seed_campaign", True)]),
            )
            report, errors = collector.collect(spec, [root], [])
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("demo::bare::seed_campaign", errors[0])
        self.assertIn("no reason", errors[0])
        self.assertEqual([row["id"] for row in report["skips"]],
                         ["demo::explained::seed_campaign"])


class RuntimeSkipsComeFromJunit(unittest.TestCase):
    """The half JUnit *does* carry: collected, started, then skipped itself."""

    JUNIT = """<?xml version="1.0"?>
    <testsuites>
      <testsuite name="gui">
        <testcase classname="tests/gui/test_vlm.py" name="test_reads_the_screen">
          <skipped message="GEMINI_API_KEY not set"/>
        </testcase>
        <testcase classname="tests/gui/test_smoke.py" name="test_launches"/>
      </testsuite>
    </testsuites>"""

    def test_a_runtime_skip_is_reported_with_its_message(self):
        with tempfile.NamedTemporaryFile("w", suffix=".xml", delete=False) as handle:
            handle.write(self.JUNIT)
            path = handle.name
        try:
            report, errors = collector.collect({"rust-suites": {}}, [], [path])
        finally:
            os.unlink(path)
        self.assertEqual(errors, [])
        self.assertEqual(report["count"], 1)
        self.assertEqual(report["skips"][0]["kind"], "runtime-skip")
        self.assertEqual(report["skips"][0]["reason"], "GEMINI_API_KEY not set")

    def test_a_runtime_skip_with_no_message_is_an_error(self):
        with tempfile.NamedTemporaryFile("w", suffix=".xml", delete=False) as handle:
            handle.write(self.JUNIT.replace('message="GEMINI_API_KEY not set"', ""))
            path = handle.name
        try:
            _, errors = collector.collect({"rust-suites": {}}, [], [path])
        finally:
            os.unlink(path)
        self.assertTrue(any("no message" in e for e in errors), errors)


class TheRealTreeHasNoUnexplainedIgnores(unittest.TestCase):
    """Whatever holds for fixtures has to hold for this workspace.

    The listing is not regenerated here -- `cargo nextest list` needs a
    full workspace build, which is minutes this lane does not have. The
    source scan alone still catches the failure this item was opened for: a
    bare `#[ignore]`. The listing-backed check runs in the lane that has
    already built the workspace.
    """

    def test_every_ignore_attribute_in_the_tree_carries_a_reason(self):
        bare = []
        for path in collector.rust_sources([REPO_ROOT]):
            for attr in collector.scan_file(path):
                if not attr["reason"]:
                    bare.append(f"{os.path.relpath(attr['file'], REPO_ROOT)}:{attr['line']}")
        self.assertEqual(bare, [], "bare #[ignore] attributes: " + ", ".join(bare))

    def test_the_scan_finds_the_ignores_that_are_known_to_be_there(self):
        """A scan that found nothing would pass the test above vacuously."""
        found = set()
        for path in collector.rust_sources([REPO_ROOT]):
            for attr in collector.scan_file(path):
                found.add(os.path.relpath(attr["file"], REPO_ROOT))
        self.assertIn("letters-core/tests/stateful.rs", found)
        self.assertIn("letters-core/tests/corpus_debug.rs", found)
        self.assertGreaterEqual(len(found), 5, sorted(found))


class TheScriptRunsAsACommand(unittest.TestCase):
    def test_an_unexplained_skip_exits_nonzero_and_says_which(self):
        with Fixture('''
            #[test]
            #[ignore]
            fn dump_failures() {}
        ''') as fixture:
            spec = os.path.join(fixture.dir.name, "list.json")
            with open(spec, "w", encoding="utf-8") as handle:
                json.dump(fixture.listing([("dump_failures", True)]), handle)
            out = os.path.join(fixture.dir.name, "skips.json")
            proc = subprocess.run(
                [sys.executable,
                 os.path.join(REPO_ROOT, "conformance", "collect_skip_reasons.py"),
                 "--nextest-list", spec, "--root", fixture.dir.name, "--out", out],
                capture_output=True, text=True, timeout=120)
            self.assertEqual(proc.returncode, 1, proc.stderr)
            self.assertIn("dump_failures", proc.stderr)
            # The report is still written: what was accounted for is useful
            # even when something was not.
            with open(out, encoding="utf-8") as handle:
                self.assertEqual(json.load(handle)["count"], 0)


if __name__ == "__main__":
    unittest.main()
