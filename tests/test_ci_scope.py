"""The heavy lanes skip what a change cannot reach, and still report.

scripts/ci_scope.py decides, from a pull request's changed paths, whether the
workspace lanes (ci.yml `test`, gui-tests.yml `smoke`) have work and which
apps' Flatpaks can change. Getting it wrong is silent in one direction only:
a green check that built nothing. So the fallback is pinned here ("unsure"
runs everything), and so is the shape of the workflows: every heavy step is
gated per step, none by a job-level `if:` or a `paths:` filter, so the
required checks keep reporting (conformance/required_checks.py).
"""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import ci_scope  # noqa: E402

ALL = list(ci_scope.APPS)


def repo_with(files):
    """A throwaway git repository holding `files` (path -> text)."""
    root = Path(tempfile.mkdtemp())
    for name, text in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    subprocess.run(["git", "add", "-A"], cwd=root, check=True)
    return root


class WhatIsInert(unittest.TestCase):
    def test_other_workspaces_and_unrelated_workflows_are_inert(self):
        for path in ("lifafa/src/lib.rs", "mandelbrot/Cargo.lock", "renovate.json",
                     ".github/workflows/publish-flatpak.yml", ".github/workflows/actions.lock"):
            self.assertTrue(ci_scope.is_inert(path), path)

    def test_the_workflows_that_run_the_heavy_lanes_are_not_inert(self):
        for path in ci_scope.HEAVY_WORKFLOWS:
            self.assertFalse(ci_scope.is_inert(path), path)

    def test_code_and_the_things_it_is_built_from_are_not_inert(self):
        for path in ("letters/src/window.rs", "Cargo.lock", "Cargo.toml", "flatpak/org.tunaos.letters.json",
                     "scripts/ci_scope.py", "tests/gui/test_smoke.py", "conformance/capabilities.json",
                     "tools/render-lab/run.sh", "docs/collab-spec/schemas/meta.schema.json"):
            self.assertFalse(ci_scope.is_inert(path), path)

    def test_unclassified_paths_are_not_inert(self):
        for path in ("brand-new-top-level-dir/file.bin", "Makefile", "justfile"):
            self.assertFalse(ci_scope.is_inert(path), path)

    def test_a_prose_file_a_source_quotes_is_not_inert(self):
        root = repo_with({
            "letters-core/src/lib.rs": 'include_str!("../../docs/FORMATS.md");\n',
            "tests/gui/tour.py": 'OUT = "docs/features/img"\n',
            "scripts/x.sh": "cat NOTES.md\n",
            "docs/FORMATS.md": "x", "docs/DESIGN.md": "x",
            "docs/features/img/a.png": "x", "NOTES.md": "x", "ROADMAP.md": "x",
        })
        self.assertFalse(ci_scope.is_inert("docs/FORMATS.md", root))
        self.assertFalse(ci_scope.is_inert("docs/features/img/a.png", root))
        self.assertFalse(ci_scope.is_inert("NOTES.md", root))
        self.assertTrue(ci_scope.is_inert("docs/DESIGN.md", root))
        self.assertTrue(ci_scope.is_inert("ROADMAP.md", root))

    def test_a_prose_file_a_directory_walk_reads_is_not_inert(self):
        root = repo_with({
            "decks-core/src/lib.rs": 'std::fs::read_dir("docs/examples")\n',
            "docs/examples/one.json": "{}",
        })
        self.assertFalse(ci_scope.is_inert("docs/examples/one.json", root))

    def test_the_real_repository_keeps_what_rust_includes(self):
        # include_str!("../../docs/FORMATS.md") is in the workspace today.
        self.assertFalse(ci_scope.is_inert("docs/FORMATS.md"))


class WhichFlatpaksCanChange(unittest.TestCase):
    def test_an_app_crate_reaches_that_app_only(self):
        self.assertEqual(ci_scope.apps_reached(["decks/src/window.rs"]), ["decks"])
        self.assertEqual(ci_scope.apps_reached(["tables-core/src/model.rs"]), ["tables"])

    def test_a_shared_crate_reaches_the_apps_that_link_it(self):
        self.assertEqual(ci_scope.apps_reached(["suite-common-core/src/units.rs"]), ALL)

    def test_shared_files_and_unknown_paths_reach_every_app(self):
        for path in ("Cargo.lock", "flatpak/org.tunaos.letters.json", "third_party/README.md",
                     "tools/flatpak-smoke/letters_page_ink.py", "no-such-dir/file"):
            self.assertEqual(ci_scope.apps_reached([path]), ALL, path)

    def test_inert_paths_add_nothing(self):
        self.assertEqual(ci_scope.apps_reached(["lifafa/src/x.rs", "decks/src/a.rs"]), ["decks"])
        self.assertEqual(ci_scope.apps_reached(["lifafa/src/x.rs"]), [])


def steps(workflow, job):
    with open(REPO_ROOT / ".github" / "workflows" / workflow, encoding="utf-8") as handle:
        return yaml.safe_load(handle)["jobs"][job]["steps"]


class TheHeavyStepsAreGated(unittest.TestCase):
    """Each step after the scope step is decided by it; none is left running."""

    def check(self, workflow, job, expression, ungated=()):
        body = steps(workflow, job)
        scope = next(i for i, step in enumerate(body) if step.get("id") == "scope")
        gated = [step for step in body[scope + 1:] if step.get("name") not in ungated]
        self.assertTrue(gated, f"{workflow} {job}: nothing follows the scope step")
        for step in gated:
            self.assertIn(expression, str(step.get("if", "")),
                          f"{workflow} {job}: step {step.get('name', step)} runs whatever the scope says")

    def test_ci_test_job(self):
        self.check("ci.yml", "test", "steps.scope.outputs.heavy == 'true'")

    def test_gui_smoke_job(self):
        self.check("gui-tests.yml", "smoke", "steps.scope.outputs.heavy == 'true'")

    def test_ci_flatpak_job(self):
        body = steps("ci.yml", "flatpak")
        for step in body:
            self.assertIn("needs.test.outputs.flatpak", str(step.get("if", "")), step.get("name", step))

    def test_no_heavy_job_is_skipped_as_a_whole(self):
        for workflow, job in (("ci.yml", "test"), ("ci.yml", "flatpak"), ("gui-tests.yml", "smoke")):
            with open(REPO_ROOT / ".github" / "workflows" / workflow, encoding="utf-8") as handle:
                self.assertNotIn("if", yaml.safe_load(handle)["jobs"][job], f"{workflow} {job}")


if __name__ == "__main__":
    unittest.main()
