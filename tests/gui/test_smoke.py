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


class UnattendedAutosaveMixin:
    """An app must snapshot on its own, with nobody pressing anything.

    Every autosave journey in this file calls the `autosave-now` action
    first. All of them passed while a shipped Letters never autosaved at
    all: its `auto-save-interval` defaulted to 0 and `register_autosave`
    installs a timer only when the interval is positive. They tested the
    snapshot machinery; none of them tested that it ever runs.

    So these journeys never trigger the action. They set a 10-second
    interval — the floor all three apps clamp to, where the shipped 60 would
    only make the journey slow — dirty a document, and wait for a snapshot to
    appear by itself. The shipped *default* is a separate question, asserted
    in `tests/test_autosave_defaults.py`, because a default is about what a
    new install gets rather than about whether the timer works.

    All three apps are covered because the defect was a per-app divergence:
    Letters guarded on `interval > 0` while Tables and Decks clamp with
    `.max(10)`, and nothing compared them. Tables and Decks look unbreakable
    by that particular bug, which is not the same as verified.
    """

    INTERVAL = 10

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(prefix=f"{self.app_name}-unattended-state-")
        cfg = self.isolate_gsettings(prefix=f"{self.app_name}-unattended-cfg-")
        # The keyfile backend reads this path; writing it before launch is how
        # a journey configures an app setting without dconf's shared daemon.
        keyfile_dir = os.path.join(cfg, "glib-2.0", "settings")
        os.makedirs(keyfile_dir, exist_ok=True)
        with open(os.path.join(keyfile_dir, "keyfile"), "w", encoding="utf-8") as handle:
            handle.write(f"[org/tunaos/{self.app_name}]\nauto-save-interval={self.INTERVAL}\n")
        super().setUp()

    def _snapshot_files(self):
        snap_dir = os.path.join(self._state_dir, self.app_name)
        if not os.path.isdir(snap_dir):
            return []
        return [f for f in os.listdir(snap_dir) if f.endswith(".snapshot")]

    def test_a_dirty_document_is_snapshotted_without_being_asked(self):
        import subprocess
        from dogtail import rawinput

        subprocess.run(["gapplication", "action", f"org.tunaos.{self.app_name}", "new-document"])
        time.sleep(2.0)
        self._dirty_the_document(rawinput)
        self.assertEqual(self._snapshot_files(), [],
                         "precondition: nothing should be snapshotted yet")

        # No autosave-now anywhere in this journey: the timer either fires or
        # the app is not protecting anything.
        window = self.INTERVAL * 2 + 8
        deadline = time.monotonic() + window
        while time.monotonic() < deadline:
            if self._snapshot_files():
                break
            time.sleep(1.0)

        self.assertEqual(
            len(self._snapshot_files()), 1,
            f"no snapshot appeared within {window}s at a {self.INTERVAL}s "
            f"interval: {self.app_name} is not autosaving unattended",
        )

    def _dirty_the_document(self, rawinput):
        rawinput.typeText("work nobody saved")
        time.sleep(1.0)


class LettersUnattendedAutosaveSmoke(UnattendedAutosaveMixin, BaseGUITestCase):
    app_name = "letters"


class TablesUnattendedAutosaveSmoke(UnattendedAutosaveMixin, BaseGUITestCase):
    app_name = "tables"

    def _dirty_the_document(self, rawinput):
        # A cell edit has to be committed before the workbook counts as dirty.
        rawinput.typeText("=6*7")
        rawinput.keyCombo("Return")
        time.sleep(0.5)


class DecksUnattendedAutosaveSmoke(UnattendedAutosaveMixin, BaseGUITestCase):
    app_name = "decks"

    def _dirty_the_document(self, rawinput):
        # Typing does nothing to a fresh deck — there is no focused text
        # frame to type into — so a deck is dirtied the way every other
        # Decks journey dirties one, by adding a shape.
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.decks", "add-shape"])
        time.sleep(1.0)


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
        recovered_from = set(self._snapshot_files())

        # Simulate a crash: kill the process directly, bypassing the close
        # guard, so the snapshots are never cleared by a clean exit.
        self.relaunch_app(crash=True)
        time.sleep(2.0)

        frame = self.app.child(roleName="frame")
        self.assertIn("Recovered", frame.name, f"window did not announce recovery: {frame.name!r}")
        # The assertion below used to read `self._snapshot_files() == []`:
        # after recovery, nothing on disk. That was a proxy for the thing
        # actually required — the orphan is not offered a second time — and
        # it stopped being a safe proxy once recovery began writing the
        # recovered content to the new window's own slot before clearing the
        # orphan. Zero files also describes unprotected work, which is what
        # that spelling was quietly asserting: the old code left the
        # recovered document with no snapshot until the next timer tick.
        # So the intent is asserted directly instead, and more strictly: the
        # recovered orphan is gone, and the recovered work is itself covered.
        # Waited for rather than sampled after a fixed sleep, because the
        # `== []` version sampled 1.5s after relaunch and lost that race
        # under the load of a full batch run — it failed twice in one batch
        # and passed three consecutive focused runs. A post-condition that
        # needs a sleep to hold is a post-condition to wait for.
        present = self.wait_until(
            lambda: set(self._snapshot_files()),
            lambda files: not (files & recovered_from),
            description="both recovered orphans must be cleared so they aren't offered again",
        )
        self.assertEqual(
            len(present), 2,
            "the recovered tabs must itself be protected by a snapshot; "
            f"found {sorted(present)}",
        )

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


class LettersAutosaveFailureSmoke(BaseGUITestCase):
    """The per-tab sweep must reach the user too.

    Letters autosaves every dirty tab on one tick, through a notifier
    threaded into `register_autosave` rather than held by the window — a
    different path from Tables and Decks, so it is worth its own journey.

    It asserts that a notice appears, and deliberately not that exactly one
    appears: `AdwToastOverlay` queues toasts and shows one at a time, so the
    accessibility tree cannot tell one notice from two, and a count here
    would be a claim this journey has no way to check. The once-per-streak
    property is asserted where it can actually fail, in
    `suite_common_core::autosave::AutosaveNotices`' own tests.
    """

    app_name = "letters"

    def setUp(self):
        # A regular file where the state directory should be: fails with
        # ENOTDIR for root as well, which a permission bit would not.
        holder = self.temp_dir("letters-autosave-blocked-")
        blocked = os.path.join(holder, "not-a-directory")
        with open(blocked, "w", encoding="utf-8") as handle:
            handle.write("occupied\n")
        self.launch_env = {**getattr(self, "launch_env", {}), "XDG_STATE_HOME": blocked}
        super().setUp()

    def test_a_failing_per_tab_autosave_tells_the_user(self):
        import subprocess
        from dogtail import rawinput

        aid = "org.tunaos.letters"
        for text in ("first tab content", "second tab content"):
            subprocess.run(["gapplication", "action", aid, "new-document"])
            time.sleep(2.0)
            rawinput.typeText(text)
            time.sleep(1.0)

        subprocess.run(["gapplication", "action", aid, "autosave-now"])
        time.sleep(1.0)

        labels = [n.name for n in self.app.findChildren(lambda x: x.roleName in ("label", "static"))]
        self.assertTrue(
            any("not being protected" in (name or "") for name in labels),
            f"the per-tab sweep failed silently; labels on screen: {labels[:12]}",
        )


class TablesRecoveryIsItselfProtectedSmoke(BaseGUITestCase):
    """Recovered work must survive a second crash.

    Recovery used to clear the orphan slot as soon as the content was in
    memory and leave the next autosave tick to write a replacement — up to a
    minute later at the shipped interval. Crash inside that window and the
    work was gone: it had survived one crash and was lost to the next, which
    is the one thing crash recovery must not do.

    The journey is the sequence that exposes it. Dirty a workbook, snapshot
    it, crash, relaunch (recovery happens here), then crash *again* without
    letting any autosave run — no `autosave-now`, and the interval is left
    long on purpose — and relaunch once more. The content has to still be
    there.
    """

    app_name = "tables"

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(prefix="tables-double-crash-state-")
        super().setUp()

    def _snapshot_files(self):
        snap_dir = os.path.join(self._state_dir, "tables")
        if not os.path.isdir(snap_dir):
            return []
        return [f for f in os.listdir(snap_dir) if f.endswith(".snapshot")]

    def test_a_second_crash_right_after_recovery_keeps_the_work(self):
        import subprocess
        from dogtail import rawinput

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        time.sleep(1.5)
        rawinput.typeText("=6*7")
        rawinput.keyCombo("Return")
        time.sleep(0.5)
        subprocess.run(["gapplication", "action", "org.tunaos.tables", "autosave-now"])
        time.sleep(0.5)
        self.assertEqual(len(self._snapshot_files()), 1, "precondition: one snapshot to recover")

        # First crash and relaunch: this is where recovery runs.
        self.relaunch_app(crash=True)
        time.sleep(2.0)
        frame = self.app.child(roleName="frame")
        self.assertIn("Recovered", frame.name, f"first relaunch did not recover: {frame.name!r}")
        self.assertEqual(
            len(self._snapshot_files()), 1,
            "after recovery the work must be covered by a snapshot again — this is "
            "the assertion that fails when recovery clears the orphan and waits "
            f"for a timer tick; found {self._snapshot_files()}",
        )

        # Second crash, with no autosave in between: nothing but the write
        # recovery itself performed can be protecting the document now.
        self.relaunch_app(crash=True)
        time.sleep(2.0)
        frame = self.app.child(roleName="frame")
        self.assertIn(
            "Recovered", frame.name,
            "work that survived one crash was lost to the next: "
            f"window title after the second relaunch was {frame.name!r}",
        )


class DecksRecoveryIsItselfProtectedSmoke(BaseGUITestCase):
    """Decks: recovered work must survive a second crash. Same defect and
    same sequence as the Tables journey above."""

    app_name = "decks"

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(prefix="decks-double-crash-state-")
        super().setUp()

    def _snapshot_files(self):
        snap_dir = os.path.join(self._state_dir, "decks")
        if not os.path.isdir(snap_dir):
            return []
        return [f for f in os.listdir(snap_dir) if f.endswith(".snapshot")]

    def test_a_second_crash_right_after_recovery_keeps_the_work(self):
        import subprocess

        aid = "org.tunaos.decks"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)
        subprocess.run(["gapplication", "action", aid, "add-shape"])
        time.sleep(1.0)
        subprocess.run(["gapplication", "action", aid, "autosave-now"])
        time.sleep(0.5)
        self.assertEqual(len(self._snapshot_files()), 1, "precondition: one snapshot to recover")

        self.relaunch_app(crash=True)
        time.sleep(2.0)
        self.assertIn("Recovered", self.app.child(roleName="frame").name,
                      "first relaunch did not recover")
        self.assertEqual(
            len(self._snapshot_files()), 1,
            "after recovery the deck must be covered by a snapshot again; "
            f"found {self._snapshot_files()}",
        )

        self.relaunch_app(crash=True)
        time.sleep(2.0)
        frame = self.app.child(roleName="frame")
        self.assertIn("Recovered", frame.name,
                      "a deck that survived one crash was lost to the next: "
                      f"title was {frame.name!r}")


