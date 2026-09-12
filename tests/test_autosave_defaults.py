"""A shipped app must actually autosave.

Letters shipped `auto-save-interval` with a default of `0` while Tables and
Decks shipped `60`, and `register_autosave` installs its timer only when the
value is positive:

    let interval = settings.int("auto-save-interval");
    if interval > 0 { glib::source::timeout_add_seconds_local(...) }

So a freshly installed Letters never wrote a crash snapshot on its own. Every
guarantee `docs/readiness-2026-09/recovery.md` makes about recovering unsaved
Letters documents held only when something invoked the `autosave-now` action —
and every Letters autosave journey invokes it explicitly, which is why they
all passed. They exercised the snapshot machinery and asserted nothing about
whether it ever runs unattended.

Nothing compared the three schemas, so the divergence sat there. This test is
that comparison. It checks the default rather than the timer because a default
is what a new install gets; that the timer fires once configured is asserted
in the GUI journey `LettersUnattendedAutosaveSmoke`, which waits for a
snapshot without triggering the action.
"""

import os
import re
import unittest
import xml.etree.ElementTree as ET

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FLATPAK = os.path.join(REPO_ROOT, "flatpak")
APPS = ("tables", "letters", "decks")
KEY = "auto-save-interval"

# The guard this test's reasoning depends on: zero means "never install a
# timer", so a zero default means a silent opt-out. If this disappears,
# the premise above needs rewriting rather than quietly passing.
GUARD = re.compile(r"let interval = settings\.int\(\"auto-save-interval\"\);\s*\n\s*if interval > 0 \{")


def _interval_key(app):
    tree = ET.parse(os.path.join(FLATPAK, f"org.tunaos.{app}.gschema.xml"))
    for key in tree.getroot().iter("key"):
        if key.get("name") == KEY:
            return key
    raise AssertionError(f"{app} has no {KEY} key")


class AutosaveDefaults(unittest.TestCase):
    def test_every_app_ships_a_positive_autosave_interval(self):
        for app in APPS:
            with self.subTest(app=app):
                default = int(_interval_key(app).find("default").text)
                self.assertGreater(
                    default, 0,
                    f"{app} ships {KEY}={default}; a non-positive default means "
                    "the autosave timer is never installed, so a new install "
                    "never writes a crash snapshot on its own",
                )

    def test_the_three_apps_agree_on_the_interval(self):
        defaults = {app: int(_interval_key(app).find("default").text) for app in APPS}
        self.assertEqual(
            len(set(defaults.values())), 1,
            f"the apps disagree on how often to autosave: {defaults}. One suite, "
            "one answer — divergence here is how Letters ended up shipping with "
            "autosave switched off while the other two ran every minute",
        )

    def test_the_default_is_inside_the_declared_range(self):
        for app in APPS:
            with self.subTest(app=app):
                key = _interval_key(app)
                default = int(key.find("default").text)
                rng = key.find("range")
                if rng is None:
                    self.skipTest(f"{app} declares no range for {KEY}")
                low, high = int(rng.get("min")), int(rng.get("max"))
                self.assertGreaterEqual(default, low, f"{app}: default {default} below min {low}")
                self.assertLessEqual(default, high, f"{app}: default {default} above max {high}")

    def test_zero_still_means_disabled_in_the_code(self):
        """The premise: a non-positive interval installs no timer."""
        src = open(os.path.join(REPO_ROOT, "letters", "src", "window.rs"), encoding="utf-8").read()
        self.assertRegex(
            src, GUARD,
            "letters no longer guards its autosave timer on `interval > 0`; "
            "this file's reasoning about why a zero default disables autosave "
            "needs rechecking",
        )


if __name__ == "__main__":
    unittest.main()
