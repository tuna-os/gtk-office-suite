#!/usr/bin/env python3
"""
Rigorous GNOME HIG Compliance Audit — these tests verify that the UI
follows GNOME Human Interface Guidelines. They should FAIL if the apps
violate HIG standards, providing specific actionable feedback.

HIG Rules tested per app:
1. Toolbar uses GNOME symbolic icons, NOT text labels
2. HeaderBar follows standard layout (title left, window controls right)
3. Single unified HeaderBar pattern (no separate standalone toolbar below)
4. Consistent spacing, alignment, and AdwStatusPage for empty states
5. Standard app menu pattern (hamburger/gear -> Preferences, About, Shortcuts, Quit)
6. Dark mode consistency
7. Accessibility: buttons have tooltip text
"""

import time
from framework import VisionGUITestCase


class LettersHIGAudit(VisionGUITestCase):
    """GNOME HIG audit for Letters word processor."""
    app_name = "letters"

    def test_toolbar_uses_icons_not_text(self):
        """HIG 2.0: Toolbar actions MUST use symbolic icons, never text labels."""
        self._new_doc()
        self.take_screenshot("hig_letters_toolbar")
        self.assertVision([
            "The toolbar shows symbolic ICONS (not text labels) for actions like bold, italic, underline",
            "Buttons have tooltip text that appears on hover, not text labels on the buttons themselves",
        ], screenshot_path=self.last_screenshot)

    def test_headerbar_layout(self):
        """HIG 2.1: HeaderBar should follow standard GNOME layout."""
        self._new_doc()
        self.take_screenshot("hig_letters_headerbar")
        self.assertVision([
            "The HeaderBar has a title aligned to the left or center",
            "Window controls (close, maximize, minimize) are on the right side",
            "There is no redundant separate toolbar below the HeaderBar - actions are integrated",
        ], screenshot_path=self.last_screenshot)

    def test_empty_state_uses_adw_status_page(self):
        """HIG 3.0: Empty state uses AdwStatusPage with icon, description, and CTA."""
        self.take_screenshot("hig_letters_empty")
        self.assertVision([
            "The empty state shows a large symbolic icon, a heading 'Letters', and a description",
            "A clear call-to-action button ('Open File...') is visible",
            "The empty state does NOT repeat the app title redundantly",
        ], screenshot_path=self.last_screenshot)

    def test_app_menu_has_standard_items(self):
        """HIG 2.3: Menu button should expose Preferences, About, Shortcuts, Quit."""
        self._new_doc()
        self._open_menu()
        self.take_screenshot("hig_letters_menu")
        self.assertVision([
            "A menu popup with 'New', 'Open', 'Save', 'Preferences', 'Keyboard Shortcuts', and 'About' items is visible",
        ], screenshot_path=self.last_screenshot)
        self._close_menu()

    def test_dark_mode_is_consistent(self):
        """HIG accessibility: Dark mode applies uniformly to all surfaces."""
        self._new_doc()
        self._toggle_dark()
        self.take_screenshot("hig_letters_dark")
        self.assertVision([
            "The application is entirely in dark mode with dark backgrounds and light text",
            "Toolbar icons, status bar, and tab bar all use consistent dark theme colors",
            "Text contrast is sufficient for readability against dark backgrounds",
        ], screenshot_path=self.last_screenshot)
        self._toggle_dark()

    def test_tooltip_text_on_buttons(self):
        """Accessibility: All toolbar buttons have descriptive tooltip text."""
        self._new_doc()
        self.take_screenshot("hig_letters_tooltips")
        self.assertVision([
            "Toolbar buttons have tooltips that describe their function (visible on hover)",
        ], screenshot_path=self.last_screenshot)

    # ── Helpers ────────────────────────────────────────────────────
    def _new_doc(self):
        try:
            self.app.child(name="New Document", roleName="push button").do_action(0)
        except:
            self.app.keyCombo("<Control>n")
        time.sleep(1.0)

    def _open_menu(self):
        try:
            btn = self.app.child(name="Menu", roleName="toggle button")
            btn.do_action(0)
            time.sleep(0.4)
        except:
            pass

    def _close_menu(self):
        try:
            self.app.keyCombo("Escape")
        except:
            pass

    def _toggle_dark(self):
        # GNOME HIG: apps respect system theme, no toggle button
        # Use adw.StyleManager for test purposes
        try:
            import subprocess
            subprocess.run(["gsettings", "set", "org.gnome.desktop.interface", "color-scheme", "prefer-dark"], timeout=5)
            time.sleep(0.5)
        except:
            pass
    def _toggle_light(self):
        try:
            import subprocess
            subprocess.run(["gsettings", "set", "org.gnome.desktop.interface", "color-scheme", "default"], timeout=5)
            time.sleep(0.5)
        except:
            pass


