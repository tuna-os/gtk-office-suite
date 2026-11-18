"""B12 (accessibility): a Lifafa button drawn as an icon alone has a name.

An icon-only GtkButton has no label for a screen reader to speak, and GTK
falls back to its tooltip. This scans Lifafa's `Button::builder()` chains
for `.icon_name(...)` with no `.tooltip_text(...)`, `.label(...)`,
accessible label or child, and accepts one only when the variable it is
assigned to is given a tooltip or accessible label within the next lines
(the helper-function pattern).

Mandelbrot declares its buttons in Blueprint. There a button whose own
properties include `icon-name` needs `tooltip-text`, a label, an
`accessibility { label: ... }` block or a child, or else an id that its
widget's Rust gives a tooltip (`self.<id>.set_tooltip_text`, the template
property pattern: CopyableRow's copy button, ActionButton's default one).
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


BLP_BUTTON = re.compile(r"\b(?:Gtk\.|Adw\.)?(?:Button|ToggleButton|MenuButton|SplitButton)(?:\s+(\w+))?\s*\{")
BLP_NAMED = re.compile(r"tooltip-text|tooltip-markup|\blabel:|accessibility\s*\{|\bchild:")


def _block(src, start):
    """The text inside the braces that open just before `start`."""
    depth, i = 1, start
    while depth and i < len(src):
        depth += {"{": 1, "}": -1}.get(src[i], 0)
        i += 1
    return src[start:i - 1]


def _own_properties(body):
    """`body` without its nested blocks: the button's own properties."""
    while True:
        flat = re.sub(r"\{[^{}]*\}", "", body)
        if flat == body:
            return flat
        body = flat


def unnamed_blueprint_icon_buttons():
    found = []
    for path in sorted(glob.glob(os.path.join(ROOT, "mandelbrot", "src", "**", "*.blp"), recursive=True)):
        src = open(path, encoding="utf-8").read()
        rust = os.path.splitext(path)[0] + ".rs"
        if not os.path.exists(rust):
            rust = os.path.join(os.path.dirname(path), "mod.rs")
        rust_src = open(rust, encoding="utf-8").read() if os.path.exists(rust) else ""
        for m in BLP_BUTTON.finditer(src):
            body = _block(src, m.end())
            own = _own_properties(body)
            if "icon-name" not in own or BLP_NAMED.search(body):
                continue
            # A child widget (a spinner, a box) carries its own content.
            if re.search(r"^\s+(?:Gtk\.|Adw\.)?[A-Z]\w*\s*\{", body, re.M):
                continue
            ident = m.group(1)
            if ident and re.search(r"\b%s\.set_tooltip_text" % re.escape(ident), rust_src):
                continue
            line = src.count("\n", 0, m.start()) + 1
            found.append("%s:%d" % (os.path.relpath(path, ROOT), line))
    return found


class IconButtons(unittest.TestCase):
    def test_every_icon_only_button_has_a_name(self):
        self.assertEqual([], unnamed_icon_buttons())

    def test_every_icon_only_blueprint_button_has_a_name(self):
        self.assertEqual([], unnamed_blueprint_icon_buttons())


if __name__ == "__main__":
    unittest.main()
