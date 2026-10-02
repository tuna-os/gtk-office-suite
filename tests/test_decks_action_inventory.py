"""docs/DECKS-ACTIONS.md lists every Decks app action,
and every test it cites exists (decks-readiness.md row 9)."""

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOC = ROOT / "docs" / "DECKS-ACTIONS.md"


def registered_actions():
    names = set()
    for path in (ROOT / "decks" / "src").glob("*.rs"):
        text = path.read_text()
        names.update(re.findall(r'SimpleAction::new\(\s*"([a-z0-9-]+)"', text))
        # Registered in a loop over (name, flag) pairs: undo/redo, present/rehearse.
        names.update(re.findall(r'\("([a-z][a-z0-9-]+)",\s*(?:true|false)\)', text))
        # slide_actions.rs: (name, title, accelerator).
        names.update(re.findall(r'\("([a-z][a-z0-9-]+)",\s*"[^"]+",\s*"<', text))
    # Test-mode hooks are not advertised.
    return {n for n in names if not n.startswith("test-")}


def rows():
    """(action, proof) for each `app.*` row of the action table."""
    return re.findall(r"^\| `app\.([a-z0-9-]+)` \| [^|]+ \| (.+?) \|$", DOC.read_text(), re.M)


def cited(proof):
    return re.findall(r"`([A-Za-z_][A-Za-z0-9_]*)`", proof)


class DecksActionInventory(unittest.TestCase):
    def test_every_action_has_a_row_and_every_row_an_action(self):
        listed = {a for a, _ in rows()}
        actions = registered_actions()
        self.assertTrue(actions, "found no actions: the scan is broken")
        self.assertEqual(sorted(actions - listed), [], "actions with no row in DECKS-ACTIONS.md")
        self.assertEqual(sorted(listed - actions), [], "rows for actions Decks no longer registers")

    def test_every_cited_test_exists(self):
        gui = "".join(p.read_text() for p in (ROOT / "tests" / "gui").glob("*.py"))
        rust = "".join(p.read_text() for p in ROOT.glob("decks*/**/*.rs"))
        proofs = [p for _, p in rows()] + re.findall(r"^\| [^`|][^|]* \| [^|]+ \| (.+?) \|$", DOC.read_text(), re.M)
        names = [n for p in proofs for n in cited(p)]
        self.assertGreater(len(names), 20)
        for name in names:
            if name[0].isupper():
                self.assertRegex(gui, rf"\nclass {name}\(", f"GUI journey {name}")
            else:
                self.assertRegex(rust, rf"\bfn {name}\(", f"Rust test {name}")

    def test_only_ui_only_rows_go_without_proof(self):
        for action, proof in rows():
            if not cited(proof):
                line = next(l for l in DOC.read_text().splitlines() if l.startswith(f"| `app.{action}`"))
                self.assertIn("*UI only*", line, f"app.{action} has no proof")


if __name__ == "__main__":
    unittest.main()
