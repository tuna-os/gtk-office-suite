"""GUI journeys wait on state, not time (#1271, gui-testing.md).

A fixed sleep before an assertion bets that the app has finished; on a
loaded runner the bet loses with a message that reads like a product
defect. Journeys use `wait_until` / `eventually`, which retry until the
state arrives and report what they last saw. A sleep that stays has to
say why, on its line or the one above:

- `# pacing:` before the next input, where there is no state to wait on;
- `# settling:` before a check that something did *not* happen, which
  would pass at once without giving the wrong outcome its chance.
"""

import ast
import pathlib
import re
import unittest

GUI = pathlib.Path(__file__).resolve().parent / "gui"
REASON = re.compile(r"#\s*(pacing|settling):")


def unexplained_sleeps(text):
    """Lines of `time.sleep(...)` calls (real calls, not prose) with no reason."""
    lines = text.splitlines()
    out = []
    for node in ast.walk(ast.parse(text)):
        if (isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and node.func.attr == "sleep"
                and isinstance(node.func.value, ast.Name) and node.func.value.id == "time"):
            i = node.lineno - 1
            if REASON.search(lines[i]) or (i and REASON.search(lines[i - 1])):
                continue
            out.append(node.lineno)
    return sorted(out)


class GuiSleeps(unittest.TestCase):
    def test_every_journey_sleep_says_why(self):
        found = {}
        for path in sorted(GUI.glob("test_*.py")):
            bad = unexplained_sleeps(path.read_text())
            if bad:
                found[path.name] = bad
        self.assertEqual(found, {}, "time.sleep with no `# pacing:` or `# settling:` reason (use wait_until/eventually)")

    def test_the_checker_catches_a_bare_sleep(self):
        self.assertEqual(unexplained_sleeps("x()\ntime.sleep(1)\nself.assertTrue(y)\n"), [2])
        self.assertEqual(unexplained_sleeps("time.sleep(1)  # pacing: typing\n"), [])
        self.assertEqual(unexplained_sleeps("# settling: a close must not happen\ntime.sleep(1)\n"), [])
        self.assertEqual(unexplained_sleeps("# time.sleep(1) in a comment\n"), [])
        self.assertEqual(unexplained_sleeps('"""time.sleep(1.5) in prose"""\n'), [])


if __name__ == "__main__":
    unittest.main()
