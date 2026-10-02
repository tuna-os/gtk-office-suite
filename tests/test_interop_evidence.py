"""docs/INTEROP-EVIDENCE.md: every supported feature has a test in both
directions through LibreOffice, and every cited test exists (#1276)."""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC = ROOT / "docs" / "INTEROP-EVIDENCE.md"
CITE = re.compile(r"^`([\w\-/]+\.rs)::([a-z0-9_]+)`$")


def rows(text):
    """(app, feature, format, ours_to_lo, lo_to_ours) per table row."""
    app = None
    for line in text.splitlines():
        if line.startswith("## "):
            app = line[3:].strip()
            continue
        if not line.startswith("|") or re.match(r"^\|[\s:\-|]+\|$", line):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if cells[0] == "Feature":
            continue
        yield (app, *cells)


def has_test(path, name):
    src = (ROOT / path).read_text()
    return re.search(r"#\[test\]\s*(?:#\[[^\]]*\]\s*)*fn " + re.escape(name) + r"\(", src) is not None


class InteropEvidence(unittest.TestCase):
    def setUp(self):
        self.rows = list(rows(DOC.read_text()))

    def test_every_app_has_rows(self):
        self.assertEqual({r[0] for r in self.rows}, {"Letters", "Tables", "Decks"})

    def test_every_row_has_both_directions(self):
        for row in self.rows:
            with self.subTest(row=row[:3]):
                self.assertEqual(len(row), 5, f"a row needs feature, format and two directions: {row}")
                for cell in row[3:]:
                    self.assertTrue(cell, f"empty cell in {row[:3]}")

    def test_every_cell_cites_a_real_test_or_says_why_not(self):
        for app, feature, fmt, *cells in self.rows:
            for cell in cells:
                with self.subTest(app=app, feature=feature, format=fmt, cell=cell):
                    if cell.startswith("n/a"):
                        reason = cell[3:].lstrip(":").strip()
                        self.assertTrue(reason, "an n/a needs a reason")
                        continue
                    m = CITE.match(cell)
                    self.assertIsNotNone(m, f"not a `path::function` citation: {cell}")
                    path, name = m.groups()
                    self.assertTrue((ROOT / path).is_file(), f"{path} does not exist")
                    self.assertTrue(has_test(path, name), f"{path} has no #[test] fn {name}")

    def test_an_unsupported_feature_that_should_work_names_its_issue(self):
        # "does not write" is a product decision (Tables writes xlsx only);
        # anything else marked n/a is a gap and must be tracked.
        for app, feature, fmt, *cells in self.rows:
            for cell in cells:
                if cell.startswith("n/a") and "does not write" not in cell:
                    with self.subTest(app=app, feature=feature, format=fmt):
                        self.assertRegex(cell, r"#\d+", "an unsupported feature needs its issue")

    def test_the_checker_rejects_a_missing_test(self):
        self.assertFalse(has_test("letters-core/tests/soffice_oracle.rs", "no_such_test_anywhere"))
        self.assertTrue(has_test("letters-core/tests/soffice_oracle.rs", "oracle_reads_plain_paragraphs"))


if __name__ == "__main__":
    unittest.main()
