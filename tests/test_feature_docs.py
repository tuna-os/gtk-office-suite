"""The feature overviews (docs/features/) and the tour that screenshots
them (tests/gui/feature_tour.py) must stay in step.

Derived from the tour itself rather than from a list kept here: every stop
the tour declares has to have its image on disk and embedded in its app's
page under its own caption, and every image a page embeds has to come
from a stop. A feature dropped from the tour then fails here instead of
leaving a screenshot nothing regenerates, and a feature added to the tour
fails until it is documented.
"""

import ast
import os
import re
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TOUR = os.path.join(ROOT, "tests", "gui", "feature_tour.py")
DOCS = os.path.join(ROOT, "docs", "features")


def tour_stops():
    """(app, name, caption) for every @stop in the tour, read from its
    source: importing it needs a display and dogtail."""
    tree = ast.parse(open(TOUR, encoding="utf-8").read())
    stops = []
    for node in ast.walk(tree):
        if isinstance(node, ast.FunctionDef):
            for d in node.decorator_list:
                if isinstance(d, ast.Call) and getattr(d.func, "id", None) == "stop":
                    stops.append(tuple(a.value for a in d.args))
    return stops


def embedded(page):
    text = open(os.path.join(DOCS, page), encoding="utf-8").read()
    return text, re.findall(r"!\[[^\]]*\]\(img/([a-z0-9-]+)\.png\)", text)


class FeatureDocsMatchTheTour(unittest.TestCase):
    def setUp(self):
        self.stops = tour_stops()
        self.assertGreater(len(self.stops), 20, "the tour declares hardly any stops: is @stop still how they are declared?")

    def test_every_stop_is_documented_on_its_apps_page_with_its_caption(self):
        for app, name, caption in self.stops:
            with self.subTest(stop=name):
                self.assertTrue(name.startswith(f"{app}-"), f"{name} should be named after its app")
                text, images = embedded(f"{app}.md")
                self.assertIn(name, images, f"docs/features/{app}.md does not embed img/{name}.png")
                self.assertRegex(text, r"\*" + re.escape(caption) + r"\.?\*", f"docs/features/{app}.md lacks the caption of {name}")
                self.assertTrue(os.path.isfile(os.path.join(DOCS, "img", f"{name}.png")),
                                f"img/{name}.png has not been captured: run tests/gui/capture_walkthrough.sh")

    def test_every_embedded_image_comes_from_a_stop(self):
        names = {name for _, name, _ in self.stops}
        for page in ("letters.md", "tables.md", "decks.md"):
            for image in embedded(page)[1]:
                with self.subTest(page=page, image=image):
                    self.assertIn(image, names, f"{page} embeds img/{image}.png, which no tour stop captures")

    def test_no_captured_image_is_left_unreferenced(self):
        names = {name for _, name, _ in self.stops}
        for f in os.listdir(os.path.join(DOCS, "img")):
            with self.subTest(image=f):
                self.assertIn(f.removesuffix(".png"), names, f"img/{f} is not captured by any stop: delete it")


if __name__ == "__main__":
    unittest.main()
