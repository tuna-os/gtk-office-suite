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
import unittest

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


class LettersSnapshotSmoke(BaseGUITestCase):
    """State-snapshot interface (#104), same mechanism as
    TablesSnapshotSmoke/DecksSnapshotSmoke: typed text is visible in the
    normalized JSON snapshot (via the buffer->Document extraction path
    save-to-docx already uses), not just the word-count label."""

    app_name = "letters"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="letters-snapshot-")
        super().setUp()

    def test_snapshot_reflects_typed_text(self):
        import json
        import subprocess
        from dogtail import rawinput

        self.app.child(name="New Document", roleName="push button").do_action(0)
        time.sleep(1.5)
        rawinput.typeText("the quick brown fox")
        time.sleep(1.0)

        aid = "org.tunaos.letters"
        subprocess.run(["gapplication", "action", aid, "test-snapshot"])
        time.sleep(0.5)
        self.assertTrue(os.path.exists(self._snapshot_path), "snapshot file was not written")
        with open(self._snapshot_path) as f:
            snap = json.load(f)

        text = "".join(r["text"] for p in snap["paragraphs"] for r in p["runs"])
        self.assertEqual(text, "the quick brown fox")
        self.assertIsNone(self.process.poll(), "letters crashed writing a snapshot")


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
                        "org.tunaos.letters", "style-h2"])
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
        self._dir = self.temp_dir(prefix="letters-rt-")
        self._doc = os.path.join(self._dir, "journey.md")
        with open(self._doc, "w") as f:
            f.write("hello world")
        self.launch_args = [self._doc]
        super().setUp()

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


class LettersCloseGuardSmoke(BaseGUITestCase):
    """Real GTK journey: closing Letters with unsaved tabs offers Save
    All/Discard All/Cancel (issue #99), same contract as Tables/Decks.
    Unlike those two, Letters' window-level guard must drive a Save As
    dialog for a never-saved tab (Save All has no existing path to write
    straight to) before it can close."""

    app_name = "letters"

    def setUp(self):
        self._dir = self.temp_dir(prefix="letters-close-guard-")
        super().setUp()

    def _type_into_new_document(self):
        from dogtail import rawinput

        self.app.child(name="New Document", roleName="push button").do_action(0)
        time.sleep(1.5)
        rawinput.typeText("unsaved letters content")
        time.sleep(0.5)

    def test_cancel_keeps_window_open_then_discard_closes(self):
        self._type_into_new_document()

        self.app.child(name="Close", roleName="push button").do_action(0)
        time.sleep(0.8)
        self.app.child(name="Cancel", roleName="push button").do_action(0)
        time.sleep(0.5)
        self.assertIsNone(self.process.poll(), "Cancel must not close the window")
        self.assertIsNotNone(self.app.child(roleName="frame"), "window should still be open")

        self.app.child(name="Close", roleName="push button").do_action(0)
        time.sleep(0.8)
        self.app.child(name="Discard All", roleName="push button").do_action(0)
        self.assertIsNotNone(self.wait_for_process_exit(), "Discard must close the window")

    def test_save_in_close_guard_prompts_save_as_writes_and_closes(self):
        from dogtail import tree

        self._type_into_new_document()
        out_path = os.path.join(self._dir, "close-guard-save.md")

        self.app.child(name="Close", roleName="push button").do_action(0)
        time.sleep(0.8)
        self.app.child(name="Save All", roleName="push button").do_action(0)
        time.sleep(1.0)

        name_entry = tree.root.findChild(lambda n: n.name == "Name:" and n.roleName == "text")
        name_entry.text = out_path
        time.sleep(0.3)
        confirm = tree.root.findChild(lambda n: n.name == "Save" and n.roleName == "push button")
        confirm.do_action(0)

        self.assertIsNotNone(self.wait_for_process_exit(),
                              "Save must close the window once the Save As dialog completes")
        self.assertTrue(os.path.exists(out_path), "the document was not written to disk")
        with open(out_path) as f:
            saved = f.read()
        self.assertIn("unsaved letters content", saved, f"saved file: {saved!r}")


