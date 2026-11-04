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
class MarkPrintedTest(unittest.TestCase):
    def setUp(self):
        import compare

        self.compare = compare

    def printed(self, text):
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
            f.write(text)
        self.addCleanup(os.remove, f.name)
        return self.compare.printed_words(f.name)

    def kept(self, reads, text):
        marked = self.compare.mark_printed([word(r, 0, 0) for r in reads], self.printed(text))
        return [w[0] for w in marked if w[5]]

    def test_misreads_of_the_reference_are_marked(self):
        self.assertEqual(
            self.kept(["spt", "text", "12pt", "aopt"], "8pt text\n\n12pt text\n\n40pt text\n"), ["text", "12pt"]
        )

    def test_a_bullet_read_as_a_letter_is_not_text(self):
        self.assertEqual(self.kept(["e", "level", "one"], "\u2022 Level one\n"), ["level", "one"])

    def test_a_raised_digit_does_not_hide_its_word(self):
        # The layer prints "mc2" and "H2O"; OCR reads the letters of mc
        # alone, and H2O without its lowered 2 is not a printed word.
        self.assertEqual(self.kept(["e", "mc", "and", "ho"], "E = mc2 and H2O\n"), ["e", "mc", "and"])

    def test_joined_and_cut_reads_of_printed_words_count(self):
        # A cut piece too short to tell from a misread ("co") is marked
        # unprinted: that only ever leaves it out when we miss it.
        self.assertEqual(self.kept(["8pttext", "co", "operate"], "8pt text co-operate\n"), ["8pttext", "operate"])

    def test_without_a_text_layer_every_read_is_printed(self):
        reads = [word("spt", 0, 0)]
        self.assertTrue(self.compare.mark_printed(reads, None)[0][5])
        self.assertTrue(self.compare.mark_printed(reads, self.printed(""))[0][5])

    def test_an_unprinted_read_counts_only_where_we_match_it(self):
        # A logo's "canada" is in no text layer but on both pages: it
        # counts. A misread "spt" we don't share is left out, not missed.
        ref = [(*word("canada", 100, 100), False), (*word("spt", 100, 200), False), (*word("text", 150, 200), True)]
        ours = [word("canada", 100, 100), word("text", 150, 200)]
        found, disp, _ = self.compare.match_words(ref, ours)
        self.assertEqual(found, 1.0)
        self.assertEqual(disp, 0.0)
        found, _, _ = self.compare.match_words(ref, [word("text", 150, 200)])
        self.assertEqual(found, 1.0, "an unmatched unprinted read is not a miss")


@unittest.skipIf(np is None, "numpy and Pillow are the render lab's")
class LettersTableOcrRegressionTest(unittest.TestCase):
    """letters/table is the tracked amber fixture
    (docs/RENDER-PARITY-ROADMAP.md, "Known metric artifacts"): its pixels and
    cell text sit within ~0.2pt of LibreOffice's, but tesseract misreads one
    cell label — "R1C1" as "rici", unhinted glyphs at 12px — so 8 of 9 cell
    words match and the fixture stays amber. That is a measurement gap, not a
    rendering gap, so this regression pins the misread and the unchanged
    budget. It is not to be "fixed" by redrawing the cell, loosening a
    threshold, or dropping the word metric: the fixture is amber on purpose
    until the metric can read the glyph.

    tools/render-lab/compare.py (match_words, verdict),
    tools/render-lab/fixtures.py (the 3x3 R1C1..R3C3 table)."""

    def setUp(self):
        import compare

        self.compare = compare

    def _grid(self):
        # The fixture's nine cell centres, a 3x3 grid 30px apart.
        words = []
        for r in range(3):
            for c in range(3):
                words.append(word(f"r{r + 1}c{c + 1}", 12 + 30 * c, 12 + 30 * r))
        return words

    def test_the_cell_label_misread_is_a_miss(self):
        # "R1C1" read as "rici" on our side: it shares no text with the
        # reference's "r1c1", so match_words finds 8 of 9 — the
        # roadmap's number — and the miss stands when nothing rescues it.
        ref = self._grid()
        ours = [word("rici", 12, 12)] + [
            word("r1c2", 42, 12), word("r1c3", 72, 12),
            word("r2c1", 12, 42), word("r2c2", 42, 42), word("r2c3", 72, 42),
            word("r3c1", 12, 72), word("r3c2", 42, 72), word("r3c3", 72, 72),
        ]
        found, disp, lost = self.compare.match_words(ref, ours)
        self.assertEqual(found, 8 / 9)
        self.assertEqual(lost, 0)
        self.assertLess(found, self.compare.GREEN_WORDS)

    def test_magnifier_rescues_two_but_not_r1c1(self):
        # Page-wide, R1C1 misreads and R1C2/R1C3 drop out of ours: 6 of 9.
        # The magnifier fallback reads the crops closely and rescues R1C2
        # and R1C3 (they read as themselves on both sides) but not R1C1,
        # which misreads even magnified: back to 8 of 9.
        ref = self._grid()
        ours = [word("rici", 12, 12)] + [
            word("r2c1", 12, 42), word("r2c2", 42, 42), word("r2c3", 72, 42),
            word("r3c1", 12, 72), word("r3c2", 42, 72), word("r3c3", 72, 72),
        ]
        found, _, lost = self.compare.match_words(ref, ours)
        self.assertEqual(found, 6 / 9)
        self.assertEqual(lost, 0)

        def reread(box):
            cx = box[0] + box[2] / 2  # the crop's centre-x
            if cx < 30:               # R1C1: misreads even under the magnifier
                return None
            return "r1c2" if cx < 55 else "r1c3"

        found, _, lost = self.compare.match_words(ref, ours, reread=reread)
        self.assertEqual(found, 8 / 9)
        self.assertEqual(lost, 0)

    def test_verdict_amber_only_for_the_word_recall(self):
        # Every metric the roadmap records for letters/table is green
        # except word recall (8/9 < 0.90); verdict() returns amber on that
        # one number — the pixels agree, the metric disagrees.
        m = {
            "ink": 1.02,
            "words": 8 / 9,
            "lost_lines": 0,
            "disp_pt": 0.3,
            "colors": 1.0,
            "ssim": 1.00,
            "ref_words": 9,
        }
        self.assertEqual(self.compare.verdict(m), "amber")
        # Amber only because words < GREEN_WORDS, and the budget is
        # unchanged: loosening it to 0.85 would let 8/9 go green, which
        # this pins. No rendering change moves it; only the metric can.
        self.assertEqual(self.compare.GREEN_WORDS, 0.90)
        self.assertLess(8 / 9, self.compare.GREEN_WORDS)


if __name__ == "__main__":
    unittest.main()
