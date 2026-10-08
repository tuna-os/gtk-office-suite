"""scripts/compare_gui_screenshots.py: two runs' journey screenshots, paired
by journey. A failed journey's screenshot (`.failed.png`) pairs with the
same journey's pass, a changed screen is reported and an unchanged one is
not."""

import os
import sys
import tempfile
import unittest

from PIL import Image, ImageDraw

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "scripts"))
import compare_gui_screenshots as compare  # noqa: E402


class CompareScreenshots(unittest.TestCase):
    def test_a_changed_journey_is_reported_and_an_unchanged_one_is_not(self):
        with tempfile.TemporaryDirectory() as d:
            before, after = os.path.join(d, "a"), os.path.join(d, "b")
            os.makedirs(before)
            os.makedirs(after)
            for name in ("m.C.test_same", "m.C.test_changed", "m.C.test_gone"):
                Image.new("RGB", (200, 100), "white").save(os.path.join(before, name + ".png"))
            Image.new("RGB", (200, 100), "white").save(os.path.join(after, "m.C.test_same.png"))
            changed = Image.new("RGB", (200, 100), "white")
            ImageDraw.Draw(changed).rectangle([0, 0, 100, 100], fill="black")
            changed.save(os.path.join(after, "m.C.test_changed.failed.png"))
            out = os.path.join(d, "out")
            self.assertEqual(compare.main([before, after, "--out", out]), 0)
            report = open(os.path.join(out, "report.html")).read()
            self.assertIn("1 of 2 journeys look different", report)
            self.assertIn("m.C.test_changed", report)
            self.assertIn("m.C.test_gone (before only)", report)
            self.assertNotIn("<h3>m.C.test_same", report)


if __name__ == "__main__":
    unittest.main()