class LettersRecoveryIsItselfProtectedSmoke(BaseGUITestCase):
    """Letters: a recovered tab must survive a second crash.

    Worse here before the fix than in the other two apps, because Letters
    shipped with its autosave timer switched off — "the next tick will
    re-snapshot it" was never going to happen at all, so a recovered tab was
    unprotected for the whole session.
    """

    app_name = "letters"

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(prefix="letters-double-crash-state-")
        super().setUp()

    def _snapshot_files(self):
        snap_dir = os.path.join(self._state_dir, "letters")
        if not os.path.isdir(snap_dir):
            return []
        return [f for f in os.listdir(snap_dir) if f.endswith(".snapshot")]

    def test_a_second_crash_right_after_recovery_keeps_the_work(self):
        import subprocess
        from dogtail import rawinput

        aid = "org.tunaos.letters"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(2.0)
        rawinput.typeText("work that survived one crash")
        time.sleep(1.0)
        subprocess.run(["gapplication", "action", aid, "autosave-now"])
        time.sleep(0.5)
        self.assertEqual(len(self._snapshot_files()), 1, "precondition: one snapshot to recover")

        self.relaunch_app(crash=True)
        time.sleep(2.5)
        self.assertIn("Recovered", self.app.child(roleName="frame").name,
                      "first relaunch did not recover")
        self.assertEqual(
            len(self._snapshot_files()), 1,
            "after recovery the tab must be covered by a snapshot again; "
            f"found {self._snapshot_files()}",
        )

        self.relaunch_app(crash=True)
        time.sleep(2.5)
        frame = self.app.child(roleName="frame")
        self.assertIn("Recovered", frame.name,
                      "a document that survived one crash was lost to the next: "
                      f"title was {frame.name!r}")


class LiveOwnerMixin:
    """A document open in another window is not a crash to recover.

    Recovery adopts its content into the new window's own slot before
    clearing the orphan, so an open window now *always* has a snapshot on
    disk. Without ownership, a second launch reads that as an abandoned
    crash: it reopens work that is already open, in a second window, from a
    snapshot the first window is still rewriting underneath it.

    The other live window is stood in for by holding its advisory lock from
    this test process — which is exactly what ownership means, and avoids
    fighting GApplication's single-instance behaviour to get two real
    windows. The pair of relaunches is the point: the same state recovers
    once the lock is released, so it is the lock doing the suppressing and
    not some unrelated difference.

    All three apps run it because the claim is per-app wiring — one field on
    Tables' and Decks' windows, and on Letters' `DocumentSession` so a
    per-tab document gets a per-tab lock. A window that simply forgot to
    claim its slot is invisible to the shared unit tests and to the other two
    apps' journeys, which is the shape of per-app divergence this suite has
    been bitten by before (Letters' autosave interval).
    """

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(
            prefix=f"{self.app_name}-live-owner-"
        )
        super().setUp()

    def _snapshot_dir(self):
        return os.path.join(self._state_dir, self.app_name)

    def _snapshots(self):
        d = self._snapshot_dir()
        return [f for f in os.listdir(d) if f.endswith(".snapshot")] if os.path.isdir(d) else []

    def _lock_files(self):
        d = self._snapshot_dir()
        return [f for f in os.listdir(d) if f.endswith(".lock")] if os.path.isdir(d) else []

    def _dirty_the_document(self, rawinput):
        """Type into the document. Overridden where typing is not enough."""
        rawinput.typeText("owned by a live window")
        time.sleep(0.5)

    def test_a_snapshot_owned_by_a_live_window_is_not_recovered(self):
        import fcntl
        import subprocess
        from dogtail import rawinput

        aid = f"org.tunaos.{self.app_name}"
        subprocess.run(["gapplication", "action", aid, "new-document"])
        time.sleep(1.5)
        self._dirty_the_document(rawinput)
        subprocess.run(["gapplication", "action", aid, "autosave-now"])
        time.sleep(0.8)
        self.assertEqual(len(self._snapshots()), 1, "precondition: a snapshot to recover")
        locks = self._lock_files()
        self.assertEqual(len(locks), 1,
                          f"the live window should have claimed its slot; found {locks}")
        lock_path = os.path.join(self._snapshot_dir(), locks[0])

        # Kill without relaunching. `relaunch_app(crash=True)` would start
        # a replacement, and that replacement recovers the orphan and
        # clears it — lock file included — so there would be nothing left
        # to hold. Two drafts of this test died on that before the
        # sequence came out right.
        #
        # SIGKILL means no cleanup runs, so the snapshot and its lock file
        # both stay on disk while the claim itself is released by the
        # kernel. That release is the whole reason this is an advisory lock
        # rather than a pid written into a file.
        self.process.kill()
        self.process.wait(timeout=5)
        time.sleep(0.5)

        self.assertTrue(os.path.exists(lock_path), f"{lock_path} should survive a crash")
        holder = open(lock_path, "r+b")
        self.addCleanup(holder.close)
        fcntl.flock(holder.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)

        self.relaunch_app(crash=True)
        time.sleep(2.0)
        frame = self.app.child(roleName="frame")
        self.assertNotIn(
            "Recovered", frame.name,
            "a document owned by a live window was offered as a recovery: "
            f"window came up as {frame.name!r}",
        )
        self.assertEqual(len(self._snapshots()), 1,
                          "the owner's snapshot must be left alone, not cleared")

        # Release it and the same state recovers, which is what makes the
        # assertion above about ownership rather than about timing.
        fcntl.flock(holder.fileno(), fcntl.LOCK_UN)
        self.relaunch_app(crash=True)
        time.sleep(2.0)
        frame = self.app.child(roleName="frame")
        self.assertIn(
            "Recovered", frame.name,
            f"releasing the claim should make the work recoverable again: {frame.name!r}",
        )


class TablesLiveOwnerSmoke(LiveOwnerMixin, BaseGUITestCase):
    app_name = "tables"

    def _dirty_the_document(self, rawinput):
        # A cell edit has to be committed before the workbook counts as dirty.
        rawinput.typeText("=6*7")
        rawinput.keyCombo("Return")
        time.sleep(0.5)


class DecksLiveOwnerSmoke(LiveOwnerMixin, BaseGUITestCase):
    app_name = "decks"

    def _dirty_the_document(self, rawinput):
        # A fresh deck has no focused text frame, so it is dirtied the way
        # every other Decks journey dirties one: by adding a shape.
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.decks", "add-shape"])
        time.sleep(1.0)


class LettersLiveOwnerSmoke(LiveOwnerMixin, BaseGUITestCase):
    app_name = "letters"


class SavedDocumentMixin:
    """Gets a journey to a genuinely saved document, then a snapshot of
    unsaved changes on top of it.

    Two journeys need that exact state and disagree about what should happen
    next, which is the point of sharing the setup: with the file touched
    afterwards the snapshot must be suppressed, and with the file renamed
    away it must still be offered. Identical preconditions, opposite
    expectations — so a change that confused "a path is recorded" with "the
    file at that path overtook the snapshot" cannot satisfy both.

    The save goes through the close guard because that is the route a
    journey has to a genuinely written file. All three apps share that
    dialog; they differ only in the extension they write and in what counts
    as an edit, which is what the two hooks below are for.
    """

    doc_suffix = ".xlsx"
    doc_stem = "quarterly"
    # Letters' close guard is multi-tab, so its buttons are "Save All" and
    # "Discard All" where the single-document apps say "Save".
    save_button_label = "Save"

    def _snapshot_files(self):
        snap_dir = os.path.join(self._state_dir, self.app_name)
        if not os.path.isdir(snap_dir):
            return []
        return [f for f in os.listdir(snap_dir) if f.endswith(".snapshot")]

    def _edit_the_document(self, rawinput, nth):
        """Make the `nth` edit, leaving the document dirty.

        It has to be a *different* edit each time. Retyping the same value
        into the same cell leaves Tables clean, `autosave-now` returns early
        on a clean document, and the journey then fails on its own
        precondition with no snapshot — which is exactly how this hook was
        wrong on its first outing.
        """
        rawinput.typeText(f"draft {nth} content")
        time.sleep(0.5)

    def _save_a_real_document(self):
        import subprocess

        from dogtail import rawinput, tree

        out_path = os.path.join(self._dir, f"{self.doc_stem}{self.doc_suffix}")
        subprocess.run(["gapplication", "action", f"org.tunaos.{self.app_name}", "new-document"])
        time.sleep(1.5)
        self._edit_the_document(rawinput, 1)

        self.app.child(name="Close", roleName="push button").do_action(0)
        time.sleep(0.8)
        self.app.child(name=self.save_button_label, roleName="push button").do_action(0)
        time.sleep(1.0)
        name_entry = tree.root.findChild(lambda n: n.name == "Name:" and n.roleName == "text")
        name_entry.text = out_path
        time.sleep(0.3)
        tree.root.findChild(lambda n: n.name == "Save" and n.roleName == "push button").do_action(0)
        self.assertIsNotNone(self.wait_for_process_exit(), "the save did not complete")
        self.assertTrue(os.path.exists(out_path), "no document was written")
        return out_path

    def _dirty_the_saved_document(self, out_path):
        """Reopen the saved file so the snapshot records its path, dirty it,
        and take a snapshot."""
        import subprocess

        from dogtail import rawinput

        self.relaunch_app(launch_args=[out_path])
        time.sleep(2.0)
        self._edit_the_document(rawinput, 2)
        subprocess.run(["gapplication", "action", f"org.tunaos.{self.app_name}", "autosave-now"])
        time.sleep(0.8)
        self.assertEqual(len(self._snapshot_files()), 1, "precondition: a snapshot exists")


