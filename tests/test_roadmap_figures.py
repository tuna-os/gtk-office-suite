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
READINESS_README = os.path.join(REPO_ROOT, "docs", "readiness-2026-09", "README.md")
# "- [~] #354 — deterministic GUI infrastructure — [gui-testing.md](gui-testing.md) 9/10"
EXECUTION_ROW = re.compile(
    r"^- \[([ xX~])\] #(\d+) — .+ — \[([^\]]+\.md)\]\(\3\) (\d+)/(\d+)$", re.M
)
# "- [x] ...", "- [ ] ..." or "- [~] ..." at the top level of a readiness
# checklist. The partial marker counts toward the total and not toward done:
# the two mapped documents happen to use none today, but crash-stress.md,
# recovery.md, interoperability.md and letters-fidelity.md all do, so a
# pattern that ignored it would silently undercount the moment one of those
# gained a figure — a wrong total, arrived at by a check.
CHECKBOX = re.compile(r"^- \[([ xX~])\]", re.M)


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


def execution_rows():
    """The readiness README's dependency-ordered execution list.

    Raises rather than returning nothing: the previous version of this list
    was hand-maintained and had drifted — #354 and #313 both showed `[ ]`
    while their documents were 9/10 and 7/8 — so a check that silently found
    no rows would restore exactly the state it exists to prevent.
    """
    with open(READINESS_README, encoding="utf-8") as handle:
        rows = EXECUTION_ROW.findall(handle.read())
    assert rows, (
        f"{READINESS_README} states no `- [x] #issue — title — [file.md](file.md) n/m` "
        "execution rows to check"
    )
    return [(mark, issue, filename, int(done), int(total)) for mark, issue, filename, done, total in rows]


def readiness_files():
    """issue → readiness document, as the execution list itself declares it.

    Derived rather than hardcoded: a mapping kept in this file could only be
    right by someone remembering to update two places.
    """
    return {issue: filename for _mark, issue, filename, _done, _total in execution_rows()}


def readiness_checklist(filename):
    """(checked, total) for one readiness document's top-level checklist."""
    path = os.path.join(REPO_ROOT, "docs", "readiness-2026-09", filename)
    with open(path, encoding="utf-8") as handle:
        boxes = CHECKBOX.findall(handle.read())
    assert boxes, f"{path} has no checklist items to count"
    return sum(1 for box in boxes if box in "xX"), len(boxes)


def partial_count(filename):
    """How many of one document's top-level checklist items are `[~]`."""
    path = os.path.join(REPO_ROOT, "docs", "readiness-2026-09", filename)
    with open(path, encoding="utf-8") as handle:
        return sum(1 for box in CHECKBOX.findall(handle.read()) if box == "~")


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
                mapping = readiness_files()
                self.assertIn(
                    issue,
                    mapping,
                    f"ROADMAP.md quotes progress for #{issue}, but the readiness "
                    "README's execution list does not say which document holds its "
                    "checklist. Add a row there rather than dropping the figure.",
                )
                actual_done, actual_total = readiness_checklist(mapping[issue])
                self.assertEqual(
                    (done, total),
                    (actual_done, actual_total),
                    f"ROADMAP.md says #{issue} is {done}/{total}; "
                    f"docs/readiness-2026-09/{mapping[issue]} is "
                    f"{actual_done}/{actual_total}.",
                )

    def test_every_mapped_readiness_document_exists(self):
        """A renamed readiness file must fail here, not quietly stop counting."""
        for issue, filename in readiness_files().items():
            with self.subTest(issue=issue):
                path = os.path.join(REPO_ROOT, "docs", "readiness-2026-09", filename)
                self.assertTrue(os.path.isfile(path), f"{path} does not exist")


class ReadinessExecutionList(unittest.TestCase):
    """The summary list must be counted, not remembered.

    `docs/readiness-2026-09/README.md` carried a hand-maintained
    dependency-ordered list of the fourteen execution issues. Three of them
    had been worked for weeks — #354 to 9/10, #313 to 7/8, #442 through a
    merged nightly campaign — and all three still showed `[ ]`, because
    ticking a second place is a step nobody takes. A reader deciding what to
    work on next was reading a list that said nothing had started.

    So the marker and the figure are both derived from the linked document's
    own checklist. Edit the document; the list has to follow.
    """

    def test_every_row_links_a_readiness_document_that_exists(self):
        for _mark, issue, filename, _done, _total in execution_rows():
            with self.subTest(issue=issue):
                path = os.path.join(REPO_ROOT, "docs", "readiness-2026-09", filename)
                self.assertTrue(
                    os.path.isfile(path),
                    f"#{issue} links {filename}, which does not exist",
                )

    def test_every_row_states_its_document_s_real_figure(self):
        for _mark, issue, filename, done, total in execution_rows():
            with self.subTest(issue=issue):
                actual = readiness_checklist(filename)
                self.assertEqual(
                    (done, total),
                    actual,
                    f"the execution list says #{issue} is {done}/{total}; "
                    f"{filename} is {actual[0]}/{actual[1]}",
                )

    def test_every_marker_follows_from_its_checklist(self):
        """`[x]` only when nothing is left, `[ ]` only when nothing has begun.

        A partially-worked row marked `[ ]` understates the work and a
        partially-worked row marked `[x]` closes it early; both were possible
        while the marker was typed by hand.
        """
        for mark, issue, filename, done, total in execution_rows():
            with self.subTest(issue=issue):
                partial = partial_count(filename)
                if done == total:
                    expected = "x"
                elif done == 0 and partial == 0:
                    expected = " "
                else:
                    expected = "~"
                self.assertEqual(
                    expected,
                    mark.lower(),
                    f"#{issue} is marked [{mark}] but {filename} has {done} of "
                    f"{total} ticked and {partial} partial: expected [{expected}]",
                )

    def test_the_list_covers_every_readiness_document(self):
        """A document nobody links is a work item nobody sees.

        `README.md` is the list itself; every other readiness document is an
        execution issue's checklist and belongs in it.
        """
        present = {
            name
            for name in os.listdir(os.path.join(REPO_ROOT, "docs", "readiness-2026-09"))
            if name.endswith(".md") and name != "README.md"
        }
        listed = {filename for _m, _i, filename, _d, _t in execution_rows()}
        self.assertEqual(
            set(),
            present - listed,
            "readiness documents that no execution row links",
        )


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
