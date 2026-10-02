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
# `<<<<<<< HEAD`, `=======`, `>>>>>>> origin/main`: git's conflict markers,
# anchored to the start of a line, and the `=======` form pinned to exactly
# seven characters with nothing after it — which is what git writes. A Setext
# heading underline in Markdown is also a run of `=`, so a heading underlined
# with exactly seven would read as a conflict here. This file underlines with
# `---` and `#`, and a false positive costs one character to fix, which is a
# better trade than letting the real thing through again.
CONFLICT_MARKER = re.compile(r"^(<<<<<<<|>>>>>>>)[ \t]|^=======\s*$")
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


README = os.path.join(REPO_ROOT, "README.md")
TESTING = os.path.join(REPO_ROOT, "docs", "TESTING.md")
# The documents whose status claims #1285 reconciled with the tracker. Any
# `n/m` figure in them must be one of the figures checked below.
STATUS_DOCUMENTS = (README, ROADMAP, TESTING)

# corpus → (baseline file, total file). The baseline is the ratchet's floor;
# the total is the corpus size, which each corpus's own
# `the_recorded_total_is_the_corpus_size` test holds the file to.
CORPORA = {
    "commonmark": ("letters-core/tests/corpus/roundtrip-baseline.txt",
                   "letters-core/tests/corpus/roundtrip-total.txt"),
    "letters": ("letters-core/tests/corpus/lo-parity-baseline.txt",
                "letters-core/tests/corpus/lo-parity-total.txt"),
    "decks": ("decks-core/tests/corpus/lo-parity-baseline.txt",
              "decks-core/tests/corpus/lo-parity-total.txt"),
    "openformula": ("tables-core/tests/corpus/openformula-baseline.txt",
                    "tables-core/tests/corpus/openformula-total.txt"),
}
# "| CommonMark 0.31.2 | 652 / 652 |" in the README's corpus table, and
# "CommonMark 652/652, LO-Letters 109/109" in ROADMAP.md's status list.
CORPUS_FIGURE = re.compile(
    r"(CommonMark(?: 0\.31\.2 \|)?|LibreOffice ↔ Letters \||LibreOffice ↔ Decks \||"
    r"OpenFormula(?: \|)?|LO-Letters|LO-Decks)\s*(\d+) ?/ ?(\d+)"
)
# Any "n/m" or "n / m" standing on its own: not part of a path, a version
# or a date.
ANY_FIGURE = re.compile(r"(?<![\w/.-])\d+ ?/ ?\d+(?![\w/.-])")
# The README's per-app render table, generated from the render-lab
# scorecard and checked against it by tools/render-lab/readme_status.py.
GENERATED_RENDER_TABLE = re.compile(r"<!-- render-status:begin.*?render-status:end -->", re.S)
# "at least 25 Letters, 20 Tables\nand 20 Decks tests" in docs/TESTING.md.
ORACLE_FLOORS = re.compile(r"at least (\d+) Letters, (\d+) Tables\s+and (\d+) Decks")
# Planning documents kept as history: each must say so before anything else.
HISTORICAL_PLANS = (
    "docs/ROADMAP.md",
    "docs/PRODUCT-QUALITY-ROADMAP-2026-07.md",
    "docs/ISSUE-BACKLOG-2026-07.md",
    "docs/TEST-PLAN.md",
    "docs/archive/HANDOFF-2026-06.md",
    "docs/archive/IMPLEMENTATION-PLAN-2026-06.md",
    "docs/archive/IMPLEMENTATION-QUEUE-2026-06.md",
)


def corpus_key(label):
    label = label.lower()
    for key in ("commonmark", "openformula", "letters", "decks"):
        if key in label:
            return key
    raise AssertionError(f"unknown corpus label {label!r}")


def read_number(rel):
    with open(os.path.join(REPO_ROOT, rel), encoding="utf-8") as handle:
        return int(handle.read().strip())


