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
        from dogtail import rawinput

        self.wait_for_node(name="New Document", roleName="push button").do_action(0)
        self.wait_for_node(roleName="text")
        rawinput.typeText("the quick brown fox")
        self.wait_for_node(name="4 words", roleName="label")

        snap = self.trigger_snapshot("org.tunaos.letters")

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


class LettersSaveFailureSmoke(BaseGUITestCase):
    """Failed writes must never retire edits or the last recovery checkpoint."""

    app_name = "letters"

    def setUp(self):
        self._dir = self.temp_dir(prefix="letters-save-failure-")
        self._source = os.path.join(self._dir, "source")
        os.mkdir(self._source)
        self._path = os.path.join(self._source, "document.md")
        with open(self._path, "w") as stream:
            stream.write("original")
        self.launch_args = [self._path]
        self._state = self.isolate_autosave_state()
        self.isolate_snapshot()
        self.isolate_gsettings()
        super().setUp()

    def _edit_and_checkpoint(self):
        from dogtail import rawinput
        self.wait_for_node(roleName="text")
        rawinput.keyCombo("<Control>End")
        rawinput.typeText(" unsaved edit")

        def edited():
            snapshot = self.trigger_snapshot("org.tunaos.letters")
            text = "".join(run["text"] for para in snapshot["paragraphs"] for run in para["runs"])
            return snapshot if "unsaved edit" in text else None

        self._edited = self.wait_for_condition(edited, description="edited document snapshot")
        self.gapplication_action("org.tunaos.letters", "autosave-now")
        from pathlib import Path
        self.wait_for_condition(lambda: list(Path(self._state).rglob("*.snapshot")),
                                description="recovery checkpoint")
        self._checkpoint = {path: path.read_bytes() for path in Path(self._state).rglob("*.snapshot*")}

    def _make_destination_unavailable(self):
        # Deterministic even as root: the old parent no longer exists.
        # Keep the original file in a renamed directory for byte comparison.
        self._backup = os.path.join(self._dir, "backup")
        os.rename(self._source, self._backup)

    def _assert_error_preserves_work(self):
        self.wait_for_node(name="Could not save document")
        self.assertIsNone(self.process.poll())
        self.assertEqual(self.trigger_snapshot("org.tunaos.letters"), self._edited)
        for path, content in self._checkpoint.items():
            self.assertEqual(path.read_bytes(), content, f"recovery checkpoint changed: {path}")
        with open(os.path.join(self._backup, "document.md")) as stream:
            self.assertEqual(stream.read(), "original")

    def test_failed_save_retains_edits_and_close_guard(self):
        self._edit_and_checkpoint()
        self._make_destination_unavailable()
        self.gapplication_action("org.tunaos.letters", "save-file")
        self._assert_error_preserves_work()
        self.wait_for_node(name="OK", roleName="push button").do_action(0)
        self.wait_for_node(name="Close", roleName="push button").do_action(0)
        self.wait_for_node(name="Save All", roleName="push button")
        self.assertIsNone(self.process.poll(), "failed save incorrectly cleared the close guard")

    def test_save_all_failure_keeps_window_and_checkpoint(self):
        self._edit_and_checkpoint()
        self._make_destination_unavailable()
        self.wait_for_node(name="Close", roleName="push button").do_action(0)
        self.wait_for_node(name="Save All", roleName="push button").do_action(0)
        self._assert_error_preserves_work()

    def test_save_as_cancel_keeps_original_identity_and_unsaved_work(self):
        from dogtail import tree
        self._edit_and_checkpoint()
        self.gapplication_action("org.tunaos.letters", "save-file-as")
        cancel = self.wait_for_condition(
            lambda: tree.root.findChild(lambda node: node.name == "Cancel" and node.roleName == "push button"),
            description="Save As cancel button",
        )
        cancel.do_action(0)
        # Ordinary save must still target the original path after cancellation.
        self.gapplication_action("org.tunaos.letters", "save-file")
        def saved():
            with open(self._path) as stream:
                return "unsaved edit" in stream.read()
        self.wait_for_condition(saved, description="save to original path after cancellation")
        self.assertEqual(self.trigger_snapshot("org.tunaos.letters"), self._edited)


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
        self.relaunch_app(crash=True)
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

        self.relaunch_app()
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

    def setUp(self):
        # Registers the #104 snapshot action before launch; the
        # sheet-actions test reads sheet_names from it.
        self._snapshot_path = self.isolate_snapshot(prefix="tables-multisheet-")
        super().setUp()

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
        # The grid's own accessible description is the observable, so wait
        # for the recalculated value rather than for a fixed 0.5s.
        self.wait_until(
            lambda: self.app.child(name="Spreadsheet grid").description,
            lambda text: "6" in text,
            description="Sheet2's grid to show the recalculated 6",
        )

        # Switch back to Sheet1 via the dropdown and confirm its own value.
        #
        # GtkDropDown's popup list is not exposed in the app's AT-SPI tree
        # (probed: opening it adds no node anywhere), so there is nothing
        # to wait *for* before pressing Up/Return — and with the old fixed
        # 0.3s, a slow open meant the keys went nowhere, the journey stayed
        # on Sheet2, and it failed with "'2' not found in 'cell A1: 6'":
        # a message that reads like the data-isolation bug this journey
        # exists to catch, rather than a dropdown that had not opened yet.
        #
        # What *is* observable is the outcome — the switcher's accessible
        # name mirrors the selected sheet — so the gesture is retried until
        # the selection actually lands. Up at the top of a two-sheet list
        # stays put, so repeating it cannot overshoot.
        def select_the_sheet_before_this_one():
            switcher.child(roleName="toggle button").do_action(0)
            rawinput.keyCombo("Up")
            rawinput.keyCombo("Return")
            return self.app.child(roleName="combo box").name

        self.wait_until(
            select_the_sheet_before_this_one,
            lambda name: name == "Sheet1",
            interval=0.6,
            description="the sheet switcher to report Sheet1 selected",
        )
        description = self.wait_until(
            lambda: self.app.child(name="Spreadsheet grid").description,
            lambda text: "2" in text,
            description="Sheet1's grid to show its own value after the switch",
        )
        self.assertNotIn("6", description, "Sheet2's value leaked into Sheet1")

    def test_sheet_operations_survive_a_hidden_toolbar(self):
        """The sheet operations exist as actions, not only as buttons.

        Below the narrow breakpoint the rename/move/delete buttons are
        hidden — a layout decision — and at 400px the Add button was
        hidden too, which left no way to manage sheets at all (#520).
        Driving the actions directly is how the capability is asserted
        independently of which buttons a given width happens to show;
        this runs at every width the display matrix uses.
        """
        import json
        import subprocess

        aid = "org.tunaos.tables"
        snapshot_path = self._snapshot_path
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)

        def sheet_names():
            subprocess.run(["gapplication", "action", aid, "test-snapshot"])
            time.sleep(0.5)
            with open(snapshot_path) as handle:
                return json.load(handle)["sheet_names"]

        self.assertEqual(len(sheet_names()), 1, "expected one sheet to start")
        subprocess.run(["gapplication", "action", aid, "add-sheet"])
        time.sleep(0.8)
        self.assertEqual(len(sheet_names()), 2, "add-sheet action did not add a sheet")
        subprocess.run(["gapplication", "action", aid, "move-sheet-left"])
        time.sleep(0.8)
        self.assertIsNone(self.process.poll(), "tables crashed running a sheet action")


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