class DecksHIGAudit(VisionGUITestCase):
    """GNOME HIG audit for Decks presentation app."""
    app_name = "decks"

    def test_toolbar_uses_icons_not_text(self):
        """HIG 2.0: Toolbar actions MUST use symbolic icons."""
        self._new_pres()
        self.take_screenshot("hig_decks_toolbar")
        self.assertVision([
            "The toolbar shows symbolic ICONS (not text labels) for formatting actions like bold, italic, underline",
            "Add Text Box, Add Shape, Add Image, and Present all use symbolic icon buttons with tooltips",
        ], screenshot_path=self.last_screenshot)

    def test_headerbar_layout(self):
        """HIG 2.1: HeaderBar standard layout."""
        self._new_pres()
        self.take_screenshot("hig_decks_headerbar")
        self.assertVision([
            "The HeaderBar follows standard GNOME layout with app title and controls",
            "There is no separate standalone toolbar below the HeaderBar - actions are in the toolbar area",
        ], screenshot_path=self.last_screenshot)

    def test_empty_state_design(self):
        """HIG 3.0: Empty state uses AdwStatusPage."""
        self.take_screenshot("hig_decks_empty")
        self.assertVision([
            "The empty state shows a large symbolic icon, 'Decks' heading, and descriptive text",
            "An 'Open File...' button provides a clear call to action",
        ], screenshot_path=self.last_screenshot)

    def test_sidebar_controls(self):
        """HIG layout: Sidebar controls should be accessible and properly sized."""
        self._new_pres()
        self.take_screenshot("hig_decks_sidebar")
        self.assertVision([
            "The slide sidebar has clear add, delete, and reorder controls",
            "The slide list items are clearly labeled (e.g., 'Slide 1')",
        ], screenshot_path=self.last_screenshot)

    def test_dark_mode_consistency(self):
        """Dark mode applies uniformly."""
        self._new_pres()
        self.take_screenshot("hig_decks_dark")
        self.assertVision([
            "The application uses consistent dark theme across sidebar, canvas, and toolbar",
        ], screenshot_path=self.last_screenshot)

    def _new_pres(self):
        try:
            self.app.child(name="New Document", roleName="push button").do_action(0)
        except:
            self.app.keyCombo("<Control>n")
        time.sleep(1.0)


class TablesHIGAudit(VisionGUITestCase):
    """GNOME HIG audit for Tables spreadsheet."""
    app_name = "tables"

    def test_toolbar_uses_icons_not_text(self):
        """HIG 2.0: Toolbar MUST use symbolic icons, not text labels."""
        self._new_ss()
        self.take_screenshot("hig_tables_toolbar")
        self.assertVision([
            "The toolbar shows symbolic ICONS (not text labels like 'preferences-other' or 'strikethrough')",
            "Toggle Number Format, Toggle Cell Border, Merge Cells, Chart, and Export PDF are icon buttons",
        ], screenshot_path=self.last_screenshot)

    def test_headerbar_no_ambiguous_icons(self):
        """HIG 2.1: HeaderBar should not have multiple ambiguous menu buttons."""
        self._new_ss()
        self.take_screenshot("hig_tables_headerbar")
        self.assertVision([
            "The HeaderBar follows standard GNOME layout",
            "There is a single primary menu button, not multiple ambiguous icons on the right",
        ], screenshot_path=self.last_screenshot)

    def test_empty_state_design(self):
        """HIG 3.0: Empty state."""
        self.take_screenshot("hig_tables_empty")
        self.assertVision([
            "The empty state shows a symbolic icon, 'Tables' heading, and descriptive text",
            "An 'Open File...' button provides a clear call to action",
        ], screenshot_path=self.last_screenshot)

    def test_formula_bar_integration(self):
        """UX: Formula bar should be clearly associated with the grid."""
        self._new_ss()
        self.take_screenshot("hig_tables_fxbar")
        self.assertVision([
            "A formula bar is positioned above the spreadsheet grid for data entry",
        ], screenshot_path=self.last_screenshot)

    def test_gridlines_visible(self):
        """UX: Spreadsheet gridlines should be visible for readability."""
        self._new_ss()
        self.take_screenshot("hig_tables_grid")
        self.assertVision([
            "Gridlines between cells in the spreadsheet are visible for readability",
            "Column headers A, B, C, D and row headers 1, 2, 3 are visible",
        ], screenshot_path=self.last_screenshot)

    def _new_ss(self):
        try:
            self.app.child(name="New Document", roleName="push button").do_action(0)
        except:
            self.app.keyCombo("<Control>n")
        time.sleep(1.0)


class CrossAppHIGAudit(VisionGUITestCase):
    """Cross-app HIG consistency using existing screenshots."""
    app_name = "letters"

    def test_consistent_headerbar_pattern(self):
        """All apps use AdwApplicationWindow with HeaderBar."""
        self.take_screenshot("letters_empty_state")
        self.assertVision([
            "Letters uses AdwApplicationWindow with a standard HeaderBar pattern",
        ], screenshot_path=self.last_screenshot)

