#!/usr/bin/env python3
"""Decks GUI tests — every test exercises a feature and verifies the result."""

import time
from framework import VisionGUITestCase

class DecksTest(VisionGUITestCase):
    app_name = "decks"

    def _new(self):
        try:
            self.app.child(name="New Document", roleName="push button").do_action(0)
        except Exception:
            self.app.keyCombo("<Control>n")
        time.sleep(1.0)

    def _click(self, name, role="toggle button"):
        try:
            self.app.child(name=name, roleName=role).do_action(0)
        except Exception:
            try:
                self.app.child(label=name, roleName=role).do_action(0)
            except Exception:
                pass

    def test_empty_state_shows_decks_branding(self):
        self.take_screenshot("empty")
        self.assertVision([
            "Window title displays 'Decks'",
            "Empty state page shows heading 'Decks'",
            "An 'Open File...' button is visible",
        ], screenshot_path=self.last_screenshot)

    def test_new_presentation_shows_canvas_and_sidebar(self):
        self._new()
        self.take_screenshot("new")
        self.assertVision([
            "A white slide canvas is visible in the center of the window",
            "A sidebar on the left lists 'Slide 1'",
            "Sidebar has Add Slide and Delete Slide buttons",
            "Toolbar shows formatting buttons (bold, italic, underline) and insert controls",
        ], screenshot_path=self.last_screenshot)

    def test_add_slide_creates_slide_2(self):
        self._new()
        self._click("Add Slide", "push button")
        time.sleep(0.5)
        self.take_screenshot("add")
        self.assertVision([
            "Sidebar shows both 'Slide 1' and 'Slide 2' in the list",
        ], screenshot_path=self.last_screenshot)

    def test_add_four_slides_all_labeled(self):
        self._new()
        for _ in range(3):
            self._click("Add Slide", "push button")
            time.sleep(0.3)
        self.take_screenshot("four")
        self.assertVision([
            "Sidebar lists four slides: 'Slide 1', 'Slide 2', 'Slide 3', and 'Slide 4'",
        ], screenshot_path=self.last_screenshot)

    def test_delete_slide_removes_from_sidebar(self):
        self._new()
        self._click("Add Slide", "push button")
        time.sleep(0.3)
        self._click("Delete Slide", "push button")
        time.sleep(0.5)
        self.take_screenshot("delete")
        self.assertVision([
            "Sidebar shows only 'Slide 1' — the added slide was removed",
        ], screenshot_path=self.last_screenshot)

    def test_bold_button_toggles_visually(self):
        self._new()
        self._click("Bold")
        time.sleep(0.3)
        self.take_screenshot("bold")
        self.assertVision([
            "The Bold toolbar button appears in a pressed/active state",
        ], screenshot_path=self.last_screenshot)

    def test_italic_button_toggles_visually(self):
        self._new()
        self._click("Italic")
        time.sleep(0.3)
        self.take_screenshot("italic")
        self.assertVision([
            "The Italic toolbar button appears in a pressed/active state",
        ], screenshot_path=self.last_screenshot)

    def test_underline_button_toggles_visually(self):
        self._new()
        self._click("Underline")
        time.sleep(0.3)
        self.take_screenshot("underline")
        self.assertVision([
            "The Underline toolbar button appears in a pressed/active state",
        ], screenshot_path=self.last_screenshot)

    def test_add_text_box_inserts_on_canvas(self):
        self._new()
        self._click("Add Text Box", "push button")
        time.sleep(0.5)
        self.take_screenshot("textbox")
        self.assertVision([
            "A text box or text area has been added to the slide canvas",
        ], screenshot_path=self.last_screenshot)

    def test_add_shape_inserts_shape_on_canvas(self):
        self._new()
        self._click("Add Shape", "push button")
        time.sleep(0.5)
        self.take_screenshot("shape")
        self.assertVision([
            "A shape (rectangle or circle) has been added to the slide canvas",
        ], screenshot_path=self.last_screenshot)

    def test_add_image_button_opens_file_dialog(self):
        self._new()
        self._click("Add Image", "push button")
        time.sleep(0.5)
        self.take_screenshot("image")
        self.assertVision([
            "A file open dialog or file picker window is visible for selecting an image",
        ], screenshot_path=self.last_screenshot)

    def test_speaker_notes_panel_is_under_the_slide(self):
        # The notes pane is always there under the slide (notes_pane.rs);
        # there is no expander to open any more.
        self._new()
        time.sleep(0.5)
        self.take_screenshot("notes")
        self.assertVision([
            "A speaker notes panel or text area is visible, likely below the slide canvas",
        ], screenshot_path=self.last_screenshot)

    def test_present_button_exists_in_toolbar(self):
        self._new()
        self.take_screenshot("present")
        self.assertVision([
            "The toolbar contains a Present or fullscreen button for starting slideshow mode",
        ], screenshot_path=self.last_screenshot)

    def test_canvas_has_proper_slide_dimensions(self):
        self._new()
        self.take_screenshot("canvas")
        self.assertVision([
            "A white slide canvas is displayed in the center with standard presentation dimensions",
        ], screenshot_path=self.last_screenshot)

    def test_slide_sidebar_has_move_controls(self):
        self._new()
        self._click("Add Slide", "push button")
        time.sleep(0.3)
        self.take_screenshot("sidebar")
        self.assertVision([
            "Sidebar shows two slides with Move Up and Move Down buttons in the controls",
        ], screenshot_path=self.last_screenshot)

    def test_undo_removes_added_slide(self):
        self._new()
        self._click("Add Slide", "push button")
        time.sleep(0.3)
        self.app.keyCombo("<Control>z")
        time.sleep(0.5)
        self.take_screenshot("undo")
        self.assertVision([
            "Sidebar shows only 'Slide 1' after undo removes the added slide",
        ], screenshot_path=self.last_screenshot)

    def test_preferences_shows_snap_to_grid(self):
        self._new()
        try:
            self.app.keyCombo("<Control>comma")
        except Exception:
            pass
        time.sleep(0.5)
        self.take_screenshot("prefs")
        self.assertVision([
            "A Preferences dialog is visible with a 'Snap to Grid' toggle switch",
        ], screenshot_path=self.last_screenshot)
        try:
            self.app.keyCombo("Escape")
        except Exception:
            pass

    def test_full_presentation_workflow(self):
        """Full workflow: 4 slides with formatting selected."""
        self._new()
        for _ in range(3):
            self._click("Add Slide", "push button")
            time.sleep(0.25)
        self._click("Bold")
        time.sleep(0.3)
        self.take_screenshot("final")
        self.assertVision([
            "Sidebar lists 4 slides: 'Slide 1' through 'Slide 4'",
            "The Bold toolbar button is active/pressed",
            "Toolbar with formatting and insert controls is visible",
        ], screenshot_path=self.last_screenshot)