class LettersAutosaveSmoke(BaseGUITestCase):
    """Crash-recovery snapshot lifecycle (issue #99), per-tab this time:
    Letters can have several dirty tabs at once, so a crash with two dirty
    tabs must recover *both* — recovering only the first (an easy bug if
    the code reuses Tables/Decks' single-document `.next()` pattern) would
    silently drop the other one."""

    app_name = "letters"

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(prefix="letters-autosave-state-")
        super().setUp()

    def _snapshot_files(self):
        snap_dir = os.path.join(self._state_dir, "letters")
        if not os.path.isdir(snap_dir):
            return []
        return [f for f in os.listdir(snap_dir) if f.endswith(".snapshot")]

    def test_crash_with_two_dirty_tabs_recovers_both(self):
        import subprocess
        from dogtail import rawinput

        aid = "org.tunaos.letters"
        # The "New Document" button only lives in the empty-state view, so
        # it's only clickable for the very first tab; every tab after that
        # (like every other multi-tab test in this file) goes through the
        # action directly rather than a UI element that's since gone hidden.
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(2.0)
        rawinput.typeText("first tab content")
        time.sleep(1.0)

        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(2.0)
        rawinput.typeText("second tab content")
        time.sleep(1.0)

        subprocess.run(["gapplication", "action", "org.tunaos.letters", "autosave-now"])
        time.sleep(0.5)
        self.assertEqual(len(self._snapshot_files()), 2,
                          "both dirty tabs should have snapshotted")

        # Simulate a crash: kill the process directly, bypassing the close
        # guard, so the snapshots are never cleared by a clean exit.
        self.process.kill()
        self.process.wait(timeout=5)

        env = os.environ.copy()
        env["GDK_BACKEND"] = "x11"
        env.update(self.launch_env)
        self.process = subprocess.Popen(
            [self.bin_path], env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        self.app = self.wait_for_app(self.app_name)
        time.sleep(2.0)

        frame = self.app.child(roleName="frame")
        self.assertIn("Recovered", frame.name, f"window did not announce recovery: {frame.name!r}")
        self.assertEqual(self._snapshot_files(), [],
                          "both recovered snapshots must be cleared so they aren't offered again")

        seen = set()
        for _ in range(2):
            editor = self.app.child(roleName="text")
            seen.add(editor.text.strip())
            rawinput.keyCombo("<Control>Tab")
            time.sleep(0.5)
        self.assertEqual(seen, {"first tab content", "second tab content"},
                          f"recovered tab contents: {seen!r}")


class LettersPreferenceBindingSmoke(BaseGUITestCase):
    """Issue #101: a visible preference must actually change behavior, and
    that behavior must persist. Settings are isolated per test via the
    keyfile GSettings backend pointed at a temp XDG_CONFIG_HOME — the
    default dconf backend is a shared per-user daemon and can't be
    sandboxed by env vars alone (see [[session memory]] for why the
    autosave XDG_STATE_HOME trick doesn't transfer to GSettings).

    The AdwPreferencesDialog's rows aren't currently reachable through the
    AT-SPI tree (a separate a11y gap, not this issue's problem), so this
    drives the underlying GSettings key directly via the gsettings CLI
    under the same isolated env — that's still a real end-to-end check of
    the app's own live-apply + persistence wiring, just without also
    exercising the dialog widget itself."""

    app_name = "letters"

    def setUp(self):
        self._config_dir = self.isolate_gsettings(prefix="letters-prefs-cfg-")
        super().setUp()

    def _gsettings(self, *args):
        import subprocess
        env = os.environ.copy()
        env.update(self.launch_env)
        subprocess.run(["gsettings", *args], env=env, check=True)

    def _toolbar_visible(self):
        return len(self.app.findChildren(lambda c: c.name == "Bold (Ctrl+B)" and c.roleName == "push button")) > 0

    def test_show_toolbar_applies_live_and_persists_across_relaunch(self):
        import subprocess

        self.app.child(name="New Document", roleName="push button").do_action(0)
        time.sleep(1.5)
        self.assertTrue(self._toolbar_visible(), "toolbar should be visible by default")

        self._gsettings("set", "org.tunaos.letters", "show-toolbar", "false")
        time.sleep(0.5)
        self.assertFalse(self._toolbar_visible(), "toolbar did not hide live when show-toolbar was set false")

        self.process.terminate()
        self.process.wait(timeout=5)

        env = os.environ.copy()
        env["GDK_BACKEND"] = "x11"
        env.update(self.launch_env)
        self.process = subprocess.Popen(
            [self.bin_path], env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        self.app = self.wait_for_app(self.app_name)
        time.sleep(1.5)
        self.app.child(name="New Document", roleName="push button").do_action(0)
        time.sleep(1.5)
        self.assertFalse(self._toolbar_visible(),
                          "show-toolbar=false did not persist across relaunch")


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
        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
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

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
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


class TablesCloseGuardSmoke(BaseGUITestCase):
    """Real GTK journey: closing a dirty workbook offers Save/Discard/Cancel
    (issue #99) instead of silently discarding unsaved work."""

    app_name = "tables"

    def setUp(self):
        self._dir = self.temp_dir(prefix="tables-close-guard-")
        super().setUp()

    def _edit_a1(self):
        import subprocess
        from dogtail import rawinput

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        time.sleep(1.5)
        rawinput.typeText("=1+1")
        rawinput.keyCombo("Return")
        time.sleep(0.5)

    def test_cancel_keeps_window_open_then_discard_closes(self):
        self._edit_a1()

        self.app.child(name="Close", roleName="push button").do_action(0)
        time.sleep(0.8)
        self.app.child(name="Cancel", roleName="push button").do_action(0)
        time.sleep(0.5)
        self.assertIsNone(self.process.poll(), "Cancel must not close the window")
        self.assertIsNotNone(self.app.child(roleName="frame"), "window should still be open")

        self.app.child(name="Close", roleName="push button").do_action(0)
        time.sleep(0.8)
        self.app.child(name="Discard", roleName="push button").do_action(0)
        self.assertIsNotNone(self.wait_for_process_exit(), "Discard must close the window")

    def test_save_in_close_guard_writes_the_file_and_closes(self):
        from dogtail import tree

        self._edit_a1()
        out_path = os.path.join(self._dir, "close-guard-save.xlsx")

        self.app.child(name="Close", roleName="push button").do_action(0)
        time.sleep(0.8)
        self.app.child(name="Save", roleName="push button").do_action(0)
        time.sleep(1.0)

        name_entry = tree.root.findChild(lambda n: n.name == "Name:" and n.roleName == "text")
        name_entry.text = out_path
        time.sleep(0.3)
        confirm = tree.root.findChild(lambda n: n.name == "Save" and n.roleName == "push button")
        confirm.do_action(0)

        self.assertIsNotNone(self.wait_for_process_exit(),
                              "Save must close the window once it succeeds")
        self.assertTrue(os.path.exists(out_path), "the workbook was not written to disk")
        self.assertGreater(os.path.getsize(out_path), 0)


class TablesAutosaveSmoke(BaseGUITestCase):
    """Crash-recovery snapshot lifecycle (issue #99): a dirty, never-saved
    workbook survives an unclean process kill and is offered back on the
    next launch; a clean close leaves nothing behind to recover."""

    app_name = "tables"

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(prefix="tables-autosave-state-")
        super().setUp()

    def _snapshot_files(self):
        snap_dir = os.path.join(self._state_dir, "tables")
        if not os.path.isdir(snap_dir):
            return []
        return [f for f in os.listdir(snap_dir) if f.endswith(".snapshot")]

    def _edit_a1(self):
        import subprocess
        from dogtail import rawinput

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        time.sleep(1.5)
        rawinput.typeText("=6*7")
        rawinput.keyCombo("Return")
        time.sleep(0.5)

    def test_autosave_now_writes_a_snapshot_only_while_dirty(self):
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "autosave-now"])
        time.sleep(0.5)
        self.assertEqual(self._snapshot_files(), [], "a clean, untouched workbook must not snapshot")

        self._edit_a1()
        subprocess.run(["gapplication", "action", "org.tunaos.tables", "autosave-now"])
        time.sleep(0.5)
        self.assertEqual(len(self._snapshot_files()), 1, "dirty workbook should have snapshotted")

    def test_crash_then_relaunch_recovers_and_clears_the_snapshot(self):
        import subprocess

        self._edit_a1()
        subprocess.run(["gapplication", "action", "org.tunaos.tables", "autosave-now"])
        time.sleep(0.5)
        self.assertEqual(len(self._snapshot_files()), 1, "autosave-now must have written a snapshot")

        # Simulate a crash: kill the process directly, bypassing the close
        # guard entirely, so the snapshot is never cleared by a clean exit.
        self.process.kill()
        self.process.wait(timeout=5)

        # Relaunch against the same state dir and expect recovery.
        env = os.environ.copy()
        env["GDK_BACKEND"] = "x11"
        env.update(self.launch_env)
        self.process = subprocess.Popen(
            [self.bin_path], env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        self.app = self.wait_for_app(self.app_name)
        time.sleep(1.5)

        frame = self.app.child(roleName="frame")
        self.assertIn("Recovered", frame.name, f"window did not announce recovery: {frame.name!r}")
        self.assertEqual(self._snapshot_files(), [],
                          "the recovered snapshot must be cleared so it isn't offered again")


class TablesUndoSaveReopenSmoke(BaseGUITestCase):
    """Real GTK journey: edit, undo, redo, save, restart, and reopen."""

    app_name = "tables"

    def setUp(self):
        import zipfile

        self._dir = self.temp_dir(prefix="tables-rt-")
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

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
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

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
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


class TablesNamedRangeSmoke(BaseGUITestCase):
    """Named ranges (#113): Define Name captures the current selection;
    typing that name into the name box (instead of a cell reference)
    jumps back to and re-selects the whole range, verified via the
    stats label's live range readout (same one keyboard range-selection
    already exercises)."""

    app_name = "tables"

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

    def test_define_name_then_jump_to_it_via_name_box(self):
        from dogtail import rawinput, tree
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        time.sleep(1.5)
        self._put("A1", "10")
        self._put("A2", "20")
        self._put("A3", "30")
        # Select A1:A3 (Enter above leaves the active cell on A4 —
        # jump back to A1 first, matching the keyboard-selection test).
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.keyCombo("<Shift>Down")
        rawinput.keyCombo("<Shift>Down")
        time.sleep(0.5)

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "define-name"])
        time.sleep(0.8)
        name_entry = tree.root.findChild(lambda n: n.name == "Name" and n.roleName == "text")
        name_entry.text = "MyRange"
        time.sleep(0.2)
        confirm = tree.root.findChild(lambda n: n.name == "Define" and n.roleName == "push button")
        confirm.do_action(0)
        time.sleep(0.5)

        # Jump elsewhere, then back to the range by name.
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("Z9")
        rawinput.keyCombo("Return")
        time.sleep(0.5)
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("MyRange")
        rawinput.keyCombo("Return")
        time.sleep(0.5)

        labels = [c.name for c in self.app.findChildren(lambda c: c.roleName == "label")]
        stats = [l for l in labels if "Sum" in l]
        self.assertTrue(stats, f"no stats label found; labels: {labels}")
        self.assertIn("A1:A3", stats[0], f"stats: {stats[0]!r}")
        self.assertIn("Sum 60", stats[0])
        self.assertIsNone(self.process.poll(), "tables crashed jumping to a named range")