class TablesStaleSnapshotSmoke(BaseGUITestCase):
    """Already-saved work must not come back as a recovery offer.

    A snapshot is cleared when the document is saved, but that clear was
    written `let _ = slot.clear()` — a read-only state directory or a full
    disk left the snapshot behind with the failure discarded. The next launch
    then offered the user their *already-saved* workbook back as
    "recovered": a scary dialog about losing nothing. Whatever they chose,
    the clear failed again, so it returned on every launch.

    Reporting the failed clear would not help much — it happens at the moment
    of a successful save, about a temporary file nobody can act on — so what
    is suppressed is the false offer itself: a snapshot the saved file has
    overtaken has nothing left to recover.

    The journey builds that state the way a failed clear leaves it. Save a
    real workbook, reopen it, edit, snapshot, then make the saved file newer
    than the snapshot and crash. The relaunch must come up as an ordinary
    window, not a recovery.
    """

    app_name = "tables"

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(prefix="tables-stale-state-")
        self._dir = self.temp_dir("tables-stale-docs-")
        super().setUp()

    def _snapshot_files(self):
        snap_dir = os.path.join(self._state_dir, "tables")
        if not os.path.isdir(snap_dir):
            return []
        return [f for f in os.listdir(snap_dir) if f.endswith(".snapshot")]

    def _save_a_real_workbook(self):
        """Save through the close guard, which is the route a journey has to
        a genuinely written file, and leaves the app closed."""
        import subprocess

        from dogtail import rawinput, tree

        out_path = os.path.join(self._dir, "quarterly.xlsx")
        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        time.sleep(1.5)
        rawinput.typeText("=6*7")
        rawinput.keyCombo("Return")
        time.sleep(0.5)

        self.app.child(name="Close", roleName="push button").do_action(0)
        time.sleep(0.8)
        self.app.child(name="Save", roleName="push button").do_action(0)
        time.sleep(1.0)
        name_entry = tree.root.findChild(lambda n: n.name == "Name:" and n.roleName == "text")
        name_entry.text = out_path
        time.sleep(0.3)
        tree.root.findChild(lambda n: n.name == "Save" and n.roleName == "push button").do_action(0)
        self.assertIsNotNone(self.wait_for_process_exit(), "the save did not complete")
        self.assertTrue(os.path.exists(out_path), "no workbook was written")
        return out_path

    def test_a_snapshot_the_save_overtook_is_not_offered_as_recovery(self):
        import subprocess

        from dogtail import rawinput

        out_path = self._save_a_real_workbook()

        # Reopen the saved file so the snapshot records it as the document's
        # path, then dirty it and snapshot.
        self.relaunch_app(launch_args=[out_path])
        time.sleep(2.0)
        rawinput.typeText("=1+1")
        rawinput.keyCombo("Return")
        time.sleep(0.5)
        subprocess.run(["gapplication", "action", "org.tunaos.tables", "autosave-now"])
        time.sleep(0.8)
        self.assertEqual(len(self._snapshot_files()), 1, "precondition: a snapshot exists")

        # What a failed clear leaves behind: the document saved after the
        # snapshot was taken, and the snapshot still on disk.
        saved_later = time.time() + 10
        os.utime(out_path, (saved_later, saved_later))

        self.relaunch_app(crash=True)
        time.sleep(2.0)
        frame = self.app.child(roleName="frame")
        self.assertNotIn(
            "Recovered", frame.name,
            "work already on disk was offered back as a recovery: the window "
            f"came up as {frame.name!r}",
        )


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
        # guard entirely, so the snapshot is never cleared by a clean exit,
        # then relaunch against the same state dir and expect recovery.
        self.relaunch_app(crash=True)
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
        # Ctrl+G is not synchronous at every window size: below the narrow
        # breakpoint it opens the Go to Cell dialog (#516) instead of
        # focusing the name box, and typing before the dialog is up goes
        # nowhere. Wait for whichever one took focus.
        rawinput.keyCombo("<Control>g")
        time.sleep(0.5)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.5)
        rawinput.typeText("=2+3")
        rawinput.keyCombo("Return")
        rawinput.keyCombo("<Control>z")
        rawinput.keyCombo("<Control><Shift>z")
        rawinput.keyCombo("<Control>s")
        time.sleep(1.2)
        with zipfile.ZipFile(self._doc) as book:
            sheet_xml = book.read("xl/worksheets/sheet1.xml").decode()
        self.assertIn("<f>2+3</f>", sheet_xml)

        self.relaunch_app(launch_args=[self._doc])
        time.sleep(1.2)
        rawinput.keyCombo("<Control>g")
        time.sleep(0.5)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.5)
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


class TablesCellEntryMixin:
    """Writing a cell in Tables, confirmed at every step.

    Every wait in here used to be a fixed sleep. Measured with
    `GUI_TEST_SLEEP_SCALE=0.25`, that lost values outright and the
    journeys failed with messages that read like product defects — a
    missing cell surfaced as `'Sum 60' not found in 'A1:A3 · Sum 50 ·
    Avg 25 · Count 2'`, which points at the formula engine rather than at
    two cells of three having been written.

    The observables, established by probing the live AT-SPI tree:

    * the two text entries are "Cell reference" (the name box) and
      "Formula input" (fx), and which one has focus says where the
      keystrokes are going;
    * Ctrl+G moves focus to the name box, and a completed jump hands it
      back to fx;
    * the grid's accessible description names the cell it last committed
      ("cell A2: 20"), which confirms the write.

    Return does not advance the active cell in this grid, so the name-box
    jump is genuinely required for each cell.
    """

    def _grid(self):
        return self.app.child(name="Spreadsheet grid").description

    def _focused(self, label):
        node = self.app.child(name=label, roleName="text")
        return bool(node and node.focused)

    def _wait_for_a_new_document(self):
        """A new document starts with A1 active; that is the ready signal."""
        return self.wait_until(self._grid, lambda text: "cell A1" in text,
                               description="a new document with A1 active")

    def _put(self, ref, value):
        from dogtail import rawinput
        rawinput.keyCombo("<Control>g")
        self.wait_until(lambda: self._focused("Cell reference"), bool,
                        description="the name box to take focus")
        rawinput.typeText(ref)
        rawinput.keyCombo("Return")
        self.wait_until(lambda: self._focused("Formula input"), bool,
                        description=f"the jump to {ref} to hand focus back to fx")
        rawinput.typeText(value)
        rawinput.keyCombo("Return")
        self.wait_until(
            self._grid,
            lambda text: f"cell {ref}: {value}" in text,
            description=f"{ref} to hold {value}",
        )


