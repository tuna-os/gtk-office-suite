"""GUI journeys move the pointer through `move_pointer`, never through a bare
`xdotool mousemove --sync`.

`--sync` waits for the pointer to move. When the pointer is already on the
target, which happens whenever the journey before left it there (often the
middle of the screen), nothing moves and the call blocks until its timeout.
Whatever was chained after it never ran. `framework/base.py`'s `move_pointer`
steps off and back. Letters' Ctrl+scroll journey still used the bare call and
failed release soaks at wide-1280-scale2 and medium-800 with
`TimeoutExpired: xdotool mousemove --sync ...`.
"""

import glob
import os
import re
import unittest

GUI = os.path.join(os.path.dirname(os.path.abspath(__file__)), "gui")
BARE = re.compile(r"""["']mousemove["']\s*,\s*["']--sync["']""")


class GuiPointerMoves(unittest.TestCase):
    def test_journeys_do_not_call_mousemove_sync_directly(self):
        offenders = []
        for path in sorted(glob.glob(os.path.join(GUI, "test_*.py"))):
            with open(path, encoding="utf-8") as handle:
                for number, line in enumerate(handle, 1):
                    if BARE.search(line):
                        offenders.append(f"{os.path.basename(path)}:{number}")
        self.assertEqual(offenders, [], "use self.move_pointer(dx, dy) instead")

    def test_the_helper_still_steps_off_and_back(self):
        with open(os.path.join(GUI, "framework", "base.py"), encoding="utf-8") as handle:
            source = handle.read()
        self.assertIn("def move_pointer(", source)
        self.assertIn("getmouselocation", source)


if __name__ == "__main__":
    unittest.main()
