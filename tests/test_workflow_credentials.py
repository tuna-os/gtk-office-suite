"""Write credentials in CI go only where they are used, and only to code that
was reviewed.

Two workflows held more than they needed (#286, #1007, #1008):

- `publish-flatpak.yml` granted `contents: write` and `packages: write` to
  every job, passed `secrets: inherit` (every secret the repository can see),
  and called the shared publish workflow at `@main`, so whatever that branch
  held at publish time ran with all of it.
- `feature-verification.yml` runs on pull requests. Its `image` job could
  build the pull request's Containerfile and push it to the shared
  `gui-test:main` tag, and its `publish` job ran the pull request's own
  scripts while holding `contents: write` and `pull-requests: write`.

Each of these is a one-line regression away from coming back, so they are
asserted here rather than remembered.
"""

import os
import re
import unittest

import yaml

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORKFLOWS = os.path.join(REPO_ROOT, ".github", "workflows")
PUBLISH = os.path.join(WORKFLOWS, "publish-flatpak.yml")
FEATURE = os.path.join(WORKFLOWS, "feature-verification.yml")
WRITE_SCOPES = {"contents", "packages", "pull-requests", "actions", "id-token"}
FULL_SHA = re.compile(r"@[0-9a-f]{40}$")


def workflow(path):
    with open(path, encoding="utf-8") as handle:
        return yaml.safe_load(handle)


def writes(permissions):
    """The write scopes a `permissions:` block grants."""
    if permissions == "write-all":
        return set(WRITE_SCOPES)
    if not isinstance(permissions, dict):
        return set()
    return {scope for scope, level in permissions.items() if level == "write"}


class PublishFlatpakCredentials(unittest.TestCase):
    def setUp(self):
        self.wf = workflow(PUBLISH)
        self.publish = self.wf["jobs"]["publish"]

    def test_workflow_default_is_read_only(self):
        self.assertEqual(
            writes(self.wf.get("permissions")), set(),
            "workflow-level permissions reach every job, including the "
            "evidence gate and build-only dispatches; grant writes per job",
        )

    def test_publish_job_has_exactly_the_scopes_it_uses(self):
        self.assertEqual(writes(self.publish.get("permissions")), {"contents", "packages"})

    def test_shared_workflow_is_pinned_to_a_commit(self):
        uses = str(self.publish.get("uses", ""))
        self.assertTrue(
            FULL_SHA.search(uses),
            f"{uses!r} is a branch or tag; a job holding write scopes and the "
            "index token must run a reviewed commit",
        )

    def test_secrets_are_named_not_inherited(self):
        secrets = self.publish.get("secrets")
        self.assertNotEqual(secrets, "inherit")
        self.assertEqual(set(secrets or {}), {"FLATPAK_INDEX_TOKEN"})


class FeatureVerificationCredentials(unittest.TestCase):
    def setUp(self):
        self.jobs = workflow(FEATURE)["jobs"]

    def test_image_job_cannot_publish_the_shared_image(self):
        image = self.jobs["image"]
        self.assertNotIn("packages", writes(image.get("permissions")))
        script = "\n".join(str(step.get("run", "")) for step in image["steps"])
        self.assertNotIn("docker push", script)
        self.assertNotIn("docker build", script)

    def test_jobs_that_run_pull_request_code_hold_no_writes(self):
        for name in ("gate", "image", "record"):
            self.assertEqual(writes(self.jobs[name].get("permissions")), set(), name)

    def test_publish_job_runs_base_branch_scripts(self):
        publish = self.jobs["publish"]
        self.assertTrue(writes(publish.get("permissions")), "precondition: publish writes")
        checkouts = [
            step for step in publish["steps"]
            if str(step.get("uses", "")).startswith("actions/checkout@")
        ]
        self.assertTrue(checkouts)
        for step in checkouts:
            ref = str((step.get("with") or {}).get("ref", ""))
            self.assertIn(
                "pull_request.base.sha", ref,
                "the job holding write scopes must not check out the pull "
                "request's head and run its scripts",
            )
            self.assertIs((step.get("with") or {}).get("persist-credentials"), False)


if __name__ == "__main__":
    unittest.main()