class TablesNamedRangeStatsSmoke(TablesCellEntryMixin, BaseGUITestCase):
    """Named ranges (#113) verified through the stats label's range readout.

    Renamed from TablesNamedRangeSmoke, which a second class of that name
    further down this file silently replaced — so this journey had not run
    since that class was added, and the suite quietly traded a crash
    reproduction for a passing test.

    Unlike its namesake it jumps to a far cell (Z9) before returning to
    the named range, which grows the grid's virtual-cell extent and used
    to abort Tables with `malloc(): unaligned fastbin chunk detected`
    (#507). Keep the far jump: it is the regression guard for that
    crash."""

    app_name = "tables"

    def test_define_name_then_jump_to_it_via_name_box(self):
        from dogtail import rawinput, tree
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        self._wait_for_a_new_document()
        self._put("A1", "10")
        self._put("A2", "20")
        self._put("A3", "30")
        # Select A1:A3. Jump back to A1 first — Return does not move the
        # active cell in this grid, so after the writes above it is still
        # on A3.
        rawinput.keyCombo("<Control>g")
        self.wait_until(lambda: self._focused("Cell reference"), bool,
                        description="the name box to take focus")
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        # A name-box jump hands focus to fx with the cell's contents
        # loaded, so Escape first: without it Shift+Down extends nothing
        # and Define Name captures a single cell instead of A1:A3 (same
        # step as the sibling journey).
        #
        # Both halves have to be observed, and at a quarter of the old
        # fixed waits neither was: Escape landed before fx had focus, the
        # selection keys then went into fx, and an empty fx was committed
        # over A1 — leaving `Sum 50 · Count 2` for a range the readout
        # still called A1:A3. That looked like a summing bug.
        self.wait_until(lambda: self._focused("Formula input"), bool,
                        description="the jump to A1 to hand focus back to fx")
        rawinput.keyCombo("Escape")
        self.wait_until(lambda: not self._focused("Formula input"), bool,
                        description="Escape to return focus to the grid")
        rawinput.keyCombo("<Shift>Down")
        rawinput.keyCombo("<Shift>Down")
        # The readout naming the extended range is the signal the
        # selection landed, and it is what Define Name will capture.
        self.wait_until(
            lambda: next((c.name for c in self.app.findChildren(
                lambda c: c.roleName == "label") if "Sum" in c.name), None),
            lambda label: label is not None and "A1:A3" in label,
            description="the selection to extend to A1:A3",
        )

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "define-name"])
        name_entry = self.wait_until(
            lambda: tree.root.findChild(
                lambda n: n.name == "Name" and n.roleName == "text"),
            lambda node: node is not None,
            description="the Define Name dialog",
        )
        name_entry.text = "MyRange"
        tree.root.findChild(
            lambda n: n.name == "Define" and n.roleName == "push button").do_action(0)
        # The dialog going away is the signal the name was defined; acting
        # on the name box while it is still up sends the keys to it.
        self.wait_until(
            lambda: tree.root.findChild(
                lambda n: n.name == "Define" and n.roleName == "push button",
                retry=False, requireResult=False),
            lambda node: node is None,
            description="the Define Name dialog to close",
        )

        # Jump elsewhere, then back to the range by name. Both jumps use
        # the same focus handshake as `_put`: the name box has to have
        # focus before the reference is typed, or it goes to fx.
        for reference in ("Z9", "MyRange"):
            rawinput.keyCombo("<Control>g")
            self.wait_until(lambda: self._focused("Cell reference"), bool,
                            description="the name box to take focus")
            rawinput.typeText(reference)
            rawinput.keyCombo("Return")
            self.wait_until(
                lambda: self._focused("Formula input"), bool,
                description=f"the jump to {reference} to hand focus back to fx",
            )

        # The readout has to catch up with the jump *and* the
        # recalculation, so wait for it rather than sampling once: on a
        # timeout `wait_until` reports what it last saw, which is the
        # difference between "the sum is wrong" and "the sum was not
        # computed yet".
        def stats_label():
            labels = [c.name for c in self.app.findChildren(lambda c: c.roleName == "label")]
            return next((l for l in labels if "Sum" in l), None)

        stats = self.wait_until(
            stats_label,
            lambda label: label is not None and "A1:A3" in label and "Sum 60" in label,
            description="the stats readout to show A1:A3 summing to 60",
        )
        self.assertIn("A1:A3", stats, f"stats: {stats!r}")
        self.assertIn("Sum 60", stats)
        self.assertIsNone(self.process.poll(), "tables crashed jumping to a named range")


class TablesNameBoxAccessibilitySmoke(TablesCellEntryMixin, BaseGUITestCase):
    """A name-box jump has to announce where it landed.

    The grid's accessible description is what a screen reader reads out,
    and it only ever named the last *committed* cell: jumping by cell
    reference or to a defined name moved the selection and said nothing,
    so a keyboard-and-screen-reader user had no way to know where they
    were. It is also the signal the GUI journeys were missing, which is
    why three of them depended on fixed waits (#354).
    """

    app_name = "tables"

    def test_jumping_by_cell_reference_announces_the_new_cell(self):
        from dogtail import rawinput
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        self._wait_for_a_new_document()
        self._put("B2", "seven")

        # Jump somewhere empty and far from both A1 and the written cell,
        # so neither a stale description nor a coincidence can pass this.
        rawinput.keyCombo("<Control>g")
        self.wait_until(lambda: self._focused("Cell reference"), bool,
                        description="the name box to take focus")
        rawinput.typeText("D7")
        rawinput.keyCombo("Return")
        description = self.wait_until(
            self._grid,
            lambda text: "D7" in text,
            description="the grid to announce the jumped-to cell",
        )
        self.assertIn("cell D7", description, f"grid: {description!r}")
        self.assertNotIn("B2", description, "the grid still announced the previous cell")

        # And jumping onto a cell that holds something announces its value.
        rawinput.keyCombo("<Control>g")
        self.wait_until(lambda: self._focused("Cell reference"), bool,
                        description="the name box to take focus")
        rawinput.typeText("B2")
        rawinput.keyCombo("Return")
        description = self.wait_until(
            self._grid,
            lambda text: "B2" in text,
            description="the grid to announce the cell jumped back to",
        )
        self.assertIn("cell B2: seven", description, f"grid: {description!r}")
        self.assertIsNone(self.process.poll(), "tables exited during a name-box jump")


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


