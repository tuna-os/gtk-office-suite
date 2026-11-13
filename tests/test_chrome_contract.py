"""C1 (one chrome): every suite app answers the same standard keys and has
the same standard items in its main menu (docs/INTEGRATION-PLAN.md).

Lifafa and Mandelbrot are separate workspaces that do not link
`suite-common`, so the chrome cannot be one code path yet; this checks the
sources instead, and fails when an app drops one of the standard pieces:

- Ctrl+Q quits,
- Ctrl+, opens Preferences (Mandelbrot: Account Settings),
- a Keyboard Shortcuts item, and
- an About item.

Letters, Tables and Decks also show their toasts through suite-common's
`toast_manager`, never with an `adw::Toast` of their own.
"""

import os
import re
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def read(*parts):
    with open(os.path.join(ROOT, *parts), encoding="utf-8") as f:
        return f.read()


def accel_for(source, action):
    """The accelerators bound to `action` by set_accels_for_action."""
    found = []
    for m in re.finditer(r'set_accels_for_action\(\s*(?:&app,\s*)?"%s",\s*&\[([^\]]*)\]' % re.escape(action), source):
        found += re.findall(r'"([^"]+)"', m.group(1))
    return [a.lower().replace("<ctrl>", "<control>") for a in found]


class SuiteApps(unittest.TestCase):
    """Letters, Tables and Decks share suite-common's chrome."""

    def setUp(self):
        self.src = read("suite-common", "src", "lib.rs")

    def test_quit_is_ctrl_q(self):
        self.assertIn("<control>q", accel_for(self.src, "app.quit"))

    def test_preferences_is_ctrl_comma(self):
        self.assertIn("<control>comma", accel_for(self.src, "app.preferences"))

    def test_the_main_menu_has_shortcuts_and_about(self):
        self.assertIn('Some("app.shortcuts")', self.src)
        self.assertIn('Some("app.about")', self.src)

    def test_toasts_go_through_suite_common(self):
        """Toasts are shown by suite_common::toast_manager, so they stay as
        long in every app (its `timeout_for`) instead of each call site
        picking a number."""
        import glob
        own = []
        for app in ("letters", "tables", "decks"):
            for path in sorted(glob.glob(os.path.join(ROOT, app, "src", "**", "*.rs"), recursive=True)):
                with open(path, encoding="utf-8") as f:
                    for n, line in enumerate(f, 1):
                        if re.search(r"\bToast::(new|builder)\b", line):
                            own.append("%s:%d" % (os.path.relpath(path, ROOT), n))
        self.assertEqual([], own)


    def test_a_plain_message_goes_through_suite_common(self):
        """A dialog with nothing but an OK button is
        suite_common::dialogs::show_message (or show_error_dialog), so it
        answers Enter and Escape the same in every app. One that does
        something when it closes, or offers a choice, is built by hand."""
        import glob
        own = []
        for app in ("letters", "tables", "decks"):
            for path in sorted(glob.glob(os.path.join(ROOT, app, "src", "**", "*.rs"), recursive=True)):
                with open(path, encoding="utf-8") as f:
                    src = f.read()
                for m in re.finditer(r"AlertDialog::(?:new|builder)\(", src):
                    window = src[m.end():m.end() + 700]
                    end = window.find(".present(")
                    if end < 0:
                        continue
                    body = window[:end]
                    responses = re.findall(r'add_response\("(\w+)"', body)
                    if responses == ["ok"] and "connect_" not in body:
                        own.append("%s:%d" % (os.path.relpath(path, ROOT), src.count("\n", 0, m.start()) + 1))
        self.assertEqual([], own)


class Lifafa(unittest.TestCase):
    def test_quit_is_ctrl_q(self):
        self.assertIn("<control>q", accel_for(read("lifafa", "src", "app", "window.rs"), "app.quit"))

    def test_preferences_is_ctrl_comma(self):
        self.assertRegex(read("lifafa", "src", "app", "keymap.rs"), r"Key::comma[^\n]*\n?[^\n]*OpenPreferences")

    def test_it_has_shortcuts_and_about(self):
        app = read("lifafa", "src", "app.rs")
        self.assertIn('"shortcuts"', app)
        self.assertIn('"about"', app)


class Mandelbrot(unittest.TestCase):
    def test_quit_is_ctrl_q(self):
        self.assertIn("<control>q", accel_for(read("mandelbrot", "src", "application.rs"), "app.quit"))

    def test_account_settings_is_ctrl_comma(self):
        self.assertIn("<ctrl>comma", read("mandelbrot", "src", "shortcuts-dialog.blp"))

    def test_the_main_menu_has_shortcuts_and_about(self):
        menu = read("mandelbrot", "src", "session_view", "sidebar", "mod.blp")
        self.assertIn('action: "app.shortcuts"', menu)
        self.assertIn('action: "app.about"', menu)


if __name__ == "__main__":
    unittest.main()
