"""ROADMAP.md's measured figures must still be measurements.

The roadmap claimed `window.rs 2.6K/2.5K/1.6K LOC in tables/letters/decks` for
a month. By the time anyone checked, Letters was 1278 lines — out by half,
because it had been decomposed and nobody re-measured. Eighteen open pull
requests proposed rewording that line; none of them corrected it, and one
deleted the numbers rather than fixing them.

So the numbers live in one table and this test compares them with the files,
and with the ceilings `scripts/release_gate.py` actually enforces. A figure in
a roadmap is a claim about the repository; if nothing checks it, it decays into
decoration.
"""

import os
import re
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ROADMAP = os.path.join(REPO_ROOT, "ROADMAP.md")
RELEASE_GATE = os.path.join(REPO_ROOT, "scripts", "release_gate.py")

# | `tables/src/window.rs` | 2226 | 2300 |
ROW = re.compile(r"^\s*\|\s*`([^`]+\.rs)`\s*\|\s*(\d+)\s*\|\s*(\d+)\s*\|\s*$", re.M)
# "letters/src/window.rs": 1800,
CEILING = re.compile(r'^\s*"([^"]+\.rs)":\s*(\d+),', re.M)
# "(#354, 9/10)" or "(#313, 7/8)" — an issue's progress through its readiness
# checklist, quoted in prose rather than in a table.
PROGRESS = re.compile(r"#(\d+),\s*(\d+)/(\d+)\)")
# The readiness file each execution issue's checklist lives in. Declared here
# because nothing in the tree declares it, and a wrong mapping must fail
# loudly: a missing file is an error below, not a skipped check.
READINESS_FILES = {
    "241": "gtk-threading.md",
    "313": "ci-gates.md",
    "354": "gui-testing.md",
}
# "- [x] ..." / "- [ ] ..." at the top level of a readiness checklist.
CHECKBOX = re.compile(r"^- \[([ xX])\]", re.M)


def claimed_rows():
    """The file/lines/ceiling rows the roadmap states.

    Raises rather than returning nothing: a test that silently finds no claims
    to check would pass forever, which is the failure this guards against.
    """
    with open(ROADMAP, encoding="utf-8") as handle:
        rows = ROW.findall(handle.read())
    assert rows, f"{ROADMAP} states no `file | lines | ceiling` rows to check"
    return [(path, int(lines), int(ceiling)) for path, lines, ceiling in rows]


def claimed_progress():
    """The `#issue, done/total` figures the roadmap states.

    Raises rather than returning nothing, for the same reason as above.
    """
    with open(ROADMAP, encoding="utf-8") as handle:
        found = PROGRESS.findall(handle.read())
    assert found, f"{ROADMAP} states no `#issue, done/total` progress figures"
    return [(issue, int(done), int(total)) for issue, done, total in found]


def readiness_checklist(filename):
    """(checked, total) for one readiness document's top-level checklist."""
    path = os.path.join(REPO_ROOT, "docs", "readiness-2026-09", filename)
    with open(path, encoding="utf-8") as handle:
        boxes = CHECKBOX.findall(handle.read())
    assert boxes, f"{path} has no checklist items to count"
    return sum(1 for box in boxes if box in "xX"), len(boxes)


def enforced_ceilings():
    with open(RELEASE_GATE, encoding="utf-8") as handle:
        found = CEILING.findall(handle.read())
    assert found, f"{RELEASE_GATE} declares no module ceilings"
    return {path: int(limit) for path, limit in found}


class RoadmapProgress(unittest.TestCase):
    """`#313, 7/8` is a measurement too, and it was the one that was wrong.

    ROADMAP.md claimed 7 of 8 for #313 while ci-gates.md had 6 of 8 ticked —
    off by one, in the optimistic direction, in a figure added by the very
    pull request that added this file to stop line counts from decaying. The
    lesson had been written down and applied to one column of the table only.
    """

    def test_every_stated_progress_figure_matches_its_checklist(self):
        for issue, done, total in claimed_progress():
            with self.subTest(issue=issue):
                self.assertIn(
                    issue,
                    READINESS_FILES,
                    f"ROADMAP.md quotes progress for #{issue}, but this test does "
                    "not know which readiness document holds its checklist. Add it "
                    "to READINESS_FILES rather than dropping the figure.",
                )
                actual_done, actual_total = readiness_checklist(READINESS_FILES[issue])
                self.assertEqual(
                    (done, total),
                    (actual_done, actual_total),
                    f"ROADMAP.md says #{issue} is {done}/{total}; "
                    f"docs/readiness-2026-09/{READINESS_FILES[issue]} is "
                    f"{actual_done}/{actual_total}.",
                )

    def test_every_mapped_readiness_document_exists(self):
        """A renamed readiness file must fail here, not quietly stop counting."""
        for issue, filename in READINESS_FILES.items():
            with self.subTest(issue=issue):
                path = os.path.join(REPO_ROOT, "docs", "readiness-2026-09", filename)
                self.assertTrue(os.path.isfile(path), f"{path} does not exist")


class RoadmapFigures(unittest.TestCase):
    def test_every_stated_line_count_matches_the_file(self):
        for path, claimed, _ceiling in claimed_rows():
            with self.subTest(path=path):
                full = os.path.join(REPO_ROOT, path)
                self.assertTrue(os.path.isfile(full), f"{path} does not exist")
                with open(full, encoding="utf-8") as handle:
                    actual = len(handle.read().splitlines())
                self.assertEqual(
                    actual,
                    claimed,
                    f"ROADMAP.md says {path} is {claimed} lines; it is {actual}. "
                    "Update the table — a stale measurement is worse than none.",
                )

    def test_every_stated_ceiling_is_the_one_that_is_enforced(self):
        enforced = enforced_ceilings()
        for path, _lines, claimed in claimed_rows():
            with self.subTest(path=path):
                self.assertIn(
                    path,
                    enforced,
                    f"ROADMAP.md quotes a ceiling for {path}, but "
                    "scripts/release_gate.py does not enforce one",
                )
                self.assertEqual(
                    enforced[path],
                    claimed,
                    f"ROADMAP.md quotes {claimed} for {path}; the gate enforces "
                    f"{enforced[path]}",
                )

    def test_the_stated_counts_are_under_their_ceilings(self):
        """Not a duplicate of the release gate: this catches a roadmap that
        describes a state the gate would reject, which is how a document ends
        up reassuring a reader about a build that fails."""
        for path, lines, ceiling in claimed_rows():
            with self.subTest(path=path):
                self.assertLessEqual(lines, ceiling, f"{path} is over its ceiling")


if __name__ == "__main__":
    unittest.main()
