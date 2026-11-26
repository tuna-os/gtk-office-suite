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


# Every job below predates the gate: it holds a write scope in a workflow a
# user with write access can start by hand on a branch of their choosing
# (#1690). The set may only shrink. A new workflow, or a new job, that grants
# write to a dispatchable workflow fails the test; so does an entry whose job
# no longer needs it, so a fix has to delete its line here.
DISPATCH_WRITE_KNOWN = {
    ("autorelease.yml", "release"),
    ("feature-verification.yml", "publish"),
    ("gui-container.yml", "publish"),
    ("gui-stress.yml", "image"),
    ("lifafa-flatpak.yml", "publish"),
    ("lifafa-flatpak.yml", "pages"),
    ("lifafa-upstream-sync.yml", "sync"),
    ("overview-videos.yml", "record"),
    ("publish-flatpak.yml", "soak"),
    ("publish-flatpak.yml", "publish"),
    ("publish-flatpak.yml", "release"),
    ("publish-mandelbrot.yml", "publish"),
    ("publish-mandelbrot.yml", "github-release"),
    ("release-evidence.yml", "visual"),
    ("release-evidence.yml", "publish"),
    ("release-soak.yml", "image"),
    ("render-parity.yml", "report-post"),
    ("render-parity.yml", "export-report-post"),
    ("render-real.yml", "publish"),
    ("screenshots.yml", "capture"),
    ("visual-matrix.yml", "image"),
}
ISSUES_WRITE = WRITE_SCOPES | {"issues"}


def triggers(wf):
    # PyYAML reads the bare key `on` as the boolean True.
    on = wf.get(True, wf.get("on"))
    if isinstance(on, dict):
        return set(on)
    return set(on) if isinstance(on, list) else {on}


def load_all():
    out = {}
    for name in sorted(os.listdir(WORKFLOWS)):
        if name.endswith((".yml", ".yaml")):
            out[name] = workflow(os.path.join(WORKFLOWS, name))
    return out


def dispatchable(workflows):
    """Workflows a manual run can reach: those with `workflow_dispatch`, and
    local reusable workflows a dispatchable workflow calls."""
    reach = {n for n, wf in workflows.items() if "workflow_dispatch" in triggers(wf)}
    changed = True
    while changed:
        changed = False
        for name in list(reach):
            for job in (workflows[name].get("jobs") or {}).values():
                uses = str(job.get("uses", ""))
                if uses.startswith("./.github/workflows/"):
                    callee = uses.rsplit("/", 1)[1]
                    if callee in workflows and callee not in reach:
                        reach.add(callee)
                        changed = True
    return reach


def job_writes(wf, job):
    perms = job["permissions"] if "permissions" in job else wf.get("permissions")
    if perms == "write-all":
        return set(ISSUES_WRITE)
    if not isinstance(perms, dict):
        return set()
    return {scope for scope, level in perms.items() if level == "write"} & ISSUES_WRITE


class DispatchableWorkflowsHoldNoWrites(unittest.TestCase):
    """A manual run can select any branch, so branch-selected workflow code
    must not run with repository write authority (#1690). Write authority
    belongs in a default-branch publisher that consumes artifacts."""

    @classmethod
    def setUpClass(cls):
        cls.workflows = load_all()
        reach = dispatchable(cls.workflows)
        cls.found = {
            (name, jname)
            for name in reach
            for jname, job in (cls.workflows[name].get("jobs") or {}).items()
            if job_writes(cls.workflows[name], job)
        }

    def test_no_new_write_job_in_a_dispatchable_workflow(self):
        new = sorted(self.found - DISPATCH_WRITE_KNOWN)
        self.assertEqual(
            new, [],
            "these jobs hold a write scope in a workflow that can be run "
            "manually on any branch; move the write steps into a "
            "default-branch workflow_run publisher that only downloads "
            "artifacts (do not add them to DISPATCH_WRITE_KNOWN)",
        )

    def test_known_list_has_no_stale_entries(self):
        stale = sorted(DISPATCH_WRITE_KNOWN - self.found)
        self.assertEqual(
            stale, [],
            "these jobs no longer hold writes in a dispatchable workflow; "
            "delete them from DISPATCH_WRITE_KNOWN so the gate tightens",
        )


if __name__ == "__main__":
    unittest.main()