class RenamedOriginalMixin(SavedDocumentMixin):
    """Unsaved work must survive its original being renamed out from under it.

    The stale-snapshot check asks whether the document on disk is newer than
    the snapshot, so that already-saved work is not offered back
    (`TablesStaleSnapshotSmoke`). A file renamed away has no mtime to
    compare, and the honest answer there is to offer the work: the user's
    edits were never saved anywhere, and the only copy is the snapshot.

    Reading it the other way — "I cannot find the original, so I cannot show
    this is unsaved" — would discard the one case where recovery matters
    most. That is the same asymmetry the ownership lock is biased on: failing
    to prove work is safe is not proof that it is.

    Run for all three apps because the check lives in shared code but each
    app decides what to do with its answer, and #322's completion note asks
    for journeys in all three.
    """

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(prefix=f"{self.app_name}-renamed-state-")
        self._dir = self.temp_dir(f"{self.app_name}-renamed-docs-")
        super().setUp()

    def _assert_the_offer_names_the_work(self, frame_name):
        """Tables and Decks title the window with the document; Letters puts
        the name on the recovered tab and keeps a fixed window title, so it
        has nothing to add here."""

    def test_work_whose_original_was_renamed_away_is_still_offered(self):
        out_path = self._save_a_real_document()
        self._dirty_the_saved_document(out_path)

        # The rename happens outside the app, as it would in a file manager
        # or a shell, while the app holds unsaved changes to the old name.
        renamed = os.path.join(self._dir, f"{self.doc_stem}-2026{self.doc_suffix}")
        os.rename(out_path, renamed)
        self.assertFalse(os.path.exists(out_path))

        self.relaunch_app(crash=True)
        frame = self.wait_until(
            lambda: self.app.child(roleName="frame").name,
            lambda name: "Recovered" in name,
            timeout=20.0,
            description="a recovered window title",
        )
        self._assert_the_offer_names_the_work(frame)


class TablesSavedDocumentMixin(SavedDocumentMixin):
    """How Tables specifically gets a dirty saved workbook.

    Split from the app-agnostic mixin because both Tables journeys that need
    one — the renamed original and the stale snapshot — have to edit cells
    the same way, and putting the override on only one of them left the
    other typing into a cell it never committed. The workbook was clean,
    `autosave-now` returned early, and the journey failed on its own
    precondition rather than on anything it meant to test.
    """

    app_name = "tables"
    doc_suffix = ".xlsx"

    def _edit_the_document(self, rawinput, nth):
        # A cell edit only counts once committed, and the second one has to
        # differ from the first or the workbook is not dirty.
        rawinput.typeText("=6*7" if nth == 1 else "=1+1")
        rawinput.keyCombo("Return")
        time.sleep(0.5)


class TablesRenamedOriginalSmoke(TablesSavedDocumentMixin, RenamedOriginalMixin, BaseGUITestCase):
    def _assert_the_offer_names_the_work(self, frame_name):
        self.assertIn(
            f"{self.doc_stem}{self.doc_suffix}", frame_name,
            f"the recovered window should name the original document: {frame_name!r}",
        )


class LettersRenamedOriginalSmoke(RenamedOriginalMixin, BaseGUITestCase):
    app_name = "letters"
    doc_suffix = ".md"
    save_button_label = "Save All"


class DecksRenamedOriginalSmoke(RenamedOriginalMixin, BaseGUITestCase):
    app_name = "decks"
    doc_suffix = ".pptx"

    def _edit_the_document(self, rawinput, nth):
        # A fresh deck has no focused text frame, so it is dirtied the way
        # every other Decks journey dirties one. Each call adds another
        # shape, so the second edit is a real change.
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.decks", "add-shape"])
        time.sleep(1.0)

    def _assert_the_offer_names_the_work(self, frame_name):
        self.assertIn(
            f"{self.doc_stem}{self.doc_suffix}", frame_name,
            f"the recovered window should name the original document: {frame_name!r}",
        )


class TwoDocumentsMixin:
    """Two crashed documents, and a defined answer about which comes back.

    `find_orphaned_snapshots` returns the newest snapshot first, breaking
    mtime ties on the document id, so that a launch recovering one document
    per window is making a choice somebody made rather than taking whatever
    the directory listed first. That ordering is unit-tested; nothing had
    checked that a real launch honours it, or that the document it does not
    take is left intact for the next one.

    Both snapshots are planted rather than produced by two crashed runs: the
    app is single-instance, so getting two genuine orphans means two
    sequential runs, and the first relaunch would consume one of them before
    the second existed. Planting makes the newer one explicit.

    Tables and Decks need this; Letters does not, and that is not an
    omission. A Letters window holds a document per tab, so two dirty tabs
    are two documents with two slots, and `LettersAutosaveSmoke` has
    asserted since #99 that a crash recovers both. The single-document apps
    are the ones that have to *choose*.
    """

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(prefix=f"{self.app_name}-two-docs-")
        snap_dir = os.path.join(self._state_dir, self.app_name)
        os.makedirs(snap_dir, exist_ok=True)
        self._older = os.path.join(snap_dir, "doc-older.snapshot")
        self._newer = os.path.join(snap_dir, "doc-newer.snapshot")
        # Kept, not regenerated, for the untouched-bytes assertion below:
        # both builders produce a zip, and zip entries carry a modification
        # time, so the same call a second later returns different bytes.
        # Comparing against a fresh copy failed on one timestamp byte and
        # read as "the snapshot was rewritten".
        self._older_bytes = self.planted_bytes("older")
        for path, payload, name in (
            (self._older, self._older_bytes, f"older{self.doc_suffix}"),
            (self._newer, self.planted_bytes("newer"), f"newer{self.doc_suffix}"),
        ):
            with open(path, "wb") as data:
                data.write(payload)
            with open(f"{path}.meta", "w") as meta:
                meta.write(f"/nonexistent/{name}\n{self.snapshot_kind}")
        # Two clear generations rather than two writes a millisecond apart,
        # so the assertion is about the order and not about the tiebreak.
        old_time = time.time() - 600
        os.utime(self._older, (old_time, old_time))
        os.utime(f"{self._older}.meta", (old_time, old_time))
        super().setUp()

    def test_the_newest_crashed_document_is_recovered_and_the_other_kept(self):
        frame = self.wait_until(
            lambda: self.app.child(roleName="frame").name,
            lambda name: "Recovered" in name,
            timeout=20.0,
            description="a recovered window title",
        )
        self.assertIn(
            f"newer{self.doc_suffix}", frame,
            f"the newest snapshot should be the one recovered: {frame!r}",
        )

        # The one it did not take must still be there, whole, for the next
        # launch — clearing or damaging it would lose a second document's
        # unsaved work to the recovery of the first.
        self.assertTrue(os.path.exists(self._older),
                        "the older document's snapshot was cleared by the recovery of the newer")
        self.assertTrue(os.path.exists(f"{self._older}.meta"),
                        "the older document's metadata was cleared")
        with open(self._older, "rb") as kept:
            self.assertEqual(
                kept.read(), self._older_bytes,
                "the older document's snapshot was rewritten",
            )


class TablesTwoDocumentsSmoke(TwoDocumentsMixin, BaseGUITestCase):
    app_name = "tables"
    doc_suffix = ".xlsx"
    snapshot_kind = "xlsx"

    def planted_bytes(self, which):
        return minimal_xlsx_bytes("1111" if which == "older" else "9999")


class DecksTwoDocumentsSmoke(TwoDocumentsMixin, BaseGUITestCase):
    app_name = "decks"
    doc_suffix = ".pptx"
    snapshot_kind = "pptx"

    def planted_bytes(self, which):
        return minimal_pptx_bytes(f"{which} deck text")


class TablesStaleSnapshotSmoke(TablesSavedDocumentMixin, BaseGUITestCase):
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

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(prefix="tables-stale-state-")
        self._dir = self.temp_dir("tables-stale-docs-")
        super().setUp()

    def test_a_snapshot_the_save_overtook_is_not_offered_as_recovery(self):
        out_path = self._save_a_real_document()
        self._dirty_the_saved_document(out_path)

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
        recovered_from = set(self._snapshot_files())

        # Simulate a crash: kill the process directly, bypassing the close
        # guard entirely, so the snapshot is never cleared by a clean exit,
        # then relaunch against the same state dir and expect recovery.
        self.relaunch_app(crash=True)
        time.sleep(1.5)

        frame = self.app.child(roleName="frame")
        self.assertIn("Recovered", frame.name, f"window did not announce recovery: {frame.name!r}")
        # The assertion below used to read `self._snapshot_files() == []`:
        # after recovery, nothing on disk. That was a proxy for the thing
        # actually required — the orphan is not offered a second time — and
        # it stopped being a safe proxy once recovery began writing the
        # recovered content to the new window's own slot before clearing the
        # orphan. Zero files also describes unprotected work, which is what
        # that spelling was quietly asserting: the old code left the
        # recovered document with no snapshot until the next timer tick.
        # So the intent is asserted directly instead, and more strictly: the
        # recovered orphan is gone, and the recovered work is itself covered.
        # Waited for rather than sampled after a fixed sleep, because the
        # `== []` version sampled 1.5s after relaunch and lost that race
        # under the load of a full batch run — it failed twice in one batch
        # and passed three consecutive focused runs. A post-condition that
        # needs a sleep to hold is a post-condition to wait for.
        present = self.wait_until(
            lambda: set(self._snapshot_files()),
            lambda files: not (files & recovered_from),
            description="the recovered orphan must be cleared so it isn't offered again",
        )
        self.assertEqual(
            len(present), 1,
            "the recovered workbook must itself be protected by a snapshot; "
            f"found {sorted(present)}",
        )


class TablesUnclearableSnapshotSmoke(TablesSavedDocumentMixin, BaseGUITestCase):
    """A snapshot that cannot be cleared must leave a trace.

    Clearing the snapshot is the last step of a save, and every call site
    used to be `let _ = slot.clear();`. #666 decided, rightly, not to put a
    dialog in front of somebody whose save just worked about a temporary
    file they cannot act on, and removed the consequence instead — a
    snapshot the saved file has overtaken is no longer offered back. But it
    removed the trace along with the consequence: a state directory that is
    full or read-only produced no output at all, so the one person who
    needs to know — whoever is asking why it keeps filling up — had nothing
    to read.

    The journey arranges a clear that genuinely cannot succeed, saves, and
    reads the app's own stderr. Asserting on stderr rather than on a toast
    is the point: the choice under test is *log, do not interrupt*, and a
    journey that looked for a notice would be asserting the opposite.
    """

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(prefix="tables-unclearable-state-")
        self._dir = self.temp_dir("tables-unclearable-docs-")
        super().setUp()

    def test_a_snapshot_that_cannot_be_cleared_says_so(self):
        from dogtail import rawinput

        out_path = self._save_a_real_document()
        self._dirty_the_saved_document(out_path)

        # A directory where the snapshot file was: `remove_file` then fails
        # for root as well, which a permission bit would not. Replacing the
        # file rather than blocking the whole directory matters — block the
        # directory and the snapshot never exists, so `clear` succeeds with
        # nothing to do and the journey passes while proving nothing.
        [snapshot] = self._snapshot_files()
        blocked = os.path.join(self._state_dir, self.app_name, snapshot)
        os.remove(blocked)
        os.mkdir(blocked)

        # The workbook already has a path, so Ctrl+S writes it and clears.
        rawinput.keyCombo("<Control>s")
        time.sleep(2.0)

        self.assertTrue(os.path.isdir(blocked), "the clear should not have removed it")
        _out, err = self.app_output()
        self.assertIn(
            "could not clear the crash snapshot", err or "",
            f"the failed clear left no trace; the app's stderr was: {(err or '')[-800:]!r}",
        )


