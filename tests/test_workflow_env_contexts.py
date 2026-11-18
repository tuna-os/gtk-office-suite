"""A workflow's or a job's `env` cannot use the `runner` context.

GitHub evaluates those blocks before a runner is assigned, so
`${{ runner.temp }}` there makes the whole workflow file invalid. Every
push then records a failed run named after the file, and the workflow
never starts (lifafa-flatpak.yml, #2214). The context is fine in a step's
`env` and in `run`.
"""

import glob
import os
import unittest

import yaml

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def runner_uses(env):
    return [k for k, v in (env or {}).items() if "runner." in str(v)]


class WorkflowEnv(unittest.TestCase):
    def test_no_runner_context_outside_steps(self):
        bad = []
        for path in sorted(glob.glob(os.path.join(ROOT, ".github", "workflows", "*.yml"))):
            with open(path, encoding="utf-8") as f:
                wf = yaml.safe_load(f)
            name = os.path.relpath(path, ROOT)
            bad += [f"{name}: env.{k}" for k in runner_uses(wf.get("env"))]
            for job_id, job in (wf.get("jobs") or {}).items():
                bad += [f"{name}: jobs.{job_id}.env.{k}" for k in runner_uses(job.get("env"))]
        self.assertEqual(bad, [], "the runner context is not available here: " + ", ".join(bad))


if __name__ == "__main__":
    unittest.main()
