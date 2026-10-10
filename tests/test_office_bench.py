"""Scoring and verdict rules of tools/render-lab/office_bench.py."""

import os
import sys
import tempfile
import unittest

try:
    import numpy as np
    import skimage  # noqa: F401
    from PIL import Image
except ImportError:  # pragma: no cover - the render lab's own dependencies
    np = None

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "render-lab"))


@unittest.skipIf(np is None, "numpy, Pillow and scikit-image are needed")
class OfficeBenchTest(unittest.TestCase):
    def setUp(self):
        import office_bench

        self.bench = office_bench
        self.dir = tempfile.mkdtemp()

    def page(self, name, size, text_box):
        image = Image.new("RGB", size, "white")
        x0, y0, x1, y1 = text_box
        image.paste((0, 0, 0), (x0, y0, x1, y1))
        path = os.path.join(self.dir, name)
        image.save(path)
        return path

    def test_an_identical_page_scores_near_its_resize_ceiling(self):
        ref = self.page("ref.png", (300, 400), (30, 40, 270, 60))
        ours = self.page("ours.png", (200, 267), (20, 27, 180, 40))
        s = self.bench.score([ref], [ours])
        self.assertEqual((s["reference_pages"], s["actual_pages"]), (1, 1))
        self.assertGreater(s["penalized_ssim"], 0.9)
        self.assertLessEqual(s["penalized_ssim"], s["resize_ceiling"] + 0.02)

    def test_an_extra_page_is_penalised(self):
        ref = self.page("ref.png", (300, 400), (30, 40, 270, 60))
        ours = self.page("ours.png", (300, 400), (30, 40, 270, 60))
        one = self.bench.score([ref], [ours])
        two = self.bench.score([ref], [ours, ours])
        self.assertAlmostEqual(two["penalized_ssim"], one["penalized_ssim"] / 2)
        self.assertEqual(two["common_page_ssim"], one["common_page_ssim"])

    def test_the_closer_engine_wins_and_near_scores_tie(self):
        def row(ours, bo):
            return {"ours": {"penalized_ssim": ours}, "betteroffice": {"penalized_ssim": bo}}

        self.assertEqual(self.bench.winner(row(0.90, 0.80)), "ours")
        self.assertEqual(self.bench.winner(row(0.80, 0.90)), "betteroffice")
        self.assertEqual(self.bench.winner(row(0.900, 0.903)), "tie")
        self.assertIsNone(self.bench.winner({"betteroffice": {"penalized_ssim": 0.9}}))

    def test_betteroffice_stays_a_reference_only_while_it_beats_us_more_often(self):
        self.assertEqual(self.bench.verdict({"ours": 3, "betteroffice": 5, "tie": 9}), "reference")
        self.assertEqual(self.bench.verdict({"ours": 5, "betteroffice": 5, "tie": 0}), "drop")
        self.assertEqual(self.bench.verdict({"ours": 6, "betteroffice": 2, "tie": 1}), "drop")


if __name__ == "__main__":
    unittest.main()