def read(path):
    with open(path, encoding="utf-8") as handle:
        return handle.read()


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
    def test_the_roadmap_has_no_unresolved_merge_in_it(self):
        """Conflict markers sat in this file's God-file table for three merges.

        Both sides of the conflict said the same thing, so resolving it by
        keeping one was all it needed — and because every assertion here reads
        *rows*, a table wrapped in `<<<<<<< HEAD` parsed fine and the figures
        all matched. The test that exists to keep this section honest read
        straight past `>>>>>>> origin/main` three times.
        """
        with open(ROADMAP, encoding="utf-8") as handle:
            for number, line in enumerate(handle, start=1):
                self.assertFalse(
                    CONFLICT_MARKER.match(line),
                    f"{ROADMAP}:{number} is a merge conflict marker: "
                    f"{line.strip()!r}",
                )

    def test_no_file_is_claimed_twice(self):
        """A duplicated row is an unresolved merge even with the markers gone.

        Two rows that agree are not a resolution — nothing keeps them agreeing,
        and the next measurement updates whichever one the editor happened to
        find. One row per file is the invariant; `assertEqual` on the counts
        names the offender rather than just failing.
        """
        seen = {}
        for path, _claimed, _ceiling in claimed_rows():
            seen[path] = seen.get(path, 0) + 1
        repeated = {path: count for path, count in seen.items() if count > 1}
        self.assertEqual(
            repeated,
            {},
            "the God-file table states these files more than once; keep one "
            "row each so there is a single figure to re-measure",
        )

    def test_every_app_window_has_a_row(self):
        """A dropped row is a claim nobody checks any more.

        A rebase resolved a conflict between two adjacent rows by keeping
        one, and `letters/src/window.rs` vanished from the table. Every other
        test here checks the rows that are present, so all of them passed:
        the file was still under its ceiling, just no longer described.

        The apps are named here as well as read from the gate, so removing a
        file from both places at once still fails.
        """
        apps = {"letters/src/window.rs", "tables/src/window.rs", "decks/src/window.rs"}
        gated = {path for path in enforced_ceilings() if path.endswith("/src/window.rs")}
        stated = {path for path, _lines, _ceiling in claimed_rows()}
        self.assertEqual(
            set(),
            (apps | gated) - stated,
            "ROADMAP.md's God-file table has no row for these app windows; "
            "add one with the measured line count and the gate's ceiling",
        )

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


class CorpusFigures(unittest.TestCase):
    """The corpus scores are the ratchets' own numbers.

    ROADMAP.md said CommonMark 630/652 for weeks after the baseline reached
    652, and `conformance/scorecard.py --no-run` printed the Letters corpus
    out of 104 after it had grown to 109. Both were typed once and never
    re-read. The numerator is the committed baseline; the denominator is the
    recorded corpus size, which the corpus's own test checks.
    """

    def claimed(self, path):
        found = [(corpus_key(label), int(n), int(m)) for label, n, m in CORPUS_FIGURE.findall(read(path))]
        self.assertTrue(found, f"{path} states no corpus scores to check")
        return found

    def test_every_status_document_states_every_corpus_once(self):
        for path in (README, ROADMAP):
            with self.subTest(path=path):
                keys = [key for key, _n, _m in self.claimed(path)]
                self.assertEqual(sorted(keys), sorted(CORPORA), f"{path} should state each corpus once")

    def test_every_corpus_score_is_its_baseline_over_its_size(self):
        for path in (README, ROADMAP):
            for key, n, m in self.claimed(path):
                with self.subTest(path=path, corpus=key):
                    baseline, total = (read_number(rel) for rel in CORPORA[key])
                    self.assertEqual(
                        (n, m), (baseline, total),
                        f"{os.path.basename(path)} says {key} is {n}/{m}; the "
                        f"baseline is {baseline} and the corpus has {total}",
                    )

    def test_no_baseline_claims_more_than_its_corpus(self):
        for key, (baseline, total) in CORPORA.items():
            with self.subTest(corpus=key):
                self.assertLessEqual(read_number(baseline), read_number(total))


class OracleFloors(unittest.TestCase):
    """docs/TESTING.md's oracle floors are numbers the suites must meet."""

    def test_each_oracle_suite_meets_its_floor(self):
        found = ORACLE_FLOORS.search(read(TESTING))
        self.assertIsNotNone(found, "docs/TESTING.md states no oracle floors")
        floors = dict(zip(("letters-core", "tables-core", "decks-core"), map(int, found.groups())))
        for crate, floor in floors.items():
            with self.subTest(crate=crate):
                suite = read(os.path.join(REPO_ROOT, crate, "tests", "soffice_oracle.rs"))
                count = suite.count("#[test]")
                self.assertGreaterEqual(count, floor, f"{crate} has {count} oracle tests, under its floor of {floor}")


class StatusDocuments(unittest.TestCase):
    """README, ROADMAP.md and docs/TESTING.md state no figure nobody checks (#1285)."""

    def test_every_figure_is_a_checked_one(self):
        for path in STATUS_DOCUMENTS:
            with self.subTest(path=path):
                text = GENERATED_RENDER_TABLE.sub("", read(path))
                checked = set()
                for pattern in (CORPUS_FIGURE, PROGRESS):
                    for found in pattern.finditer(text):
                        checked.update(range(*found.span()))
                stray = [
                    f"line {text.count(chr(10), 0, found.start()) + 1}: {found.group(0)!r}"
                    for found in ANY_FIGURE.finditer(text)
                    if found.start() not in checked
                ]
                self.assertEqual(
                    [], stray,
                    f"{os.path.basename(path)} states figures no test checks; derive them "
                    "from the tracker and check them here, or link the tracker instead",
                )

    def test_the_historical_plans_say_so_first(self):
        for rel in HISTORICAL_PLANS:
            with self.subTest(plan=rel):
                head = "\n".join(read(os.path.join(REPO_ROOT, rel)).splitlines()[:6]).lower()
                self.assertIn("historical, not current", head, f"{rel} does not open by saying it is history")


if __name__ == "__main__":
    unittest.main()