class CrossAppClipboardSmoke(BaseGUITestCase):
    """Two live applications, one X11 selection (#442).

    The suite's clipboard design is one fragment format shared by all
    three apps, and its *conversions* are pure functions with their own
    unit tests. What none of that covers is the transfer: on X11 the
    clipboard is a negotiation between two processes, where one owns the
    selection and the other asks it for a format. A single app copying and
    pasting to itself — which is what every clipboard journey did until
    now — never exercises that at all.

    Tables is the primary app here and Letters the second, because a grid
    fragment arriving in a word processor is the conversion most likely to
    be wrong: it is the only pair where the source and destination models
    genuinely differ.
    """

    app_name = "tables"

    def setUp(self):
        self.isolate_xdg()
        super().setUp()

    def _put_cell(self, ref, value):
        from dogtail import rawinput
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText(ref)
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.typeText(value)
        rawinput.keyCombo("Return")
        time.sleep(0.3)

    def _select_cell(self, ref):
        from dogtail import rawinput
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText(ref)
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.keyCombo("Escape")
        time.sleep(0.3)

    def test_a_grid_copied_in_tables_pastes_into_letters(self):
        from dogtail import rawinput
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        time.sleep(1.5)
        self._put_cell("A1", "alpha")
        self._put_cell("B1", "beta")
        self._select_cell("A1")
        rawinput.keyCombo("<Control>c")
        time.sleep(0.8)

        letters = self.launch_second_app("letters")
        self.focus_app(letters)
        time.sleep(0.5)
        letters.app.child(name="New Document", roleName="push button").do_action(0)
        time.sleep(1.5)
        rawinput.keyCombo("<Control>v")
        time.sleep(1.5)

        editor = letters.app.child(roleName="text")
        self.assertIn(
            "alpha", editor.text or "",
            f"a cell copied in Tables did not arrive in Letters: {editor.text!r}",
        )
        # Both processes, not just the receiver: a transfer that crashes
        # the selection owner is as much a failure as one that loses data,
        # and the assertion above would not notice.
        self.assert_still_running(letters)

    def test_text_copied_in_letters_pastes_into_tables(self):
        from dogtail import rawinput
        import subprocess

        letters = self.launch_second_app("letters")
        self.focus_app(letters)
        time.sleep(0.5)
        letters.app.child(name="New Document", roleName="push button").do_action(0)
        time.sleep(1.5)
        rawinput.typeText("gamma")
        time.sleep(0.5)
        rawinput.keyCombo("<Control>a")
        time.sleep(0.3)
        rawinput.keyCombo("<Control>c")
        time.sleep(0.8)

        self.focus_app()
        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        time.sleep(1.5)
        self._select_cell("A1")
        rawinput.keyCombo("<Control>v")
        time.sleep(1.5)
        rawinput.keyCombo("Right")
        rawinput.keyCombo("Left")
        time.sleep(0.5)

        grid = self.app.child(name="Spreadsheet grid")
        self.assertIn(
            "gamma", grid.description or "",
            f"text copied in Letters did not arrive in Tables: {grid.description!r}",
        )
        self.assert_still_running(letters)

    def test_the_clipboard_survives_the_copying_app_being_closed(self):
        """X11 hands the selection to the owning *process*: close it and
        the content is gone unless something persisted it. A user copies
        in Tables, closes Tables, then pastes in Letters — so whatever
        this suite does here, it should be known rather than discovered.

        This asserts the honest outcome and not a wish: either the content
        survives, or the paste is a no-op. What it refuses to accept is a
        crash, or a paste that inserts garbage.
        """
        from dogtail import rawinput
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        time.sleep(1.5)
        self._put_cell("A1", "orphaned")
        self._select_cell("A1")
        rawinput.keyCombo("<Control>c")
        time.sleep(0.8)

        letters = self.launch_second_app("letters")
        self.focus_app(letters)
        time.sleep(0.5)
        letters.app.child(name="New Document", roleName="push button").do_action(0)
        time.sleep(1.5)

        # Now close the owner and paste into the survivor.
        self.process.terminate()
        self.process.wait(timeout=5)
        time.sleep(1.0)
        self.focus_app(letters)
        rawinput.keyCombo("<Control>v")
        time.sleep(1.5)

        text = letters.app.child(roleName="text").text or ""
        self.assertIsNone(
            letters.process.poll(),
            "Letters crashed pasting from a clipboard whose owner had exited",
        )
        print(f"clipboard after owner exit: {text!r}")
        if "orphaned" not in text:
            # Recorded, not asserted away: with no clipboard manager in the
            # container this is the expected X11 behaviour, and the journey
            # exists to prove it degrades quietly rather than crashing.
            self.assertEqual(
                text.strip(), "",
                "paste after the owner exited inserted something that was "
                f"neither the copied content nor nothing: {text!r}",
            )


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
        from dogtail import rawinput

        aid = "org.tunaos.tables"
        self.gapplication_action(aid, "new-document")
        self.wait_for_node(name="Spreadsheet grid")
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
        self.wait_for_condition(
            lambda: "84" in self.app.child(name="Spreadsheet grid").description,
            description="formula result 84 in the spreadsheet grid",
        )

        snap = self.trigger_snapshot(aid)

        cells = {(c["row"], c["col"]): c for c in snap["sheet"]["cells"]}
        self.assertEqual(cells[(0, 0)]["value"], "42")
        self.assertIsNone(cells[(0, 0)]["formula"])
        self.assertEqual(cells[(1, 0)]["value"], "84")
        self.assertEqual(cells[(1, 0)]["formula"], "A1*2")
        self.assertIsNone(self.process.poll(), "tables crashed writing a snapshot")