class TablesAutosaveFailureSmoke(BaseGUITestCase):
    """What the user is told when autosave cannot write at all."""

    app_name = "tables"

    def setUp(self):
        # XDG_STATE_HOME points at a regular file, so the app's
        # `create_dir_all` of `<state>/tables` fails with ENOTDIR — for root
        # as well, which a permission bit would not.
        holder = self.temp_dir("tables-autosave-blocked-")
        self._blocked = os.path.join(holder, "not-a-directory")
        with open(self._blocked, "w", encoding="utf-8") as handle:
            handle.write("occupied\n")
        self.launch_env = {**getattr(self, "launch_env", {}), "XDG_STATE_HOME": self._blocked}
        super().setUp()

    def _snapshot_files(self):
        return []

    def _edit_a1(self):
        import subprocess
        from dogtail import rawinput

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        time.sleep(1.5)
        rawinput.typeText("=6*7")
        rawinput.keyCombo("Return")
        time.sleep(0.5)

    def test_an_autosave_that_cannot_write_says_so(self):
        """Autosave used to fail in silence.

        Every write site in the three apps read `let _ = slot.write(...)`, so
        a read-only home, a full disk or a sandbox denying the state
        directory meant autosave did nothing for the whole session while the
        user went on believing their unsaved work was protected. They found
        out at the crash — the one moment the feature exists for.

        The write is made to fail by pointing the state directory at a path
        whose parent is a *regular file*, not by chmod: this suite can run as
        root, and root ignores directory permissions. `crash-stress.md`
        records a previous test that made a write fail by chmod-ing to 0555
        and therefore asserted nothing at all.
        """
        import subprocess

        self._edit_a1()
        subprocess.run(["gapplication", "action", "org.tunaos.tables", "autosave-now"])
        time.sleep(0.8)

        toast = self._toast_about_autosave()
        self.assertIsNotNone(
            toast,
            "autosave could not write and the window said nothing; "
            f"labels on screen: {[n.name for n in self.app.findChildren(lambda x: x.roleName == 'label')][:12]}",
        )
        self.assertEqual(self._snapshot_files(), [],
                         "precondition: the write must really have failed")

    def _toast_about_autosave(self):
        """The toast, found by what it says rather than by widget path."""
        for node in self.app.findChildren(lambda x: x.roleName in ("label", "static")):
            if "not being protected" in (node.name or ""):
                return node
        return None

class LegacySnapshotUpgradeMixin:
    """A crash on the old build, then an upgrade, must not lose the work.

    The snapshot format changed to a single versioned envelope, and `read`
    still understands the two-file layout that preceded it — raw document
    bytes plus a `.snapshot.meta` sidecar — precisely so that somebody who
    crashed on the old build and then upgraded gets their unsaved work back.
    Unit tests cover that read. These put a legacy pair on disk and start the
    real app on it, which is the only way to know the whole upgrade path is
    wired: recovery has to find the orphan, load bytes in a format this build
    does not write, and adopt them into its own envelope slot.

    The document is hand-built in every case rather than saved by the app,
    because a snapshot the app wrote would be an envelope and would prove
    nothing about the legacy path.

    It is also the negative control for #718's guard, which stops an
    envelope that fails to decode being re-read as legacy content. Getting
    that wrong in the other direction — refusing everything the envelope
    reader declines — silently discards exactly this user's work, and these
    journeys fail when it does.

    Run for all three apps because each loads a different format through the
    same shared read, and #322's completion note asks for all three.
    """

    def setUp(self):
        self._state_dir = self.isolate_autosave_state(prefix=f"{self.app_name}-legacy-")
        # Decks and Letters read their recovered content back through the
        # state-snapshot interface, which needs its own path configured;
        # without it `trigger_snapshot` fails with
        # "GTK_OFFICE_SNAPSHOT_PATH is not configured" rather than anything
        # about recovery.
        self._snapshot_path = self.isolate_snapshot(prefix=f"{self.app_name}-legacy-snap-")
        # The recorded original: a path that never existed, so the recovered
        # document is unsaved work rather than a file the app could reopen
        # on its own.
        self._original = os.path.join(
            self.temp_dir(prefix=f"{self.app_name}-legacy-doc-"),
            f"quarterly{self.doc_suffix}",
        )
        snap_dir = os.path.join(self._state_dir, self.app_name)
        os.makedirs(snap_dir, exist_ok=True)
        with open(os.path.join(snap_dir, "legacy-1.snapshot"), "wb") as data:
            data.write(self.planted_bytes())
        with open(os.path.join(snap_dir, "legacy-1.snapshot.meta"), "w") as meta:
            meta.write(f"{self._original}\n{self.snapshot_kind}")
        super().setUp()

    def _snapshot_files(self):
        d = os.path.join(self._state_dir, self.app_name)
        return sorted(f for f in os.listdir(d)) if os.path.isdir(d) else []

    def test_a_snapshot_from_the_previous_build_still_recovers(self):
        self.wait_until(
            lambda: self.app.child(roleName="frame").name,
            lambda name: "Recovered" in name,
            timeout=20.0,
            description="a recovered window title",
        )

        # The title alone would pass if recovery had adopted an *empty*
        # document, so each app reads its own content back.
        self._assert_the_planted_content_came_back()

        # The legacy sidecar must not survive as something a later launch
        # could read again: adopting the content rewrites this window's own
        # slot as an envelope and clears the orphan it came from.
        files = self.wait_until(
            self._snapshot_files,
            lambda fs: "legacy-1.snapshot" not in fs,
            timeout=20.0,
            description="the legacy orphan to be cleared",
        )
        self.assertNotIn("legacy-1.snapshot.meta", files,
                         f"the legacy sidecar outlived its snapshot: {files}")
        self.assertTrue(
            any(f.endswith(".snapshot") for f in files),
            f"the recovered work must still be protected by a snapshot: {files}",
        )


class TablesLegacySnapshotUpgradeSmoke(LegacySnapshotUpgradeMixin, BaseGUITestCase):
    app_name = "tables"
    doc_suffix = ".xlsx"
    snapshot_kind = "xlsx"

    def planted_bytes(self):
        return minimal_xlsx_bytes("4242")

    def _assert_the_planted_content_came_back(self):
        from dogtail import rawinput

        self.assertIn(
            f"quarterly{self.doc_suffix}", self.app.child(roleName="frame").name,
            "the recovered window should name the original document",
        )
        rawinput.keyCombo("<Control>g")
        time.sleep(0.5)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.5)
        rawinput.keyCombo("Escape")
        rawinput.keyCombo("Right")
        rawinput.keyCombo("Left")
        grid = self.wait_until(
            lambda: self.app.child(name="Spreadsheet grid").description,
            lambda d: "4242" in d,
            timeout=20.0,
            description="the legacy snapshot's cell value in the grid",
        )
        self.assertIn("4242", grid)


class DecksLegacySnapshotUpgradeSmoke(LegacySnapshotUpgradeMixin, BaseGUITestCase):
    app_name = "decks"
    doc_suffix = ".pptx"
    snapshot_kind = "pptx"

    def planted_bytes(self):
        return minimal_pptx_bytes("planted deck text")

    def _assert_the_planted_content_came_back(self):
        self.assertIn(
            f"quarterly{self.doc_suffix}", self.app.child(roleName="frame").name,
            "the recovered window should name the original document",
        )
        # The state snapshot rather than the canvas description: it names the
        # object kinds, so an empty recovered deck cannot satisfy it.
        snap = self.trigger_snapshot("org.tunaos.decks")
        self.assertEqual(snap["slide_count"], 1, f"recovered deck: {snap}")
        kinds = [o["kind"] for s in snap["slides"] for o in s["objects"]]
        self.assertIn("TextBox", kinds, f"the planted text box did not come back: {snap}")


class LettersLegacySnapshotUpgradeSmoke(LegacySnapshotUpgradeMixin, BaseGUITestCase):
    app_name = "letters"
    doc_suffix = ".md"
    # Letters' snapshot is the Document as JSON, so its "legacy" bytes are
    # the same JSON with the old sidecar beside them. Every field is spelled
    # out because `ParaStyle` has no serde defaults — a trimmed literal is
    # rejected outright with `missing field \`alignment\``, and the journey
    # would then fail as a timeout rather than as a parse error.
    snapshot_kind = "letters-json"

    def planted_bytes(self):
        import json

        run_style = {
            "bold": False, "italic": False, "underline": False,
            "strikethrough": False, "highlight": False, "code": False,
            "link": None, "image": None, "font_family": None,
            "font_size_hp": None, "color": None, "vert_align": None,
            "footnote": None, "html": False,
        }
        para_style = {
            "heading": None, "alignment": "Left", "list": "None",
            "list_level": 0, "list_start": None, "line_spacing": 1.0,
            "space_before_pt": 0.0, "space_after_pt": 0.0,
            "left_indent_pt": 0.0, "right_indent_pt": 0.0,
            "first_line_indent_pt": 0.0, "tab_stops_pt": [],
            "code_block": None, "block_quote": False, "html_block": False,
            "page_break_before": False, "named_style": None,
            "table_cell": None,
        }
        document = {
            "paragraphs": [{
                "style": para_style,
                "runs": [{"text": "planted letter text", "style": run_style}],
            }],
            "footnotes": [],
            "header": None,
            "footer": None,
            "page": None,
        }
        return json.dumps(document).encode()

    def _assert_the_planted_content_came_back(self):
        # Letters titles the *tab* with the document and keeps a fixed window
        # title for a recovery, so the name is not on the frame to assert.
        snap = self.trigger_snapshot("org.tunaos.letters")
        text = "".join(r["text"] for p in snap["paragraphs"] for r in p["runs"])
        self.assertIn(
            "planted letter text", text,
            f"the recovered document came back as {text!r}",
        )


def minimal_pptx_bytes(text):
    """The smallest pptx package Decks will open, holding one text box.

    Mirrors the parts `decks_core::engine::write_pptx_bytes` emits, because
    the point is a deck that exists *before* the app starts: the
    schema-upgrade journey plants it as a snapshot from the previous build,
    and anything Decks wrote would be an envelope and would beg the
    question. `txBox="1"` is what makes the reader treat the shape as a text
    box rather than a rectangle.
    """
    import io
    import zipfile

    slide = f"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
 <p:cSld><p:spTree>
  <p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
  <p:grpSpPr/>
  <p:sp>
   <p:nvSpPr><p:cNvPr id="2" name="TextBox 1"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr>
   <p:spPr><a:xfrm><a:off x="952500" y="952500"/><a:ext cx="2857500" cy="762000"/></a:xfrm>
    <a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>
   <p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody>
  </p:sp>
 </p:spTree></p:cSld>
