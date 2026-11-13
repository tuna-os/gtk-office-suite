"""The release candidate's soak must actually gate the release.

`release-revision.yml` refuses to certify a tag whose expensive evidence was
recorded at a different revision. It had run on the tag for weeks — beside
`publish-flatpak.yml`, never before it. Nothing connected the two, so a tag
the ledger check refused was published anyway and the refusal was a report.
The same hole swallowed #442's release-candidate soak, which did not exist at
all: no larger soak, no complete display matrix at a tag, and nothing that
would have noticed either absence.

So the gate is asserted here rather than remembered: the publish job must
depend on both evidence workflows, must not publish through a failure, and
the soak's repetition count must still meet the stability target the roadmap
states — read out of `crash-stress.md` rather than copied into this file,
because a target that lives in two places is a target that will disagree
with itself.
"""

import os
import re
import unittest

import yaml

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORKFLOWS = os.path.join(REPO_ROOT, ".github", "workflows")
PUBLISH = os.path.join(WORKFLOWS, "publish-flatpak.yml")
SOAK = os.path.join(WORKFLOWS, "release-soak.yml")
CRASH_STRESS = os.path.join(REPO_ROOT, "docs", "readiness-2026-09", "crash-stress.md")
# "zero crashes/data-loss/assertion failures in 20 consecutive runs of each
# critical journey per app at baseline"
TARGET = re.compile(r"(\d+) consecutive runs")


def workflow(path):
    with open(path, encoding="utf-8") as handle:
        loaded = yaml.safe_load(handle)
    # PyYAML reads the `on:` key as the boolean True.
    loaded["triggers"] = loaded.get("on") or loaded.get(True)
    return loaded


def stability_target():
    """The consecutive-run count the roadmap states, as an integer."""
    with open(CRASH_STRESS, encoding="utf-8") as handle:
        found = TARGET.findall(handle.read())
    assert found, (
        f"{CRASH_STRESS} states no `N consecutive runs` stability target, so "
        "this test has nothing to hold the soak to"
    )
    return max(int(n) for n in found)


# A call to a workflow in this repository: `$/` is GitHub's same-repository
# form, `./` the older relative one. Both run the caller's own commit.
LOCAL_PREFIXES = ("$/", "./")


def gate_jobs():
    """The publish workflow's jobs that call another workflow in this repo."""
    jobs = workflow(PUBLISH)["jobs"]
    return {
        name: body
        for name, body in jobs.items()
        if str(body.get("uses", "")).startswith(LOCAL_PREFIXES)
    }


class ReleaseSoakGatesThePublish(unittest.TestCase):
    def test_publishing_depends_on_every_local_evidence_workflow(self):
        gates = gate_jobs()
        self.assertTrue(gates, f"{PUBLISH} calls no local evidence workflow")
        needs = workflow(PUBLISH)["jobs"]["publish"].get("needs") or []
        for name in gates:
            self.assertIn(
                name,
                needs,
                f"the {name!r} gate runs but `publish` does not wait for it, "
                "which is how a refusal arrives after publication",
            )

    def test_every_gate_names_a_workflow_that_exists_and_is_callable(self):
        for name, body in gate_jobs().items():
            with self.subTest(gate=name):
                # Not lstrip("./"): that strips *characters*, and would eat
                # the leading directory's dot-prefix along with them.
                path = os.path.join(REPO_ROOT, body["uses"][2:])
                self.assertTrue(os.path.isfile(path), f"{body['uses']} does not exist")
                self.assertIn(
                    "workflow_call",
                    workflow(path)["triggers"],
                    f"{body['uses']} is called by {name!r} but does not declare "
                    "workflow_call, so the tag would fail before any gate ran",
                )

    def test_a_failed_gate_blocks_the_publish(self):
        """`needs` alone skips on a skipped gate; `always()` publishes through
        a failed one. The condition has to name each result."""
        condition = str(workflow(PUBLISH)["jobs"]["publish"].get("if", ""))
        self.assertTrue(condition, "`publish` has no condition, so a skipped "
                                   "gate blocks the manual build-only path")
        self.assertNotIn(
            "always()",
            condition,
            "always() publishes through a failed gate",
        )
        for name in gate_jobs():
            with self.subTest(gate=name):
                self.assertIn(
                    f"needs.{name}.result != 'failure'",
                    condition,
                    f"`publish` does not refuse a failed {name!r}",
                )

    def test_the_soak_meets_the_stated_stability_target(self):
        repeat = workflow(SOAK)["triggers"]["workflow_call"]["inputs"]["repeat"]
        self.assertGreaterEqual(
            int(repeat["default"]),
            stability_target(),
            f"crash-stress.md asks for {stability_target()} consecutive runs; "
            f"the release soak defaults to {repeat['default']}",
        )

    def test_the_shards_together_still_make_the_stated_repetitions(self):
        """One hosted job is cut off at six hours, whatever `timeout-minutes`
        says (v2.1.9's soak died at attempt 14 of 20), so the repetitions are
        sharded. The shards' attempts, added up, must still reach the target,
        and no job may be given longer than GitHub will let it run."""
        spec = workflow(SOAK)
        plan_env = spec["jobs"]["plan"]["steps"][0]["env"]
        per_shard = int(plan_env["PER_SHARD"])
        with open(SOAK, encoding="utf-8") as handle:
            body = handle.read()
        self.assertIn(
            f"--repeat {per_shard} ",
            body,
            "the shard runs a different number of attempts than the plan counts",
        )
        repeat = int(spec["triggers"]["workflow_call"]["inputs"]["repeat"]["default"])
        shards = -(-repeat // per_shard)
        self.assertGreaterEqual(shards * per_shard, stability_target())
        self.assertIn("fromJSON(needs.plan.outputs.shards)", body)
        self.assertIs(spec["jobs"]["soak"]["strategy"]["fail-fast"], False)
        for name in ("soak", "matrix"):
            self.assertLessEqual(
                int(spec["jobs"][name]["timeout-minutes"]), 355,
                f"{name!r} would be cancelled by GitHub's six-hour limit",
            )

    def test_both_campaigns_pace_their_typing_and_verify_delivery(self):
        """The first soak lost a letter of "apple" in 7 of 14 attempts at
        xdotool's 40ms default under release binaries. Both campaigns pace
        the input and record whether it landed."""
        with open(SOAK, encoding="utf-8") as handle:
            body = handle.read()
        self.assertGreaterEqual(body.count("GUI_TEST_TYPING_DELAY:"), 2)
        self.assertGreaterEqual(body.count("GUI_TEST_VERIFY_TYPING:"), 2)

    def test_the_soak_runs_the_complete_display_matrix(self):
        """Repetition at one size proves nothing about 400px, and one pass over
        every size proves nothing about flakiness. Both runs, or neither
        question is answered."""
        with open(SOAK, encoding="utf-8") as handle:
            body = handle.read()
        self.assertIn(
            "--matrix display",
            body,
            "the release soak never runs the display matrix",
        )
        self.assertIn(
            "--repeat 1 --matrix display",
            body,
            "the matrix pass must be a pass over every configuration, not a "
            "repetition of one",
        )


if __name__ == "__main__":
    unittest.main()
