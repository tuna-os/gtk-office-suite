"""The GTK-diagnostics parser the GUI harness fails journeys on (#1209)."""

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "gui", "framework"))

import gtk_diagnostics as gd  # noqa: E402

STDERR = """\
libEGL warning: DRI3 error: Could not get DRI3 device
SpellChecker: loaded /usr/share/hunspell/en_US
(letters:4242): Gtk-CRITICAL **: 12:01:02.345: gtk_widget_unparent: assertion 'GTK_IS_WIDGET (widget)' failed
(tables:77): GLib-GObject-WARNING **: 12:01:03.000: invalid cast from 'GtkLabel' to 'GtkButton'
Adwaita-WARNING **: 12:01:04.100: AdwDialog must have a child
(decks:9): Gtk-WARNING **: Failed to set text 'A & B' from markup due to error parsing markup
"""


class GtkDiagnostics(unittest.TestCase):
    def test_criticals_and_warnings_are_found_in_order(self):
        found = gd.parse(STDERR)
        self.assertEqual([(d["domain"], d["level"]) for d in found], [
            ("Gtk", "CRITICAL"), ("GLib-GObject", "WARNING"), ("Adwaita", "WARNING"), ("Gtk", "WARNING"),
        ])
        self.assertEqual(found[0]["message"], "gtk_widget_unparent: assertion 'GTK_IS_WIDGET (widget)' failed")
        self.assertTrue(found[3]["message"].startswith("Failed to set text 'A & B'"))

    def test_only_criticals_fail_a_journey(self):
        self.assertEqual([d["domain"] for d in gd.criticals(gd.parse(STDERR))], ["Gtk"])

    def test_other_stderr_is_not_a_diagnostic(self):
        self.assertEqual(gd.parse("libEGL warning: DRI3 error\nwarning: something\nCRITICAL failure\n"), [])
        self.assertEqual(gd.parse(""), [])
        self.assertEqual(gd.parse(None), [])


if __name__ == "__main__":
    unittest.main()
