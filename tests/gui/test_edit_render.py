"""Editing journeys whose saved file the render lab compares (#1201).

A render-parity fixture proves the editors draw what they read. These
journeys prove they write what they draw: each one edits a new document
through the real GUI (type, bold, a bullet / a cell format / a shape),
saves it with Save As, and reopens it in a fresh process. The journey
itself checks the saved parts and the reopened model; the render lab
(tools/render-lab/edit_journeys.py) then adds each saved file to its
fixture manifest as `<app>/edited-journey`, so LibreOffice opening the
saved file is compared against our app opening it, like any fixture.

`EDIT_RENDER_OUT` names the directory the saved files go to (the render
lab's fixtures directory); without it they go to a per-test temp dir.
"""

import os
import re
import subprocess
import time
import zipfile

from framework import BaseGUITestCase
from test_smoke import TablesCellEntryMixin

PARAGRAPH = "Edited in the app, saved, and reopened."


def out_path(test, app, ext):
    root = os.environ.get("EDIT_RENDER_OUT") or test.temp_dir(prefix=f"edit-render-{app}-")
    os.makedirs(os.path.join(root, app), exist_ok=True)
    path = os.path.join(root, app, f"edited-journey.{ext}")
    if os.path.exists(path):
        os.remove(path)
    return path


def save_as(test, aid, path):
    """Save As through the file chooser, then wait for the file on disk."""
    from dogtail import tree
    subprocess.run(["gapplication", "action", aid, "save-file-as"])
    entry = test.wait_until(
        lambda: tree.root.findChild(lambda n: n.name == "Name:" and n.roleName == "text" and n.showing,
                                    retry=False, requireResult=False),
        bool, description="the Save As name entry")
    entry.text = path
    time.sleep(0.3)
    test.wait_until(
        lambda: tree.root.findChild(lambda n: n.name == "Save" and n.roleName == "push button" and n.showing,
                                    retry=False, requireResult=False),
        bool, description="the Save button").do_action(0)

    def parts():
        try:
            with zipfile.ZipFile(path) as z:
                return {n: z.read(n).decode("utf-8", "replace") for n in z.namelist()}
        except (OSError, zipfile.BadZipFile):
            return None
    return test.wait_until(parts, bool, interval=0.25, description=f"{os.path.basename(path)} on disk")


class LettersEditRenderJourney(BaseGUITestCase):
    """Type a paragraph, bold a word, add a bulleted item, save as DOCX,
    reopen: the saved document and the reopened model both carry the bold
    run and the bullet."""

    app_name = "letters"

    def setUp(self):
        self.isolate_snapshot(prefix="letters-edit-render-")
        self.isolate_autosave_state()
        super().setUp()

    def _model(self):
        s = self.trigger_snapshot("org.tunaos.letters")
        return [("".join(r["text"] for r in p["runs"]),
                 [r["text"] for r in p["runs"] if r.get("style", {}).get("bold")],
                 p["style"].get("list")) for p in s["paragraphs"]]

    def test_type_bold_bullet_save_and_reopen(self):
        from dogtail import rawinput

        # Toolbar presses go over AT-SPI and keystrokes over X, which keep
        # no order between them: a Bold pressed right after typing can land
        # after the next keystrokes. Each step waits for the model first.
        def settled(want, description):
            self.wait_until(self._model, lambda got: [(t, b, l if l != "None" else None) for t, b, l in got] == want,
                            interval=0.3, description=description)

        bold = lambda: self.app.child(name="Bold (Ctrl+B)", roleName="push button").do_action(0)
        self.wait_for_node(name="New Document", roleName="push button").do_action(0)
        self.wait_for_node(roleName="text")
        rawinput.typeText(PARAGRAPH + " ")
        settled([(PARAGRAPH + " ", [], None)], "the paragraph typed")
        bold()
        time.sleep(0.5)
        rawinput.typeText("Bold words.")
        settled([(PARAGRAPH + " Bold words.", ["Bold words."], None)], "the bold words typed bold")
        bold()
        time.sleep(0.5)
        rawinput.keyCombo("Return")
        rawinput.typeText("A bulleted item")
        settled([(PARAGRAPH + " Bold words.", ["Bold words."], None), ("A bulleted item", [], None)],
                "the second paragraph typed plain")
        self.gapplication_action("org.tunaos.letters", "bullet-list")
        want = [(PARAGRAPH + " Bold words.", ["Bold words."], None), ("A bulleted item", [], "Bullet")]
        settled(want, "the second paragraph to become a bullet item")

        path = out_path(self, "letters", "docx")
        parts = save_as(self, "org.tunaos.letters", path)
        body = parts["word/document.xml"]
        self.assertRegex(body, re.compile(r"<w:r>\s*<w:rPr>\s*<w:b/>.*?<w:t>Bold words\.</w:t>", re.S),
                         "the bold run was not saved bold")
        self.assertIn("<w:numPr>", body, "the bullet was not saved as a list item")

        self.relaunch_app(launch_args=[path])
        self.wait_until(self._model, lambda got: [(t, b, l if l != "None" else None) for t, b, l in got] == want,
                        interval=0.3, description="the reopened document to match what was saved")
        self.assertIsNone(self.process.poll(), "letters crashed in the edit-render journey")