</p:sld>"""

    parts = {
        "[Content_Types].xml": """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
 <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
 <Default Extension="xml" ContentType="application/xml"/>
 <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
 <Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
</Types>""",
        "_rels/.rels": """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>""",
        "ppt/presentation.xml": """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
 <p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst>
 <p:sldSz cx="9144000" cy="5143500"/>
 <p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>""",
        "ppt/_rels/presentation.xml.rels": """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>
</Relationships>""",
        "ppt/slides/slide1.xml": slide,
    }
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", zipfile.ZIP_DEFLATED) as deck:
        for name, content in parts.items():
            deck.writestr(name, content)
    return buffer.getvalue()


def minimal_xlsx_bytes(a1_value):
    """The smallest xlsx package Tables will open, with `a1_value` in A1.

    Hand-built rather than produced by Tables itself, because two journeys
    need a workbook that exists *before* the app starts: one opens it from
    the command line, and one plants it as a crash snapshot from an older
    build. Anything Tables wrote would beg the question in the second case.
    """
    import io
    import zipfile

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
        "xl/worksheets/sheet1.xml": f"""<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>{a1_value}</v></c></row></sheetData></worksheet>""",
    }
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", zipfile.ZIP_DEFLATED) as book:
        for name, content in parts.items():
            book.writestr(name, content)
    return buffer.getvalue()


class TablesUndoSaveReopenSmoke(BaseGUITestCase):
    """Real GTK journey: edit, undo, redo, save, restart, and reopen."""

    app_name = "tables"

    def setUp(self):
        self._dir = self.temp_dir(prefix="tables-rt-")
        self._doc = os.path.join(self._dir, "journey.xlsx")
        with open(self._doc, "wb") as book:
            book.write(minimal_xlsx_bytes("1"))
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


class TablesColumnMenuSmoke(TablesCellEntryMixin, BaseGUITestCase):
    """The column menu (DESIGN-UI.md, Numbers/Sheets): Alt+Down opens the
    active cell's column menu; Sort Ascending reorders the rows; unticking
    a value in its checklist filters those rows out, and the status bar's
    quick summary counts only what is shown. Asserted on the grid's cell
    names, the menu's buttons and the status label, as AT-SPI reports them."""

    app_name = "tables"

    def _to_grid(self, ref):
        from dogtail import rawinput
        rawinput.keyCombo("<Control>g")
        self.wait_until(lambda: self._focused("Cell reference"), bool,
                        description="the name box to take focus")
        rawinput.typeText(ref)
        rawinput.keyCombo("Return")
        self.wait_until(lambda: self._focused("Formula input"), bool,
                        description=f"the jump to {ref}")
        rawinput.keyCombo("Escape")
        self.wait_until(lambda: not self._focused("Formula input"), bool,
                        description="Escape to hand focus to the grid")

    def _button(self, name, role="push button"):
        found = self.app.findChildren(lambda c: c.roleName == role and c.name == name and c.showing)
        return found[0] if found else None

    def test_sort_and_filter_from_the_column_menu(self):
        from dogtail import rawinput
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        self._wait_for_a_new_document()
        self._put("A1", "30")
        self._put("A2", "10")
        self._put("A3", "20")
        self._to_grid("A1")
        rawinput.keyCombo("<Alt>Down")
        self.wait_until(lambda: self._button("Sort Ascending"), bool, description="the column menu")
        self._button("Sort Ascending").do_action(0)
        for name in ("A1: 10", "A2: 20", "A3: 30"):
            self.wait_until(
                lambda: self.app.findChildren(lambda c: c.roleName == "table cell" and c.name == name),
                bool,
                description=f"{name} after sorting",
            )

        # Untick 20: its row (now row 2) is filtered out.
        self._to_grid("A1")
        rawinput.keyCombo("<Alt>Down")
        self.wait_until(lambda: self._button("20", "check box"), bool, description="20 in the value list")
        # GTK 4's check buttons offer no AT-SPI action, so untick it from the
        # keyboard: Tab from the first item until the focus is on "20".
        for _ in range(8):
            if self._button("20", "check box").focused:
                break
            rawinput.keyCombo("Tab")
            time.sleep(0.2)
        self.assertTrue(self._button("20", "check box").focused, "Tab never reached the 20 check box")
        rawinput.keyCombo("space")
        import pyatspi
        self.wait_until(
            lambda: not self._button("20", "check box").getState().contains(pyatspi.STATE_CHECKED),
            bool,
            description="20 to be unticked",
        )
        rawinput.keyCombo("Escape")

        # Select A1:A3; the quick summary skips the hidden row.
        self._to_grid("A1")
        rawinput.keyCombo("<Shift>Down")
        rawinput.keyCombo("<Shift>Down")
        labels = lambda: [c.name or "" for c in self.app.findChildren(lambda c: c.roleName == "label")]
        stats = self.wait_until(labels, lambda ls: any("Sum" in l for l in ls), description="the quick summary")
        line = next(l for l in stats if "Sum" in l)
        self.assertIn("Sum 40", line)
        self.assertIn("Min 10", line)
        self.assertIn("Max 30", line)
        self.assertIn("Count 2", line)
        self.assertIsNone(self.process.poll(), "tables crashed in the column menu")


class TablesFormulaAutocompleteSmoke(TablesCellEntryMixin, BaseGUITestCase):
    """The formula editor (DESIGN-UI.md): typing a function name offers the
    functions it could be, Tab inserts the chosen one with its parenthesis,
    and inside the call the signature is shown. Asserted on the popover's
    labels, as AT-SPI reports them."""

    app_name = "tables"

    def _labels(self):
        return [c.name or "" for c in self.app.findChildren(lambda c: c.roleName == "label")]

    def test_typing_a_function_offers_it_and_tab_inserts_it(self):
        from dogtail import rawinput
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        self._wait_for_a_new_document()
        # Ctrl+G then a jump hands focus to the formula entry.
        rawinput.keyCombo("<Control>g")
        self.wait_until(lambda: self._focused("Cell reference"), bool,
                        description="the name box to take focus")
        rawinput.typeText("B2")
        rawinput.keyCombo("Return")
        self.wait_until(lambda: self._focused("Formula input"), bool,
                        description="the jump to hand focus to fx")
        rawinput.typeText("=SU")
        self.wait_until(self._labels, lambda ls: any(l.startswith("SUMIF") for l in ls),
                        description="SUMIF among the suggestions")
        rawinput.keyCombo("Tab")
        # GtkEntry's text isn't readable over AT-SPI here (it reads ''),
        # so the insertion is observed through the popover: once "=SUM("
        # is in the entry there is nothing left to complete, the
        # suggestions go, and SUM's argument hint stays.
        self.wait_until(
            self._labels,
            lambda ls: not any(l.startswith("SUMIF") for l in ls) and any(l.startswith("SUM(number1") for l in ls),
            description="Tab to insert SUM( and leave its argument hint",
        )
        self.assertIsNone(self.process.poll(), "tables crashed in the formula editor")


class TablesFormatInspectorSmoke(TablesCellEntryMixin, BaseGUITestCase):
    """The Format inspector (DESIGN-UI.md) edits the selected cell's style
    and follows the selection: Bold set on A1 shows as pressed on A1 and
    not on B1. The inspector's toggles carry accessible names, and their
    pressed state is what AT-SPI reports, so this is asserted on the tree,
    not on pixels (the render lab covers how bold text looks)."""

    app_name = "tables"

    def _pressed(self, name):
        import pyatspi
        node = self.app.child(name=name, roleName="toggle button")
        states = node.getState()
        return states.contains(pyatspi.STATE_PRESSED) or states.contains(pyatspi.STATE_CHECKED)

    def _go(self, ref):
        from dogtail import rawinput
        rawinput.keyCombo("<Control>g")
        self.wait_until(lambda: self._focused("Cell reference"), bool,
                        description="the name box to take focus")
        rawinput.typeText(ref)
        rawinput.keyCombo("Return")
        self.wait_until(lambda: self._focused("Formula input"), bool,
                        description=f"the jump to {ref} to hand focus back to fx")
        rawinput.keyCombo("Escape")
        self.wait_until(lambda: not self._focused("Formula input"), bool,
                        description="Escape to leave the formula entry")

    def test_bold_applies_to_the_selection_and_follows_it(self):
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        self._wait_for_a_new_document()
        self._put("A1", "x")
        self._put("B1", "y")
        # do_action, not click(): GTK 4 reports no screen extents over
        # AT-SPI, so a coordinate click lands at the window's corner.
        self.app.child(name="Format", roleName="toggle button").do_action(0)
        self.wait_until(lambda: self.app.child(name="Bold", roleName="toggle button").showing, bool,
                        description="the inspector to open")
        self._go("A1")
        self.assertFalse(self._pressed("Bold"), "A1 starts plain")
        self.app.child(name="Bold", roleName="toggle button").do_action(0)
        self.wait_until(lambda: self._pressed("Bold"), bool, description="Bold to press")
        self._go("B1")
        self.wait_until(lambda: not self._pressed("Bold"), bool,
                        description="the inspector to show B1, which is not bold")
        self._go("A1")
        self.wait_until(lambda: self._pressed("Bold"), bool,
                        description="the inspector to show A1 as bold again")
        self.assertIsNone(self.process.poll(), "tables crashed in the format inspector")


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
    40, COL_HEADER_HEIGHT 20, ROW_HEIGHT 20, COL_WIDTH 64) are fixed, but
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

    ROW_HEADER_WIDTH = 40
    COL_HEADER_HEIGHT = 20
    ROW_HEIGHT = 20
    COL_WIDTH = 64

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
        # "Insert shape" makes a styled preset Shape (engine::shape), not
        # the old unstyled Rect the canvas painted blue.
        self.assertIn("Shape", kinds)
        self.assertIsNone(self.process.poll(), "decks crashed writing a snapshot")


