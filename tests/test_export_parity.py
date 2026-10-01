"""Export parity's separate ratchet (tools/render-lab/export_compare.py).

Our-PDF-vs-LibreOffice-PDF verdicts move independently of screenshot
parity: they ratchet against baseline-export.json, never baseline.json.
These tests drive the ratchet directly with synthetic verdict cards, so
they run anywhere (no images, no OCR, no LibreOffice).
"""

import json
import os
import sys
import tempfile
import types
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "render-lab"))

try:
    import numpy  # noqa: F401 (export_compare imports compare, which needs it)
    import export_compare
    HAVE_EXPORT = True
except ImportError:  # pragma: no cover - the render lab's own image has them
    HAVE_EXPORT = False


def fx(app, feature):
    return {
        "app": app,
        "feature": feature,
        "file": f"{feature}.docx",
        "expect": f"{feature} renders",
    }


MANIFEST = [fx("letters", "toc"), fx("letters", "plain-paragraph")]


def card(verdicts):
    # Same shape as export_compare.main()'s scorecard: {key: {tier: metrics}}.
    return {k: {"export": {"verdict": v}} for k, v in verdicts.items()}


def base(verdicts):
    # Same shape as baseline-export.json: verdicts only, never metrics.
    return {k: {"export": v} for k, v in verdicts.items()}


def write_json(path, obj):
    with open(path, "w") as f:
        json.dump(obj, f)


@unittest.skipIf(not HAVE_EXPORT, "numpy and Pillow are the render lab's")
class ExportRatchetTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = os.path.join(self.tmp.name, "baseline-export.json")

    def args(self, update=False):
        return types.SimpleNamespace(baseline=self.base, update_baseline=update, out=self.tmp.name)

    def read_base(self):
        with open(self.base) as f:
            return json.load(f)

    def test_matching_verdicts_pass_clean(self):
        json.dump(base({"letters/toc": "green", "letters/plain-paragraph": "amber"}), open(self.base, "w"))
        export_compare.ratchet(
            self.args(), MANIFEST, card({"letters/toc": "green", "letters/plain-paragraph": "amber"})
        )
        summary = json.load(open(os.path.join(self.tmp.name, "summary-export.json")))
        self.assertEqual(summary["regressed"], [])
        self.assertEqual(summary["improved"], [])

    def test_regression_fails(self):
        json.dump(base({"letters/toc": "green"}), open(self.base, "w"))
        with self.assertRaises(SystemExit) as cm:
            export_compare.ratchet(self.args(), MANIFEST[:1], card({"letters/toc": "amber"}))
        self.assertEqual(cm.exception.code, 1)

    def test_improvement_without_update_fails_stale(self):
        json.dump(base({"letters/toc": "amber"}), open(self.base, "w"))
        with self.assertRaises(SystemExit) as cm:
            export_compare.ratchet(self.args(), MANIFEST[:1], card({"letters/toc": "green"}))
        self.assertEqual(cm.exception.code, 1)

    def test_update_baseline_locks_in_and_merges(self):
        json.dump(
            base({"letters/toc": "amber", "letters/plain-paragraph": "amber", "decks/autofit": "green"}),
            open(self.base, "w"),
        )
        export_compare.ratchet(
            self.args(update=True), MANIFEST, card({"letters/toc": "green", "letters/plain-paragraph": "amber"})
        )
        # The gain is locked in; the other app's entry survives the merge,
        # so a single-app run (--app) never drops the other app.
        self.assertEqual(
            self.read_base(),
            base({"letters/toc": "green", "letters/plain-paragraph": "amber", "decks/autofit": "green"}),
        )

    def test_new_fixture_against_an_existing_baseline_is_stale(self):
        json.dump(base({"letters/toc": "green"}), open(self.base, "w"))
        with self.assertRaises(SystemExit):
            export_compare.ratchet(
                self.args(), MANIFEST, card({"letters/toc": "green", "letters/plain-paragraph": "amber"})
            )

    def test_a_baselined_fixture_with_no_capture_is_a_regression(self):
        # The ratchet used to `continue` past a fixture with no measurement,
        # so a verdict could be locked in and then never checked again. That
        # is how every Tables export fixture sat "green" in the baseline
        # while CI never exported a single Tables PDF (#1194): an app missing
        # from the lab is evidence lost, not a clean run.
        json.dump(base({"letters/toc": "green", "letters/plain-paragraph": "green"}), open(self.base, "w"))
        with self.assertRaises(SystemExit) as cm:
            export_compare.ratchet(self.args(), MANIFEST, card({"letters/toc": "green"}))
        self.assertEqual(cm.exception.code, 1)
        summary = json.load(open(os.path.join(self.tmp.name, "summary-export.json")))
        self.assertEqual(
            summary["regressed"],
            [{"fixture": "letters/plain-paragraph", "tier": "export", "from": "green", "to": "missing"}],
        )

    def test_a_fixture_baselined_as_missing_with_no_capture_still_passes(self):
        # Nothing was ever measured, so nothing was lost: no regression.
        json.dump(base({"letters/toc": "green", "letters/plain-paragraph": "missing"}), open(self.base, "w"))
        export_compare.ratchet(self.args(), MANIFEST, card({"letters/toc": "green"}))
        summary = json.load(open(os.path.join(self.tmp.name, "summary-export.json")))
        self.assertEqual(summary["regressed"], [])

    def test_missing_baseline_means_not_yet_measured(self):
        # The seeded baseline-export.json records every opt-in fixture as
        # missing until real export captures exist; the first measured run
        # must fail stale so the verdicts get locked in honestly.
        json.dump(base({"letters/toc": "missing"}), open(self.base, "w"))
        with self.assertRaises(SystemExit):
            export_compare.ratchet(self.args(), MANIFEST[:1], card({"letters/toc": "amber"}))


