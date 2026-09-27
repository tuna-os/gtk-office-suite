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

    def test_missing_baseline_means_not_yet_measured(self):
        # The seeded baseline-export.json records every opt-in fixture as
        # missing until real export captures exist; the first measured run
        # must fail stale so the verdicts get locked in honestly.
        json.dump(base({"letters/toc": "missing"}), open(self.base, "w"))
        with self.assertRaises(SystemExit):
            export_compare.ratchet(self.args(), MANIFEST[:1], card({"letters/toc": "amber"}))


try:
    import fixtures  # noqa: F401 (needs python-docx; the lab image has it)
    HAVE_FIXTURES = True
except ImportError:
    HAVE_FIXTURES = False


@unittest.skipIf(not HAVE_FIXTURES, "python-docx and Pillow live in the render-lab image")
class ExportOptInTest(unittest.TestCase):
    def test_opt_in_is_docx_pptx_only_and_small(self):
        for key in fixtures.EXPORT:
            app, _ = key.split("/", 1)
            self.assertIn(app, ("letters", "decks"), f"{key}: xlsx is out of scope")
        self.assertLessEqual(len(fixtures.EXPORT), 23, "batch 5: 18 + 5 green-baseline fixtures; grow deliberately")

    def test_every_opt_in_fixture_is_defined(self):
        # An EXPORT key with no save() site would score missing forever.
        src = open(fixtures.__file__).read()
        for key in fixtures.EXPORT:
            _, feature = key.split("/", 1)
            self.assertIn(f'"{feature}"', src, f"{key} has no fixture")


if __name__ == "__main__":
    unittest.main()
