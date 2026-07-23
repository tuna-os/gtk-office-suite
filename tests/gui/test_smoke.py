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
        entry = self.app.child(name="Command Palette")
        self.assertIsNotNone(entry, "palette dialog not shown")
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


class TablesMultiSheetSmoke(BaseGUITestCase):
    """Real GTK journey: add a sheet, edit both independently, and confirm
    switching never leaks one sheet's data into the other (issue #98)."""

    app_name = "tables"

    def test_add_sheet_keeps_data_isolated_across_switches(self):
        from dogtail import rawinput
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables-rust", "new-document"])
        time.sleep(1.5)
        rawinput.typeText("=1+1")
        rawinput.keyCombo("Return")
        time.sleep(0.5)

        self.app.child(name="Add sheet", roleName="push button").do_action(0)
        time.sleep(0.8)
        # GtkDropDown's accessible name mirrors the selected item's label
        # ("Sheet2" once added), not a fixed string, so match by role.
        switcher = self.app.child(roleName="combo box")

        rawinput.typeText("=3+3")
        rawinput.keyCombo("Return")
        time.sleep(0.5)
        grid = self.app.child(name="Spreadsheet grid")
        self.assertIn("6", grid.description, f"Sheet2 grid: {grid.description!r}")

        # Switch back to Sheet1 via the dropdown and confirm its own value.
        switcher.child(name="Sheet2", roleName="toggle button").do_action(0)
        time.sleep(0.3)
        rawinput.keyCombo("Up")
        rawinput.keyCombo("Return")
        time.sleep(0.5)
        grid = self.app.child(name="Spreadsheet grid")
        self.assertIn("2", grid.description, f"Sheet1 grid: {grid.description!r}")
        self.assertNotIn("6", grid.description, "Sheet2's value leaked into Sheet1")