class TablesCanvasCoordsMixin:
    """Screen coordinates for mouse journeys on the Tables grid.

    The grid's own layout constants (tables-core::sheet's ROW_HEADER_WIDTH
    50, COL_HEADER_HEIGHT 26, ROW_HEIGHT 28, COL_WIDTH 90) are fixed, but
    the canvas *origin* is not: it sits below the header/toolbar/name-box
    chrome, whose height moves with toolbar metrics (the 44sp touch targets
    in #118 shifted it). AT-SPI can't be asked either — it mis-reports the
    position of widgets nested in box containers (#132; the virtual cell
    nodes have the right size but the wrong position). So the app reports
    the canvas origin in the #104 test snapshot and these tests read it,
    instead of hardcoding a chrome height that silently rots. Under
    matchbox the window is fullscreen at 0,0, so the window-relative origin
    doubles as the screen origin (same assumption as DecksSelectionSmoke's
    canvas_at hint).
    """

    ROW_HEADER_WIDTH = 50
    COL_HEADER_HEIGHT = 26
    ROW_HEIGHT = 28
    COL_WIDTH = 90

    def canvas_origin(self) -> tuple[float, float]:
        origin = self.trigger_snapshot("org.tunaos.tables").get("grid_origin")
        self.assertIsNotNone(
            origin, "snapshot has no grid_origin — is the grid canvas realized?")
        return float(origin["x"]), float(origin["y"])


class TablesFillHandleSmoke(TablesCanvasCoordsMixin, BaseGUITestCase):
    """Fill handle (#113): dragging the handle at a selection's bottom-
    right corner tiles its content downward — a real mouse drag, not
    keyboard-only, verified against the #104 state snapshot rather than
    AT-SPI cell text/position (this grid's virtual cell nodes share the
    same container-position bridge gap as #132 — size is right, position
    isn't — so this test computes screen coordinates from the grid's own
    layout constants plus the snapshot-reported canvas origin instead of
    trusting AT-SPI node position)."""

    app_name = "tables"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="tables-fill-")
        super().setUp()

    def _cell_bottom_right(self, origin: tuple[float, float], row: int, col: int) -> tuple[float, float]:
        x = origin[0] + self.ROW_HEADER_WIDTH + (col + 1) * self.COL_WIDTH
        y = origin[1] + self.COL_HEADER_HEIGHT + (row + 1) * self.ROW_HEIGHT
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

        origin = self.canvas_origin()
        hx, hy = self._cell_bottom_right(origin, 0, 0)
        _, target_y = self._cell_bottom_right(origin, 3, 0)
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


class TablesSortIndicatorSmoke(TablesCanvasCoordsMixin, BaseGUITestCase):
    """Sort (#113's "visible criteria"): clicking a column header toggles
    sort on that column, drawn as a small triangle in the header — clicking
    again reverses direction, a third time clears it. Verified via the
    #104 state snapshot's sorted_col field, not AT-SPI (the header text
    nodes share the same virtual-cell fragility documented for
    TablesFillHandleSmoke and #137)."""

    app_name = "tables"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="tables-sort-")
        super().setUp()

    def _col_header_center(self, origin: tuple[float, float], col: int) -> tuple[float, float]:
        x = origin[0] + self.ROW_HEADER_WIDTH + col * self.COL_WIDTH + self.COL_WIDTH / 2
        y = origin[1] + self.COL_HEADER_HEIGHT / 2
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
        origin = self.canvas_origin()
        x, y = self._col_header_center(origin, 0)
        corner_x, corner_y = origin[0] + self.ROW_HEADER_WIDTH / 2, origin[1] + self.COL_HEADER_HEIGHT / 2

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


class TablesFilterSmoke(TablesCellEntryMixin, BaseGUITestCase):
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
        self._wait_for_a_new_document()

        for row, value in enumerate(["apple", "banana", "apple"]):
            self._put(f"A{row + 1}", value)

        # The snapshot is written on demand, so re-take it until it shows
        # the state being waited for. Sampling it once after a fixed 0.5s
        # is what made this journey fail with `hidden_rows: []` — the
        # filter had simply not been applied yet, which reads as "filtering
        # does not work".
        def snapshot():
            subprocess.run(["gapplication", "action", aid, "test-snapshot"])
            with open(self._snapshot_path) as f:
                return json.load(f)

        # Selection is on column A after the loop above — open the filter
        # dialog and filter to "apple".
        subprocess.run(["gapplication", "action", aid, "filter-by-column"])
        value_entry = self.wait_until(
            lambda: tree.root.findChild(
                lambda n: n.name == "Filter value" and n.roleName == "text"),
            lambda node: node is not None,
            description="the Filter by Column dialog",
        )
        value_entry.text = "apple"
        tree.root.findChild(
            lambda n: n.name == "Filter" and n.roleName == "push button").do_action(0)

        # Every other row is empty and so also fails the "apple" match —
        # only assert on the 3 rows this test actually populated. A
        # snapshot file that never appears surfaces here as the last
        # observed value, `<FileNotFoundError: ...>`.
        snap = self.wait_until(
            snapshot,
            lambda snap: 1 in set(snap["sheet"]["hidden_rows"]),
            interval=0.3,
            description="the filter to hide the non-matching row",
        )
        self.assertTrue(os.path.exists(self._snapshot_path), "snapshot file was not written")
        hidden = set(snap["sheet"]["hidden_rows"])
        self.assertIn(1, hidden, f"snapshot: {snap}")
        self.assertNotIn(0, hidden, f"snapshot: {snap}")
        self.assertNotIn(2, hidden, f"snapshot: {snap}")

        subprocess.run(["gapplication", "action", aid, "clear-filter"])
        self.wait_until(
            snapshot,
            lambda snap: snap["sheet"]["hidden_rows"] == [],
            interval=0.3,
            description="clear-filter to unhide every row",
        )
        self.assertIsNone(self.process.poll(), "tables crashed during filter/clear-filter")


class TablesNamedRangeSmoke(BaseGUITestCase):
    """Named ranges (#113): Define Name captures the current selection;
    typing that name into the name box (instead of a cell reference)
    jumps back to and re-selects the whole range. Verified via the #104
    state snapshot's selection field rather than AT-SPI cell text.

    Keeps to a near selection; the far-jump variant (Z9 and back, which
    grows then shrinks the grid's virtual-cell accessible extent) lives
    in test_jump_far_and_back_to_a_range_no_longer_crashes below and in
    TablesNamedRangeStatsSmoke. Both used to abort the app on the next
    AT-SPI interaction (#137/#507) — GTK's
    gtk_accessible_update_next_accessible_sibling() unrefs a parent
    accessible it does not own — and both now pass."""

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

    def test_jump_far_and_back_to_a_range_no_longer_crashes(self):
        """Regression guard for #137/#507: jumping to a cell far from
        the origin (growing the grid's virtual-cell AT-SPI extent), then
        back to a small selection (shrinking it), then any
        AT-SPI-registry interaction at all — here an ordinary D-Bus
        action call — used to kill the app. Reproduces with keyboard
        input alone, independent of named ranges, which is why this
        repro does not use Define Name."""
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
        self.assertIsNone(
            self.process.poll(),
            "tables died after a far jump and back — #137/#507 regression",
        )


