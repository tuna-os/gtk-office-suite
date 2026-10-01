"""The screenshot refresh keeps real UI changes and drops rendering noise
(tests/gui/keep_changed_screenshots.py). Both directions are pinned: a
filter that let noise through would open a refresh every run, and one that
swallowed a real change would leave the docs showing an old UI."""

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "tests", "gui"))

from PIL import Image, ImageDraw  # noqa: E402

import keep_changed_screenshots as k  # noqa: E402


def window():
    img = Image.new("RGB", (960, 680), (250, 250, 250))
    d = ImageDraw.Draw(img)
    d.rectangle((0, 0, 960, 46), fill=(235, 235, 235))      # header bar
    d.rectangle((80, 110, 880, 680), fill=(255, 255, 255))  # page
    d.text((180, 220), "Q2 2026 Quarterly Report", fill=(0, 0, 0))
    return img


class NoiseFilter(unittest.TestCase):
    def test_an_identical_capture_is_noise(self):
        self.assertTrue(k.is_noise(window(), window()))

    def test_anti_aliasing_shifts_are_noise(self):
        new = window().point(lambda v: max(0, v - 8))  # every pixel a little darker
        self.assertTrue(k.is_noise(window(), new))

    def test_a_changed_timestamp_is_noise(self):
        old, new = window(), window()
        ImageDraw.Draw(old).text((14, 180), "09:31:40", fill=(0, 0, 0))
        ImageDraw.Draw(new).text((14, 180), "09:42:02", fill=(0, 0, 0))
        self.assertTrue(k.is_noise(old, new))

    def test_a_new_panel_is_a_change(self):
        new = window()
        ImageDraw.Draw(new).rectangle((660, 46, 960, 680), fill=(235, 235, 238))  # a sidebar's grey, about 15 off
        self.assertFalse(k.is_noise(window(), new))

    def test_a_moved_button_is_a_change(self):
        old, new = window(), window()
        ImageDraw.Draw(old).rectangle((10, 8, 110, 38), fill=(53, 132, 228))
        ImageDraw.Draw(new).rectangle((200, 8, 300, 38), fill=(53, 132, 228))
        self.assertFalse(k.is_noise(old, new))

    def test_a_different_size_is_a_change(self):
        self.assertFalse(k.is_noise(window(), window().resize((1000, 680))))


if __name__ == "__main__":
    unittest.main()