@unittest.skipIf(not HAVE_EXPORT, "numpy and Pillow are the render lab's")
class ScreenRatchetTest(unittest.TestCase):
    """The screenshot ratchet (compare.py) had the same hole as the export
    one. Each app runs in its own CI job, so an app whose job never uploaded
    simply had no measurements, and its fixtures dropped out of the ratchet
    without a word. A tier is counted as run when any fixture was measured
    in it; within a run tier, a baselined fixture with no measurement is a
    regression. A tier nobody ran (a local `--tier A`) stays out of it."""

    def setUp(self):
        import compare
        self.compare = compare
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = os.path.join(self.tmp.name, "baseline.json")

    def run_ratchet(self, baseline, measured):
        write_json(self.base, baseline)
        manifest = [fx("letters", "toc"), fx("decks", "autofit")]
        card = {k: {t: {"verdict": v} for t, v in tiers.items()} for k, tiers in measured.items()}
        args = types.SimpleNamespace(baseline=self.base, update_baseline=False, out=self.tmp.name)
        self.compare.ratchet(args, manifest, card, {})
        return json.load(open(os.path.join(self.tmp.name, "summary.json")))

    def test_an_app_missing_from_a_tier_that_ran_is_a_regression(self):
        with self.assertRaises(SystemExit) as cm:
            self.run_ratchet(
                {"letters/toc": {"A": "green"}, "decks/autofit": {"A": "green"}},
                {"letters/toc": {"A": "green"}},
            )
        self.assertEqual(cm.exception.code, 1)

    def test_a_tier_nobody_ran_is_not_a_regression(self):
        # Baselined in A and B; this run measured A only, for every fixture.
        summary = self.run_ratchet(
            {"letters/toc": {"A": "green", "B": "green"}, "decks/autofit": {"A": "green", "B": "green"}},
            {"letters/toc": {"A": "green"}, "decks/autofit": {"A": "green"}},
        )
        self.assertEqual(summary["regressed"], [])


try:
    import fixtures  # noqa: F401 (needs python-docx; the lab image has it)
    HAVE_FIXTURES = True
except ImportError:
    HAVE_FIXTURES = False


