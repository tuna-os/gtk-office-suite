#!/usr/bin/env python3
"""
GNOME HIG Design Review — evaluates each app's UI against GNOME Human Interface Guidelines.

The VLM reviews screenshots looking for:
- AdwApplicationWindow + ToolbarView pattern
- HeaderBar with proper start (new doc) / end (menu, dark mode) placement
- Toolbar: Icon consistency — toolbar now uses symbolic icons with tooltips per GNOME HIG
- Status bar with word count and zoom
- Empty state via AdwStatusPage
- Dark mode compatibility
- Responsive layout / breakpoint usage
- Tab view pattern (Letters)
- Slide sidebar + OverlaySplitView pattern (Decks)
- Formula bar + sheet tabs pattern (Tables)
- Spacing, margins, alignment consistency
- Button labeling and tooltip conventions
"""

import time
from framework import VisionGUITestCase


class LettersDesignReview(VisionGUITestCase):
    """GNOME HIG review for Letters (word processor)."""
    app_name = "letters"

    def test_gnome_hig_review(self):
        """Comprehensive design review of Letters against GNOME HIG."""
        # Create a document first so we see the full UI
        try:
            new_btn = self.app.child(name="New Document", roleName="push button")
            new_btn.do_action(0)
        except:
            self.app.keyCombo("<Control>n")
        time.sleep(1.0)

        self.take_screenshot("letters_design_review")
        self.assertVision([
            # Window chrome
            "The window should use AdwApplicationWindow with a HeaderBar containing a New Document button (left) and Menu button (right)",
            # Toolbar
            "The formatting toolbar should use GNOME symbolic ICONS for bold, italic, underline actions with tooltip text on hover",
            # Empty state when no doc
            "The style dropdown should be visible showing paragraph style options",
            "A ruler should be visible at the top of the editor area",
            # Status bar
            "A status bar at the bottom should show word count ('0 words') and zoom controls",
            # Tab view
            "A tab bar should be present showing 'Untitled Document' as the open tab",
            # Overall
            "The overall design should follow GNOME HIG conventions: consistent spacing, proper alignment, and libadwaita styling",
        ], screenshot_path=self.last_screenshot)


class LettersDarkModeDesignReview(VisionGUITestCase):
    """Dark mode design review for Letters."""
    app_name = "letters"

    def test_gnome_hig_dark_mode(self):
        """Verify dark mode design is coherent."""
        # Create a document
        try:
            new_btn = self.app.child(name="New Document", roleName="push button")
            new_btn.do_action(0)
        except:
            self.app.keyCombo("<Control>n")
        time.sleep(1.0)

        # Dark mode follows system theme per GNOME HIG (no toggle button)
        try:
            import subprocess
            subprocess.run(["gsettings", "set", "org.gnome.desktop.interface", "color-scheme", "prefer-dark"], timeout=5)
            time.sleep(0.5)
        except Exception as e:
            print(f"Could not set dark mode: {e}")

        self.take_screenshot("letters_dark_mode_design")
        self.assertVision([
            "The application should be in dark mode with dark backgrounds and light text",
            "All toolbar buttons and controls should be clearly readable against the dark background",
            "The tab bar, ruler, and status bar should all have consistent dark mode styling",
            "Color contrast should meet accessibility standards (text clearly visible on dark backgrounds)",
        ], screenshot_path=self.last_screenshot)


class DecksDesignReview(VisionGUITestCase):
    """GNOME HIG review for Decks (presentation editor)."""
    app_name = "decks"

    def test_gnome_hig_review(self):
        """Comprehensive design review of Decks against GNOME HIG."""
        try:
            new_btn = self.app.child(name="New Document", roleName="push button")
            new_btn.do_action(0)
        except:
            self.app.keyCombo("<Control>n")
        time.sleep(1.0)

        self.take_screenshot("decks_design_review")
        self.assertVision([
            # Window chrome
            "The window should use AdwApplicationWindow with HeaderBar proper",
            # Split view
            "A sidebar should show the slide list with 'Slide 1' using OverlaySplitView pattern",
            "The sidebar should have Add/Delete/Move controls at the bottom",
            # Toolbar
            "The toolbar should show formatting buttons (bold, italic, underline) as symbolic ICONS with tooltips",
            "The toolbar should have Add Text Box, Add Shape, Add Image, and Present buttons",
            # Canvas
            "A drawing canvas should be visible showing the current slide",
            # Overall
            "The layout should follow GNOME HIG conventions for creative/content apps",
        ], screenshot_path=self.last_screenshot)


class TablesDesignReview(VisionGUITestCase):
    """GNOME HIG review for Tables (spreadsheet)."""
    app_name = "tables"

    def test_gnome_hig_review(self):
        """Comprehensive design review of Tables against GNOME HIG."""
        try:
            new_btn = self.app.child(name="New Document", roleName="push button")
            new_btn.do_action(0)
        except:
            self.app.keyCombo("<Control>n")
        time.sleep(1.0)

        self.take_screenshot("tables_design_review")
        self.assertVision([
            # Window chrome
            "The window should use AdwApplicationWindow with HeaderBar",
            # Grid
            "A spreadsheet grid should be visible with column headers (A, B, C...) and row headers (1, 2, 3...)",
            "Gridlines should be visible between cells",
            # Formula bar
            "A formula bar (text entry field) should be above the spreadsheet grid",
            # Sheets
            "Sheet tabs at the bottom should show 'Sheet1' with a '+' button to add sheets",
            # Toolbar
            "The toolbar should contain spreadsheet-specific controls (sort, chart, etc.)",
            # Overall
            "The design should follow GNOME HIG conventions for data-entry applications",
        ], screenshot_path=self.last_screenshot)


if __name__ == "__main__":
    import unittest
    unittest.main()
