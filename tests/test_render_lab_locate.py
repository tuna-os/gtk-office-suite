"""Tier B finds the page on screen by matching the app's own render of it
(tools/render-lab/capture.py locate). A page size from geom.json within a
pixel of the template's is the same page, not a rescale (#1200)."""

import os
import sys
import tempfile
import unittest

try:
    import numpy as np
    from PIL import Image
except ImportError:  # pragma: no cover - the render lab's own image has them
    np = None

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "render-lab"))


@unittest.skipIf(np is None, "numpy and Pillow are the render lab's")
class LocateTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        rng = np.random.default_rng(1200)
        # A text-dense page: short dark strokes on white, like lines of glyphs.
        page = np.full((561, 397), 255, dtype=np.uint8)
        for top in range(20, 540, 14):
            for left in range(20, 370, 7):
                if rng.random() < 0.8:
                    page[top : top + 9, left : left + 2] = 0
        self.page = page
        self.template = os.path.join(self.dir, "A-1.png")
        Image.fromarray(page).save(self.template)

    def screen(self, page, at=(31, 47)):
        shot = np.full((700, 500), 192, dtype=np.uint8)
        shot[at[1] : at[1] + page.shape[0], at[0] : at[0] + page.shape[1]] = page
        path = os.path.join(self.dir, "B-full.png")
        Image.fromarray(shot).save(path)
        return path

    def test_a_fractional_page_size_is_not_a_rescale(self):
        import capture

        full = self.screen(self.page)
        # geom.json's page is 560.5 px tall; the template was drawn 561.
        found, mad = capture.locate(self.template, full, (397.0, 560.5), (31, 47))
        self.assertEqual(found, (31, 47))
        self.assertLess(mad, 1.0, "the page itself must match, well under MAX_LOCATE_DIFF")

    def test_a_real_rescale_still_resizes_the_template(self):
        import capture

        small = np.asarray(Image.fromarray(self.page).resize((199, 281)))
        full = self.screen(small, at=(60, 80))
        found, mad = capture.locate(self.template, full, (199, 281), (60, 80))
        self.assertEqual(found, (60, 80))
        self.assertLess(mad, capture.MAX_LOCATE_DIFF)


if __name__ == "__main__":
    unittest.main()
