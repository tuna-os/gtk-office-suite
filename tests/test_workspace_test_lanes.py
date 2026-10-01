"""Every job that runs the workspace's tests installs what they need.

The workspace suite runs in two jobs: `ci.yml`'s `test` (nextest, every pull
request) and `nightly.yml`'s `coverage` (llvm-cov, every night). Each
installs its system packages with its own `apt-get install` line, and
nothing compared the two. When the Decks PDF-export tests began shelling out
to poppler (`pdfinfo`, `pdftotext`, `pdftoppm`) the PR job gained
`poppler-utils` and the coverage job did not. Those tests deliberately panic
in CI rather than skip when poppler is missing, so the nightly went red for
every run from then on (#1191) while every pull request stayed green — the
same code, measured by two lanes that had quietly diverged.

This is the fifth instance of one shape in this repository: a hand-written
list in a workflow that omits the thing it polices (see test_ci_test_list.py
for the other four). Closed the same way: derive the requirement instead of
restating it. The pull-request job is the reference, because it is the one a
contributor sees fail; any job that runs the same suite must install at least
what that job does, and a new such job is covered the moment it exists.
"""

import glob
import os
import re
import unittest

import yaml

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORKFLOWS = os.path.join(REPO_ROOT, ".github", "workflows")
REFERENCE = ("ci.yml", "test")

# A command that runs the workspace's own tests, whatever the runner.
WORKSPACE_SUITE = re.compile(
    r"cargo\s+(?:nextest\s+run|llvm-cov|test)\b[^\n]*--workspace"
)
APT_INSTALL = re.compile(r"apt-get\s+install([^\n]*)")


def apt_packages(job):
    """The packages a job's steps install with apt-get, flags excluded."""
    packages = set()
    for step in job.get("steps", []) or []:
        for match in APT_INSTALL.finditer(str(step.get("run", ""))):
            packages |= {
                token for token in match.group(1).split()
                if not token.startswith("-") and not token.startswith("$")
            }
    return packages


def workspace_suite_jobs():
    """(workflow, job name, job) for every host job running the suite.

    A job with a `container:` gets its packages from the image, which is
    built and checked elsewhere, so only host jobs are compared here.
    """
    found = []
    for path in sorted(glob.glob(os.path.join(WORKFLOWS, "*.yml"))):
        with open(path, encoding="utf-8") as handle:
            workflow = yaml.safe_load(handle) or {}
        for name, job in (workflow.get("jobs") or {}).items():
            if job.get("container"):
                continue
            runs = [str(s.get("run", "")) for s in job.get("steps", []) or []]
            if any(WORKSPACE_SUITE.search(run) for run in runs):
                found.append((os.path.basename(path), name, job))
    return found


class WorkspaceSuiteLanesInstallTheSamePackages(unittest.TestCase):
    def test_the_reference_job_is_found_and_installs_something(self):
        """Guards the check itself: if the reference job moved or stopped
        installing packages, every comparison below would pass vacuously."""
        jobs = {(wf, name): job for wf, name, job in workspace_suite_jobs()}
        self.assertIn(REFERENCE, jobs, f"{REFERENCE} no longer runs the workspace suite")
        self.assertTrue(apt_packages(jobs[REFERENCE]), f"{REFERENCE} installs nothing")
        self.assertGreater(len(jobs), 1, "only one job runs the suite; nothing to compare")

    def test_every_job_running_the_suite_installs_what_the_pr_job_does(self):
        jobs = workspace_suite_jobs()
        reference = next(job for wf, name, job in jobs if (wf, name) == REFERENCE)
        needed = apt_packages(reference)
        for workflow, name, job in jobs:
            if (workflow, name) == REFERENCE:
                continue
            with self.subTest(job=f"{workflow}:{name}"):
                missing = needed - apt_packages(job)
                self.assertFalse(
                    missing,
                    f"{workflow}:{name} runs the workspace tests but does not "
                    f"install {sorted(missing)}, which {REFERENCE[0]}:{REFERENCE[1]} "
                    "installs for them",
                )


if __name__ == "__main__":
    unittest.main()
