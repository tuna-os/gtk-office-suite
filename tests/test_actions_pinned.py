"""Every third-party action is pinned to a commit, not a movable tag.

A tag such as `actions/checkout@v7` is a pointer its owner can move. Whoever
controls it — or whoever compromises that account — can change the code
every workflow here runs, with this repository's token, on the next run.
Pinning to the full commit SHA makes the code that runs the code that was
reviewed (#1097, #1158, #1188). The version stays as a trailing comment,
which is both what a reader needs and what Renovate reads to keep the pin
current.

Two actions take their *configuration* from the ref: `dtolnay/rust-toolchain`
picks the toolchain by branch name (`@stable`, `@nightly`) and
`taiki-e/install-action` picks the tool (`@nextest`). Pinning those to a SHA
silently drops that choice unless the same value is passed as an input, so
that is checked too — the one way pinning could change behaviour.

Derived from the workflow files themselves, so a new workflow or step is
covered the moment it exists.
"""

import glob
import os
import re
import unittest

import yaml

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORKFLOWS = sorted(glob.glob(os.path.join(REPO_ROOT, ".github", "workflows", "*.yml")))
SHA = re.compile(r"[0-9a-f]{40}")
USES_LINE = re.compile(r"^\s*(?:- )?uses:\s*(\S+)\s*(#\s*\S.*)?$")
REF_AS_INPUT = {"dtolnay/rust-toolchain": "toolchain", "taiki-e/install-action": "tool"}


def uses_lines():
    """(workflow, line number, target, ref, trailing comment) for every
    non-local `uses:` line."""
    found = []
    for path in WORKFLOWS:
        with open(path, encoding="utf-8") as handle:
            for number, line in enumerate(handle, 1):
                match = USES_LINE.match(line)
                if not match or match.group(1).startswith("./"):
                    continue
                target, _, ref = match.group(1).partition("@")
                found.append((os.path.basename(path), number, target, ref, match.group(2)))
    assert found, "no `uses:` lines found; this check is vacuous"
    return found


def steps_using(repo):
    """Every step whose `uses` names `repo`, parsed as YAML."""
    for path in WORKFLOWS:
        with open(path, encoding="utf-8") as handle:
            workflow = yaml.safe_load(handle) or {}
        for name, job in (workflow.get("jobs") or {}).items():
            for step in job.get("steps", []) or []:
                if str(step.get("uses", "")).startswith(repo + "@"):
                    yield os.path.basename(path), name, step


class EveryActionIsPinned(unittest.TestCase):
    def test_every_action_is_pinned_to_a_full_commit_sha(self):
        for workflow, number, target, ref, _ in uses_lines():
            with self.subTest(at=f"{workflow}:{number}"):
                self.assertRegex(
                    ref, SHA,
                    f"{workflow}:{number} uses {target}@{ref}, a movable ref; "
                    "pin it to the full commit SHA with the version as a comment",
                )

    def test_every_pin_says_which_version_it_is(self):
        for workflow, number, target, ref, comment in uses_lines():
            with self.subTest(at=f"{workflow}:{number}"):
                self.assertTrue(
                    comment,
                    f"{workflow}:{number} pins {target} to {ref[:12]} with no "
                    "`# version` comment; neither a reader nor Renovate can tell what it is",
                )

    def test_a_ref_that_was_configuration_is_passed_as_an_input(self):
        for repo, key in REF_AS_INPUT.items():
            for workflow, job, step in steps_using(repo):
                with self.subTest(at=f"{workflow}:{job}", action=repo):
                    self.assertIn(
                        key, step.get("with") or {},
                        f"{repo} pinned to a SHA in {workflow}:{job} without `with: {key}:`; "
                        "the ref used to choose it, and a SHA cannot",
                    )


if __name__ == "__main__":
    unittest.main()
