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


def claimed_rows():
    """The file/lines/ceiling rows the roadmap states.

    Raises rather than returning nothing: a test that silently finds no claims
    to check would pass forever, which is the failure this guards against.
    """
    with open(ROADMAP, encoding="utf-8") as handle:
        rows = ROW.findall(handle.read())
    assert rows, f"{ROADMAP} states no `file | lines | ceiling` rows to check"
    return [(path, int(lines), int(ceiling)) for path, lines, ceiling in rows]


def enforced_ceilings():
    with open(RELEASE_GATE, encoding="utf-8") as handle:
        found = CEILING.findall(handle.read())
    assert found, f"{RELEASE_GATE} declares no module ceilings"
    return {path: int(limit) for path, limit in found}


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
