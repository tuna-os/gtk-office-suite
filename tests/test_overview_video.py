"""The overview videos (tests/gui/overview_video.py) stay in step with the
feature tour they record: each app's video is made of the tour's stops, the
suite trailer names stops that exist, and every app has the icon and the
metainfo summary its title card shows. A renamed or removed stop fails here
rather than as a missing segment in a video nobody watches for weeks."""

import os
import re
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "tests", "gui"))

import feature_tour as ft  # noqa: E402
import overview_video as ov  # noqa: E402


class OverviewVideoTest(unittest.TestCase):
    def test_every_app_in_the_tour_has_a_video(self):
        self.assertEqual(set(ov.APPS), {app for app, *_ in ft.STOPS})

    def test_the_trailer_names_stops_the_tour_has(self):
        stops = {(app, name) for app, name, *_ in ft.STOPS}
        for app, names in ov.TRAILER.items():
            for name in names:
                self.assertIn((app, name), stops, f"the trailer's {name} is not a {app} stop")

    def test_every_title_card_has_its_icon_and_summary(self):
        for app in ov.APPS:
            self.assertTrue(os.path.exists(f"{REPO}/flatpak/icons/org.tunaos.{app}.svg"), app)
            self.assertTrue(ov.summary(app), f"{app} has no metainfo summary")

    def test_accents_are_gnome_palette_colours(self):
        for name, (light, dark) in ov.APPS.values():
            for colour in (light, dark):
                self.assertRegex(colour, r"^#[0-9a-f]{6}$", name)

    def test_the_workflow_records_them(self):
        workflow = open(f"{REPO}/.github/workflows/overview-videos.yml").read()
        self.assertIn("tests/gui/overview_video.sh", workflow)
        self.assertTrue(re.search(r"release:\s*\n\s*types: \[published\]", workflow))


if __name__ == "__main__":
    unittest.main()
