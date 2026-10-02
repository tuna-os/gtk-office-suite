"""Visual matrix journeys (#1284): each app in four states, screenshotted
and compared with a committed expected image.

One run of this module covers one display config, the one the harness was
started with (screen size, colour scheme, high contrast, scale); the
runner, `visual_matrix.py`, starts it once per config. Each journey
captures the screen in a state, keeps the screenshot, the app's state
snapshot and (on a mismatch) a diff next to the expected image, and fails
when more of the image differs than the cell's threshold allows
(`visual_compare.py`). With GUI_VISUAL_UPDATE=1 it writes the screenshot
as the new expected image instead: expectations change on purpose only.

The states:
  editor    — a fixed document, just opened
  selection — part of it selected
  dialog    — the Keyboard Shortcuts window over it
  error     — the "Could not open" message a corrupt file gets
"""

import json
import os
import shutil
import tempfile

from framework import BaseGUITestCase
import visual_compare

GUI_DIR = os.path.dirname(os.path.abspath(__file__))
FONTS_CONF = os.path.join(GUI_DIR, "visual", "fonts.conf")


def _odp():
    """A one-slide odp with a title text box."""
    return [
        ("mimetype", "application/vnd.oasis.opendocument.presentation"),
        ("content.xml",
         '<?xml version="1.0" encoding="UTF-8"?>'
         '<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
         'xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" '
         'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
         'xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" office:version="1.3">'
         '<office:body><office:presentation><draw:page draw:name="One">'
         '<draw:frame svg:x="2cm" svg:y="2cm" svg:width="20cm" svg:height="3cm"><draw:text-box>'
         '<text:p>Quarterly review</text:p></draw:text-box></draw:frame>'
         '<draw:frame svg:x="2cm" svg:y="6cm" svg:width="20cm" svg:height="6cm"><draw:text-box>'
         '<text:p>Revenue grew in every region.</text:p></draw:text-box></draw:frame>'
         '</draw:page></office:presentation></office:body>'
         '</office:document-content>'),
        ("META-INF/manifest.xml",
         '<?xml version="1.0" encoding="UTF-8"?>'
         '<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">'
         '<manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.presentation"/>'
         '<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>'
         '</manifest:manifest>'),
    ]


def _write_package(path, parts):
    import zipfile
    with zipfile.ZipFile(path, "w") as z:
        for part, data in parts:
            z.writestr(part, data, compress_type=zipfile.ZIP_STORED if part == "mimetype" else zipfile.ZIP_DEFLATED)


class VisualMatrixMixin:
    """The four states for one app. A subclass names the app, writes its
    document and says how to select part of it."""

    AID = None
    DOC = None  # file name of the fixed document
    BROKEN = None  # file name the corrupt file gets

    def setUp(self):
        # A fixed directory, not a random temp one: Letters' error message
        # quotes the file's full path, and the image has to match run to run.
        self._dir = os.path.join(tempfile.gettempdir(), "gtk-office-visual", self.app_name)
        shutil.rmtree(self._dir, ignore_errors=True)
        os.makedirs(self._dir)
        self.addCleanup(shutil.rmtree, self._dir, True)
        state = self._testMethodName.removeprefix("test_")
        if state == "error":
            path = os.path.join(self._dir, self.BROKEN)
            with open(path, "wb") as f:
                f.write(b"this is not a document\n")
        else:
            path = os.path.join(self._dir, self.DOC)
            self.write_document(path)
        self.launch_args = [path]
        self.isolate_autosave_state()
        self.isolate_snapshot(prefix=f"visual-{self.app_name}-snap-")
        # Cairo, not GL: the software GL the runners use and a developer's
        # GPU draw the same frame a few pixels apart.
        self.launch_env = {**getattr(self, "launch_env", {}), "FONTCONFIG_FILE": FONTS_CONF, "GSK_RENDERER": "cairo"}
        super().setUp()

    def configure_deterministic_environment(self):
        # No blinking caret: a screenshot taken in its off phase would
        # differ from one taken in its on phase.
        super().configure_deterministic_environment()
        gtk = os.path.join(self._xdg["config"], "gtk-4.0")
        os.makedirs(gtk, exist_ok=True)
        with open(os.path.join(gtk, "settings.ini"), "w") as f:
            f.write("[Settings]\ngtk-cursor-blink=false\ngtk-enable-animations=false\n")

    def _capture(self, state):
        """Compare the screen with the cell's expected image; keep the
        evidence either way."""
        from PIL import Image
        import mss
        config = visual_compare.config_name()
        out = visual_compare.out_dir(config)
        os.makedirs(out, exist_ok=True)
        cell = f"{self.app_name}-{state}"
        actual = os.path.join(out, f"{cell}.actual.png")
        with mss.mss() as sct:
            shot = sct.grab(sct.monitors[0])
            Image.frombytes("RGB", shot.size, shot.rgb).save(actual)
        try:
            snap = self.trigger_snapshot(self.AID)
        except Exception as e:  # the error state may have no document
            snap = {"unavailable": str(e)}
        with open(os.path.join(out, f"{cell}.snapshot.json"), "w") as f:
            json.dump(snap, f, indent=1, sort_keys=True)
        result = visual_compare.check(config, cell, actual)
        with open(os.path.join(out, f"{cell}.result.json"), "w") as f:
            json.dump(result, f, indent=1, sort_keys=True)
        self.assertTrue(result["ok"], f"{config}/{cell}: {result['message']}")

    def _settle(self):
        """Wait for two identical screenshots in a row: layout, fonts and
        the first frame have landed."""
        import mss
        last = [None]

        def frame():
            with mss.mss() as sct:
                return bytes(sct.grab(sct.monitors[0]).rgb)

        def steady():
            now = frame()
            same = now == last[0]
            last[0] = now
            return same
        self.wait_until(steady, bool, interval=0.5, timeout=20.0, description="the screen to stop changing")

    def _opened(self):
        raise NotImplementedError

    def test_editor(self):
        self.wait_until(self._opened, bool, description=f"{self.DOC} to open")
        self._settle()
        self._capture("editor")

    def test_selection(self):
        self.wait_until(self._opened, bool, description=f"{self.DOC} to open")
        self._activate_window()
        self.select_part()
        self._settle()
        self._capture("selection")

    def test_dialog(self):
        import subprocess
        self.wait_until(self._opened, bool, description=f"{self.DOC} to open")
        windows = len(self.app.children)
        subprocess.run(["gapplication", "action", self.AID, "shortcuts"], check=True)
        self.wait_until(lambda: len(self.app.children), lambda n: n > windows, description="the shortcuts window")
        self._settle()
        self._capture("dialog")

    def test_error(self):
        self.wait_until(self._open_error, bool, description="the could-not-open message")
        self._settle()
        self._capture("error")

    def _open_error(self):
        return self.app.findChild(lambda n: (n.name or "").startswith("Could not open"), retry=False, requireResult=False)