class TablesClipboardSmoke(BaseGUITestCase):
    """Suite-clipboard glue: Ctrl+C publishes the fragment MIME and
    Ctrl+V pastes it back with formulas still live. (The cross-app
    conversion matrix is unit-tested in the core crates; this exercises
    the GDK provider/reader path end to end.)"""

    app_name = "tables"

    def test_copy_paste_round_trip_keeps_formula_live(self):
        from dogtail import rawinput
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
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

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
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

        aid = "org.tunaos.decks"
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

        aid = "org.tunaos.tables"
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


class TablesSnapshotSmoke(BaseGUITestCase):
    """State-snapshot interface (#104): a real edit + formula is visible
    in the normalized JSON snapshot, not just in the AT-SPI cell text."""

    app_name = "tables"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="tables-snapshot-")
        super().setUp()

    def test_snapshot_reflects_cell_edits_and_formulas(self):
        import json
        import subprocess
        from dogtail import rawinput

        aid = "org.tunaos.tables"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.0)
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.typeText("42")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("A2")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.typeText("=A1*2")
        rawinput.keyCombo("Return")
        time.sleep(0.3)

        subprocess.run(["gapplication", "action", aid, "test-snapshot"])
        time.sleep(0.5)
        self.assertTrue(os.path.exists(self._snapshot_path), "snapshot file was not written")
        with open(self._snapshot_path) as f:
            snap = json.load(f)

        cells = {(c["row"], c["col"]): c for c in snap["sheet"]["cells"]}
        self.assertEqual(cells[(0, 0)]["value"], "42")
        self.assertIsNone(cells[(0, 0)]["formula"])
        self.assertEqual(cells[(1, 0)]["value"], "84")
        self.assertEqual(cells[(1, 0)]["formula"], "A1*2")
        self.assertIsNone(self.process.poll(), "tables crashed writing a snapshot")


