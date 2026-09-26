"""The render lab's OCR sees text, not the lines around it: erase_rules
removes grid-, border- and axis lines and coloured legend keys, and keeps
filled areas and glyphs (tools/render-lab/compare.py)."""

import os
import sys
import unittest

try:
    import numpy as np
    from PIL import Image  # noqa: F401  (compare.py imports it)
except ImportError:  # pragma: no cover - the render lab's own image has them
    np = None

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "render-lab"))


@unittest.skipIf(np is None, "numpy and Pillow are the render lab's")
class EraseRulesTest(unittest.TestCase):
    def setUp(self):
        import compare

        self.compare = compare
        self.g = np.full((80, 120), 255, dtype=np.uint8)
        self.rgb = np.full((80, 120, 3), 255, dtype=np.uint8)

    def erased(self):
        return self.compare.erase_rules(self.g, self.rgb)

    def test_gridlines_and_their_crossings_go(self):
        self.g[20, :] = 180
        self.g[40, :] = 180
        self.g[:, 30] = 180
        self.g[:, 60] = 180
        self.assertTrue((self.erased() == 255).all())

    def test_a_fill_and_the_text_on_it_stay(self):
        self.g[10:50, 10:90] = 120  # a dark fill
        self.g[25:35, 40:42] = 255  # a light stroke of text on it
        out = self.erased()
        self.assertTrue((out[10:50, 10:90] == self.g[10:50, 10:90]).all())

    def test_a_short_stroke_is_text(self):
        self.g[30:32, 20:40] = 0  # 20 px: a glyph's bar, not a rule
        self.assertTrue((self.erased() == self.g).all())

    def test_a_coloured_legend_key_goes_but_a_black_square_stays(self):
        self.g[10:18, 10:18] = 110
        self.rgb[10:18, 10:18] = (0x4F, 0x81, 0xBD)
        self.g[40:48, 10:18] = 0  # black: could be a glyph
        self.rgb[40:48, 10:18] = (0, 0, 0)
        out = self.erased()
        self.assertTrue((out[10:18, 10:18] == 255).all())
        self.assertTrue((out[40:48, 10:18] == 0).all())


@unittest.skipIf(np is None, "numpy and Pillow are the render lab's")
class MagnifierFallbackTest(unittest.TestCase):
    """A word the page-wide pass misses gets one closer look: match_words
    calls reread with the word's box, and a rescued word counts as found
    where it stands (tools/render-lab/compare.py)."""

    def setUp(self):
        import compare

        self.compare = compare

    def word(self, text, x=0.0, y=0.0, line=(1, 1, 1), box=(0, 0, 10, 10)):
        return (text, x, y, line, box)

    def test_no_reread_means_no_rescue(self):
        ref = [self.word("a"), self.word("b")]
        ours = [self.word("a")]
        self.assertEqual(self.compare.match_words(ref, ours), (0.5, 0.0, 0))

    def test_a_rescued_word_counts_where_it_stands(self):
        ref = [self.word("a"), self.word("b", box=(5, 5, 6, 8))]
        ours = [self.word("a")]
        seen = []
        words, disp, lost = self.compare.match_words(ref, ours, reread=lambda b: seen.append(b) or "b")
        self.assertEqual(seen, [(5, 5, 6, 8)])
        self.assertEqual(words, 1.0)
        self.assertEqual(disp, 0.0)
        self.assertEqual(lost, 0)

    def test_a_refused_word_stays_missed(self):
        ref = [self.word("a"), self.word("b")]
        ours = [self.word("a")]
        self.assertEqual(self.compare.match_words(ref, ours, reread=lambda b: None), (0.5, 0.0, 0))

    def test_a_rescue_updates_its_line(self):
        # Four words on one line, three missed: a lost line. Rescuing one
        # still leaves most missed (lost); rescuing all but one clears it.
        ref = [
            self.word("a"),
            self.word("b", box=(1, 0, 10, 10)),
            self.word("c", box=(2, 0, 10, 10)),
            self.word("d", box=(3, 0, 10, 10)),
        ]
        ours = [self.word("a")]
        _, _, lost = self.compare.match_words(ref, ours, reread=lambda b: None)
        self.assertEqual(lost, 1)
        _, _, lost = self.compare.match_words(ref, ours, reread=lambda b: "x" if b[0] == 1 else None)
        self.assertEqual(lost, 1)
        _, _, lost = self.compare.match_words(ref, ours, reread=lambda b: "x" if b[0] in (1, 2) else None)
        self.assertEqual(lost, 0)


if __name__ == "__main__":
    unittest.main()
