"""A missing LibreOffice is a failure in every lane that runs the oracle (#1206).

The oracle suites (`soffice_oracle`, `lo_parity`) return early and report
*ok* when no `soffice` is installed, unless `REQUIRE_SOFFICE` is set, in
which case they panic. That skip is right on a contributor's machine and
wrong in CI: a lane that runs them without the variable reports
"compatible" for a check that never ran, and nothing in the log says so
unless someone reads the "skipping" line.

So three things are checked here, from the sources rather than from a list
kept beside them:

1. every Rust test target that starts `soffice` has its skip gated on
   `REQUIRE_SOFFICE` (it panics when the variable is set);
2. every workflow step that runs one of those targets sets
   `REQUIRE_SOFFICE: "1"`;
3. the pull-request test lane, which has no LibreOffice, excludes each of
   those targets from the evidence it offers (conformance/lanes.json), so
   its skip-passes cannot vouch for a capability claim.
"""

import json
import os
import re
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORKFLOWS = os.path.join(REPO_ROOT, ".github", "workflows")
CRATES = ("letters-core", "tables-core", "decks-core", "suite-common-core")
STARTS_SOFFICE = re.compile(r'Command::new\("(soffice|libreoffice)"\)|for cand in \["soffice"')
STEP_START = re.compile(r"^(\s*)- (name|uses|run|id|if|env|with):?")


def oracle_targets():
    """(crate, target) for every integration-test file that starts soffice."""
    found = []
    for crate in CRATES:
        tests = os.path.join(REPO_ROOT, crate, "tests")
        if not os.path.isdir(tests):
            continue
        for name in sorted(os.listdir(tests)):
            if not name.endswith(".rs"):
                continue
            with open(os.path.join(tests, name)) as f:
                source = f.read()
            if STARTS_SOFFICE.search(source):
                found.append((crate, name[:-3], source))
    return found


def steps_running(target):
    """(workflow, step text) for every step whose script runs `--test target`."""
    pattern = re.compile(r"--test\s+" + re.escape(target) + r"\b")
    hits = []
    for name in sorted(os.listdir(WORKFLOWS)):
        if not name.endswith((".yml", ".yaml")):
            continue
        with open(os.path.join(WORKFLOWS, name)) as f:
            lines = f.read().splitlines()
        for i, line in enumerate(lines):
            if line.lstrip().startswith("#") or not pattern.search(line):
                continue
            start = i
            while start >= 0 and not STEP_START.match(lines[start]):
                start -= 1
            if start < 0:
                raise AssertionError(f"{name}:{i + 1}: no step encloses this line")
            indent = len(STEP_START.match(lines[start]).group(1))
            end = start + 1
            while end < len(lines):
                stripped = lines[end].strip()
                lead = len(lines[end]) - len(lines[end].lstrip())
                if stripped and lead <= indent:
                    break
                end += 1
            hits.append((f"{name}:{i + 1}", "\n".join(lines[start:end])))
    return hits


class OracleLanesRequireSoffice(unittest.TestCase):
    def test_the_oracle_targets_are_found(self):
        # A scan that finds nothing would pass every check below.
        targets = {(c, t) for c, t, _ in oracle_targets()}
        for expected in [("letters-core", "soffice_oracle"), ("letters-core", "lo_parity"),
                         ("tables-core", "soffice_oracle"), ("decks-core", "soffice_oracle"),
                         ("decks-core", "lo_parity")]:
            self.assertIn(expected, targets)

    def test_every_oracle_skip_is_gated_on_require_soffice(self):
        for crate, target, source in oracle_targets():
            with self.subTest(f"{crate}/tests/{target}.rs"):
                self.assertIn("REQUIRE_SOFFICE", source,
                              "starts soffice but never reads REQUIRE_SOFFICE, so it can skip in CI")
                self.assertRegex(source, r'REQUIRE_SOFFICE"\)\.is_ok\(\)\s*\{\s*panic!',
                                 "REQUIRE_SOFFICE must turn a missing soffice into a panic")

    def test_every_step_that_runs_an_oracle_target_requires_soffice(self):
        ran = 0
        for _, target, _ in oracle_targets():
            for where, step in steps_running(target):
                ran += 1
                with self.subTest(where):
                    self.assertRegex(step, r'REQUIRE_SOFFICE:\s*"1"',
                                     f"runs --test {target} without REQUIRE_SOFFICE: \"1\"; "
                                     "a missing LibreOffice would pass as compatible")
        self.assertGreater(ran, 0, "no workflow step runs the oracle at all")

    def test_the_pull_request_lane_offers_no_oracle_evidence(self):
        with open(os.path.join(REPO_ROOT, "conformance", "lanes.json")) as f:
            lanes = json.load(f)["lanes"]
        test_lane = next(l for l in lanes if l["job"] == "test")
        for crate, target, _ in oracle_targets():
            with self.subTest(f"{crate}::{target}"):
                self.assertIn(f"{crate}::{target}::", test_lane.get("excludes", []))

    def test_the_step_scan_finds_a_step_without_the_variable(self):
        # The scanner itself: a step that runs the oracle bare must be seen.
        import tempfile
        global WORKFLOWS
        saved = WORKFLOWS
        with tempfile.TemporaryDirectory() as d:
            with open(os.path.join(d, "bad.yml"), "w") as f:
                f.write("jobs:\n  x:\n    steps:\n      - name: oracle\n"
                        "        run: cargo test -p letters-core --test soffice_oracle\n"
                        "      - name: next\n        env:\n          REQUIRE_SOFFICE: \"1\"\n"
                        "        run: true\n")
            WORKFLOWS = d
            try:
                hits = steps_running("soffice_oracle")
            finally:
                WORKFLOWS = saved
        self.assertEqual(len(hits), 1)
        self.assertNotRegex(hits[0][1], r'REQUIRE_SOFFICE:\s*"1"')


if __name__ == "__main__":
    unittest.main()
