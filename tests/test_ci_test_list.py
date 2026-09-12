"""A test file nothing names is a test file nothing runs.

`ci.yml`'s fast lane names its Python test files one per line, twice — once
to run them and once to collect them for the capability inventory. Nothing
compared either list with the directory, so adding `tests/test_*.py` and
forgetting the workflow produced a file that passed locally, was never
executed in CI, and reported nothing when it broke. Silence is the worst
failure mode a test can have: a suite that does not run cannot be seen to be
missing.

This is the fourth instance of one shape in this repository — a hand-written
list in a workflow that omits the thing it polices, after the oracle's
`suite-common-core`, the journey selector, and the release gate's ceiling
files. Each was closed the same way: derive the requirement instead of
restating it.
"""

import os
import unittest

import yaml

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TESTS = os.path.join(REPO_ROOT, "tests")
CI = os.path.join(REPO_ROOT, ".github", "workflows", "ci.yml")


def discovered():
    """Every top-level Python test file, from the directory itself."""
    found = {
        name
        for name in os.listdir(TESTS)
        if name.startswith("test_") and name.endswith(".py")
    }
    assert found, f"{TESTS} holds no test_*.py files, so this check is vacuous"
    return found


def pytest_steps():
    """Every ci.yml step whose command names Python test files.

    Derived rather than named: a third pytest step added to the workflow is
    covered the moment it exists, which is the whole point.
    """
    with open(CI, encoding="utf-8") as handle:
        workflow = yaml.safe_load(handle)
    steps = []
    for job in workflow.get("jobs", {}).values():
        for step in job.get("steps", []) or []:
            command = str(step.get("run", ""))
            if "pytest" in command and "tests/test_" in command:
                steps.append((step.get("name", "<unnamed step>"), command))
    assert steps, f"{CI} has no pytest step naming tests/test_*.py"
    return steps


class EveryTestFileIsNamedByCI(unittest.TestCase):
    def test_every_test_file_is_run_and_collected(self):
        files = discovered()
        for name, command in pytest_steps():
            for filename in sorted(files):
                with self.subTest(step=name, file=filename):
                    self.assertIn(
                        f"tests/{filename}",
                        command,
                        f"tests/{filename} exists but the {name!r} step does "
                        "not name it, so it does not run in CI",
                    )

    def test_every_file_ci_names_exists(self):
        """The other direction: a renamed or deleted file must fail loudly
        rather than leave a line in the workflow that quietly matches
        nothing."""
        for name, command in pytest_steps():
            for token in command.split():
                if token.startswith("tests/") and token.endswith(".py"):
                    with self.subTest(step=name, file=token):
                        self.assertTrue(
                            os.path.isfile(os.path.join(REPO_ROOT, token)),
                            f"the {name!r} step names {token}, which does not exist",
                        )


if __name__ == "__main__":
    unittest.main()
