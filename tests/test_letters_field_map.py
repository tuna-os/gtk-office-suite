"""docs/LETTERS-FIELD-MAP.md: every field of the Letters model has a row,
and every test a row cites exists (#1205)."""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC = ROOT / "docs" / "LETTERS-FIELD-MAP.md"
MODEL = ROOT / "letters-core" / "src" / "model.rs"
# Not document state: an edit command, and a value derived for layout.
NOT_STATE = {"StylePatch", "ParagraphLayout"}
CITE = re.compile(r"`([\w\-/]+\.rs)::([a-z0-9_]+)`")


def model_fields():
    src = MODEL.read_text()
    out = set()
    for m in re.finditer(r"pub struct (\w+)\s*\{(.*?)\n\}", src, re.S):
        if m.group(1) in NOT_STATE:
            continue
        for field in re.findall(r"^\s*pub (\w+):", m.group(2), re.M):
            out.add(f"{m.group(1)}.{field}")
    return out


def rows():
    for line in DOC.read_text().splitlines():
        m = re.match(r"^\| `([\w.]+)` \|(.*)\|$", line)
        if m:
            yield m.group(1), [c.strip() for c in m.group(2).split("|")]


def has_test(path, name):
    src = (ROOT / path).read_text()
    return re.search(r"#\[test\]\s*(?:#\[[^\]]*\]\s*)*fn " + re.escape(name) + r"\(", src) is not None


class LettersFieldMap(unittest.TestCase):
    def test_every_model_field_has_a_row_and_no_row_is_stale(self):
        mapped = {name for name, _ in rows()}
        fields = model_fields()
        self.assertEqual(sorted(fields - mapped), [], "model fields with no row in the map")
        self.assertEqual(sorted(mapped - fields), [], "rows for fields the model no longer has")

    def test_every_row_has_where_and_both_formats(self):
        for name, cells in rows():
            with self.subTest(field=name):
                self.assertEqual(len(cells), 3, cells)
                self.assertTrue(all(cells), f"an empty cell: {cells}")

    def test_every_format_cell_cites_a_real_test_or_says_why_not(self):
        for name, (_where, *formats) in rows():
            for cell in formats:
                with self.subTest(field=name, cell=cell):
                    if cell.startswith("n/a"):
                        self.assertTrue(cell[3:].lstrip(":").strip(), "an n/a needs a reason")
                        continue
                    cites = CITE.findall(cell)
                    self.assertTrue(cites, f"no test cited: {cell}")
                    for path, test in cites:
                        self.assertTrue(has_test(path, test), f"{path} has no #[test] fn {test}")

    def test_a_gap_names_its_issue(self):
        for name, (_where, *formats) in rows():
            for cell in formats:
                if cell.startswith("n/a") and not any(w in cell for w in ("container", "Markdown source only")):
                    with self.subTest(field=name):
                        self.assertRegex(cell, r"#\d+")


if __name__ == "__main__":
    unittest.main()
