"""The oracle's path filter must cover the code the oracle can catch (#313).

The LibreOffice oracle runs on pull requests that touch format or model
code, and the list of what counts lives in `nightly.yml` as a raw YAML
`paths:` filter — a list nothing could check. The first version of it
omitted `suite-common-core/**`, which holds `interop.rs`, `atomic_save.rs`,
`zip_guard.rs`, `format.rs`, `units.rs` and `style.rs`: the packaging and
save paths the oracle exists to check. A change to the ZIP writer could
have merged without the oracle ever seeing it, and nothing would have said
so.

Getting this wrong is silent in the same way journey selection is: running
the oracle too often only costs minutes, while running it too rarely
produces a green pull request that never checked the thing it changed. So
the required list is derived here from the crates the oracle actually
tests, plus everything those crates depend on inside the workspace, rather
than copied from the filter it is supposed to police.
"""

import os
import re
import tomllib
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORKFLOW = os.path.join(REPO_ROOT, ".github", "workflows", "nightly.yml")

# `- 'some/path/**'` inside a block, comments and blank lines ignored.
PATTERN_LINE = re.compile(r"^\s*-\s*'([^']+)'\s*$")
TESTED_CRATES = re.compile(r"cargo test((?:\s+-p\s+[\w-]+)+)")


def oracle_path_filter():
    """The `paths:` list under the workflow's `pull_request:` trigger.

    Parsed by hand rather than with PyYAML, which is not a standard-library
    module and so is not guaranteed on the runner that executes this lane.
    Strict about structure: an unreadable filter raises instead of
    returning an empty list, because a check that silently finds nothing to
    check is the failure this whole area keeps producing.
    """
    with open(WORKFLOW, encoding="utf-8") as handle:
        lines = handle.read().splitlines()

    try:
        start = next(i for i, line in enumerate(lines) if line.strip() == "pull_request:")
    except StopIteration:
        raise AssertionError(f"{WORKFLOW} has no pull_request trigger") from None
    try:
        paths_at = next(
            i for i in range(start + 1, len(lines)) if lines[i].strip() == "paths:"
        )
    except StopIteration:
        raise AssertionError("the pull_request trigger declares no paths:") from None

    patterns = []
    for line in lines[paths_at + 1:]:
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        match = PATTERN_LINE.match(line)
        if not match:
            break  # out of the list
        patterns.append(match.group(1))
    assert patterns, "the pull_request paths: filter is empty"
    return patterns


def crates_the_oracle_tests():
    """Every `-p` crate named by a `cargo test` step in the workflow."""
    with open(WORKFLOW, encoding="utf-8") as handle:
        text = handle.read()
    crates = set()
    for group in TESTED_CRATES.findall(text):
        crates.update(re.findall(r"-p\s+([\w-]+)", group))
    assert crates, "no `cargo test -p <crate>` command found in the workflow"
    return crates


def workspace_dependencies(crate, seen=None):
    """`crate` plus every workspace crate it depends on, transitively.

    A path dependency is code that can change the oracle's verdict just as
    much as the crate under test, which is the whole reason this is derived
    rather than listed.
    """
    seen = set() if seen is None else seen
    if crate in seen:
        return seen
    seen.add(crate)
    manifest = os.path.join(REPO_ROOT, crate, "Cargo.toml")
    if not os.path.isfile(manifest):
        return seen
    with open(manifest, "rb") as handle:
        parsed = tomllib.load(handle)
    for table in ("dependencies", "dev-dependencies", "build-dependencies"):
        for name, spec in (parsed.get(table) or {}).items():
            if isinstance(spec, dict) and "path" in spec:
                workspace_dependencies(os.path.basename(spec["path"].rstrip("/")), seen)
    return seen


class OraclePathFilter(unittest.TestCase):
    def setUp(self):
        self.patterns = oracle_path_filter()

    def _covers(self, crate):
        return f"{crate}/**" in self.patterns

    def test_every_crate_the_oracle_tests_is_in_the_filter(self):
        for crate in sorted(crates_the_oracle_tests()):
            with self.subTest(crate=crate):
                self.assertTrue(
                    self._covers(crate),
                    f"the oracle tests {crate} but a change to it does not trigger the run",
                )

    def test_every_crate_those_crates_depend_on_is_in_the_filter(self):
        """The omission this test was written for: `suite-common-core` holds
        the packaging and save code and is a dependency of all three oracle
        crates, so a change to it changes what the oracle would find."""
        required = set()
        for crate in crates_the_oracle_tests():
            required |= workspace_dependencies(crate)
        missing = sorted(crate for crate in required if not self._covers(crate))
        self.assertEqual(
            missing,
            [],
            "a change to these crates changes what the oracle would find, but does "
            f"not trigger it: {missing}",
        )

    def test_the_lockfile_triggers_the_oracle(self):
        """A dependency bump can move any format library underneath the
        crates above without touching one of their files."""
        self.assertIn("Cargo.lock", self.patterns)

    def test_the_workflow_triggers_itself(self):
        """Otherwise a change to the oracle's own commands ships untested."""
        self.assertIn(".github/workflows/nightly.yml", self.patterns)


class TheFilterIsActuallyRead(unittest.TestCase):
    """The parser has to fail loudly on a filter it cannot read, or these
    tests pass by finding nothing — the defect this area keeps producing."""

    def test_the_real_workflow_parses_to_a_non_empty_filter(self):
        self.assertTrue(oracle_path_filter())

    def test_a_workflow_without_the_trigger_is_an_error_not_an_empty_list(self):
        import tempfile
        import unittest.mock as mock

        with tempfile.NamedTemporaryFile("w", suffix=".yml", delete=False) as handle:
            handle.write("on:\n  schedule:\n    - cron: '0 5 * * *'\n")
            path = handle.name
        self.addCleanup(os.unlink, path)
        with mock.patch(f"{__name__}.WORKFLOW", path):
            with self.assertRaises(AssertionError):
                oracle_path_filter()


if __name__ == "__main__":
    unittest.main()