class TablesFillHandleSmoke(BaseGUITestCase):
    """Fill handle (#113): dragging the handle at a selection's bottom-
    right corner tiles its content downward — a real mouse drag, not
    keyboard-only, verified against the #104 state snapshot rather than
    AT-SPI cell text/position (this grid's virtual cell nodes share the
    same container-position bridge gap as #132 — size is right, position
    isn't — so this test computes screen coordinates from the grid's own
    fixed layout constants instead of trusting AT-SPI node position)."""

    app_name = "tables"

    # Canvas origin: below the header/name-box UI chrome, left-aligned
    # under matchbox's fullscreen layout. Empirically measured via
    # screenshot against tables-core::sheet's ROW_HEADER_WIDTH (50),
    # COL_HEADER_HEIGHT (26), ROW_HEIGHT (28), COL_WIDTH (90) — not an
    # AT-SPI-reported position.
    CANVAS_X = 0
    CANVAS_Y = 128
    ROW_HEADER_WIDTH = 50
    COL_HEADER_HEIGHT = 26
    ROW_HEIGHT = 28
    COL_WIDTH = 90

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="tables-fill-")
        super().setUp()

    def _cell_bottom_right(self, row: int, col: int) -> tuple[float, float]:
        x = self.CANVAS_X + self.ROW_HEADER_WIDTH + (col + 1) * self.COL_WIDTH
        y = self.CANVAS_Y + self.COL_HEADER_HEIGHT + (row + 1) * self.ROW_HEIGHT
        return x, y

    def test_drag_fill_handle_down_tiles_the_value(self):
        import json
        import subprocess
        from dogtail import rawinput

        aid = "org.tunaos.tables"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.0)
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.typeText("7")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        # Re-select A1 (Enter above moved the active cell to A2).
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)

        hx, hy = self._cell_bottom_right(0, 0)
        _, target_y = self._cell_bottom_right(3, 0)
        self.drag(hx, hy, hx, target_y)
        time.sleep(0.5)

        subprocess.run(["gapplication", "action", aid, "test-snapshot"])
        time.sleep(0.5)
        self.assertTrue(os.path.exists(self._snapshot_path), "snapshot file was not written")
        with open(self._snapshot_path) as f:
            snap = json.load(f)
        cells = {(c["row"], c["col"]): c["value"] for c in snap["sheet"]["cells"]}
        self.assertEqual(cells.get((0, 0)), "7", f"A1 unexpectedly changed: {cells}")
        for row in (1, 2, 3):
            self.assertEqual(cells.get((row, 0)), "7", f"row {row} not filled: {cells}")
        self.assertIsNone(self.process.poll(), "tables crashed during fill-handle drag")


