"""Automatic releases (autorelease.yml, tools/release/cut_release.py).

A release commit records that the scheduled lanes passed at its parent and
adds AppStream release entries; nothing else, or the evidence gate refuses
it. The workflow runs those lanes before it tags anything.
"""

import json
import os
import sys
import unittest

import yaml

REPO_ROOT = os.path.join(os.path.dirname(__file__), "..")
sys.path.insert(0, os.path.join(REPO_ROOT, "tools", "release"))
sys.path.insert(0, os.path.join(REPO_ROOT, "conformance"))

import cut_release  # noqa: E402
import validate_capabilities as vc  # noqa: E402

WORKFLOWS = os.path.join(REPO_ROOT, ".github", "workflows")


def workflow(name):
    with open(os.path.join(WORKFLOWS, name), encoding="utf-8") as handle:
        body = yaml.safe_load(handle)
    # PyYAML reads the bare key `on` as True.
    body["triggers"] = body.pop(True, body.pop("on", {}))
    return body


class CutRelease(unittest.TestCase):
    def test_versions_compare_numerically(self):
        self.assertEqual(cut_release.version_key("1.0"), (1, 0, 0))
        self.assertGreater(cut_release.version_key("2.10.0"), cut_release.version_key("2.9.1"))

    def test_every_claim_on_a_scheduled_lane_gets_the_revision(self):
        with open(os.path.join(REPO_ROOT, "conformance", "capabilities.json")) as f:
            text = f.read()
        with open(os.path.join(REPO_ROOT, "conformance", "lanes.json")) as f:
            lanes = json.load(f)
        ledger = json.loads(text)
        ids = cut_release.scheduled_claims(ledger, lanes)
        self.assertTrue(ids, "no claim rests on a scheduled lane")
        revision = "c" * 40
        after = cut_release.record_revision(text, ids, revision)
        recorded = {f["id"]: f["revision"] for f in json.loads(after)["features"]}
        before = {f["id"]: f["revision"] for f in ledger["features"]}
        for fid in recorded:
            self.assertEqual(recorded[fid], revision if fid in ids else before[fid], fid)
        # And the gate then certifies that revision.
        self.assertEqual(vc.check_release_revision(json.loads(after), lanes, revision), [])
        # Only revision lines change: the file's layout is kept.
        changed = [a for a, b in zip(text.splitlines(), after.splitlines()) if a != b]
        self.assertEqual(len(changed), len(ids))
        self.assertTrue(all('"revision"' in line for line in changed))

    def test_release_entries_go_first_and_skip_what_is_listed(self):
        text = ("<component>\n  <releases>\n"
                '    <release version="2.1.0" date="2026-07-27"/>\n'
                "  </releases>\n</component>\n")
        after = cut_release.add_releases(text, [
            ("2.3.0", "2026-11-05", ["Letters: <tabs> & leaders"]),
            ("2.2.0", "2026-10-29", []),
            ("2.1.0", "2026-07-27", []),
            ("1.0", "2026-06-23", []),
        ])
        versions = [line.split('"')[1] for line in after.splitlines()
                    if "<release version" in line]
        self.assertEqual(versions, ["2.3.0", "2.2.0", "2.1.0"])
        self.assertIn("<li>Letters: &lt;tabs&gt; &amp; leaders</li>", after)
        self.assertIn("<p>Maintenance release.</p>", after)
        import xml.dom.minidom
        xml.dom.minidom.parseString(after)

    def test_notes_leave_out_what_users_do_not_see(self):
        for subject in ("chore(deps): update rust crate zip", "test(render-lab): reduce x",
                        "fix(deps): bump", "docs: git workflow", "render-lab: lock in",
                        "fix(test): reject incomplete evidence"):
            self.assertTrue(cut_release.QUIET.match(subject), subject)
        for subject in ("Letters: a line is a whole number of twips tall",
                        "fix(tables-core): resolve XLSX hyperlink theme colours"):
            self.assertFalse(cut_release.QUIET.match(subject), subject)


class Autorelease(unittest.TestCase):
    def setUp(self):
        self.flow = workflow("autorelease.yml")
        self.jobs = self.flow["jobs"]

    def test_it_runs_on_a_schedule(self):
        self.assertIn("schedule", self.flow["triggers"])
        self.assertIn("workflow_dispatch", self.flow["triggers"])

    def test_nothing_is_tagged_before_the_scheduled_lanes_pass(self):
        """The tag must follow the lanes the evidence gate asks about, run at
        the same revision; a tag first would publish on stale evidence."""
        with open(os.path.join(REPO_ROOT, "conformance", "lanes.json")) as f:
            lanes = json.load(f)["lanes"]
        called = {os.path.basename(body["uses"]) for body in self.jobs.values() if "uses" in body}
        for lane in lanes:
            if lane["runs_on_every_revision"] is False:
                self.assertIn(os.path.basename(lane["workflow"]), called, lane["job"])
        needs = self.jobs["release"]["needs"]
        for name, body in self.jobs.items():
            if "uses" in body:
                self.assertIn(name, needs)
                self.assertIn("workflow_call", workflow(os.path.basename(body["uses"]))["triggers"])

    def test_the_tag_is_published_through_the_gated_workflow(self):
        steps = "\n".join(step.get("run", "") for step in self.jobs["release"]["steps"])
        self.assertIn("gh workflow run publish-flatpak.yml", steps)
        self.assertIn("-f publish=true", steps)
        self.assertIn("--release-revision", steps)
        self.assertNotIn("git push origin main", steps)
        publish = workflow("publish-flatpak.yml")["jobs"]
        for gate in ("evidence", "soak"):
            self.assertIn("inputs.publish", publish[gate]["if"], gate)
        self.assertIn("needs.evidence.result == 'success'", publish["publish"]["if"])
        self.assertIn("needs.soak.result == 'success'", publish["publish"]["if"])

    def test_its_concurrency_group_is_not_one_a_called_workflow_computes(self):
        group = str(self.flow["concurrency"]["group"])
        self.assertNotIn("github.workflow", group)

    def test_a_push_to_main_does_not_cancel_the_release_journeys(self):
        group = str(workflow("collab-letters-gui.yml")["concurrency"]["group"])
        self.assertIn("github.workflow", group)

    def test_the_called_oracle_runs_alone(self):
        nightly = workflow("nightly.yml")
        self.assertEqual(self.jobs["oracle"].get("with"), {"oracle-only": True})
        for name, job in nightly["jobs"].items():
            if name != "oracle":
                self.assertIn("!inputs.oracle-only", job.get("if", ""), name)


if __name__ == "__main__":
    unittest.main()