@unittest.skipIf(not HAVE_FIXTURES, "python-docx and Pillow live in the render-lab image")
class ExportOptInTest(unittest.TestCase):
    def test_opt_in_is_docx_pptx_only_and_small(self):
        # Only the fourteen both-green tables fixtures may opt in
        # (docs/TABLES-EXPORT-PARITY.md carve-out); the rest of xlsx stays
        # out of scope. tables/chart-pie is both-green on screenshots and
        # opted-in-as-amber: its export amber is Tesseract noise, not an
        # exporter gap (the Q4 legend glyph OCRs as "ma" in LibreOffice's
        # PDF and "mq4" in ours, so exact-match word recall is 0.833 < 0.90
        # while the pixels are equivalent) — same class as the
        # letters/nested-list amber, so it stays inside the ratchet.
        for key in fixtures.EXPORT:
            app, _ = key.split("/", 1)
            self.assertIn(app, ("letters", "decks", "tables"), f"{key}: xlsx is out of scope")
            if app == "tables":
                self.assertIn(key, ("tables/values", "tables/merged", "tables/frozen", "tables/alignment", "tables/borders", "tables/cell-fonts", "tables/fills", "tables/col-row-size", "tables/chart", "tables/chart-area", "tables/chart-line", "tables/chart-pie", "tables/chart-scatter", "tables/conditional"), f"{key}: not an opted-in tables fixture")
        self.assertLessEqual(len(fixtures.EXPORT), 49, "batch 11: 43 + 6 tables/* carve-out incl. chart-pie as documented amber")

    def test_every_opt_in_fixture_is_defined(self):
        # An EXPORT key with no save() site would score missing forever.
        src = open(fixtures.__file__).read()
        for key in fixtures.EXPORT:
            _, feature = key.split("/", 1)
            self.assertIn(f'"{feature}"', src, f"{key} has no fixture")


if __name__ == "__main__":
    unittest.main()


class EveryExportAppRunsInCI(unittest.TestCase):
    """The apps with an --export-pdf hook are the apps CI must export.

    `export_render.EXPORT_APPS` names the apps that can export; the export
    lab's matrix, its download steps and its arrival check each restated the
    list by hand. Tables gained a hook and opted fixtures in, but was added
    to none of the three, so its export verdicts were never measured in CI
    and its baseline entries were never enforced (#1194). Derived here so a
    fourth app cannot repeat it.
    """

    WORKFLOW = os.path.join(
        os.path.dirname(__file__), "..", ".github", "workflows", "render-parity.yml"
    )

    def setUp(self):
        import yaml
        sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "render-lab"))
        import export_render
        self.apps = set(export_render.EXPORT_APPS)
        with open(self.WORKFLOW, encoding="utf-8") as handle:
            self.jobs = yaml.safe_load(handle)["jobs"]
        self.assertTrue(self.apps, "export_render.EXPORT_APPS is empty; this check is vacuous")

    def test_the_export_lab_matrix_runs_every_export_app(self):
        self.assertEqual(set(self.jobs["export-lab"]["strategy"]["matrix"]["app"]), self.apps)

    def test_the_report_downloads_and_checks_every_export_app(self):
        steps = self.jobs["export-report"]["steps"]
        downloaded = {
            step["with"]["name"].removeprefix("export-lab-")
            for step in steps
            if "download-artifact" in str(step.get("uses", ""))
        }
        self.assertEqual(downloaded, self.apps)
        check = next(s["run"] for s in steps if s.get("name", "").startswith("Check every app"))
        for app in sorted(self.apps):
            with self.subTest(app=app):
                self.assertRegex(check, rf"for app in [^;]*\b{app}\b")


class ReportsSkipCancelledRuns(unittest.TestCase):
    """The jobs that compare the labs' captures and post the verdict run on
    a failed lab (its missing fixtures are the finding) but not on a
    cancelled run, whose partial captures read as regressions."""

    def test_report_jobs_do_not_run_on_a_cancelled_run(self):
        import yaml
        wf = yaml.safe_load(open(os.path.join(os.path.dirname(__file__), "..", ".github", "workflows", "render-parity.yml")))
        jobs = wf["jobs"]
        reports = {name: job for name, job in jobs.items()
                   if any(n in ("lab", "export-lab") for n in ([job.get("needs")] if isinstance(job.get("needs"), str) else job.get("needs") or []))}
        self.assertEqual(sorted(reports), ["export-report", "report"])
        for name, job in reports.items():
            with self.subTest(job=name):
                self.assertIn("!cancelled()", str(job.get("if", "")), f"{name} would compare a cancelled run's partial captures")
