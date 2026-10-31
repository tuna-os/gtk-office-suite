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
    # Page 1 is guidance prose: it shares more words with the sheet than
    # page 3 does, but they are a small part of it.
    OURS = ["total", "2024", "2025", "north", "south", "the", "of", "and", "costs"]
    PAGES = {
        1: ["the", "of", "and", "costs", "total", "you", "must", "read", "these", "rules", "before", "claiming", "any", "support"],
        2: ["notes", "2024", "learner", "details"],
        3: ["total", "2024", "2025", "north", "east"],
    }

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

    def test_a_sheet_printed_over_several_pages_starts_at_its_first(self):
        # A long sheet prints over pages 2 and 3, both held by our capture,
        # which starts at the sheet's top.
        pages = {1: ["cover", "contents"], 2: ["total", "2024"], 3: ["2025", "north"]}
        m = self.score("tables", {1: 0.1, 2: 0.6, 3: 0.3}, pages=pages)
        self.assertEqual(m["lo_page"], 2)

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


@unittest.skipIf(np is None, "numpy and Pillow are the render lab's")
class VerdictWordsTest(unittest.TestCase):
    def setUp(self):
        import compare

        self.verdict = compare.verdict
        self.m = {"ink": 0.95, "ssim": 0.94, "words": 0.0, "lost_lines": 0, "disp_pt": None, "colors": 1.0}

    def test_a_slide_without_text_is_not_held_back_by_ocr_noise(self):
        self.assertEqual(self.verdict(dict(self.m, ref_words=1)), "green")

    def test_text_that_is_not_recalled_still_holds_a_page_back(self):
        self.assertEqual(self.verdict(dict(self.m, ref_words=3, words=0.6)), "amber")
        self.assertEqual(self.verdict(dict(self.m, ref_words=12, words=0.2)), "red")

@unittest.skipIf(np is None, "numpy and Pillow are the render lab's")
class HorizontalTilesTest(unittest.TestCase):
    """Spreadsheets may span multiple horizontal pages when printed, or
    omit printed headings (#1341)."""

    def setUp(self):
        import compare

        self.compare = compare

    def test_is_monochrome_rule(self):
        # Black border
        black = np.zeros((10, 100, 3), dtype=np.uint8)
        self.assertTrue(self.compare.is_monochrome_rule(black, 0, 1))

        # Grey border
        grey = np.full((10, 100, 3), 100, dtype=np.uint8)
        self.assertTrue(self.compare.is_monochrome_rule(grey, 0, 1))

        # Blue hyperlink underline
        blue = np.full((10, 100, 3), 255, dtype=np.uint8)
        blue[5, :, :] = (0, 0, 128)
        self.assertFalse(self.compare.is_monochrome_rule(blue, 5, 1))

    def test_stitch_horizontal_tiles(self):
        # Two tiles sharing vertical band y=10..50
        im1 = Image.new("RGB", (100, 100), "white")
        arr1 = np.asarray(im1).copy()
        arr1[10:50, 20:60] = 0  # w=40, h=40
        im1 = Image.fromarray(arr1)

        im2 = Image.new("RGB", (100, 100), "white")
        arr2 = np.asarray(im2).copy()
        arr2[12:48, 10:70] = 0  # w=60, h=36, overlaps y=12..48
        im2 = Image.fromarray(arr2)

        stitched = self.compare.stitch_horizontal_tiles([im1, im2])
        # Width should be 40 + 60 = 100
        self.assertEqual(stitched.width, 100)
        # Height should span min y (10) to max y (49): 49 - 10 + 1 = 40
        self.assertEqual(stitched.height, 40)

    def test_collect_sheet_tiles(self):
        import tempfile

        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "lo-1.png")
            p2 = os.path.join(d, "lo-2.png")
            p3 = os.path.join(d, "lo-3.png")
            ours_p = os.path.join(d, "A-1.png")

            # Page 1: left tile (w=50, h=60)
            im1 = Image.new("RGB", (100, 150), "white")
            a1 = np.asarray(im1).copy()
            a1[20:80, 10:60] = 0
            Image.fromarray(a1).save(p1)

            # Page 2: right continuation tile (w=50, h=60, same vertical band)
            im2 = Image.new("RGB", (100, 150), "white")
            a2 = np.asarray(im2).copy()
            a2[20:80, 10:60] = 0
            Image.fromarray(a2).save(p2)

            # Page 3: different sheet / vertical continuation (y=90..140)
            im3 = Image.new("RGB", (100, 150), "white")
            a3 = np.asarray(im3).copy()
            a3[90:140, 10:60] = 0
            Image.fromarray(a3).save(p3)

            # Ours: wide capture (w=160) with headers ox=40, oy=20 -> cells w=120
            ours_im = Image.new("RGB", (160, 100), "white")
            ours_im.paste((240, 240, 240), (0, 0, 160, 20))
            ours_im.paste((240, 240, 240), (0, 0, 40, 100))
            a_ours = np.asarray(ours_im).copy()
            a_ours[30:80, 50:140] = 0
            Image.fromarray(a_ours).save(ours_p)

            lo_pages = [p1, p2, p3]
            tiles = self.compare.collect_sheet_tiles(lo_pages, p1, ours_p)
            self.assertEqual(tiles, [p1, p2])

    def test_align_unprinted_sheet_with_hyperlink_underline(self):
        import tempfile

        # Sheet without printed headings containing blue underline
        ref = Image.new("RGB", (200, 100), "white")
        ref_arr = np.asarray(ref).copy()
        ref_arr[20:30, 20:80] = 0  # text
        ref_arr[40, 20:180] = (0, 0, 128)  # long blue hyperlink underline (> 50% width)
        ref = Image.fromarray(ref_arr)

        ours = Image.new("RGB", (240, 120), "white")
        ours.paste((240, 240, 240), (0, 0, 240, 20))
        ours.paste((240, 240, 240), (0, 0, 40, 120))
        ours_arr = np.asarray(ours).copy()
        ours_arr[40:50, 60:120] = 0  # text
        ours_arr[60, 60:220] = (0, 0, 128)
        ours = Image.fromarray(ours_arr)

        with tempfile.TemporaryDirectory() as d:
            rp, op = os.path.join(d, "ref.png"), os.path.join(d, "ours.png")
            ref.save(rp)
            ours.save(op)
            r, o, scale = self.compare.align("tables", rp, op)
            # The blue underline should not trigger ruled=True, so unprinted headings
            # alignment is used and scale is 1.0.
            self.assertAlmostEqual(scale, 1.0, places=2)


if __name__ == "__main__":
    unittest.main()
