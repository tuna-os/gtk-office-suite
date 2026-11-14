"""Every third-party action runs a reviewed commit, not whatever a tag holds.

A tag such as `actions/checkout@v7` is a pointer its owner can move. Whoever
controls it — or whoever compromises that account — can change the code
every workflow here runs, with this repository's token, on the next run
(#1097, #1158, #1188). `.github/workflows/actions.lock`, written by
`gh actions-lock`, records the commit each `uses:` ref resolved to, and
GitHub runs that commit. The `lockfile` job in actions-security.yml fails
when the lockfile and the workflows drift apart, or when a locked commit no
longer verifies. So a `uses:` line names a version, and the lockfile pins it.

Calls to another repository's reusable workflow are outside the lockfile, so
they stay pinned to the full commit SHA, with the branch or version as a
trailing comment for readers and Renovate.

Two actions take their *configuration* from the ref: `dtolnay/rust-toolchain`
picks the toolchain by branch name (`@stable`, `@nightly`) and
`taiki-e/install-action` can pick the tool (`@releases/nextest`). Pinning
those to a SHA silently drops that choice unless the same value is passed as
an input, so that is checked too. install-action is on a version tag with
`tool: nextest`, not on its `releases/nextest` branch: upstream rewrote that
branch, the locked commit stopped verifying, and every release-cutting run
failed at "Set up job" until the pin moved.

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
LOCKFILE = os.path.join(REPO_ROOT, ".github", "workflows", "actions.lock")
SHA = re.compile(r"[0-9a-f]{40}")
LOCKED_COMMIT = re.compile(r"sha1-[0-9a-f]{40}")
USES_LINE = re.compile(r"^\s*(?:- )?uses:\s*(\S+)\s*(#\s*\S.*)?$")
REF_AS_INPUT = {"dtolnay/rust-toolchain": "toolchain", "taiki-e/install-action": "tool"}


def uses_lines():
    """(workflow, line number, target, ref, trailing comment) for every
    `uses:` line that names another repository."""
    found = []
    for path in WORKFLOWS:
        with open(path, encoding="utf-8") as handle:
            for number, line in enumerate(handle, 1):
                match = USES_LINE.match(line)
                if not match or match.group(1).startswith(("./", "$/", "docker://")):
                    continue
                target, _, ref = match.group(1).partition("@")
                found.append((os.path.basename(path), number, target, ref, match.group(2)))
    assert found, "no `uses:` lines found; this check is vacuous"
    return found


def is_reusable_workflow(target):
    return "/.github/workflows/" in target


def lock_key(target, ref):
    """The lockfile's name for an action: owner/repo, lower case, without a
    subdirectory, at the ref the workflow names."""
    return "/".join(target.split("/")[:2]).lower() + "@" + ref


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
    @classmethod
    def setUpClass(cls):
        with open(LOCKFILE, encoding="utf-8") as handle:
            cls.lock = yaml.safe_load(handle)

    def test_every_action_is_locked_to_a_commit(self):
        for workflow, number, target, ref, _ in uses_lines():
            if is_reusable_workflow(target):
                continue
            key = lock_key(target, ref)
            with self.subTest(at=f"{workflow}:{number}"):
                listed = self.lock["workflows"].get(f".github/workflows/{workflow}") or []
                self.assertIn(
                    key, listed,
                    f"{workflow}:{number} uses {target}@{ref}, which actions.lock does "
                    "not list for this workflow; run `gh actions-lock`",
                )
                commit = (self.lock["dependencies"].get(key) or {}).get("commit", "")
                self.assertRegex(commit, LOCKED_COMMIT, f"actions.lock pins {key} to no commit")

    def test_every_reusable_workflow_is_pinned_to_a_full_commit_sha(self):
        for workflow, number, target, ref, comment in uses_lines():
            if not is_reusable_workflow(target):
                continue
            with self.subTest(at=f"{workflow}:{number}"):
                self.assertRegex(
                    ref, SHA,
                    f"{workflow}:{number} calls {target}@{ref}, a movable ref the "
                    "lockfile does not cover; pin it to the full commit SHA",
                )
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
                        f"{repo} in {workflow}:{job} without `with: {key}:`; "
                        "the ref chooses it, and a SHA pin would drop that choice",
                    )


if __name__ == "__main__":
    unittest.main()
