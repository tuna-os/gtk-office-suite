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


class LettersPaletteSmoke(BaseGUITestCase):
    """Command palette coverage (DESIGN-UI.md): Ctrl+K opens a searchable
    list of every parameterless app action; any action missing a label
    renders as "unlabeled: app.x", so asserting that marker's absence
    forces every new action to register a human name."""

    app_name = "letters"

    def test_palette_lists_labeled_actions(self):
        from dogtail import rawinput

        # Open a document first so the lazily registered formatting
        # actions exist and must therefore be labeled.
        self.app.child(name="New Document", roleName="push button").do_action(0)
        time.sleep(1.5)
        rawinput.keyCombo("<Control>k")
        time.sleep(1.0)

        labels = {c.name for c in self.app.findChildren(
            lambda c: c.roleName == "label")}
        self.assertIn("Bold", labels, "palette missing formatting action")
        self.assertIn("Command Palette", labels, "palette dialog not shown")
        unlabeled = sorted(l for l in labels if l.startswith("unlabeled:"))
        self.assertEqual(unlabeled, [],
                         "actions without registry labels: %s" % unlabeled)
        self.assertIsNone(self.process.poll(), "letters crashed opening palette")


class LettersSelectionUXSmoke(BaseGUITestCase):
    """Selection popover + live cursor style readout (DESIGN-UI §1/§3)."""

    app_name = "letters"

    def test_selection_popover_and_style_readout(self):
        from dogtail import rawinput
        import subprocess

        self.app.child(name="New Document", roleName="push button").do_action(0)
        time.sleep(1.5)
        rawinput.typeText("style readout test")
        time.sleep(0.5)
        # Select all → the floating format popover should appear.
        rawinput.keyCombo("<Control>a")
        time.sleep(1.0)
        bold_btn = self.app.child(name="Bold", roleName="push button")
        self.assertIsNotNone(bold_btn, "selection popover did not appear")
        # Apply Heading 2 via its action; the status readout must follow.
        subprocess.run(["gapplication", "action",
                        "org.tunaos.letters-rust", "style-h2"])
        time.sleep(1.0)
        labels = {c.name for c in self.app.findChildren(
            lambda c: c.roleName == "label")}
        self.assertTrue(any("Heading 2" in l for l in labels),
                        f"no Heading 2 readout; labels: {sorted(labels)}")
        self.assertIsNone(self.process.poll(), "letters crashed during selection UX")


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


class TablesNameBoxSmoke(BaseGUITestCase):
    app_name = "tables"

    def test_name_box_jump_and_edit(self):
        """Type a ref in the name box → selection jumps; typed value lands
        in that cell (asserted via the grid's accessible description)."""
        from dogtail import rawinput
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables-rust", "new-document"])
        time.sleep(1.5)
        # Ctrl+G (Go to Cell) focuses the name box with text selected.
        rawinput.keyCombo("<Control>g")
        time.sleep(0.3)
        rawinput.typeText("C5")
        rawinput.keyCombo("Return")
        time.sleep(0.5)
        # The jump hands focus to the fx entry; type a value there.
        rawinput.typeText("42")
        rawinput.keyCombo("Return")
        time.sleep(0.8)
        grid = self.app.child(name="Spreadsheet grid")
        self.assertIn("C5", grid.description,
                      f"grid description: {grid.description!r}")
        self.assertIn("42", grid.description)
        self.assertIsNone(self.process.poll(), "tables crashed during name-box jump")

    def _put(self, ref, value):
        from dogtail import rawinput
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText(ref)
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.typeText(value)
        rawinput.keyCombo("Return")
        time.sleep(0.3)

    def test_keyboard_range_selection_updates_stats(self):
        """Shift+arrows extend the selection; the status area shows live
        sum/avg/count for the numeric cells (DESIGN-UI: status is live)."""
        from dogtail import rawinput
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables-rust", "new-document"])
        time.sleep(1.5)
        self._put("A1", "10")
        self._put("A2", "20")
        self._put("A3", "30")
        # Committing in fx hands focus back to the grid (Calc behavior),
        # so keyboard selection works straight away.
        rawinput.keyCombo("<Shift>Up")
        rawinput.keyCombo("<Shift>Up")
        time.sleep(0.8)
        # GtkLabel's AT-SPI name follows its text; find the stats readout
        # by content.
        labels = [c.name for c in self.app.findChildren(
            lambda c: c.roleName == "label")]
        stats = [l for l in labels if "Sum" in l]
        self.assertTrue(stats, f"no stats label found; labels: {labels}")
        self.assertIn("Sum 60", stats[0])
        self.assertIn("Count 3", stats[0])
        self.assertIn("A1:A3", stats[0])
        self.assertIsNone(self.process.poll(), "tables crashed during keyboard selection")


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