class TablesUndoSaveReopenSmoke(BaseGUITestCase):
    """Real GTK journey: edit, undo, redo, save, restart, and reopen."""

    app_name = "tables"

    def setUp(self):
        import tempfile
        import zipfile

        self._dir = tempfile.mkdtemp(prefix="tables-rt-")
        self._doc = os.path.join(self._dir, "journey.xlsx")
        parts = {
            "[Content_Types].xml": """<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
 <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
 <Default Extension="xml" ContentType="application/xml"/>
 <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
 <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>""",
            "_rels/.rels": """<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>""",
            "xl/workbook.xml": """<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
 <sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>
</workbook>""",
            "xl/_rels/workbook.xml.rels": """<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>""",
            "xl/worksheets/sheet1.xml": """<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>""",
        }
        with zipfile.ZipFile(self._doc, "w", zipfile.ZIP_DEFLATED) as book:
            for name, content in parts.items():
                book.writestr(name, content)
        self.launch_args = [self._doc]
        super().setUp()

    def tearDown(self):
        super().tearDown()
        import shutil
        shutil.rmtree(self._dir, ignore_errors=True)

    def test_edit_undo_redo_save_and_reopen(self):
        from dogtail import rawinput
        import subprocess
        import zipfile

        time.sleep(1.0)
        rawinput.keyCombo("<Control>g")
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        rawinput.typeText("=2+3")
        rawinput.keyCombo("Return")
        rawinput.keyCombo("<Control>z")
        rawinput.keyCombo("<Control><Shift>z")
        rawinput.keyCombo("<Control>s")
        time.sleep(1.2)
        with zipfile.ZipFile(self._doc) as book:
            sheet_xml = book.read("xl/worksheets/sheet1.xml").decode()
        self.assertIn("<f>2+3</f>", sheet_xml)

        self.process.terminate()
        self.process.wait(timeout=3)
        env = os.environ.copy()
        env["GDK_BACKEND"] = "x11"
        self.process = subprocess.Popen(
            [self.bin_path, self._doc], env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        self.app = self.wait_for_app(self.app_name)
        time.sleep(1.2)
        rawinput.keyCombo("<Control>g")
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        rawinput.keyCombo("Escape")
        rawinput.keyCombo("Right")
        rawinput.keyCombo("Left")
        time.sleep(0.5)
        grid = self.app.child(name="Spreadsheet grid")
        self.assertIn("5", grid.description, f"reopened grid: {grid.description!r}")


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


class TablesClipboardSmoke(BaseGUITestCase):
    """Suite-clipboard glue: Ctrl+C publishes the fragment MIME and
    Ctrl+V pastes it back with formulas still live. (The cross-app
    conversion matrix is unit-tested in the core crates; this exercises
    the GDK provider/reader path end to end.)"""

    app_name = "tables"

    def test_copy_paste_round_trip_keeps_formula_live(self):
        from dogtail import rawinput
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables-rust", "new-document"])
        time.sleep(1.5)

        def put(ref, value):
            rawinput.keyCombo("<Control>g")
            time.sleep(0.2)
            rawinput.typeText(ref)
            rawinput.keyCombo("Return")
            time.sleep(0.3)
            rawinput.typeText(value)
            rawinput.keyCombo("Return")
            time.sleep(0.3)

        put("A1", "2")
        put("B1", "3")
        put("C1", "=A1+B1")
        # Select C1 (grid has focus after the commit) and copy.
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("C1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.keyCombo("Escape")
        time.sleep(0.3)
        rawinput.keyCombo("<Control>c")
        time.sleep(0.5)
        # Jump to E1 and paste; the formula re-evaluates there.
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("E1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.keyCombo("Escape")
        time.sleep(0.3)
        rawinput.keyCombo("<Control>v")
        time.sleep(1.0)
        # Navigate away and back so the a11y description reflects E1.
        rawinput.keyCombo("Right")
        rawinput.keyCombo("Left")
        time.sleep(0.5)
        grid = self.app.child(name="Spreadsheet grid")
        self.assertIn("E1", grid.description, f"desc: {grid.description!r}")
        self.assertIn("5", grid.description,
                      f"pasted formula did not evaluate: {grid.description!r}")
        self.assertIsNone(self.process.poll(), "tables crashed during clipboard round trip")


class LettersClipboardSmoke(BaseGUITestCase):
    """Ctrl+C on a selection offers the suite fragment; Ctrl+V pastes it
    back through the fragment path (not the TextView default)."""

    app_name = "letters"

    def test_copy_paste_round_trip(self):
        from dogtail import rawinput

        self.app.child(name="New Document", roleName="push button").do_action(0)
        time.sleep(1.5)
        rawinput.typeText("alpha beta")
        time.sleep(0.5)
        rawinput.keyCombo("<Control>a")
        time.sleep(0.3)
        rawinput.keyCombo("<Control>c")
        time.sleep(0.5)
        rawinput.keyCombo("<Control>End")
        time.sleep(0.3)
        rawinput.keyCombo("<Control>v")
        time.sleep(1.0)
        editor = self.app.child(roleName="text")
        self.assertEqual(editor.text, "alpha betaalpha beta",
                         f"editor text: {editor.text!r}")
        self.assertIsNone(self.process.poll(), "letters crashed during clipboard round trip")


class TablesA11yCellsSmoke(BaseGUITestCase):
    """Virtual a11y children (issue #87): the grid exposes each used
    cell as a real AT-SPI node with role, name, and selection state —
    not just one opaque drawing."""

    app_name = "tables"

    def test_cells_are_accessible_nodes(self):
        from dogtail import rawinput
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables-rust", "new-document"])
        time.sleep(1.5)
        for ref, value in [("A1", "10"), ("B2", "20")]:
            rawinput.keyCombo("<Control>g")
            time.sleep(0.2)
            rawinput.typeText(ref)
            rawinput.keyCombo("Return")
            time.sleep(0.3)
            rawinput.typeText(value)
            rawinput.keyCombo("Return")
            time.sleep(0.3)
        cell = self.app.child(name="A1: 10", roleName="table cell")
        self.assertIsNotNone(cell)
        cell2 = self.app.child(name="B2: 20", roleName="table cell")
        self.assertIsNotNone(cell2)
        # The active cell (B2 after its commit) carries the selected state.
        import pyatspi
        self.assertTrue(cell2.getState().contains(pyatspi.STATE_SELECTED),
                        "active cell not marked selected")
        self.assertIsNone(self.process.poll(), "tables crashed exposing cells")


class DecksA11yObjectsSmoke(BaseGUITestCase):
    """Virtual a11y children: slide objects are AT-SPI nodes."""

    app_name = "decks"

    def test_objects_are_accessible_nodes(self):
        import subprocess

        aid = "org.tunaos.decks-rust"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)
        subprocess.run(["gapplication", "action", aid, "add-text-box"])
        subprocess.run(["gapplication", "action", aid, "add-shape"])
        time.sleep(1.5)
        box = self.app.child(name="Text box: Text", roleName="list item")
        self.assertIsNotNone(box)
        rect = self.app.child(name="Rectangle", roleName="list item")
        self.assertIsNotNone(rect)
        self.assertIsNone(self.process.poll(), "decks crashed exposing objects")


class TablesFormatCellsSmoke(BaseGUITestCase):
    """Number formats are visible: cycling a format changes the value a
    screen reader hears, and the Format Cells sheet opens via action."""

    app_name = "tables"

    def test_format_visible_and_dialog_opens(self):
        from dogtail import rawinput
        import subprocess

        aid = "org.tunaos.tables-rust"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.typeText("1234.5")
        rawinput.keyCombo("Return")
        time.sleep(0.5)
        # Cycle to Number(2): the a11y cell should read the formatted value.
        subprocess.run(["gapplication", "action", aid, "cycle-number-format"])
        time.sleep(0.3)
        # Nudge the selection so the refresh runs.
        rawinput.keyCombo("Right")
        rawinput.keyCombo("Left")
        time.sleep(0.8)
        cells = [c.name for c in self.app.findChildren(
            lambda c: c.roleName == "table cell")]
        formatted = [n for n in cells if "1,234.50" in n or "1234.50" in n]
        self.assertTrue(formatted, f"no formatted cell value: {cells}")
        # The Format Cells sheet opens from the action registry.
        subprocess.run(["gapplication", "action", aid, "format-cells"])
        time.sleep(1.0)
        apply_btn = self.app.child(name="Apply", roleName="push button")
        self.assertIsNotNone(apply_btn)
        apply_btn.do_action(0)
        time.sleep(0.5)
        self.assertIsNone(self.process.poll(), "tables crashed in format cells")


class DecksSelectionSmoke(BaseGUITestCase):
    """Object selection updates the canvas a11y description and the
    inspector (fit-to-viewport geometry keeps coordinates stable)."""

    app_name = "decks"

    def test_click_selects_object(self):
        from dogtail import rawinput
        import subprocess

        aid = "org.tunaos.decks-rust"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)
        subprocess.run(["gapplication", "action", aid, "add-shape"])
        time.sleep(1.0)
        # Default Rect is at slide (200,200,200x150); under the default
        # 960x680 window this lands here on the fitted canvas.
        rawinput.click(417, 372)
        time.sleep(1.0)
        canvas = self.app.child(name="Slide canvas")
        self.assertIn("selected", canvas.description,
                      f"canvas description: {canvas.description!r}")
        self.assertIsNone(self.process.poll(), "decks crashed during selection")


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
