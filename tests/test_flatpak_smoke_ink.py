"""The Flatpak smoke check finds Letters' page on the screen and counts the
text's ink on it, in either style, from Xvfb's framebuffer file
(tools/flatpak-smoke/xwd_ink.py)."""

import os
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "flatpak-smoke"))

import xwd_ink

W, H = 640, 400
WHITE, BLACK = (255, 255, 255), (0, 0, 0)


def screen(backdrop, text=True):
    """A window as Letters draws it: a white header bar across the top, the
    backdrop around a white page, and (with `text`) three lines of ink."""
    rows = [[BLACK] * W for _ in range(H)]
    for y in range(0, 300):
        for x in range(0, 500):
            rows[y][x] = WHITE if y < 50 else backdrop
    for y in range(70, 290):
        for x in range(60, 440):
            rows[y][x] = WHITE
    if text:
        for line in range(3):
            for y in range(100 + 20 * line, 108 + 20 * line):
                for x in range(90, 380, 2):
                    rows[y][x] = BLACK
    return rows


def write_xwd(path, rows):
    """`rows` as Xvfb writes its screen: XWD 7, ZPixmap, 32 bits per
    pixel, LSB first (B, G, R, X), no colormap."""
    name = b"Xvfb main window\0"
    header_size = 100 + len(name)
    fields = [header_size, 7, 2, 24, W, H, 0, 0, 32, 0, 32, 32, W * 4, 4,
              0xFF0000, 0xFF00, 0xFF, 8, 256, 0, W, H, 0, 0, 0]
    with open(path, "wb") as f:
        f.write(struct.pack(">25I", *fields) + name)
        for row in rows:
            f.write(b"".join(bytes((b, g, r, 0)) for (r, g, b) in row))


class XwdInkTest(unittest.TestCase):
    def measure(self, rows):
        path = os.path.join(tempfile.mkdtemp(), "Xvfb_screen0")
        write_xwd(path, rows)
        return xwd_ink.measure(path, png=path + ".png")

    def test_the_page_not_the_header_bar(self):
        m = self.measure(screen((192, 192, 192)))
        self.assertEqual(m["page"], (60, 70, 440, 290))

    def test_the_texts_ink_in_the_light_style(self):
        self.assertEqual(self.measure(screen((192, 192, 192)))["ink"], 3 * 8 * 145)

    def test_the_texts_ink_in_the_dark_style(self):
        m = self.measure(screen((36, 36, 36)))
        self.assertEqual((m["page"], m["ink"]), ((60, 70, 440, 290), 3 * 8 * 145))

    def test_a_page_without_its_text_has_no_ink(self):
        self.assertEqual(self.measure(screen((192, 192, 192), text=False))["ink"], 0)

    def test_no_page_on_the_screen(self):
        m = self.measure([[BLACK] * W for _ in range(H)])
        self.assertEqual((m["page"], m["ink"]), (None, 0))


if __name__ == "__main__":
    unittest.main()
