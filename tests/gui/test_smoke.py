#!/usr/bin/env python3
"""Deterministic smoke tests — AT-SPI assertions only, no VLM.

These gate CI. They answer one question per app: does the binary launch,
show a window, and respond to basic input? Failures here mean the build
is broken for real users.

Unlike the vision tests (test_letters.py etc.), these need no API keys
and no screenshot judging, so they are fast and cannot flake on model
output.
"""

import os
import time

from framework import BaseGUITestCase


class LettersSmoke(BaseGUITestCase):
    app_name = "letters"

    def test_launch_shows_window(self):
        self.assertIsNotNone(self.app.child(roleName="frame"))
        self.assertIsNone(self.process.poll(), "letters exited after launch")

    def test_new_document_and_type_updates_word_count(self):
        # The editor TextView is not currently exposed via AT-SPI (PageContainer
        # allocates its child inside snapshot(), which breaks the a11y tree —
        # tracked as a separate issue). Until that is fixed, type via raw input
        # and assert on the word-count label, which is exposed.
        from dogtail import rawinput

        self.app.child(name="New Document", roleName="push button").do_action(0)
        time.sleep(1.5)
        rawinput.typeText("the quick brown fox")
        time.sleep(1.0)
        label = self.app.child(name="4 words", roleName="label")
        self.assertIsNotNone(label)
        self.assertIsNone(self.process.poll(), "letters crashed while typing")


class LettersFormattingSmoke(BaseGUITestCase):
    app_name = "letters"

    def test_bold_toolbar_interaction(self):
        """Click the real Bold toggle, type through it, assert state + text.

        Formatting *fidelity* is asserted at the model layer (bridge tests,
        DOCX corpus); GTK 4.14 doesn't expose font-weight text attributes
        over AT-SPI, so at GUI level we assert the interaction chain:
        button toggles, typing lands, word count follows.
        """
        from dogtail import rawinput

        self.app.child(name="New Document", roleName="push button").do_action(0)
        time.sleep(1.5)
        rawinput.typeText("plain ")
        # Toolbar buttons are action-bound push buttons named by tooltip.
        bold = self.app.child(name="Bold (Ctrl+B)", roleName="push button")
        bold.do_action(0)
        time.sleep(0.3)
        # Formatting fidelity is proven by the model tests; here we assert
        # the interaction chain (action fires, typing lands) stays alive.
        rawinput.typeText("bolded")
        time.sleep(0.8)
        editor = self.app.child(roleName="text")
        self.assertEqual(editor.text, "plain bolded")
        self.assertIsNotNone(self.app.child(name="2 words", roleName="label"))
        self.assertIsNone(self.process.poll(), "letters crashed during formatting")


class LettersFileRoundTripSmoke(BaseGUITestCase):
    """The full user journey: open a file from the CLI, edit through real
    input, Ctrl+S, and assert the bytes on disk. This is the GUI-level
    fidelity test the model corpora back up."""

    app_name = "letters"

    def setUp(self):
        import tempfile
        self._dir = tempfile.mkdtemp(prefix="letters-rt-")
        self._doc = os.path.join(self._dir, "journey.md")
        with open(self._doc, "w") as f:
            f.write("hello world")
        self.launch_args = [self._doc]
        super().setUp()

    def tearDown(self):
        super().tearDown()
        import shutil
        shutil.rmtree(self._dir, ignore_errors=True)

    def test_open_edit_save_round_trip(self):
        from dogtail import rawinput

        editor = self.app.child(roleName="text")
        self.assertEqual(editor.text, "hello world", "file did not open into editor")
        # editor self-focuses on map (see window.rs); jump to end and type
        rawinput.keyCombo("<Control>End")
        rawinput.typeText(" edited")
        time.sleep(0.5)
        rawinput.keyCombo("<Control>s")
        time.sleep(1.5)
        with open(self._doc) as f:
            saved = f.read()
        self.assertIn("hello world edited", saved, f"saved file: {saved!r}")


class TablesSmoke(BaseGUITestCase):
    app_name = "tables"

    def test_launch_shows_window(self):
        self.assertIsNotNone(self.app.child(roleName="frame"))
        self.assertIsNone(self.process.poll(), "tables exited after launch")

    def test_formula_entry_updates_grid_a11y(self):
        """Type a formula in the fx entry; the grid's accessible
        description must show the evaluated value for the active cell.
        This is the first semantically-assertable grid interaction
        (issue #87 groundwork)."""
        from dogtail import rawinput
        import subprocess

        # Tables starts on an empty-state page; the grid maps on new-document.
        subprocess.run(["gapplication", "action", "org.tunaos.tables-rust", "new-document"])
        time.sleep(1.5)
        # new-document focuses the formula entry; type straight into it.
        rawinput.typeText("=2+3")
        rawinput.keyCombo("Return")
        time.sleep(0.8)
        grid = self.app.child(name="Spreadsheet grid")
        self.assertIn("5", grid.description,
                      f"grid description: {grid.description!r}")
        self.assertIn("A1", grid.description)


class DecksSmoke(BaseGUITestCase):
    app_name = "decks"

    def test_launch_shows_window(self):
        self.assertIsNotNone(self.app.child(roleName="frame"))
        self.assertIsNone(self.process.poll(), "decks exited after launch")

    def test_canvas_reports_slide_state(self):
        # Decks starts on an empty-state page; the canvas maps (and enters
        # the AT-SPI tree) once a deck exists.
        import subprocess
        subprocess.run(["gapplication", "action", "org.tunaos.decks-rust", "new-document"])
        time.sleep(1.5)
        canvas = self.app.child(name="Slide canvas")
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if "slide 1 of" in canvas.description:
                break
            time.sleep(0.3)
        self.assertIn("slide 1 of", canvas.description,
                      f"canvas description: {canvas.description!r}")