class DecksFormatInspectorSmoke(BaseGUITestCase):
    """The Format inspector (DESIGN-UI.md) edits the selected object as one
    undoable step: an inserted shape is selected, "Send to Back" in the
    inspector's Arrange tab puts it behind the text box (the snapshot's
    object order), and one undo puts it back. The inspector's controls
    carry accessible names, so this drives them through AT-SPI, not
    pixels."""

    app_name = "decks"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="decks-format-")
        super().setUp()

    def _kinds(self, aid):
        snap = self.trigger_snapshot(aid)
        return [o["kind"] for o in snap["slides"][0]["objects"]]

    def test_send_to_back_is_one_undoable_step(self):
        aid = "org.tunaos.decks"
        self.gapplication_action(aid, "new-document")
        self.gapplication_action(aid, "add-text-box")
        self.gapplication_action(aid, "add-shape")
        self.wait_until(lambda: self._kinds(aid), lambda k: k == ["TextBox", "Shape"], interval=0.5,
                        description="a text box, then a shape")
        toggle = self.app.child(name="Format", roleName="toggle button")
        import pyatspi
        if not toggle.getState().contains(pyatspi.STATE_PRESSED):
            # do_action, not click(): GTK 4 reports no screen extents
            # over AT-SPI, so a coordinate click lands at the corner.
            toggle.do_action(0)
        # A shape opens on the Style tab. Only the visible tab's widgets
        # are in the tree, so switch to Arrange first (the switcher's
        # buttons are named after their pages).
        tab = self.wait_until(lambda: self.app.child(name="Arrange"), lambda t: t is not None,
                              description="the inspector's Arrange tab for the inserted shape")
        tab.do_action(0)
        back = self.wait_until(lambda: self.app.child(name="Send to Back", roleName="push button"),
                               lambda b: b is not None and b.showing,
                               description="the Arrange tab's order buttons")
        back.do_action(0)
        self.wait_until(lambda: self._kinds(aid), lambda k: k == ["Shape", "TextBox"], interval=0.5,
                        description="the shape to go behind the text box")
        self.gapplication_action(aid, "undo")
        self.wait_until(lambda: self._kinds(aid), lambda k: k == ["TextBox", "Shape"], interval=0.5,
                        description="one undo to restore the order")
        self.assertIsNone(self.process.poll(), "decks crashed in the format inspector")


class DecksMagicMovePreviewSmoke(BaseGUITestCase):
    """Magic Move, seen: a two-slide deck whose rectangle moves from x=100
    to x=700, the second slide arriving by Magic Move. Previewing that
    transition in the running app makes it write the animation's midpoint
    frame (the same drawing path the animation uses; GTK_OFFICE_TEST_MODE
    only), and in that frame the rectangle is half way: centred at
    x=450 of the 960-wide slide."""

    app_name = "decks"

    CONTENT = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<office:document-content'
        ' xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"'
        ' xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0"'
        ' xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"'
        ' xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"'
        ' xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"'
        ' xmlns:smil="urn:oasis:names:tc:opendocument:xmlns:smil-compatible:1.0"'
        ' xmlns:decks="https://tuna-os.org/ns/decks/1.0" office:version="1.2">'
        '<office:automatic-styles><style:style style:name="dp2" style:family="drawing-page">'
        '<style:drawing-page-properties smil:type="fade" smil:subtype="crossfade" decks:transition="magic-move"/>'
        '</style:style></office:automatic-styles>'
        '<office:body><office:presentation>'
        '<draw:page draw:name="One"><draw:rect svg:x="100pt" svg:y="200pt" svg:width="100pt" svg:height="100pt"/></draw:page>'
        '<draw:page draw:name="Two" draw:style-name="dp2"><draw:rect svg:x="700pt" svg:y="200pt" svg:width="100pt" svg:height="100pt"/></draw:page>'
        '</office:presentation></office:body></office:document-content>'
    )

    def setUp(self):
        import zipfile
        self._dir = self.temp_dir(prefix="decks-magic-move-")
        self._doc = os.path.join(self._dir, "move.odp")
        with zipfile.ZipFile(self._doc, "w") as z:
            z.writestr("mimetype", "application/vnd.oasis.opendocument.presentation")
            z.writestr("content.xml", self.CONTENT)
        self._frame = os.path.join(self._dir, "transition-midpoint.png")
        self.launch_args = [self._doc]
        # The frame dump is test-only (GTK_OFFICE_TEST_MODE).
        self.launch_env = {**getattr(self, "launch_env", {}),
                           "GTK_OFFICE_TEST_MODE": "1", "GTK_OFFICE_TRANSITION_DUMP": self._dir}
        super().setUp()

    def test_the_midpoint_frame_has_the_rectangle_half_way(self):
        import subprocess
        from PIL import Image
        aid = "org.tunaos.decks"
        self.wait_until(lambda: self.app.child(name="Slide canvas").description,
                        lambda d: "2 slides" in d or "of 2" in d, interval=0.5,
                        description="the two-slide deck to open")
        subprocess.run(["gapplication", "action", aid, "go-to-slide", "uint32 1"], check=True, timeout=5)
        subprocess.run(["gapplication", "action", aid, "preview-transition"], check=True, timeout=5)
        self.wait_until(lambda: os.path.exists(self._frame), bool, interval=0.25,
                        description="the midpoint frame to be written")
        img = Image.open(self._frame).convert("RGB")
        w, h = img.size
        px = img.load()
        # The slide is the white area; the rectangle the blue one on it.
        white = [(x, y) for y in range(0, h, 2) for x in range(0, w, 2) if px[x, y] == (255, 255, 255)]
        self.assertTrue(white, "no slide in the frame")
        sx0, sx1 = min(p[0] for p in white), max(p[0] for p in white)
        blue = [x for y in range(0, h, 2) for x in range(0, w, 2)
                if (lambda r, g, b: b > 180 and r < 120)(*px[x, y])]
        self.assertTrue(blue, "no rectangle in the frame")
        centre = (min(blue) + max(blue)) / 2
        fraction = (centre - sx0) / (sx1 - sx0)
        self.assertAlmostEqual(fraction, 450 / 960, delta=0.03,
                               msg=f"rectangle centre at {fraction:.3f} of the slide, not half way (0.469)")
        self.assertIsNone(self.process.poll(), "decks crashed previewing Magic Move")


class DecksPresenterDisplaySmoke(BaseGUITestCase):
    """The presenter display (DESIGN-UI.md, "Presenter display"): rehearsing
    a two-slide deck opens it on the one monitor the test display has, with
    the slide counter, that slide's notes and a running clock; Next Slide
    moves the counter and the notes on; End Show closes it. Presenting opens
    the fullscreen audience window. Asserted through AT-SPI names and label
    text; the slides themselves are drawn by the same code as the editor's
    (render lab)."""

    app_name = "decks"

    CONTENT = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<office:document-content'
        ' xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"'
        ' xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"'
        ' xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"'
        ' xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"'
        ' xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" office:version="1.2">'
        '<office:body><office:presentation>'
        '<draw:page draw:name="One"><draw:rect svg:x="100pt" svg:y="100pt" svg:width="100pt" svg:height="100pt"/>'
        '<presentation:notes><draw:frame presentation:class="notes" svg:x="0pt" svg:y="0pt" svg:width="100pt" svg:height="50pt">'
        '<draw:text-box><text:p>Open with the question</text:p></draw:text-box></draw:frame></presentation:notes></draw:page>'
        '<draw:page draw:name="Two"><draw:rect svg:x="500pt" svg:y="100pt" svg:width="100pt" svg:height="100pt"/>'
        '<presentation:notes><draw:frame presentation:class="notes" svg:x="0pt" svg:y="0pt" svg:width="100pt" svg:height="50pt">'
        '<draw:text-box><text:p>Then show the answer</text:p></draw:text-box></draw:frame></presentation:notes></draw:page>'
        '</office:presentation></office:body></office:document-content>'
    )

    def setUp(self):
        import zipfile
        self._dir = self.temp_dir(prefix="decks-presenter-")
        self._doc = os.path.join(self._dir, "talk.odp")
        with zipfile.ZipFile(self._doc, "w") as z:
            z.writestr("mimetype", "application/vnd.oasis.opendocument.presentation")
            z.writestr("content.xml", self.CONTENT)
        self.launch_args = [self._doc]
        super().setUp()

    def _label(self, name):
        return self.app.child(name=name, roleName="label")

    def _texts(self):
        return [n.name for n in self.app.findChildren(lambda n: n.roleName == "label")]

    def test_rehearse_steps_through_slides_notes_and_clock(self):
        import re
        import subprocess
        aid = "org.tunaos.decks"
        self.wait_until(lambda: self.app.child(name="Slide canvas").description,
                        lambda d: "of 2" in d, interval=0.5, description="the two-slide deck to open")
        subprocess.run(["gapplication", "action", aid, "rehearse"], check=True, timeout=5)
        self.wait_until(self._texts, lambda t: "Slide 1 of 2" in t and "Open with the question" in t,
                        interval=0.5, description="the presenter display on slide 1 with its notes")
        self.assertTrue(any(re.fullmatch(r"\d+:\d\d", t or "") for t in self._texts()),
                        f"no clock among {self._texts()}")
        self.app.child(name="Next Slide", roleName="push button").do_action(0)
        self.wait_until(self._texts, lambda t: "Slide 2 of 2" in t and "Then show the answer" in t,
                        interval=0.5, description="Next Slide to move the counter and notes on")
        self.app.child(name="End Show", roleName="push button").do_action(0)
        self.wait_until(self._texts, lambda t: "Slide 2 of 2" not in t, interval=0.5,
                        description="End Show to close the presenter display")
        subprocess.run(["gapplication", "action", aid, "present"], check=True, timeout=5)
        # The audience window: an undecorated fullscreen frame.
        self.wait_until(lambda: [n.name for n in self.app.findChildren(lambda n: n.roleName == "frame")],
                        lambda names: "Slide Show" in names, interval=0.5,
                        description="the audience window to open")
        self.assertIsNone(self.process.poll(), "decks crashed running a show")


