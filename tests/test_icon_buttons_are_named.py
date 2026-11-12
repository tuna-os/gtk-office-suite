"""B12 (accessibility): a Lifafa button drawn as an icon alone has a name.

An icon-only GtkButton has no label for a screen reader to speak, and GTK
falls back to its tooltip. This scans Lifafa's `Button::builder()` chains
for `.icon_name(...)` with no `.tooltip_text(...)`, `.label(...)`,
accessible label or child, and accepts one only when the variable it is
assigned to is given a tooltip or accessible label within the next lines
(the helper-function pattern).

Mandelbrot declares its buttons in Blueprint, where titles, template
properties and Rust set the names, so it is not scanned here.
"""

import glob
import os
import re
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NAMED = re.compile(r"tooltip_text|\.label\(|accessible|\.child\(")


def unnamed_icon_buttons():
    found = []
    for path in sorted(glob.glob(os.path.join(ROOT, "lifafa", "src", "**", "*.rs"), recursive=True)):
        src = open(path, encoding="utf-8").read()
        for m in re.finditer(r"(?:Toggle|Menu)?Button::builder\(\)(.*?)\.build\(\)", src, re.S):
            body = m.group(1)
            if "icon_name" not in body or NAMED.search(body):
                continue
            after = src[m.end():m.end() + 400]
            if re.search(r"set_tooltip_text|update_property|set_accessible", after):
                continue
            line = src.count("\n", 0, m.start()) + 1
            found.append("%s:%d" % (os.path.relpath(path, ROOT), line))
    return found


class IconButtons(unittest.TestCase):
    def test_every_icon_only_button_has_a_name(self):
        self.assertEqual([], unnamed_icon_buttons())


if __name__ == "__main__":
    unittest.main()
