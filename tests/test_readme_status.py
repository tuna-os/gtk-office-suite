"""README's status table is generated from the render-lab scorecard (#1211).

RENDER-PARITY-ROADMAP.md rule 3: the "usable for" claim cannot say more than
the fixtures support. These pin the gate itself (what it takes to be called
usable) and that the committed README matches what the scorecard produces.
"""

import os
import sys
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "tools", "render-lab"))

import readme_status as rs  # noqa: E402

USABLE = "**Usable for everyday documents**"


def green(*tiers):
    return {t: "green" for t in tiers}


class ReadmeStatusGate(unittest.TestCase):
    def test_the_committed_readme_matches_the_scorecard(self):
        self.assertEqual(rs.main(["--check"]), 0)

    def test_the_real_documents_come_from_their_own_baseline(self):
        import json
        import tempfile
        with tempfile.TemporaryDirectory() as d:
            base, real = os.path.join(d, "baseline.json"), os.path.join(d, "baseline-real.json")
            with open(base, "w") as f:
                json.dump({"decks/a": green("A", "B")}, f)
            self.assertEqual(rs.load(baseline=base, baseline_real=real), {"decks/a": green("A", "B")},
                             "a missing real-document baseline is not an error")
            with open(real, "w") as f:
                json.dump({"decks/real-pitch": {"A": "amber", "B": "green"}}, f)
            v = rs.load(baseline=base, baseline_real=real)
        self.assertEqual(rs.app_row("decks", v)[1], "0 / 1 green")

    def test_single_feature_green_alone_is_not_usable(self):
        v = {"decks/a": green("A", "B"), "decks/b": green("A", "B")}
        cell, real, c, status = rs.app_row("decks", v)
        self.assertEqual(cell, "2 / 2 green")
        self.assertIn("#1200", real)
        self.assertIn("#1199", c)
        self.assertNotEqual(status, USABLE)

    def test_every_gate_green_is_usable(self):
        v = {"decks/a": green("A", "B", "C"), "decks/real-pitch": green("A", "B", "C")}
        self.assertEqual(rs.app_row("decks", v)[3], USABLE)

    def test_one_red_fixture_blocks_usable_and_is_named(self):
        v = {"decks/a": green("A", "B", "C"), "decks/real-pitch": green("A", "B", "C"),
             "decks/b": {"A": "red", "B": "green", "C": "green"}}
        cell, _, _, status = rs.app_row("decks", v)
        self.assertIn("not green: `decks/b`", cell)
        self.assertNotEqual(status, USABLE)

    def test_only_recorded_artifacts_count_as_accepted_amber(self):
        v = {"tables/wrap-text": {"A": "amber", "B": "amber"},
             "tables/borders": {"A": "amber", "B": "amber"}}
        cell = rs.app_row("tables", v)[0]
        self.assertIn("`tables/wrap-text` (accepted amber)", cell)
        self.assertIn("not green: `tables/borders`", cell)

    def test_tier_c_must_cover_every_fixture(self):
        v = {"decks/a": green("A", "B", "C"), "decks/real-pitch": green("A", "B"),
             "decks/b": green("A", "B", "C")}
        _, _, c, status = rs.app_row("decks", v)
        self.assertEqual(c, "2 / 3 green")
        self.assertNotEqual(status, USABLE)

    def test_check_fails_on_a_hand_edited_table(self):
        import tempfile
        with open(rs.README) as f:
            text = f.read()
        edited = text.replace("Not yet usable for everyday documents", USABLE, 1)
        with tempfile.NamedTemporaryFile("w", suffix=".md", delete=False) as f:
            f.write(edited)
        try:
            self.assertEqual(rs.main(["--check", "--readme", f.name]), 1)
        finally:
            os.unlink(f.name)


if __name__ == "__main__":
    unittest.main()