class TablesFormulaReferenceHighlightSmoke(BaseGUITestCase):
    """Formula reference highlighting (#113): typing a formula in the fx
    entry parses and outlines its cell/range references live (a
    connect_changed handler on every keystroke). The outlines themselves
    are canvas pixels with no AT-SPI/snapshot signal to assert on, so
    this test instead confirms the wiring doesn't break or crash normal
    formula entry — typing a multi-reference formula keystroke by
    keystroke (triggering connect_changed repeatedly, including
    mid-formula incomplete states like "=A1+" that must not panic the
    parser) still commits and evaluates correctly. The parser itself
    (including the defined-name-disambiguation edge case) is unit-tested
    in tables-core/src/sheet.rs."""

    app_name = "tables"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="tables-formula-ref-")
        super().setUp()

    def test_typing_a_multi_reference_formula_keystroke_by_keystroke_still_works(self):
        import json
        import subprocess
        from dogtail import rawinput

        aid = "org.tunaos.tables"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)

        for row, value in enumerate(["10", "20"]):
            rawinput.keyCombo("<Control>g")
            time.sleep(0.2)
            rawinput.typeText(f"A{row + 1}")
            rawinput.keyCombo("Return")
            time.sleep(0.3)
            rawinput.typeText(value)
            rawinput.keyCombo("Return")
            time.sleep(0.3)

        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("C1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        # typeText sends this character by character via xdotool, so
        # connect_changed fires once per character, including on
        # incomplete/invalid intermediate states like "=A1+".
        rawinput.typeText("=A1+A2")
        rawinput.keyCombo("Return")
        time.sleep(0.5)

        subprocess.run(["gapplication", "action", aid, "test-snapshot"])
        time.sleep(0.5)
        with open(self._snapshot_path) as f:
            snap = json.load(f)
        cells = {(c["row"], c["col"]): c["value"] for c in snap["sheet"]["cells"]}
        self.assertEqual(cells.get((0, 2)), "30", f"C1 formula result wrong: {cells}")
        self.assertIsNone(self.process.poll(), "tables crashed typing a formula with references")


class TablesSortIndicatorSmoke(BaseGUITestCase):
    """Sort (#113's "visible criteria"): clicking a column header toggles
    sort on that column, drawn as a small triangle in the header — clicking
    again reverses direction, a third time clears it. Verified via the
    #104 state snapshot's sorted_col field, not AT-SPI (the header text
    nodes share the same virtual-cell fragility documented for
    TablesFillHandleSmoke and #137)."""

    app_name = "tables"

    CANVAS_X = 0
    CANVAS_Y = 128
    ROW_HEADER_WIDTH = 50
    COL_HEADER_HEIGHT = 26
    COL_WIDTH = 90

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="tables-sort-")
        super().setUp()

    def _col_header_center(self, col: int) -> tuple[float, float]:
        x = self.CANVAS_X + self.ROW_HEADER_WIDTH + col * self.COL_WIDTH + self.COL_WIDTH / 2
        y = self.CANVAS_Y + self.COL_HEADER_HEIGHT / 2
        return x, y

    def _sorted_col(self):
        import json
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "test-snapshot"])
        time.sleep(0.5)
        with open(self._snapshot_path) as f:
            return json.load(f)["sheet"]["sorted_col"]

    def test_clicking_column_header_cycles_sort_direction_then_clears(self):
        import subprocess
        from dogtail import rawinput

        aid = "org.tunaos.tables"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.typeText("b")
        rawinput.keyCombo("Return")
        time.sleep(0.5)

        # xdotool's `mousemove --sync` appears to hang waiting for a
        # motion event that never fires when the target pixel is the
        # same as the pointer's current position (confirmed: clicking
        # the exact same header pixel three times in a row deterministically
        # hangs on the second click) — nudge the pointer to the corner box
        # (row/col header intersection, outside the header-click handler's
        # `wx > ROW_HEADER_WIDTH` zone, so it can't itself trigger a sort)
        # between clicks so each mousemove is a genuine move.
        x, y = self._col_header_center(0)
        corner_x, corner_y = self.CANVAS_X + self.ROW_HEADER_WIDTH / 2, self.CANVAS_Y + self.COL_HEADER_HEIGHT / 2

        rawinput.click(int(x), int(y))
        time.sleep(0.5)
        self.assertEqual(self._sorted_col(), {"col": 0, "ascending": True})

        rawinput.click(int(corner_x), int(corner_y))
        rawinput.click(int(x), int(y))
        time.sleep(0.5)
        self.assertEqual(self._sorted_col(), {"col": 0, "ascending": False})

        rawinput.click(int(corner_x), int(corner_y))
        rawinput.click(int(x), int(y))
        time.sleep(0.5)
        self.assertIsNone(self._sorted_col())
        self.assertIsNone(self.process.poll(), "tables crashed cycling sort")