class LettersVisualMatrix(VisualMatrixMixin, BaseGUITestCase):
    app_name = "letters"
    AID = "org.tunaos.letters"
    DOC = "notes.md"
    BROKEN = "broken.docx"

    def write_document(self, path):
        with open(path, "w") as f:
            f.write("# Meeting notes\n\nThe team agreed on three priorities for the quarter.\n\n"
                    "- Ship the new editor\n- Fix the reported crashes\n- Write the release notes\n\n"
                    "Next review: the first Monday of the month.\n")

    def _opened(self):
        paras = self.trigger_snapshot(self.AID).get("paragraphs", [])
        return len(paras) >= 3

    def select_part(self):
        from dogtail import rawinput
        rawinput.keyCombo("<Control>a")
        self.wait_until(lambda: self.app.findChild(
            lambda n: n.name == "Print Layout" and n.roleName == "text", retry=False, requireResult=False),
            lambda n: n is not None and n.queryText().getNSelections() > 0, description="Ctrl+A to select the text")


class TablesVisualMatrix(VisualMatrixMixin, BaseGUITestCase):
    app_name = "tables"
    AID = "org.tunaos.tables"
    DOC = "budget.csv"
    BROKEN = "broken.xlsx"

    def write_document(self, path):
        with open(path, "w") as f:
            f.write("Item,Q1,Q2,Q3\nRent,1200,1200,1250\nPower,310,280,295\nTravel,90,450,120\n")

    def _opened(self):
        return len(self.trigger_snapshot(self.AID)["sheet"]["cells"]) >= 16

    def select_part(self):
        from dogtail import rawinput
        rawinput.keyCombo("<Control>g")
        rawinput.typeText("B2")
        rawinput.keyCombo("Return")
        self.wait_until(lambda: self.trigger_snapshot(self.AID)["sheet"]["selection"], lambda s: s == [1, 1, 1, 1],
                        description="the jump to B2")
        rawinput.keyCombo("Escape")
        for key in ("<Shift>Right", "<Shift>Right", "<Shift>Down", "<Shift>Down"):
            rawinput.keyCombo(key)
        self.wait_until(lambda: self.trigger_snapshot(self.AID)["sheet"]["selection"], lambda s: s == [1, 1, 3, 3],
                        description="B2:D4 selected")


class DecksVisualMatrix(VisualMatrixMixin, BaseGUITestCase):
    app_name = "decks"
    AID = "org.tunaos.decks"
    DOC = "review.odp"
    BROKEN = "broken.pptx"

    def write_document(self, path):
        _write_package(path, _odp())

    def _opened(self):
        snap = self.trigger_snapshot(self.AID)
        return snap["slides"] and len(snap["slides"][0]["objects"]) == 2

    def select_part(self):
        import subprocess
        subprocess.run(["gapplication", "action", self.AID, "add-shape"], check=True)
        self.wait_until(lambda: self.trigger_snapshot(self.AID)["selected_object"], lambda i: i == 2,
                        description="the added shape to be selected")