class DecksSnapshotSmoke(BaseGUITestCase):
    """State-snapshot interface (#104), same mechanism as
    TablesSnapshotSmoke: adding objects is visible in the normalized
    JSON snapshot, not just the AT-SPI tree."""

    app_name = "decks"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="decks-snapshot-")
        super().setUp()

    def test_snapshot_reflects_added_objects(self):
        aid = "org.tunaos.decks"
        self.gapplication_action(aid, "new-document")
        self.gapplication_action(aid, "add-text-box")
        self.gapplication_action(aid, "add-shape")

        snap = self.trigger_snapshot(aid)

        self.assertEqual(snap["slide_count"], 1)
        kinds = [o["kind"] for o in snap["slides"][0]["objects"]]
        self.assertIn("TextBox", kinds)
        self.assertIn("Rect", kinds)
        self.assertIsNone(self.process.poll(), "decks crashed writing a snapshot")


class DecksSelectionSmoke(BaseGUITestCase):
    """Object selection updates the canvas a11y description and the
    inspector (fit-to-viewport geometry keeps coordinates stable)."""

    app_name = "decks"

    def test_click_selects_object(self):
        import re
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
        #
        # AT-SPI Component.position is broken for widgets nested in box
        # containers (upstream GTK4 bridge gap, #132): the bridge doesn't
        # accumulate ancestor allocation offsets. The app works around
        # this by embedding the canvas position (computed via
        # translate_coordinates relative to the toplevel) in the
        # accessible description as "canvas_at=x,y".
        canvas_for_geom = self.app.child(name="Slide canvas")
        cw, ch = canvas_for_geom.size
        match = re.search(r"canvas_at=(-?\d+),(-?\d+)", canvas_for_geom.description)
        if not match:
            self.fail(f"canvas_at not found in description: {canvas_for_geom.description!r}")
        cx, cy = int(match.group(1)), int(match.group(2))
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
        self.relaunch_app(crash=True)
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


class LettersStructuredEditingSmoke(BaseGUITestCase):
    """Structured editing: tables, lists, and page layout journeys (#110).

    These assert the *document*, not the editor's text. Asserting text let
    the insert-table journey pass while the action wrote a literal
    "| Header 1 |" grid into the buffer and separately appended a second,
    real table to the model — two tables and a paragraph of pipes, from one
    Insert Table (#438). A model assertion cannot be satisfied that way.
    """

    app_name = "letters"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="letters-structured-")
        super().setUp()

    def _tables(self, snapshot):
        """{table id: {(row, col): cell text}} from a document snapshot."""
        tables = {}
        for para in snapshot["paragraphs"]:
            cell = para["style"].get("table_cell")
            if not cell:
                continue
            text = "".join(run["text"] for run in para["runs"])
            tables.setdefault(cell["table"], {})[(cell["row"], cell["col"])] = text
        return tables

    def _prose(self, snapshot):
        return [
            "".join(run["text"] for run in para["runs"])
            for para in snapshot["paragraphs"]
            if not para["style"].get("table_cell")
        ]

    def test_insert_table_and_structured_actions(self):
        from dogtail import rawinput

        self.wait_for_node(name="New Document", roleName="push button").do_action(0)
        self.wait_for_node(roleName="text")
        # The word count is the app's own confirmation that the editor
        # exists and is taking input. Without waiting for it, a slow tab
        # creation meant the keystrokes — and then insert-table, which
        # needs an active buffer — arrived before there was a document to
        # put them in, and the journey failed further down with a
        # misleading "no table" timeout.
        self.wait_for_node(name="0 words", roleName="label")
        rawinput.typeText("intro")
        self.wait_for_node(name="1 word", roleName="label")

        self.gapplication_action("org.tunaos.letters", "insert-table")

        def table_snapshot():
            snapshot = self.trigger_snapshot("org.tunaos.letters")
            return snapshot if self._tables(snapshot) else None

        snapshot = self.wait_for_condition(
            table_snapshot, description="a table in the document")
        tables = self._tables(snapshot)
        self.assertEqual(len(tables), 1, f"one Insert Table must make one table: {tables}")
        cells = next(iter(tables.values()))
        self.assertEqual(
            sorted(cells), [(r, c) for r in range(3) for c in range(3)],
            "a 3x3 grid, addressed by row and column",
        )
        self.assertTrue(all(text == "" for text in cells.values()),
                        f"new cells start empty, ready to type in: {cells}")
        self.assertIn("intro", self._prose(snapshot),
                      "the paragraph the user was writing must survive")
        self.assertFalse([p for p in self._prose(snapshot) if "|" in p],
                         "no literal pipe grid may be left behind as prose")

        # What the user sees: the buffer has no table widget, so a table is
        # shown as a pipe grid — the first row is the header, so a 3x3
        # table is a header line, a delimiter line and two body lines —
        # directly below the paragraph they were writing.
        shown = self.wait_for_node(roleName="text").text
        self.assertEqual(
            shown.splitlines(),
            ["intro", "|  |  |  |", "| --- | --- | --- |", "|  |  |  |", "|  |  |  |"],
            f"unexpected editor contents: {shown!r}",
        )

        # A row added from inside the table grows that table.
        self.gapplication_action("org.tunaos.letters", "table-insert-row-below")
        grown = self.wait_for_condition(
            lambda: self.trigger_snapshot("org.tunaos.letters"),
            description="a snapshot after adding a row",
        )
        rows = {row for (row, _col) in next(iter(self._tables(grown).values()))}
        self.assertEqual(sorted(rows), [0, 1, 2, 3], "insert-row-below adds one row")

        self.assertIsNone(self.process.poll(), "letters crashed during structured editing actions")

    def test_list_and_page_break_apply_to_the_cursors_paragraph(self):
        """List and page-break commands edit where the caret is (#438).

        Each of these used to write its own marker text into the buffer and
        set the style on paragraph 0, so the document collected literal
        bullets and "---" lines while the paragraph the user was on kept
        its old style.
        """
        from dogtail import rawinput

        self.wait_for_node(name="New Document", roleName="push button").do_action(0)
        self.wait_for_node(roleName="text")
        self.wait_for_node(name="0 words", roleName="label")
        rawinput.typeText("first")
        rawinput.keyCombo("Return")
        rawinput.typeText("second")
        self.wait_for_node(name="2 words", roleName="label")

        self.gapplication_action("org.tunaos.letters", "bullet-list")
        self.gapplication_action("org.tunaos.letters", "list-indent")
        self.gapplication_action("org.tunaos.letters", "insert-page-break")

        def second_is_an_indented_bullet():
            snapshot = self.trigger_snapshot("org.tunaos.letters")
            styles = [p["style"] for p in snapshot["paragraphs"]]
            if len(styles) != 2 or styles[1].get("list") != "Bullet":
                return None
            return snapshot

        snapshot = self.wait_for_condition(
            second_is_an_indented_bullet,
            description="the caret's paragraph becoming a bullet item")

        first, second = (p["style"] for p in snapshot["paragraphs"])
        texts = ["".join(r["text"] for r in p["runs"]) for p in snapshot["paragraphs"]]
        self.assertEqual(texts, ["first", "second"],
                         "markers and page breaks are style, never document text")
        self.assertEqual(first.get("list"), "None", "the first paragraph was not the caret's")
        self.assertEqual(second.get("list_level"), 1, "list-indent nests one level")
        self.assertTrue(second.get("page_break_before"), "the break lands on the caret's paragraph")
        self.assertFalse(first.get("page_break_before"))

        # On screen: exactly one marker, indented one level (four spaces).
        shown = self.wait_for_node(roleName="text").text
        self.assertEqual(shown.splitlines(), ["first", "    - second"],
                         f"unexpected editor contents: {shown!r}")

        self.gapplication_action("org.tunaos.letters", "list-outdent")
        outdented = self.wait_for_condition(
            lambda: self.trigger_snapshot("org.tunaos.letters"),
            description="a snapshot after outdenting")
        self.assertEqual(outdented["paragraphs"][1]["style"].get("list_level"), 0)
        self.assertIsNone(self.process.poll(), "letters crashed during list editing")