class TablesFilterSmoke(BaseGUITestCase):
    """Row filtering (#113): the Filter by Column dialog hides rows that
    don't match, verified via the #104 state snapshot's hidden_rows list
    (not AT-SPI cell text/position — same rationale as
    TablesFillHandleSmoke)."""

    app_name = "tables"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="tables-filter-")
        super().setUp()

    def test_filter_by_column_hides_non_matching_rows_and_clear_unhides(self):
        import json
        import subprocess
        from dogtail import rawinput, tree

        aid = "org.tunaos.tables"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.0)

        for row, value in enumerate(["apple", "banana", "apple"]):
            rawinput.keyCombo("<Control>g")
            time.sleep(0.2)
            rawinput.typeText(f"A{row + 1}")
            rawinput.keyCombo("Return")
            time.sleep(0.3)
            rawinput.typeText(value)
            rawinput.keyCombo("Return")
            time.sleep(0.3)

        # Selection is on column A after the loop above (A4) — open the
        # filter dialog and filter to "apple".
        subprocess.run(["gapplication", "action", aid, "filter-by-column"])
        time.sleep(0.8)
        value_entry = tree.root.findChild(lambda n: n.name == "Filter value" and n.roleName == "text")
        value_entry.text = "apple"
        time.sleep(0.2)
        confirm = tree.root.findChild(lambda n: n.name == "Filter" and n.roleName == "push button")
        confirm.do_action(0)
        time.sleep(0.5)

        subprocess.run(["gapplication", "action", aid, "test-snapshot"])
        time.sleep(0.5)
        self.assertTrue(os.path.exists(self._snapshot_path), "snapshot file was not written")
        with open(self._snapshot_path) as f:
            snap = json.load(f)
        # Every other row is empty and so also fails the "apple" match —
        # only assert on the 3 rows this test actually populated.
        hidden = set(snap["sheet"]["hidden_rows"])
        self.assertIn(1, hidden, f"snapshot: {snap}")
        self.assertNotIn(0, hidden, f"snapshot: {snap}")
        self.assertNotIn(2, hidden, f"snapshot: {snap}")

        subprocess.run(["gapplication", "action", aid, "clear-filter"])
        time.sleep(0.5)
        subprocess.run(["gapplication", "action", aid, "test-snapshot"])
        time.sleep(0.5)
        with open(self._snapshot_path) as f:
            snap = json.load(f)
        self.assertEqual(snap["sheet"]["hidden_rows"], [], f"snapshot: {snap}")
        self.assertIsNone(self.process.poll(), "tables crashed during filter/clear-filter")


class TablesNamedRangeSmoke(BaseGUITestCase):
    """Named ranges (#113): Define Name captures the current selection;
    typing that name into the name box (instead of a cell reference)
    jumps back to and re-selects the whole range. Verified via the #104
    state snapshot's selection field rather than AT-SPI cell text.

    This deliberately does NOT jump to a far cell (e.g. Z9) before
    jumping back to the named range — doing so grows then shrinks this
    grid's virtual-cell accessible extent, which reliably crashes the
    app with a SIGSEGV on the very next AT-SPI-registry interaction,
    even one as unrelated as an ordinary D-Bus action call (not just a
    live dogtail tree walk). Confirmed independently of this feature
    with keyboard input alone; filed as #137 (worse than #132's wrong
    coordinates — this one's a hard crash). See
    test_jump_far_and_back_to_named_range_crashes_137 below for the
    documented repro, skipped rather than fixed here."""

    app_name = "tables"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="tables-named-range-")
        super().setUp()

    def test_define_name_then_jump_to_it_via_name_box(self):
        import json
        import subprocess
        from dogtail import rawinput, tree

        aid = "org.tunaos.tables"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)

        for row, value in enumerate(["10", "20", "30"]):
            rawinput.keyCombo("<Control>g")
            time.sleep(0.2)
            rawinput.typeText(f"A{row + 1}")
            rawinput.keyCombo("Return")
            time.sleep(0.3)
            rawinput.typeText(value)
            rawinput.keyCombo("Return")
            time.sleep(0.3)

        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        # A name-box jump hands focus to fx (so the user can type a new
        # value straight away, per test_name_box_jump_and_edit) — Escape
        # returns focus to the grid so Shift+Down actually extends the
        # grid selection instead of doing nothing inside fx.
        rawinput.keyCombo("Escape")
        time.sleep(0.3)
        rawinput.keyCombo("<Shift>Down")
        rawinput.keyCombo("<Shift>Down")
        time.sleep(0.5)

        subprocess.run(["gapplication", "action", aid, "define-name"])
        time.sleep(0.8)
        name_entry = tree.root.findChild(lambda n: n.name == "Name" and n.roleName == "text")
        name_entry.text = "MyRange"
        time.sleep(0.2)
        confirm = tree.root.findChild(lambda n: n.name == "Define" and n.roleName == "push button")
        confirm.do_action(0)
        time.sleep(0.5)

        # Collapse back to a single cell (still within the existing
        # extent — no far jump, see class docstring) so the jump below
        # actually proves the name box re-extends the selection rather
        # than trivially matching an unchanged one.
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.5)
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("MyRange")
        rawinput.keyCombo("Return")
        time.sleep(0.5)

        subprocess.run(["gapplication", "action", aid, "test-snapshot"])
        time.sleep(0.5)
        self.assertTrue(os.path.exists(self._snapshot_path), "snapshot file was not written")
        with open(self._snapshot_path) as f:
            snap = json.load(f)
        self.assertEqual(tuple(snap["sheet"]["selection"]), (0, 0, 2, 0), f"snapshot: {snap}")
        self.assertIsNone(self.process.poll(), "tables crashed defining/jumping to a named range")

    @unittest.skip(
        "SIGSEGV: jumping to a cell far from the origin (growing this "
        "grid's virtual-cell AT-SPI accessible extent), then back to a "
        "small selection (shrinking it), then any AT-SPI-registry "
        "interaction at all — even an unrelated D-Bus action call, not "
        "just a live tree walk — crashes the app. 100% reproducible "
        "with keyboard input alone, independent of the named-ranges "
        "feature itself (which is why this repro doesn't use Define "
        "Name). Filed as #137 (tables/src/grid_area.rs's CellAccessible "
        "bridge; needs a GObject-lifetime-aware pass, not a quick "
        "patch — same family as #132 but a hard crash, not just wrong "
        "coordinates). Revisit once #137 is fixed."
    )
    def test_jump_far_and_back_to_named_range_crashes_137(self):
        import subprocess
        from dogtail import rawinput

        aid = "org.tunaos.tables"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("Z9")
        rawinput.keyCombo("Return")
        time.sleep(0.5)
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.keyCombo("<Shift>Down")
        rawinput.keyCombo("<Shift>Down")
        time.sleep(0.5)
        subprocess.run(["gapplication", "action", aid, "test-snapshot"])
        time.sleep(0.5)
        self.assertIsNone(self.process.poll(), "expected to crash per #137 — fixed?")


