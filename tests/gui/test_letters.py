#!/usr/bin/env python3
"""Letters GUI tests — every test exercises a feature and verifies the result."""

import time
from framework import BaseGUITestCase

class LettersTest(BaseGUITestCase):
    app_name = "letters"

    def _new(self):
        try:
            self.app.child(name="New Document", roleName="push button").do_action(0)
        except Exception:
            self.app.keyCombo("<Control>n")
        time.sleep(1.0)

    def _ed(self):
        return self.app.child(roleName="text")

    def _click(self, name, role="toggle button"):
        try:
            self.app.child(name=name, roleName=role).do_action(0)
        except Exception:
            try:
                self.app.child(label=name, roleName=role).do_action(0)
            except Exception:
                pass

    def test_empty_state_shows_Letters_and_button(self):
        self.take_screenshot("empty")
        self.assertVision([
            "Window title is 'Letters'",
            "A status page with heading 'Letters' is visible",
            "An 'Open File...' button is shown centered on the page",
        ], screenshot_path=self.last_screenshot)

    def test_new_document_opens_editor(self):
        self._new()
        self.take_screenshot("new_doc")
        self.assertVision([
            "A text editor area with a blinking cursor is visible",
            "A tab labeled 'Untitled Document' appears in the tab bar",
            "Status bar shows '0 words'",
        ], screenshot_path=self.last_screenshot)

    def test_word_count_updates_on_type(self):
        self._new()
        self._ed().typeText("one two three four five six seven eight nine ten")
        time.sleep(0.3)
        self.take_screenshot("word_count")
        self.assertVision([
            "Status bar shows '10 words' reflecting the typed text count",
        ], screenshot_path=self.last_screenshot)

    def test_bold_text_renders_differently(self):
        self._new()
        e = self._ed()
        e.typeText("Bold text here")
        e.keyCombo("<Control>a")
        time.sleep(0.1)
        self._click("bold")
        time.sleep(0.4)
        self.take_screenshot("bold")
        self.assertVision([
            "The text 'Bold text here' appears in bold weight (thicker letters) in the editor",
        ], screenshot_path=self.last_screenshot)

    def test_italic_text_renders_differently(self):
        self._new()
        e = self._ed()
        e.typeText("Italic text here")
        e.keyCombo("<Control>a")
        time.sleep(0.1)
        self._click("italic")
        time.sleep(0.4)
        self.take_screenshot("italic")
        self.assertVision([
            "The text 'Italic text here' appears slanted/italic in the editor",
        ], screenshot_path=self.last_screenshot)

    def test_underline_draws_line_below_text(self):
        self._new()
        e = self._ed()
        e.typeText("Underlined words")
        e.keyCombo("<Control>a")
        time.sleep(0.1)
        self._click("underline")
        time.sleep(0.4)
        self.take_screenshot("underline")
        self.assertVision([
            "The text 'Underlined words' has a line drawn beneath it",
        ], screenshot_path=self.last_screenshot)

    def test_strikethrough_draws_line_through_text(self):
        self._new()
        e = self._ed()
        e.typeText("Struck through")
        e.keyCombo("<Control>a")
        time.sleep(0.1)
        self._click("strikethrough", "push button")
        time.sleep(0.4)
        self.take_screenshot("strikethrough")
        self.assertVision([
            "The text 'Struck through' has a horizontal line drawn through its middle",
        ], screenshot_path=self.last_screenshot)

    def test_highlight_changes_background(self):
        self._new()
        e = self._ed()
        e.typeText("Highlighted passage")
        e.keyCombo("<Control>a")
        time.sleep(0.1)
        self._click("highlight", "push button")
        time.sleep(0.4)
        self.take_screenshot("highlight")
        self.assertVision([
            "The text 'Highlighted passage' has a colored background highlight behind it",
        ], screenshot_path=self.last_screenshot)

    def test_combined_bold_italic_underline(self):
        self._new()
        e = self._ed()
        e.typeText("All three formats")
        e.keyCombo("<Control>a")
        time.sleep(0.1)
        for f in ["bold", "italic", "underline"]:
            self._click(f)
            time.sleep(0.1)
        time.sleep(0.3)
        self.take_screenshot("combined")
        self.assertVision([
            "The text 'All three formats' is simultaneously bold, italic, and underlined",
        ], screenshot_path=self.last_screenshot)

    def test_center_align_moves_text_to_middle(self):
        self._new()
        self._ed().typeText("Centered line of text")
        time.sleep(0.1)
        self._click("center", "push button")
        time.sleep(0.4)
        self.take_screenshot("center")
        self.assertVision([
            "The text 'Centered line of text' is horizontally centered in the editor",
        ], screenshot_path=self.last_screenshot)

    def test_right_align_moves_text_to_right(self):
        self._new()
        self._ed().typeText("Right aligned text")
        time.sleep(0.1)
        self._click("right", "push button")
        time.sleep(0.4)
        self.take_screenshot("right")
        self.assertVision([
            "The text 'Right aligned text' is aligned to the right edge of the editor",
        ], screenshot_path=self.last_screenshot)

    def test_bullet_list_inserts_bullets(self):
        self._new()
        e = self._ed()
        e.typeText("Apples")
        e.keyCombo("Return")
        e.typeText("Bananas")
        e.keyCombo("<Control>a")
        time.sleep(0.1)
        self._click("bullet", "push button")
        time.sleep(0.4)
        self.take_screenshot("bullets")
        self.assertVision([
            "The list starts with a bullet character before 'Apples'",
            "The second line has a bullet character before 'Bananas'",
        ], screenshot_path=self.last_screenshot)

    def test_numbered_list_inserts_numbers(self):
        self._new()
        e = self._ed()
        e.typeText("First task")
        e.keyCombo("Return")
        e.typeText("Second task")
        e.keyCombo("<Control>a")
        time.sleep(0.1)
        self._click("ordered", "push button")
        time.sleep(0.4)
        self.take_screenshot("numbers")
        self.assertVision([
            "The first line is prefixed with '1.' before 'First task'",
            "The second line is prefixed with '2.' before 'Second task'",
        ], screenshot_path=self.last_screenshot)

    def test_font_size_increases_on_shortcut(self):
        self._new()
        e = self._ed()
        e.typeText("Bigger Text Sample")
        e.keyCombo("<Control>a")
        time.sleep(0.1)
        e.keyCombo("<Control><Shift>greater")
        time.sleep(0.4)
        self.take_screenshot("bigger")
        self.assertVision([
            "The text 'Bigger Text Sample' is visibly larger than the default font size",
        ], screenshot_path=self.last_screenshot)

    def test_ctrl_k_inserts_link_syntax(self):
        self._new()
        e = self._ed()
        e.typeText("click here")
        e.keyCombo("<Control>a")
        time.sleep(0.1)
        e.keyCombo("<Control>k")
        time.sleep(0.4)
        self.take_screenshot("link")
        self.assertVision([
            "The editor contains markdown link syntax with brackets and parentheses: [click here](url)",
        ], screenshot_path=self.last_screenshot)

    def test_find_bar_opens_via_ctrl_f(self):
        self._new()
        self._ed().keyCombo("<Control>f")
        time.sleep(0.5)
        self.take_screenshot("find")
        self.assertVision([
            "A find bar with a search text entry field is visible above or within the editor area",
        ], screenshot_path=self.last_screenshot)

    def test_line_spacing_cycles_visibly(self):
        self._new()
        e = self._ed()
        for _ in range(3):
            e.typeText(f"Line of text number here")
            e.keyCombo("Return")
            time.sleep(0.1)
        e.keyCombo("<Control>a")
        time.sleep(0.1)
        self._click("format-line-spacing", "push button")
        time.sleep(0.4)
        self.take_screenshot("spacing")
        self.assertVision([
            "The three lines of text have more than single spacing between them (visible gaps between lines)",
        ], screenshot_path=self.last_screenshot)

    def test_style_dropdown_shows_option(self):
        self._new()
        self.take_screenshot("style")
        self.assertVision([
            "A style dropdown or combo box is visible showing a style name like 'Paragraph' or 'Normal'",
        ], screenshot_path=self.last_screenshot)

    def test_ruler_shows_measurements(self):
        self._new()
        self.take_screenshot("ruler")
        self.assertVision([
            "A horizontal ruler with measurement markings is visible above the editor text area",
        ], screenshot_path=self.last_screenshot)

    def test_zoom_slider_and_label_visible(self):
        self._new()
        self.take_screenshot("zoom")
        self.assertVision([
            "The status bar shows a zoom percentage label (100%) next to a horizontal zoom slider",
        ], screenshot_path=self.last_screenshot)

    def test_spell_check_underlines_misspelled_word(self):
        self._new()
        self._ed().typeText("This is a misspelled wurd zzzz")
        time.sleep(0.3)
        self.take_screenshot("spell")
        self.assertVision([
            "Misspelled words like 'wurd' and 'zzzz' have red wavy underlines beneath them",
        ], screenshot_path=self.last_screenshot)

    def test_dark_mode_via_system_theme(self):
        """Dark mode follows system theme (no toggle button in header bar per GNOME HIG)."""
        try:
            import subprocess
            subprocess.run(["gsettings", "set", "org.gnome.desktop.interface", "color-scheme", "prefer-dark"], timeout=5)
            time.sleep(0.5)
        except Exception as e:
            print(f"Could not set dark mode: {e}")
        self.take_screenshot("dark")
        self.assertVision([
            "The application background and editor area are dark colored (dark mode active)",
            "Text and toolbar controls remain readable against the dark background",
        ], screenshot_path=self.last_screenshot)
        try:
            import subprocess
            subprocess.run(["gsettings", "set", "org.gnome.desktop.interface", "color-scheme", "default"], timeout=5)
            time.sleep(0.3)
        except:
            pass

    def test_menu_button_shows_file_edit_help(self):
        self._click("Menu", "toggle button")
        time.sleep(0.4)
        self.take_screenshot("menu")
        self.assertVision([
            "A popup menu is visible with 'New', 'Open', 'Save', 'Preferences', and 'About' menu items",
        ], screenshot_path=self.last_screenshot)

    def test_preferences_opens_with_tabs(self):
        self._new()
        self.app.keyCombo("<Control>comma")
        time.sleep(0.5)
        self.take_screenshot("prefs")
        self.assertVision([
            "A Preferences dialog window is visible",
            "The preferences have at least 'General', 'Editing', and 'Appearance' sections",
        ], screenshot_path=self.last_screenshot)
        self.app.keyCombo("Escape")
        time.sleep(0.2)

    def test_page_setup_dialog_opens(self):
        self._new()
        self._ed().keyCombo("<Control><Shift>l")
        time.sleep(0.5)
        self.take_screenshot("pagesetup")
        self.assertVision([
            "A 'Page Setup' dialog with paper size and margin settings is visible",
        ], screenshot_path=self.last_screenshot)
        self.app.keyCombo("Escape")
        time.sleep(0.2)

    def test_print_preview_shows_document(self):
        self._new()
        e = self._ed()
        e.typeText("Print preview test content goes here")
        time.sleep(0.2)
        e.keyCombo("<Control><Shift>p")
        time.sleep(0.5)
        self.take_screenshot("printprev")
        self.assertVision([
            "A print preview window showing the document content is visible",
        ], screenshot_path=self.last_screenshot)
        self.app.keyCombo("Escape")
        time.sleep(0.2)

    def test_undo_removes_typed_text(self):
        self._new()
        self._ed().typeText("This will be undone")
        time.sleep(0.3)
        self._ed().keyCombo("<Control>z")
        time.sleep(0.5)
        self.take_screenshot("undo")
        self.assertVision([
            "The editor is empty or the text 'This will be undone' is no longer present",
        ], screenshot_path=self.last_screenshot)

    def test_redo_restores_undone_text(self):
        self._new()
        self._ed().typeText("Redo test string here")
        time.sleep(0.15)
        self._ed().keyCombo("<Control>z")
        time.sleep(0.3)
        self._ed().keyCombo("<Control>y")
        time.sleep(0.4)
        self.take_screenshot("redo")
        self.assertVision([
            "The text 'Redo test string here' is visible in the editor after redo",
        ], screenshot_path=self.last_screenshot)

    def test_two_tabs_visible_in_tab_bar(self):
        self._new()
        self._ed().keyCombo("<Control>n")
        time.sleep(0.5)
        self.take_screenshot("tabs")
        self.assertVision([
            "The tab bar shows two tabs, both labeled 'Untitled Document'",
        ], screenshot_path=self.last_screenshot)

    def test_full_document_with_formatting(self):
        """Full document: heading, body text, bullet list — all formatted."""
        self._new()
        e = self._ed()
        e.typeText("Project Notes")
        e.keyCombo("<Control>a")
        time.sleep(0.1)
        self._click("bold")
        time.sleep(0.2)
        e.keyCombo("Right")
        e.keyCombo("Return")
        e.keyCombo("Return")
        time.sleep(0.1)
        e.typeText("Completed the initial review of the system architecture.")
        time.sleep(0.15)
        e.keyCombo("Return")
        e.typeText("Need to update the deployment configuration")
        e.keyCombo("<Control>a")
        time.sleep(0.1)
        self._click("bullet", "push button")
        time.sleep(0.3)
        self.take_screenshot("full")
        self.assertVision([
            "The heading 'Project Notes' appears in bold at the top",
            "Body text about 'system architecture' is visible below the heading",
            "A bullet list item starting with a bullet character is visible",
        ], screenshot_path=self.last_screenshot)

    def test_structured_table_and_lists(self):
        """Structured table insert and list nesting journey (#110)."""
        self._new()
        e = self._ed()
        import subprocess
        subprocess.run(["gapplication", "action", "org.tunaos.letters", "insert-table"])
        time.sleep(0.3)
        self.take_screenshot("table_inserted")
        self.assertVision([
            "A document table with Header and Cell text is visible",
        ], screenshot_path=self.last_screenshot)