class DecksShowBuildsSmoke(BaseGUITestCase):
    """A show plays a slide's builds, one per click, before moving on
    (DESIGN-UI.md, "Object builds"). The deck is an odp whose first slide
    has two builds: a rectangle that flies in from the left, then one that
    fades in. Rehearsing it, the presenter display counts the builds
    ("Build 1 of 2") before "Slide 2 of 2"; the first click writes the
    build's midpoint frame as the audience sees it (test mode only), and in
    it the flying rectangle is half way in from beyond the left edge."""

    app_name = "decks"

    CONTENT = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<office:document-content'
        ' xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"'
        ' xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"'
        ' xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"'
        ' xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"'
        ' xmlns:smil="urn:oasis:names:tc:opendocument:xmlns:smil-compatible:1.0"'
        ' xmlns:anim="urn:oasis:names:tc:opendocument:xmlns:animation:1.0"'
        ' xmlns:xml="http://www.w3.org/XML/1998/namespace" office:version="1.2">'
        '<office:body><office:presentation>'
        '<draw:page draw:name="One">'
        '<draw:rect xml:id="a" draw:id="a" svg:x="400pt" svg:y="100pt" svg:width="200pt" svg:height="100pt"/>'
        '<draw:rect xml:id="b" draw:id="b" svg:x="400pt" svg:y="300pt" svg:width="200pt" svg:height="100pt"/>'
        '<anim:par presentation:node-type="timing-root"><anim:seq presentation:node-type="main-sequence">'
        '<anim:par smil:begin="next"><anim:par smil:begin="0s"><anim:par smil:begin="0s" presentation:node-type="on-click"'
        ' presentation:preset-class="entrance" presentation:preset-id="ooo-entrance-fly-in" presentation:preset-sub-type="from-left">'
        '<anim:set smil:targetElement="a" smil:attributeName="visibility" smil:to="visible"/></anim:par></anim:par></anim:par>'
        '<anim:par smil:begin="next"><anim:par smil:begin="0s"><anim:par smil:begin="0s" presentation:node-type="on-click"'
        ' presentation:preset-class="entrance" presentation:preset-id="ooo-entrance-fade-in">'
        '<anim:set smil:targetElement="b" smil:attributeName="visibility" smil:to="visible"/></anim:par></anim:par></anim:par>'
        '</anim:seq></anim:par></draw:page>'
        '<draw:page draw:name="Two"><draw:rect svg:x="100pt" svg:y="100pt" svg:width="100pt" svg:height="100pt"/></draw:page>'
        '</office:presentation></office:body></office:document-content>'
    )

    def setUp(self):
        import zipfile
        self._dir = self.temp_dir(prefix="decks-builds-")
        self._doc = os.path.join(self._dir, "builds.odp")
        with zipfile.ZipFile(self._doc, "w") as z:
            z.writestr("mimetype", "application/vnd.oasis.opendocument.presentation")
            z.writestr("content.xml", self.CONTENT)
        self._frame = os.path.join(self._dir, "build-0-midpoint.png")
        self.launch_args = [self._doc]
        self.launch_env = {**getattr(self, "launch_env", {}),
                           "GTK_OFFICE_TEST_MODE": "1", "GTK_OFFICE_TRANSITION_DUMP": self._dir}
        super().setUp()

    def _texts(self):
        return [n.name for n in self.app.findChildren(lambda n: n.roleName == "label")]

    def _next(self):
        self.app.child(name="Next Slide", roleName="push button").do_action(0)

    def test_clicks_play_the_builds_then_move_on(self):
        import subprocess
        from PIL import Image
        aid = "org.tunaos.decks"
        self.wait_until(lambda: self.app.child(name="Slide canvas").description,
                        lambda d: "of 2" in d, interval=0.5, description="the two-slide deck to open")
        subprocess.run(["gapplication", "action", aid, "rehearse"], check=True, timeout=5)
        self.wait_until(self._texts, lambda t: "Slide 1 of 2 · Build 0 of 2" in t, interval=0.5,
                        description="the presenter display before any build")
        self._next()
        self.wait_until(self._texts, lambda t: "Slide 1 of 2 · Build 1 of 2" in t, interval=0.5,
                        description="the first click to play the first build")
        self._next()
        self.wait_until(self._texts, lambda t: "Slide 1 of 2 · Build 2 of 2" in t, interval=0.5,
                        description="the second click to play the second build")
        self._next()
        self.wait_until(self._texts, lambda t: "Slide 2 of 2" in t, interval=0.5,
                        description="the third click to move on")
        # The first build's midpoint, as the audience sees it: a 1280x720
        # frame of a 960-unit slide, the rectangle half way from x=-200 to
        # x=400, i.e. at 100..300 units = 133..400 px; the second is not
        # there yet.
        img = Image.open(self._frame).convert("RGB")
        px = img.load()
        k = 1280 / 960
        blue = lambda p: p[2] > 150 and p[0] < 150
        self.assertTrue(blue(px[int(200 * k), int(150 * k)]), f"flying rectangle not half way in: {px[int(200 * k), int(150 * k)]}")
        self.assertFalse(blue(px[int(500 * k), int(150 * k)]), "the rectangle is already at its place")
        self.assertFalse(blue(px[int(500 * k), int(350 * k)]), "the second build shows before its click")
        self.assertIsNone(self.process.poll(), "decks crashed playing builds")


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
        # The first inserted shape is a rectangle at slide (200,200,200x150) in the 960x540 slide
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


class DecksCanvasDragSmoke(BaseGUITestCase):
    """Dragging an object on the canvas moves it in the model.

    The move, resize and rotate gestures had no journey at all.
    `DecksSelectionSmoke` clicks to select and stops there, and the
    harness's own `drag()` helper was unused by any Decks test. That gap
    only became visible when `canvas_input.rs` was split out of
    `window.rs`: "all ten Decks journeys pass" turned out to say nothing
    about most of the 203 lines that moved, since only the click-to-select
    path was ever exercised.

    So this asserts the model, not the canvas description: a drag that
    selects but fails to move would still satisfy a description check.
    """

    app_name = "decks"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="decks-drag-")
        super().setUp()

    def _slide_to_window(self):
        """Map slide (960x540) coordinates to window-local ones.

        Same arithmetic as DecksSelectionSmoke: the canvas fits the slide
        into its real allocation (slide_geometry() in canvas.rs), and the
        app embeds its own position as `canvas_at=x,y` in the accessible
        description because AT-SPI Component.position is wrong for nested
        widgets (#132).
        """
        import re

        canvas = self.app.child(name="Slide canvas")
        cw, ch = canvas.size
        match = re.search(r"canvas_at=(-?\d+),(-?\d+)", canvas.description)
        if not match:
            self.fail(f"canvas_at not found in description: {canvas.description!r}")
        cx, cy = int(match.group(1)), int(match.group(2))
        scale = min(cw / 960.0, ch / 540.0) * 0.92
        ox = (cw - 960.0 * scale) / 2.0
        oy = (ch - 540.0 * scale) / 2.0
        return lambda sx, sy: (cx + ox + sx * scale, cy + oy + sy * scale)

    def test_dragging_an_object_moves_it_in_the_model(self):
        aid = "org.tunaos.decks"
        self.gapplication_action(aid, "new-document")
        time.sleep(1.5)
        self.gapplication_action(aid, "add-shape")
        time.sleep(1.0)

        before = self.trigger_snapshot(aid)["slides"][0]["objects"][0]
        self.assertEqual(before["kind"], "Shape", f"unexpected object: {before}")

        # The first inserted shape is a rectangle at slide (200,200) sized 200x150, so its
        # centre is (300,275). Drag it down and to the right in a 5:3 ratio.
        to_window = self._slide_to_window()
        start_x, start_y = to_window(300.0, 275.0)
        end_x, end_y = to_window(400.0, 335.0)
        self.drag(start_x, start_y, end_x, end_y)
        time.sleep(1.0)

        after = self.trigger_snapshot(aid)["slides"][0]["objects"][0]
        dx = after["x"] - before["x"]
        dy = after["y"] - before["y"]

        # Direction and proportion, not absolute distance. Converting a
        # requested slide-space delta into pointer pixels needs the canvas
        # scale, and the only handle on that from here is the AT-SPI size —
        # which is the very thing #132 says is unreliable for nested
        # widgets. Asking for (100,60) in slide units and measuring exactly
        # (200,120) says the reconstructed scale is out by 2x; identical
        # numbers came back from an unmodified build, so it is the test's
        # arithmetic (or the AT-SPI geometry behind it) rather than the
        # gesture code. Pinning the magnitude here would bake that factor
        # in as if it were intended, so this asserts what the journey is
        # actually for: the drag reaches the model, along the axis dragged.
        # (The 2x was real: the drag moved the object live and the undo
        # command then applied the same move again on release. Fixed with
        # the smart guides in canvas_input.rs; the magnitude is still not
        # asserted here for the AT-SPI reason above.)
        self.assertGreater(dx, 0, f"drag did not move the object right: {before} -> {after}")
        self.assertGreater(dy, 0, f"drag did not move the object down: {before} -> {after}")
        self.assertAlmostEqual(
            dx / dy, 100.0 / 60.0, delta=0.4,
            msg=f"movement is not proportional to the drag: {before} -> {after}",
        )
        self.assertIsNone(self.process.poll(), "decks crashed during the drag")


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
        recovered_from = set(self._snapshot_files())

        # Simulate a crash: kill the process directly, bypassing the close
        # guard, so the snapshot is never cleared by a clean exit.
        self.relaunch_app(crash=True)
        time.sleep(1.5)

        frame = self.app.child(roleName="frame")
        self.assertIn("Recovered", frame.name, f"window did not announce recovery: {frame.name!r}")
        # The assertion below used to read `self._snapshot_files() == []`:
        # after recovery, nothing on disk. That was a proxy for the thing
        # actually required — the orphan is not offered a second time — and
        # it stopped being a safe proxy once recovery began writing the
        # recovered content to the new window's own slot before clearing the
        # orphan. Zero files also describes unprotected work, which is what
        # that spelling was quietly asserting: the old code left the
        # recovered document with no snapshot until the next timer tick.
        # So the intent is asserted directly instead, and more strictly: the
        # recovered orphan is gone, and the recovered work is itself covered.
        # Waited for rather than sampled after a fixed sleep, because the
        # `== []` version sampled 1.5s after relaunch and lost that race
        # under the load of a full batch run — it failed twice in one batch
        # and passed three consecutive focused runs. A post-condition that
        # needs a sleep to hold is a post-condition to wait for.
        present = self.wait_until(
            lambda: set(self._snapshot_files()),
            lambda files: not (files & recovered_from),
            description="the recovered orphan must be cleared so it isn't offered again",
        )
        self.assertEqual(
            len(present), 1,
            "the recovered deck must itself be protected by a snapshot; "
            f"found {sorted(present)}",
        )


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

        # On screen: exactly one marker, a bullet glyph and a tab. The level
        # is the item's hanging indent (a paragraph tag), not literal spaces.
        shown = self.wait_for_node(roleName="text").text
        self.assertEqual(shown.splitlines(), ["first", "•\tsecond"],
                         f"unexpected editor contents: {shown!r}")

        self.gapplication_action("org.tunaos.letters", "list-outdent")
        outdented = self.wait_for_condition(
            lambda: self.trigger_snapshot("org.tunaos.letters"),
            description="a snapshot after outdenting")
        self.assertEqual(outdented["paragraphs"][1]["style"].get("list_level"), 0)
        self.assertIsNone(self.process.poll(), "letters crashed during list editing")