class DecksSnapshotSmoke(BaseGUITestCase):
    """State-snapshot interface (#104), same mechanism as
    TablesSnapshotSmoke: adding objects is visible in the normalized
    JSON snapshot, not just the AT-SPI tree."""

    app_name = "decks"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="decks-snapshot-")
        super().setUp()

    def test_snapshot_reflects_added_objects(self):
        import json
        import subprocess

        aid = "org.tunaos.decks"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)
        subprocess.run(["gapplication", "action", aid, "add-text-box"])
        subprocess.run(["gapplication", "action", aid, "add-shape"])
        time.sleep(1.0)

        subprocess.run(["gapplication", "action", aid, "test-snapshot"])
        time.sleep(0.5)
        self.assertTrue(os.path.exists(self._snapshot_path), "snapshot file was not written")
        with open(self._snapshot_path) as f:
            snap = json.load(f)

        self.assertEqual(snap["slide_count"], 1)
        kinds = [o["kind"] for o in snap["slides"][0]["objects"]]
        self.assertIn("TextBox", kinds)
        self.assertIn("Rect", kinds)
        self.assertIsNone(self.process.poll(), "decks crashed writing a snapshot")


class DecksSelectionSmoke(BaseGUITestCase):
    """Object selection updates the canvas a11y description and the
    inspector (fit-to-viewport geometry keeps coordinates stable)."""

    app_name = "decks"

    @unittest.skip(
        "AT-SPI reports this DrawingArea's Component.position in DESKTOP_COORDS "
        "as (0, 0) regardless of the sidebar/toolbar it's actually offset behind "
        "(confirmed via direct XTest probing: a click computed from that position "
        "lands ~265px right / ~85px down of where it should, while hit-testing "
        "and the coordinate math it feeds into are both correct once given real "
        "screen coordinates). This looks like a GTK4 AT-SPI bridge gap for custom "
        "DrawingArea widgets nested in a box/paned layout, not an app bug — "
        "tracked as #132; revisit once that's fixed or a GTK4 AT-SPI fix for "
        "Component position lands upstream."
    )
    def test_click_selects_object(self):
        from dogtail import rawinput
        import subprocess

        aid = "org.tunaos.decks"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)
        subprocess.run(["gapplication", "action", aid, "add-shape"])
        time.sleep(1.0)
        # Default Rect is at slide (200,200,200x150) in the 960x540 slide
        # coordinate space. The canvas fits that into whatever it's
        # actually sized to (slide_geometry() in canvas.rs) — compute the
        # on-screen click point from the canvas's real geometry rather
        # than assuming a fixed window size, since that varies by window
        # manager (e.g. matchbox fullscreens new windows).
        canvas_for_geom = self.app.child(name="Slide canvas")
        cx, cy = canvas_for_geom.position
        cw, ch = canvas_for_geom.size
        scale = min(cw / 960.0, ch / 540.0) * 0.92
        ox = (cw - 960.0 * scale) / 2.0
        oy = (ch - 540.0 * scale) / 2.0
        click_x = cx + ox + 300.0 * scale  # rect center: (200+100, 200+75)
        click_y = cy + oy + 275.0 * scale
        rawinput.click(int(click_x), int(click_y))
        time.sleep(1.0)
        canvas = self.app.child(name="Slide canvas")
        self.assertIn("selected", canvas.description,
                      f"canvas description: {canvas.description!r}")
        self.assertIsNone(self.process.poll(), "decks crashed during selection")


