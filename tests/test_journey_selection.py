"""Which apps' journeys a change selects (#354).

The selector's job is to run fewer journeys on a small change. Getting
that wrong in the cheap direction is loud — a filter naming an app that
does not exist collects nothing and the run fails. Getting it wrong in the
other direction is silent: too few apps still produces a green run, just
one that never launched the app the change broke. So the cases that matter
most here are the fallbacks, which is exactly what could not be tested
while this logic lived inline in gui-tests.yml.
"""

import os
import subprocess
import unittest

SELECTOR = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "gui", "select_journeys.sh"
)

# What the workflow does with an empty filter: pass no -k at all, so every
# journey in the file runs.
EVERY_APP = ""


def select(*changed_paths):
    """The filter the selector prints for these changed paths."""
    finished = subprocess.run(
        [SELECTOR],
        input="\n".join(changed_paths) + "\n",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=60,
        check=True,
    )
    return finished.stdout.strip(), finished.stderr.strip()


class SelectsTheChangedApps(unittest.TestCase):
    def test_one_app_runs_only_that_app(self):
        self.assertEqual(select("letters/src/window.rs")[0], "Letters")
        self.assertEqual(select("tables-core/src/sheet.rs")[0], "Tables")
        self.assertEqual(select("decks/src/canvas.rs")[0], "Decks")

    def test_several_apps_are_joined_for_pytest(self):
        # `-k` takes a Python-ish expression; "or" is what makes it a union
        # rather than a name nothing matches.
        self.assertEqual(
            select("letters/src/window.rs", "decks/src/canvas.rs")[0],
            "Letters or Decks",
        )

    def test_the_order_does_not_depend_on_the_order_of_the_diff(self):
        forwards = select("letters/a.rs", "tables/b.rs", "decks/c.rs")[0]
        backwards = select("decks/c.rs", "tables/b.rs", "letters/a.rs")[0]
        self.assertEqual(forwards, backwards)
        self.assertEqual(forwards, "Letters or Tables or Decks")

    def test_a_core_crate_selects_its_own_app(self):
        self.assertEqual(select("letters-core/src/save.rs")[0], "Letters")

    def test_an_apps_own_manifest_selects_only_that_app(self):
        # Anchored: `letters/Cargo.toml` is not the workspace lockfile.
        self.assertEqual(select("letters/Cargo.toml")[0], "Letters")


class FallsBackToEveryApp(unittest.TestCase):
    """The conservative branch. Each of these used to be untested, and each
    of them fails silently if it stops working."""

    def _assert_runs_everything(self, *changed_paths):
        filter_expression, reason = select(*changed_paths)
        self.assertEqual(
            filter_expression,
            EVERY_APP,
            f"{changed_paths} must run every app's journeys, got {filter_expression!r}",
        )
        self.assertTrue(reason, "a fallback must say why it is running everything")

    def test_a_shared_crate_runs_everything(self):
        for crate in ("suite-common", "suite-common-core", "suite-export"):
            with self.subTest(crate=crate):
                self._assert_runs_everything(f"{crate}/src/lib.rs")

    def test_the_workspace_lockfile_and_manifest_run_everything(self):
        # A lockfile change can move any dependency underneath any app.
        self._assert_runs_everything("Cargo.lock")
        self._assert_runs_everything("Cargo.toml")

    def test_a_harness_change_runs_everything(self):
        for path in (
            "tests/gui/framework/base.py",
            "tests/gui/conftest.py",
            "tests/gui/run_gui_tests.sh",
            # Including this selector: a selector that selects itself
            # wrongly is the failure the fallback exists for.
            "tests/gui/select_journeys.sh",
        ):
            with self.subTest(path=path):
                self._assert_runs_everything(path)

    def test_an_unrecognized_path_runs_everything(self):
        self._assert_runs_everything("docs/readiness-2026-09/gui-testing.md")
        self._assert_runs_everything("README.md")

    def test_an_empty_diff_runs_everything(self):
        # Not "nothing to test": an empty list means the caller could not
        # work out what changed.
        self._assert_runs_everything("")

    def test_an_app_name_that_is_not_a_top_level_path_runs_everything(self):
        # `docs/letters/...` is documentation, not the Letters crate. A
        # substring match here would quietly narrow the run on exactly the
        # paths the selector has never been taught about.
        self._assert_runs_everything("docs/letters/architecture.md")

    def test_a_shared_change_beats_an_app_change(self):
        # Both present: the shared half is the one that decides, because it
        # can affect the apps the diff does not name. This is also what
        # makes the two cases above mean anything — on a diff that names no
        # app, dropping a path from the shared pattern lands in the
        # unrecognized-path fallback and runs everything regardless, so the
        # only way to observe the pattern itself is to pair it with an app.
        self._assert_runs_everything("letters/src/window.rs", "suite-common/src/lib.rs")
        self._assert_runs_everything("letters/src/window.rs", "Cargo.lock")

    def test_a_harness_change_beats_an_app_change(self):
        for path in (
            "tests/gui/framework/base.py",
            "tests/gui/conftest.py",
            "tests/gui/run_gui_tests.sh",
            "tests/gui/select_journeys.sh",
        ):
            with self.subTest(path=path):
                self._assert_runs_everything("letters/src/window.rs", path)


if __name__ == "__main__":
    unittest.main()
