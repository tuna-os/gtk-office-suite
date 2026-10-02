"""The visual matrix's shape (#1284): every config, every cell, an expected
image for each, a threshold table that names only real cells, the pinned
fonts, and a nightly workflow that runs it. The comparison itself runs in
the GUI image (it needs PIL and a display); this checks what has to be
true before it can mean anything."""

import json
import os
import sys
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
GUI = os.path.join(ROOT, "tests", "gui")
sys.path.insert(0, GUI)
import visual_compare  # noqa: E402


class VisualMatrix(unittest.TestCase):
    def test_the_matrix_is_three_widths_three_themes_two_scales(self):
        names = [name for name, _ in visual_compare.configs()]
        self.assertEqual(len(names), 18)
        self.assertEqual(len(set(names)), 18)
        for w in (400, 800, 1280):
            for theme in ("light", "dark", "hc"):
                for scale in (1, 2):
                    self.assertIn(f"w{w}-{theme}-s{scale}", names)

    def test_a_config_names_itself_from_its_own_environment(self):
        for name, env in visual_compare.configs():
            self.assertEqual(visual_compare.config_name(env), name)

    def test_every_cell_has_an_expected_image(self):
        missing = [
            os.path.relpath(visual_compare.expected_path(config, cell), ROOT)
            for config, _ in visual_compare.configs()
            for cell in visual_compare.cells()
            if not os.path.exists(visual_compare.expected_path(config, cell))
        ]
        self.assertEqual(missing, [], "record them with tests/gui/visual_matrix.py --update")
        self.assertEqual(len(visual_compare.cells()), 12)

    def test_no_expected_image_is_left_over(self):
        known = {visual_compare.expected_path(c, cell) for c, _ in visual_compare.configs() for cell in visual_compare.cells()}
        stray = []
        for dirpath, _, files in os.walk(visual_compare.EXPECTED_DIR):
            stray += [os.path.join(dirpath, f) for f in files if os.path.join(dirpath, f) not in known]
        self.assertEqual(stray, [])

    def test_thresholds_name_real_cells(self):
        with open(visual_compare.THRESHOLDS) as f:
            table = json.load(f)
        self.assertLess(table["default"], 0.05)
        cells = {f"{c}/{cell}" for c, _ in visual_compare.configs() for cell in visual_compare.cells()}
        for key, value in table.get("cells", {}).items():
            self.assertIn(key, cells)
            self.assertLess(value, 0.2, key)

    def test_fonts_are_pinned_to_the_bundled_ones(self):
        conf = open(os.path.join(visual_compare.VISUAL_DIR, "fonts.conf")).read()
        self.assertIn('<dir prefix="relative">fonts</dir>', conf)
        self.assertNotIn("/etc/fonts", conf, "system fonts would leak in")
        for font in ("DejaVuSans.ttf", "DejaVuSansMono.ttf", "LICENSE"):
            self.assertTrue(os.path.exists(os.path.join(visual_compare.VISUAL_DIR, "fonts", font)), font)

    def test_a_nightly_workflow_runs_it(self):
        wf = open(os.path.join(ROOT, ".github", "workflows", "visual-matrix.yml")).read()
        self.assertIn("schedule:", wf)
        self.assertIn("tests/gui/visual_matrix.py", wf)
        self.assertIn("upload-artifact", wf)


if __name__ == "__main__":
    unittest.main()