class _SettingsIsolationProbe:
    """A journey's settings must not reach the next journey, or the machine.

    Only 2 of 38 journey classes used to isolate GSettings, so the rest
    launched against the real XDG stores — and dconf keeps its storage in
    `$XDG_CONFIG_HOME/dconf/user`, which the private D-Bus session does not
    redirect. Measured with this pair before isolation became a default:

        A: starts at false    <- left over from a *previous run*
        B: starts at false    <- A's change leaked into B

    and after:

        A: starts at true
        B: starts at true

    So a journey that touched a preference rewrote the machine's real
    settings and every later journey inherited them. CI's fresh runners
    hid it; the stress campaign's randomised order would have surfaced it
    as a flake with no visible cause (#354).

    Two classes rather than two methods because the leak crossed journeys.
    Each assertion stands alone — both expect the schema default — so
    neither depends on the other having run, while together they catch the
    regression.
    """

    app_name = "letters"

    SETTING = ("org.tunaos.letters", "show-toolbar")

    def _read(self):
        """The value as the launched app sees it, not as this process does."""
        import subprocess
        env = os.environ.copy()
        env.update(self.launch_env)
        result = subprocess.run(["gsettings", "get", *self.SETTING],
                                capture_output=True, text=True, env=env)
        return (result.stdout.strip() or result.stderr.strip())

    def _start_clean_then_change(self):
        import subprocess
        self.assertEqual(self._read(), "true",
                         "this journey did not start from the schema default")
        env = os.environ.copy()
        env.update(self.launch_env)
        subprocess.run(["gsettings", "set", *self.SETTING, "false"],
                       env=env, check=True)
        self.assertEqual(self._read(), "false", "the change did not take")


class LettersSettingsIsolationSmoke(_SettingsIsolationProbe, BaseGUITestCase):
    """First half: start from the default, then change it."""

    def test_a_journey_starts_from_the_schema_default_and_may_change_it(self):
        self._start_clean_then_change()


class LettersSettingsIsolationFollowerSmoke(_SettingsIsolationProbe, BaseGUITestCase):
    """Second half: the journey after one that changed a setting."""

    def test_a_later_journey_does_not_inherit_the_change(self):
        self.assertEqual(
            self._read(), "true",
            "a previous journey's setting reached this one — isolation is not "
            "in effect, so journeys share (and rewrite) real user settings",
        )


class SuitePlatformIntegrationSmoke(BaseGUITestCase):
    """Platform integration: recent files, templates, help, and shortcuts (#119)."""

    app_name = "letters"

    def test_help_and_template_actions(self):
        import subprocess

        # Help dialog action
        subprocess.run(["gapplication", "action", "org.tunaos.letters", "help"])
        time.sleep(1.0)

        # Shortcuts dialog action
        subprocess.run(["gapplication", "action", "org.tunaos.letters", "show-shortcuts"])
        time.sleep(1.0)

        # Clear recent files action
        subprocess.run(["gapplication", "action", "org.tunaos.letters", "clear-recent-files"])
        time.sleep(0.5)

        self.assertIsNone(self.process.poll(), "letters crashed during platform integration actions")


class LettersSaveFormatSmoke(BaseGUITestCase):
    """Real GTK journey: the extension the user types decides the bytes.

    Letters used to resolve the save format with a two-arm match and a
    Markdown catch-all, so `notes.txt` received `# Heading` and `**bold**`
    and `page.html` — pre-filled by the "HTML" default-format preference —
    received Markdown too (#436). These journeys go through the real Save
    As chooser, because the defect was in what the app did with the name
    the chooser returned, not in the writer underneath.
    """

    app_name = "letters"

    def setUp(self):
        self._dir = self.temp_dir(prefix="letters-save-format-")
        self._doc = os.path.join(self._dir, "journey.md")
        with open(self._doc, "w") as f:
            f.write("# Quarterly report\n\n**bold** and plain\n")
        self.launch_args = [self._doc]
        super().setUp()

    def _save_as(self, name):
        """Drive Save As to `name` inside this test's directory."""
        from dogtail import tree

        out_path = os.path.join(self._dir, name)
        self.gapplication_action("org.tunaos.letters", "save-file-as")
        name_entry = self.wait_for_condition(
            lambda: tree.root.findChild(
                lambda n: n.name == "Name:" and n.roleName == "text"
            ),
            description="the Save As name entry",
        )
        name_entry.text = out_path
        time.sleep(0.3)
        tree.root.findChild(
            lambda n: n.name == "Save" and n.roleName == "push button"
        ).do_action(0)
        return out_path

    def test_saving_as_plain_text_writes_text_not_markdown(self):
        # The document really is rich: markdown opened into headings and a
        # bold run, so the editor shows no markup.
        editor = self.app.child(roleName="text")
        self.assertNotIn("**", editor.text, f"editor text: {editor.text!r}")

        out_path = self._save_as("notes.txt")
        self.wait_for_file(out_path)
        # The dialog reports the formatting the format cannot hold, rather
        # than dropping it silently.
        self.wait_for_node(name="Saved, with formatting this format cannot hold")

        with open(out_path) as f:
            saved = f.read()
        self.assertIn("Quarterly report", saved, f"saved file: {saved!r}")
        self.assertIn("bold and plain", saved, f"saved file: {saved!r}")
        self.assertNotIn("**", saved, f"markdown emphasis leaked into .txt: {saved!r}")
        self.assertNotIn("#", saved, f"markdown heading leaked into .txt: {saved!r}")
        self.assertIsNone(self.process.poll(), "letters exited during a save")

    def test_saving_as_html_writes_html_not_markdown(self):
        out_path = self._save_as("page.html")
        self.wait_for_file(out_path)

        with open(out_path) as f:
            saved = f.read()
        self.assertTrue(
            saved.startswith("<!DOCTYPE html>"), f"saved file: {saved[:80]!r}"
        )
        self.assertIn("<h1>Quarterly report</h1>", saved, f"saved file: {saved!r}")
        self.assertIn("<strong>bold</strong>", saved, f"saved file: {saved!r}")
        self.assertNotIn("**bold**", saved, f"markdown leaked into .html: {saved!r}")
        self.assertIsNone(self.process.poll(), "letters exited during a save")

    def test_an_unwritable_format_is_refused_and_writes_nothing(self):
        # RTF was offered as a default format with no writer behind it. An
        # extension Letters cannot write must now say so and leave the disk
        # alone, instead of putting Markdown under that name.
        out_path = self._save_as("report.rtf")
        self.wait_for_node(name="Could not save document")
        self.assertFalse(
            os.path.exists(out_path), "a format with no writer was written anyway"
        )
        self.assertIsNone(self.process.poll(), "letters exited on a refused save")