class TablesEditRenderJourney(TablesCellEntryMixin, BaseGUITestCase):
    """Enter a header and numbers, bold the header, save as XLSX, reopen:
    the saved sheet and the reopened model carry the values and the bold."""

    app_name = "tables"

    def setUp(self):
        self.isolate_snapshot(prefix="tables-edit-render-")
        self.isolate_autosave_state()
        super().setUp()

    def _cells(self):
        s = self.trigger_snapshot("org.tunaos.tables")
        return sorted((c["row"], c["col"], c["value"]) for c in s["sheet"]["cells"])

    def test_values_bold_header_save_and_reopen(self):
        from dogtail import rawinput

        aid = "org.tunaos.tables"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        self._wait_for_a_new_document()
        for ref, value in (("A1", "Item"), ("B1", "Amount"), ("A2", "Paper"), ("B2", "12"),
                           ("A3", "Ink"), ("B3", "30")):
            self._put(ref, value)
        want = [(0, 0, "Item"), (0, 1, "Amount"), (1, 0, "Paper"), (1, 1, "12"), (2, 0, "Ink"), (2, 1, "30")]
        self.wait_until(self._cells, lambda got: got == want, interval=0.3, description="the six cells")

        # Bold A1 and B1 through the Format inspector.
        self.app.child(name="Format", roleName="toggle button").do_action(0)
        self.wait_until(lambda: self.app.child(name="Bold", roleName="toggle button").showing, bool,
                        description="the inspector to open")
        for ref in ("A1", "B1"):
            rawinput.keyCombo("<Control>g")
            self.wait_until(lambda: self._focused("Cell reference"), bool, description="the name box")
            rawinput.typeText(ref)
            rawinput.keyCombo("Return")
            self.wait_until(lambda: self._focused("Formula input"), bool, description=f"the jump to {ref}")
            rawinput.keyCombo("Escape")
            self.wait_until(lambda: not self._focused("Formula input"), bool, description="Escape to the grid")
            self.app.child(name="Bold", roleName="toggle button").do_action(0)
            time.sleep(0.3)

        path = out_path(self, "tables", "xlsx")
        parts = save_as(self, aid, path)
        self.assertIn("<b/>", parts["xl/styles.xml"], "no bold font was saved")
        sheet = parts["xl/worksheets/sheet1.xml"]
        self.assertRegex(sheet, r'<c r="A1"[^>]* s="[1-9]', "A1 was saved without its bold style")
        self.assertRegex(sheet, r"<v>30</v>", "B3's value was not saved")

        self.relaunch_app(launch_args=[path])
        self.wait_until(self._cells, lambda got: got == want, interval=0.3,
                        description="the reopened workbook's cells")
        self.assertIsNone(self.process.poll(), "tables crashed in the edit-render journey")


class DecksEditRenderJourney(BaseGUITestCase):
    """Add a text box and a shape to a new deck, save as PPTX, reopen: the
    saved slide and the reopened model carry both objects."""

    app_name = "decks"

    def setUp(self):
        self.isolate_snapshot(prefix="decks-edit-render-")
        self.isolate_autosave_state()
        super().setUp()

    def _kinds(self):
        s = self.trigger_snapshot("org.tunaos.decks")
        return [sorted(o.get("kind", o.get("type", "?")) for o in slide.get("objects", [])) for slide in s["slides"]]

    def test_text_box_and_shape_save_and_reopen(self):
        aid = "org.tunaos.decks"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        self.wait_until(lambda: len(self._kinds()) >= 1, bool, description="a new deck")
        before = self._kinds()[0]
        self.gapplication_action(aid, "add-text-box")
        self.gapplication_action(aid, "add-shape")
        added = self.wait_until(lambda: self._kinds()[0], lambda got: len(got) == len(before) + 2,
                                interval=0.3, description="the text box and the shape on slide 1")

        path = out_path(self, "decks", "pptx")
        parts = save_as(self, aid, path)
        slide = parts["ppt/slides/slide1.xml"]
        self.assertIn(">Text<", slide, "the text box's text was not saved")
        self.assertGreaterEqual(len(re.findall(r"<p:sp>", slide)), 2, "the slide lost a shape")

        self.relaunch_app(launch_args=[path])
        self.wait_until(lambda: self._kinds()[0] if self._kinds() else None, lambda got: got == added,
                        interval=0.3, description="the reopened slide to hold the same objects")
        self.assertIsNone(self.process.poll(), "decks crashed in the edit-render journey")
