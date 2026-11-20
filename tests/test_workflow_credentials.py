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
        self.assertEqual(writes(self.publish.get("permissions")), {"packages"})

    def test_build_job_holds_no_writes(self):
        self.assertEqual(writes(self.wf["jobs"]["build"].get("permissions")), set())

    def test_shared_index_action_is_pinned_to_a_commit(self):
        """The index action comes from a checkout of tuna-os/.github; a job
        holding write scopes and the index deploy key must run a reviewed
        commit of it, not a branch."""
        checkouts = [
            step.get("with") or {} for step in self.publish["steps"]
            if str(step.get("uses", "")).startswith("actions/checkout@")
            and (step.get("with") or {}).get("repository") == "tuna-os/.github"
        ]
        self.assertTrue(checkouts, "the publish job no longer checks out tuna-os/.github")
        for checkout in checkouts:
            ref = str(checkout.get("ref", ""))
            self.assertTrue(re.fullmatch(r"[0-9a-f]{40}", ref), f"{ref!r} is not a commit")

    def test_secrets_are_named_not_inherited(self):
        text = yaml.safe_dump(self.publish)
        self.assertEqual(set(re.findall(r"secrets\.([A-Z_]+)", text)), {"FLATPAK_INDEX_DEPLOY_KEY"})


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
        for name in ("image", "record"):
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