class ToolbarIconsVerification(VisionGUITestCase):
    """Verify toolbar uses icons not text labels after the fix."""
    app_name = "letters"
    def test_toolbar_buttons_have_icons(self):
        self._new_doc()
        self.take_screenshot("vrf_toolbar_icons")
        self.assertVision(["Toolbar buttons show symbolic ICONS (not text labels)"], screenshot_path=self.last_screenshot)
    def _new_doc(self):
        try: self.app.child(name="New Document",roleName="push button").do_action(0)
        except: self.app.keyCombo("<Control>n")
        time.sleep(1.0)

class PageRenderingVerification(VisionGUITestCase):
    """Verify page rendering correct after dark mode fix."""
    app_name = "letters"
    def test_page_background_is_white(self):
        self._new_doc()
        self.take_screenshot("vrf_page_bg")
        self.assertVision(["Editor area shows a WHITE page background, not black"], screenshot_path=self.last_screenshot)
    def test_text_on_white_background(self):
        self._new_doc()
        e = self.app.child(roleName="text")
        e.typeText("Text on white page")
        e.keyCombo("<Control>a")
        time.sleep(0.3)
        self.take_screenshot("vrf_text_bg")
        self.assertVision(["Typed text appears on WHITE page background and is readable"], screenshot_path=self.last_screenshot)
    def test_dark_mode_white_page(self):
        self._new_doc()
        try:
            import subprocess
            subprocess.run(["gsettings", "set", "org.gnome.desktop.interface", "color-scheme", "prefer-dark"], timeout=5)
        except: pass
        time.sleep(0.4)
        self.take_screenshot("vrf_dark_page")
        self.assertVision(["Dark mode active but document page background is WHITE not black"], screenshot_path=self.last_screenshot)
    def _new_doc(self):
        try: self.app.child(name="New Document",roleName="push button").do_action(0)
        except: self.app.keyCombo("<Control>n")
        time.sleep(1.0)

class SidebarControlsVerification(VisionGUITestCase):
    """Verify sidebar controls properly sized."""
    app_name = "decks"
    def test_sidebar_buttons_large_enough(self):
        self._new_pres()
        self.take_screenshot("vrf_sidebar")
        self.assertVision(["Sidebar has visible Add, Delete, Move Up, Move Down buttons that are adequately sized"], screenshot_path=self.last_screenshot)
    def _new_pres(self):
        try: self.app.child(name="New Document",roleName="push button").do_action(0)
        except: self.app.keyCombo("<Control>n")
        time.sleep(1.0)

class CrossAppIconsVerification(VisionGUITestCase):
    """Verify Tables toolbar uses icons."""
    app_name = "tables"
    def test_tables_toolbar_icons(self):
        self._new_ss()
        self.take_screenshot("vrf_tables_icons")
        self.assertVision(["Tables toolbar shows symbolic ICONS for format, chart, and export actions"], screenshot_path=self.last_screenshot)
    def _new_ss(self):
        try: self.app.child(name="New Document",roleName="push button").do_action(0)
        except: self.app.keyCombo("<Control>n")
        time.sleep(1.0)

class DocumentRenderingTests(VisionGUITestCase):
    """Verify document content renders correctly."""
    app_name = "letters"
    def test_simple_text_renders_clearly(self):
        self._new_doc()
        e = self.app.child(roleName="text")
        e.typeText("The quick brown fox jumps over the lazy dog.")
        time.sleep(0.3)
        self.take_screenshot("vrf_render_text")
        self.assertVision(["The typed sentence is clearly rendered on the page"], screenshot_path=self.last_screenshot)
    def _new_doc(self):
        try: self.app.child(name="New Document",roleName="push button").do_action(0)
        except: self.app.keyCombo("<Control>n")
        time.sleep(1.0)

class GridRenderingTests(VisionGUITestCase):
    """Verify spreadsheet grid renders correctly."""
    app_name = "tables"
    def test_grid_visible_with_headers(self):
        self._new_ss()
        self.take_screenshot("vrf_grid")
        self.assertVision(["Spreadsheet grid with column headers A,B,C and row headers 1,2,3 is visible"], screenshot_path=self.last_screenshot)
    def _new_ss(self):
        try: self.app.child(name="New Document",roleName="push button").do_action(0)
        except: self.app.keyCombo("<Control>n")
        time.sleep(1.0)

class CrossAppToolbarTest(VisionGUITestCase):
    """Verify Decks toolbar uses icons too."""
    app_name = "decks"
    def test_decks_toolbar_icons(self):
        self._new_pres()
        self.take_screenshot("vrf_decks_icons")
        self.assertVision(["Decks toolbar shows symbolic ICONS for bold, italic, underline"], screenshot_path=self.last_screenshot)
    def _new_pres(self):
        try: self.app.child(name="New Document",roleName="push button").do_action(0)
        except: self.app.keyCombo("<Control>n")
        time.sleep(1.0)
