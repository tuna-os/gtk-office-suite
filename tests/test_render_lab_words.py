"""The render lab's word score measures our rendering, not tesseract: same-
text words pair nearest first over the page, and a word OCR reads off the
reference that LibreOffice never printed is set aside
(tools/render-lab/compare.py)."""

import os
import sys
import tempfile
import unittest

try:
    import numpy as np
    from PIL import Image  # noqa: F401  (compare.py imports it)
except ImportError:  # pragma: no cover - the render lab's own image has them
    np = None

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "render-lab"))


def word(text, x, y, line=0):
    return (text, x, y, ("1", "1", str(line)), (x - 5, y - 5, 10, 10))


@unittest.skipIf(np is None, "numpy and Pillow are the render lab's")
class MatchWordsTest(unittest.TestCase):
    def setUp(self):
        import compare

        self.compare = compare

    def test_one_missed_word_does_not_shift_every_later_repeat(self):
        # letters/font-sizes: "<n>pt text" on five lines, and OCR ran our
        # first line into one word. Its reference "text" must stay unpaired
        # rather than take line two's, which would push each later "text"
        # a line down.
        ys = (100, 130, 170, 225, 300)
        ref = [word("text", 150, y, i) for i, y in enumerate(ys)]
        ours = [word("8pttext", 135, 100, 0)] + [word("text", 150, y, i) for i, y in enumerate(ys[1:], 1)]
        found, disp, lost = self.compare.match_words(ref, ours)
        self.assertEqual(found, 0.8)
        self.assertEqual(disp, 0.0)
        self.assertEqual(lost, 0)

    def test_a_shifted_page_still_reports_its_shift(self):
        ref = [word("alpha", 100, 100), word("beta", 200, 100)]
        ours = [word("alpha", 100, 112), word("beta", 200, 112)]
        found, disp, _ = self.compare.match_words(ref, ours)
        self.assertEqual(found, 1.0)
        self.assertAlmostEqual(disp, 12 * self.compare.PX_TO_PT)


@unittest.skipIf(np is None, "numpy and Pillow are the render lab's")
class KeepPrintedTest(unittest.TestCase):
    def setUp(self):
        import compare

        self.compare = compare

    def printed(self, text):
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
            f.write(text)
        self.addCleanup(os.remove, f.name)
        return self.compare.printed_words(f.name)

    def kept(self, reads, text):
        return [w[0] for w in self.compare.keep_printed([word(r, 0, 0) for r in reads], self.printed(text))]

    def test_misreads_of_the_reference_are_set_aside(self):
        self.assertEqual(self.kept(["spt", "text", "12pt", "aopt"], "8pt text\n\n12pt text\n\n40pt text\n"), ["text", "12pt"])

    def test_a_bullet_read_as_a_letter_is_not_text(self):
        self.assertEqual(self.kept(["e", "level", "one"], "• Level one\n"), ["level", "one"])

    def test_a_raised_digit_does_not_hide_its_word(self):
        # The layer prints "mc2" and "H2O"; OCR reads the letters of mc
        # alone, and H2O without its lowered 2 is not a printed word.
        self.assertEqual(self.kept(["e", "mc", "and", "ho"], "E = mc2 and H2O\n"), ["e", "mc", "and"])

    def test_joined_and_cut_reads_of_printed_words_count(self):
        # A cut piece too short to tell from a misread ("co") is set aside:
        # dropping a reference word never counts against us.
        self.assertEqual(self.kept(["8pttext", "co", "operate"], "8pt text co-operate\n"), ["8pttext", "operate"])

    def test_without_a_text_layer_every_read_stays(self):
        reads = [word("spt", 0, 0)]
        self.assertEqual(self.compare.keep_printed(reads, None), reads)
        self.assertEqual(self.compare.keep_printed(reads, self.printed("")), reads)


if __name__ == "__main__":
    unittest.main()