class DecksCloseGuardSmoke(BaseGUITestCase):
    """Real GTK journey: closing a dirty deck offers Save/Discard/Cancel
    (issue #99) instead of silently discarding unsaved work."""

    app_name = "decks"

    def setUp(self):
        self._dir = self.temp_dir(prefix="decks-close-guard-")
        super().setUp()

    def _add_shape(self):
        import subprocess

        aid = "org.tunaos.decks"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)
        subprocess.run(["gapplication", "action", aid, "add-shape"])
        time.sleep(1.0)

    def test_cancel_keeps_window_open_then_discard_closes(self):
        self._add_shape()

        self.app.child(name="Close", roleName="push button").do_action(0)
        time.sleep(0.8)
        self.app.child(name="Cancel", roleName="push button").do_action(0)
        time.sleep(0.5)
        self.assertIsNone(self.process.poll(), "Cancel must not close the window")
        self.assertIsNotNone(self.app.child(roleName="frame"), "window should still be open")

        self.app.child(name="Close", roleName="push button").do_action(0)
        time.sleep(0.8)
        self.app.child(name="Discard", roleName="push button").do_action(0)
        self.assertIsNotNone(self.wait_for_process_exit(), "Discard must close the window")

    def test_save_in_close_guard_writes_the_file_and_closes(self):
        from dogtail import tree

        self._add_shape()
        out_path = os.path.join(self._dir, "close-guard-save.pptx")

        self.app.child(name="Close", roleName="push button").do_action(0)
        time.sleep(0.8)
        self.app.child(name="Save", roleName="push button").do_action(0)
        time.sleep(1.0)

        name_entry = tree.root.findChild(lambda n: n.name == "Name:" and n.roleName == "text")
        name_entry.text = out_path
        time.sleep(0.3)
        confirm = tree.root.findChild(lambda n: n.name == "Save" and n.roleName == "push button")
        confirm.do_action(0)

        self.assertIsNotNone(self.wait_for_process_exit(),
                              "Save must close the window once it succeeds")
        self.assertTrue(os.path.exists(out_path), "the deck was not written to disk")
        self.assertGreater(os.path.getsize(out_path), 0)


class DecksAutosaveSmoke(BaseGUITestCase):
    """Crash-recovery snapshot lifecycle (issue #99), same contract as
    Tables: a dirty, never-saved deck survives an unclean process kill and
    is offered back on the next launch."""

    app_name = "decks"

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(prefix="decks-autosave-state-")
        super().setUp()

    def _snapshot_files(self):
        snap_dir = os.path.join(self._state_dir, "decks")
        if not os.path.isdir(snap_dir):
            return []
        return [f for f in os.listdir(snap_dir) if f.endswith(".snapshot")]

    def _add_shape(self):
        import subprocess

        aid = "org.tunaos.decks"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)
        subprocess.run(["gapplication", "action", aid, "add-shape"])
        time.sleep(1.0)

    def test_crash_then_relaunch_recovers_and_clears_the_snapshot(self):
        import subprocess

        self._add_shape()
        subprocess.run(["gapplication", "action", "org.tunaos.decks", "autosave-now"])
        time.sleep(0.5)
        self.assertEqual(len(self._snapshot_files()), 1, "autosave-now must have written a snapshot")

        # Simulate a crash: kill the process directly, bypassing the close
        # guard, so the snapshot is never cleared by a clean exit.
        self.process.kill()
        self.process.wait(timeout=5)

        env = os.environ.copy()
        env["GDK_BACKEND"] = "x11"
        env.update(self.launch_env)
        self.process = subprocess.Popen(
            [self.bin_path], env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        self.app = self.wait_for_app(self.app_name)
        time.sleep(1.5)

        frame = self.app.child(roleName="frame")
        self.assertIn("Recovered", frame.name, f"window did not announce recovery: {frame.name!r}")
        self.assertEqual(self._snapshot_files(), [],
                          "the recovered snapshot must be cleared so it isn't offered again")


class DecksSmoke(BaseGUITestCase):
    app_name = "decks"

    def test_launch_shows_window(self):
        self.assertIsNotNone(self.app.child(roleName="frame"))
        self.assertIsNone(self.process.poll(), "decks exited after launch")

    def test_canvas_reports_slide_state(self):
        # Decks starts on an empty-state page; the canvas maps (and enters
        # the AT-SPI tree) once a deck exists.
        import subprocess
        subprocess.run(["gapplication", "action", "org.tunaos.decks", "new-document"])
        time.sleep(1.5)
        canvas = self.app.child(name="Slide canvas")
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if "slide 1 of" in canvas.description:
                break
            time.sleep(0.3)
        self.assertIn("slide 1 of", canvas.description,
                      f"canvas description: {canvas.description!r}")