class HarnessRepeatedLaunchSmoke(BaseGUITestCase):
    """Launching and closing the same app repeatedly must leave exactly one
    live copy behind, never a pile of them (#354).

    The roadmap asks for the harness's own isolation to be tested with
    repeated launch/close, and the reason is specific: the apps are
    `GtkApplication`s, so the first surviving copy owns the bus name and
    every later launch hands its window back to *that* process. A journey
    then passes or fails against a process some earlier test started, and
    the failure surfaces somewhere else entirely.

    The close is a real one (SIGTERM, so the app runs its own shutdown),
    not a kill, because a shutdown path that hangs is the way a copy
    survives in practice.
    """

    app_name = "letters"
    CYCLES = 4

    def test_repeated_launch_and_close_never_accumulates_copies(self):
        from framework.owned_processes import owned

        pids = [self.process.pid]
        self.assertIsNotNone(self.app.child(roleName="frame"))

        for cycle in range(1, self.CYCLES + 1):
            previous = self.process
            self.relaunch_app()

            self.assertIsNotNone(
                previous.poll(), f"cycle {cycle}: the previous copy is still running"
            )
            self.assertIsNone(
                self.process.poll(), f"cycle {cycle}: the new copy exited at startup"
            )
            self.assertNotIn(
                self.process.pid, pids, f"cycle {cycle}: reused pid, so nothing restarted"
            )
            pids.append(self.process.pid)

            # A window per launch, and it must be this launch's window: a
            # frame handed back by a surviving older copy is the exact
            # failure being tested for, and it looks identical from here
            # apart from which process is alive.
            self.assertIsNotNone(
                self.app.child(roleName="frame"), f"cycle {cycle}: no window after relaunch"
            )

            # The harness may only reap what it registered (#241), so an
            # unregistered replacement is a leak with no owner. This is the
            # assertion that fails if a relaunch stops registering.
            live = owned(self.app_name)
            self.assertEqual(
                [process.pid for process in live],
                [self.process.pid],
                f"cycle {cycle}: the harness owns {len(live)} live copies, expected 1",
            )

        self.assertEqual(len(set(pids)), self.CYCLES + 1)


class TablesChartDialogSmoke(BaseGUITestCase):
    """The chart dialog opens, previews the selected column, and inserts.

    Added with the extraction of that dialog out of `TablesWindow::new`
    (`tables/src/chart_dialog.rs`): moving 119 lines of dialog construction
    to another module is exactly the change that can leave a dialog that
    compiles and never appears, and nothing was asserting this one. It
    needs numeric data in the selected column — the dialog returns without
    presenting when there is none, which is also worth pinning.
    """

    app_name = "tables"

    def test_insert_chart_opens_the_dialog_and_inserts_onto_the_sheet(self):
        from dogtail import rawinput
        import subprocess

        aid = "org.tunaos.tables"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)

        # Labels in column A, numbers in column B: the dialog charts the
        # selected column against column A.
        for cell, text in (("A1", "North"), ("A2", "South"), ("A3", "East")):
            rawinput.keyCombo("<Control>g")
            time.sleep(0.2)
            rawinput.typeText(cell)
            rawinput.keyCombo("Return")
            time.sleep(0.3)
            rawinput.typeText(text)
            rawinput.keyCombo("Return")
            time.sleep(0.3)
        for cell, text in (("B1", "12"), ("B2", "34"), ("B3", "56")):
            rawinput.keyCombo("<Control>g")
            time.sleep(0.2)
            rawinput.typeText(cell)
            rawinput.keyCombo("Return")
            time.sleep(0.3)
            rawinput.typeText(text)
            rawinput.keyCombo("Return")
            time.sleep(0.3)
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("B2")
        rawinput.keyCombo("Return")
        time.sleep(0.4)
        rawinput.keyCombo("Escape")
        time.sleep(0.3)

        subprocess.run(["gapplication", "action", aid, "insert-chart"])

        insert = self.wait_until(
            lambda: [c for c in self.app.findChildren(
                lambda c: c.roleName == "push button"
                and "Insert into Sheet" in (c.name or ""))],
            lambda found: bool(found),
            timeout=10.0,
            description="the chart dialog's Insert button",
        )[0]
        # The type chooser is the dialog's other half; if only the button
        # showed up, the dialog was built but not populated.
        types = [c for c in self.app.findChildren(
            lambda c: c.roleName == "combo box")]
        self.assertTrue(types, "chart dialog has no type chooser")

        insert.do_action(0)
        time.sleep(1.0)
        self.assertIsNone(self.process.poll(), "tables crashed inserting a chart")
        # Inserting closes the dialog, which is how the action reports that
        # it wrote a chart onto the sheet.
        self.wait_until(
            lambda: [c for c in self.app.findChildren(
                lambda c: c.roleName == "push button"
                and "Insert into Sheet" in (c.name or ""))],
            lambda found: not found,
            timeout=10.0,
            description="the chart dialog to close after inserting",
        )
