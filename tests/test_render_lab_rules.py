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
class BlankRulesTest(unittest.TestCase):
    """A rescue crop loses the rules that cross it and keeps its glyph
    (#1199: a light cell border beside a lone "3" left tesseract reading
    nothing at all)."""

    def setUp(self):
        import compare

        self.compare = compare
        self.g = np.full((18, 15), 255, dtype=np.uint8)
        self.g[6:14, 6:11] = 40  # a digit's strokes, 8 of 18 rows

    def blanked(self):
        return np.asarray(self.compare.blank_rules(Image.fromarray(self.g)))

    def test_a_light_full_height_border_goes(self):
        self.g[1:18, 13] = 217  # a grid line, 17 of 18 rows
        out = self.blanked()
        self.assertTrue((out[:, 13] == 255).all())
        self.assertTrue((out[6:14, 6:11] == 40).all(), "the glyph stays")

    def test_a_full_width_underline_goes(self):
        self.g[16, :] = 120
        self.assertTrue((self.blanked()[16] == 255).all())

    def test_a_glyph_stroke_is_not_a_rule(self):
        self.g[3:16, 8] = 0  # a tall stroke, 13 of 18 rows
        out = self.blanked()
        self.assertTrue((out[3:16, 8] == 0).all())


@unittest.skipIf(np is None, "numpy and Pillow are the render lab's")
class UnionWordsTest(unittest.TestCase):
    """union_words (tools/render-lab/compare.py): a second OCR scale may
    only add reads, never take them."""

    def setUp(self):
        import compare

        self.union = compare.union_words

    def test_second_scale_adds_a_missed_token(self):
        base = [("25", 10.0, 10.0, (1, 1, 1), (8, 8, 5, 5))]
        extra = [("7", 20.0, 10.0, (1, 1, 2), (18, 8, 5, 5))]
        out = self.union(base, extra)
        self.assertEqual([t for t, _, _, _, _ in out], ["25", "7"])

    def test_same_text_nearby_is_one_read(self):
        first = [("7", 20.0, 10.0, (1, 1, 2), (18, 8, 5, 5))]
        second = [("7", 22.0, 11.0, (1, 1, 2), (20, 9, 5, 5))]  # same digit, other scale
        self.assertEqual(len(self.union(first, second)), 1)

    def test_same_text_far_apart_stays_two_reads(self):
        first = [("7", 20.0, 10.0, (1, 1, 2), (18, 8, 5, 5))]
        second = [("7", 200.0, 10.0, (1, 1, 3), (198, 8, 5, 5))]  # another "7" elsewhere
        self.assertEqual(len(self.union(first, second)), 2)

    def test_first_scale_loses_nothing(self):
        first = [("q2", 10.0, 10.0, (1, 1, 1), (8, 8, 5, 5)), ("qs", 30.0, 10.0, (1, 1, 1), (28, 8, 5, 5))]
        out = self.union(first, [])
        self.assertEqual(out, first)


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



@unittest.skipIf(np is None, "numpy and Pillow are the render lab's")
class OpeningSheetPageTest(unittest.TestCase):
    """Tables opens a workbook's active sheet; LibreOffice prints from the
    first. The opening sheet is scored against the printed page that shows
    it, not against page 1 (#1512)."""

    # Our opening sheet's words, and each printed page's.
    OURS = ["total", "2024", "2025", "north", "south"]
    PAGES = {1: ["cover", "contents"], 2: ["notes", "2024"], 3: ["total", "2024", "2025", "north", "east"]}

    def score(self, app, words_by_page, pages=None):
        import tempfile
        from collections import Counter
        from unittest import mock

        import compare

        pages = pages if pages is not None else self.PAGES
        d = tempfile.mkdtemp()
        for n in words_by_page:
            open(os.path.join(d, f"lo-{n}.png"), "w").close()
        open(os.path.join(d, "A-1.png"), "w").close()
        scored = []

        def page(app, lo, ours, ref_words):
            n = compare.page_no(lo)
            scored.append(n)
            return {"ink": 1.0, "words": words_by_page[n], "lost_lines": 0, "disp_pt": 0.0, "colors": 1.0, "ssim": 0.9, "ref_words": 10}

        def bag(path):
            name = os.path.basename(path)
            return Counter(self.OURS if name.startswith("A-") else pages[compare.page_no(path)])

        # `align` hands back the paths: `ref.size` is all score_fixture
        # asks of a picture.
        with mock.patch.object(compare, "align", lambda app, lo, ours: (_Sized(lo), ours, 1.0)), mock.patch.object(
            compare, "ocr_words", lambda ref, tables: []
        ), mock.patch.object(compare, "compare_page", page), mock.patch.object(compare, "page_bag", bag):
            m = compare.score_fixture(app, d, "A", {})
        m["scored"] = scored
        return m

    def test_the_sheet_is_scored_against_the_page_that_shows_it(self):
        m = self.score("tables", {1: 0.1, 2: 0.2, 3: 0.95})
        self.assertEqual(m["lo_page"], 3)
        self.assertAlmostEqual(m["words"], 0.95)
        self.assertEqual(m["verdict"], "green")

    def test_only_the_chosen_page_is_scored_in_full(self):
        # Scoring every candidate in full ran the Tables job past its
        # 90-minute timeout; the pick is a quick read per page.
        m = self.score("tables", {1: 0.1, 2: 0.2, 3: 0.95})
        self.assertEqual(m["scored"], [3])

    def test_a_tie_or_no_shared_words_keeps_the_first_page(self):
        m = self.score("tables", {1: 0.1, 2: 0.2, 3: 0.95}, pages={1: ["a"], 2: ["b"], 3: ["c"]})
        self.assertEqual(m["lo_page"], 1)

    def test_a_document_is_still_scored_page_by_page(self):
        m = self.score("letters", {1: 0.1, 2: 0.2, 3: 0.95})
        self.assertNotIn("lo_page", m)
        self.assertFalse(m["page_count_match"])


class _Sized(str):
    """A page path standing in for its picture: score_fixture keys its OCR
    cache on the picture's size."""

    size = (1, 1)


if __name__ == "__main__":
    unittest.main()