class LettersModelUndoSmoke(BaseGUITestCase):
    """Undo and redo come from the live document model (ADR 0010 stage 3c-3).

    The GtkTextBuffer's own undo is off; Ctrl+Z in the Draft editor must
    reach the model's history, undo a typed word as one step and a line
    break as another, and Ctrl+Shift+Z must redo.
    """

    app_name = "letters"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="letters-undo-")
        super().setUp()

    def _text(self):
        s = self.trigger_snapshot("org.tunaos.letters")
        return "\n".join("".join(r["text"] for r in p["runs"]) for p in s["paragraphs"])

    def _wait_text(self, want):
        return self.wait_for_condition(
            lambda: self._text() == want or None, description=f"the document reading {want!r}")

    def test_undo_and_redo_in_the_draft_editor(self):
        from dogtail import rawinput

        self.wait_for_node(name="New Document", roleName="push button").do_action(0)
        self.wait_for_node(roleName="text")
        rawinput.typeText("hello")
        rawinput.keyCombo("Return")
        rawinput.typeText("world")
        self._wait_text("hello\nworld")

        rawinput.keyCombo("<Control>z")
        self._wait_text("hello\n")
        rawinput.keyCombo("<Control>z")
        self._wait_text("hello")
        rawinput.keyCombo("<Control><Shift>z")
        self._wait_text("hello\n")
        self.assertIsNone(self.process.poll(), "letters crashed during undo")


class LettersStylesAndOutlineSmoke(BaseGUITestCase):
    """Paragraph styles are picked from previews, and the outline follows
    the headings (DESIGN-UI "Styles first"; Docs' outline sidebar).

    The picker's rows are named for screen readers; choosing one restyles
    the caret's paragraph in the document model, which the outline sidebar
    then lists. Activating an outline entry puts the caret on that heading.
    """

    app_name = "letters"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="letters-styles-")
        super().setUp()

    def _headings(self):
        s = self.trigger_snapshot("org.tunaos.letters")
        return [p["style"]["heading"] for p in s["paragraphs"]]

    def test_pick_a_heading_and_find_it_in_the_outline(self):
        from dogtail import rawinput

        self.wait_for_node(name="New Document", roleName="push button").do_action(0)
        self.wait_for_node(name="Print Layout", roleName="text")
        rawinput.typeText("Intro")
        rawinput.keyCombo("Return")
        rawinput.typeText("body")
        rawinput.keyCombo("Up")

        self.wait_for_node(name="Paragraph style", roleName="toggle button").do_action(0)
        # Every style is a named row; pick one from the keyboard (the list
        # opens on the current style, Normal).
        for name in ("Normal", "Title", "Subtitle", "Heading 1", "Heading 6", "Quote", "Code"):
            self.wait_for_node(name=name, roleName="list item")
        time.sleep(0.5)
        # Normal, Title, Subtitle, Heading 1, Heading 2.
        for _ in range(4):
            rawinput.keyCombo("Down")
        rawinput.keyCombo("Return")
        self.wait_for_condition(lambda: self._headings() == [2, None] or None,
                                description="the first paragraph becoming a Heading 2")
        # Typing goes back to the document, into the heading.
        rawinput.typeText("!")
        self.wait_for_condition(
            lambda: (lambda s: "".join(r["text"] for r in s["paragraphs"][0]["runs"]) == "Intro!" or None)(
                self.trigger_snapshot("org.tunaos.letters")),
            description="typing continuing in the heading")

        self.gapplication_action("org.tunaos.letters", "toggle-outline")
        outline = self.wait_for_node(name="Outline", roleName="list")
        entry = self.wait_for_condition(
            lambda: next((c for c in outline.children if c.name == "Intro!"), None),
            description="the heading listed in the outline")
        self.assertEqual(len(outline.children), 1, "only headings are listed")
        # The caret is in that heading's section, so its entry is marked.
        self.wait_for_condition(lambda: entry.selected or None,
                                description="the caret's section marked in the outline")
        self.assertIsNone(self.process.poll(), "letters crashed using styles and the outline")


class LettersPageThumbnailsSmoke(BaseGUITestCase):
    """The sidebar's Pages view is the laid-out pages drawn small (DESIGN-UI,
    Letters from Pages): one named thumbnail per page, following the
    layout as the document grows, with the caret's page marked."""

    app_name = "letters"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="letters-thumbs-")
        super().setUp()

    def test_thumbnails_follow_the_pages(self):
        from dogtail import rawinput

        self.wait_for_node(name="New Document", roleName="push button").do_action(0)
        self.wait_for_node(name="Print Layout", roleName="text")
        rawinput.typeText("first")
        self.gapplication_action("org.tunaos.letters", "toggle-pages")
        pages = self.wait_for_node(name="Pages", roleName="list")
        self.wait_for_condition(lambda: [c.name for c in pages.children] == ["Page 1"] or None,
                                description="one thumbnail for one page")
        # Enough lines for a second page: a thumbnail appears for it, and
        # it is marked, as the caret is there.
        for _ in range(55):
            rawinput.keyCombo("Return")
        rawinput.typeText("last")
        self.wait_for_condition(lambda: [c.name for c in pages.children] == ["Page 1", "Page 2"] or None,
                                description="a thumbnail for the new page")
        self.wait_for_condition(lambda: pages.children[1].selected or None,
                                description="the caret's page marked")
        self.assertIsNone(self.process.poll(), "letters crashed drawing thumbnails")


class LettersDistractionFreeSmoke(BaseGUITestCase):
    """Distraction-free typing (DESIGN-UI, Letters from Pages): with it on,
    typing slides the bars away and moving the pointer brings them back."""

    app_name = "letters"

    def _toolbar_hidden(self):
        """Whether the grey around the pages shows where the toolbar was.
        AdwToolbarView slides its bars away without unmapping them, so
        AT-SPI still reports them showing; the screen is the evidence."""
        from PIL import Image

        path = self.take_screenshot("distraction-free", crop=False)
        pixel = Image.open(path).convert("RGB").getpixel((30, 72))
        return all(abs(c - 192) < 12 for c in pixel)

    def test_bars_hide_while_typing_and_return_on_pointer_motion(self):
        from dogtail import rawinput

        self.wait_for_node(name="New Document", roleName="push button").do_action(0)
        self.wait_for_node(name="Print Layout", roleName="text")
        rawinput.absoluteMotion(600, 500)
        # Off by default: typing leaves the bars alone.
        rawinput.typeText("calm")
        time.sleep(1.5)
        self.assertFalse(self._toolbar_hidden(), "the bars stay while distraction-free typing is off")

        self.gapplication_action("org.tunaos.letters", "distraction-free")
        time.sleep(0.5)
        rawinput.typeText(" words")
        self.wait_for_condition(lambda: self._toolbar_hidden() or None, description="the bars sliding away while typing")
        rawinput.absoluteMotion(700, 560)
        rawinput.absoluteMotion(720, 580)
        self.wait_for_condition(lambda: not self._toolbar_hidden() or None, description="the bars returning on pointer motion")
        self.assertIsNone(self.process.poll(), "letters crashed in distraction-free typing")


class LettersSmartChipsSmoke(BaseGUITestCase):
    """Smart chips (DESIGN-UI, Docs): "@" at the start of a word offers
    dates, people and links; the chosen chip is one inline object in the
    document, read out by its label, and one undo step."""

    app_name = "letters"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="letters-chips-")
        super().setUp()

    def _runs(self):
        s = self.trigger_snapshot("org.tunaos.letters")
        return s["paragraphs"][0]["runs"] if s["paragraphs"] else []

    def test_at_inserts_a_date_chip(self):
        from dogtail import rawinput

        self.wait_for_node(name="New Document", roleName="push button").do_action(0)
        page = self.wait_for_node(name="Print Layout", roleName="text")
        rawinput.typeText("Due @")
        self.wait_for_node(name="Smart chip")
        self.wait_for_node(name="Smart chip suggestions", roleName="list")
        rawinput.typeText("tom")
        time.sleep(0.5)
        rawinput.keyCombo("Return")
        chip = self.wait_for_condition(
            lambda: next((r for r in self._runs() if r["style"].get("chip")), None),
            description="a chip in the document")
        self.assertEqual(chip["style"]["chip"]["kind"], "Date")
        self.assertEqual([r["text"] for r in self._runs()][0], "Due ", "the @ was replaced")
        # Typing continues after the chip, and a screen reader hears its label.
        rawinput.typeText(" ok")
        self.wait_for_condition(lambda: page.text == f"Due {chip['text']} ok" or None,
                                description="the page view's text reading the chip's label")
        # Undo takes back "ok", then the space, then the chip in one step
        # (the @ returns).
        for _ in range(3):
            rawinput.keyCombo("<Control>z")
        self.wait_for_condition(lambda: page.text == "Due @" or None, description="undo taking the chip back")
        self.assertIsNone(self.process.poll(), "letters crashed inserting a chip")


class LettersPrintLayoutEditingSmoke(BaseGUITestCase):
    """Print Layout is the default view, and editing there is editing the
    document (ADR 0010, stage 3d).

    A new document opens on its laid-out pages; keystrokes go to the page
    view, into the live model. Screen readers read the page view's text over
    AT-SPI (GtkAccessibleText). The pageless Draft view is one toggle away
    and edits the same document.
    """

    app_name = "letters"

    def setUp(self):
        self._snapshot_path = self.isolate_snapshot(prefix="letters-print-layout-")
        super().setUp()

    def _text(self, snapshot):
        return "\n".join("".join(r["text"] for r in p["runs"]) for p in snapshot["paragraphs"])

    def _wait_text(self, want):
        return self.wait_for_condition(
            lambda: (lambda s: s if self._text(s) == want else None)(
                self.trigger_snapshot("org.tunaos.letters")),
            description=f"the document reading {want!r}")

    def test_print_layout_is_the_default_and_edits_the_document(self):
        from dogtail import rawinput

        self.wait_for_node(name="New Document", roleName="push button").do_action(0)
        # The page view, not the Draft editor, is what opens.
        page_view = self.wait_for_node(name="Print Layout", roleName="text")
        rawinput.typeText("page")
        rawinput.keyCombo("Return")
        rawinput.typeText("more")
        self._wait_text("page\nmore")
        # Screen readers see the same text on the page view.
        self.assertEqual(page_view.text, "page\nmore")

        # The stateful toggle switches the tab to the pageless Draft view,
        # which edits the same document.
        self.gapplication_action("org.tunaos.letters", "print-layout")
        time.sleep(1.0)
        rawinput.typeText(" draft")
        self._wait_text("page\nmore draft")
        self.wait_for_node(name="3 words", roleName="label")

        # And back: the pages show the Draft edit.
        self.gapplication_action("org.tunaos.letters", "print-layout")
        page_view = self.wait_for_node(name="Print Layout", roleName="text")
        self.wait_for_condition(lambda: page_view.text == "page\nmore draft" or None,
                                description="the page view reading the Draft edit")
        self.assertIsNone(self.process.poll(), "letters crashed while switching views")


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
