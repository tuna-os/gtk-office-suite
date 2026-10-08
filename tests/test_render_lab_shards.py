"""The render lab's real corpus split across CI jobs: `fetch_real_corpus.py
--shard K/N` gives each job a share, and `merge_summaries.py` joins the
shares' verdicts. Every document must be judged by exactly one shard, and
the merged summary must count what the shards counted."""

import json
import os
import subprocess
import sys
import tempfile
import unittest

LAB = os.path.join(os.path.dirname(__file__), "..", "tools", "render-lab")
sys.path.insert(0, LAB)
import fetch_real_corpus  # noqa: E402
import merge_summaries  # noqa: E402


class Shards(unittest.TestCase):
    def test_every_document_is_in_exactly_one_shard(self):
        corpus = json.load(open(fetch_real_corpus.CORPUS))
        for app in ("letters", "tables", "decks"):
            entries = [e for e in corpus if e["app"] == app]
            for n in (1, 6, 7):
                shares = [fetch_real_corpus.shard(entries, f"{k}/{n}") for k in range(n)]
                files = sorted(e["file"] for s in shares for e in s)
                self.assertEqual(files, sorted(e["file"] for e in entries), (app, n))
                self.assertLessEqual(max(map(len, shares)) - min(map(len, shares)), 1)

    def test_a_bad_shard_is_refused(self):
        with self.assertRaises(SystemExit):
            fetch_real_corpus.shard([], "6/6")


class Merge(unittest.TestCase):
    def test_counts_add_up_and_changes_are_kept(self):
        a = {"counts": {"A": {"green": 2, "amber": 1, "red": 0, "missing": 0}},
             "regressed": [{"fixture": "letters/x", "tier": "A", "from": "amber", "to": "red"}],
             "improved": [], "tier_disagreements": ["letters/x"], "print_disagreements": [],
             "fixtures": [{"fixture": "letters/x"}, {"fixture": "letters/a"}]}
        b = {"counts": {"A": {"green": 1, "amber": 0, "red": 1, "missing": 1}, "B": {"green": 1}},
             "regressed": [], "improved": [{"fixture": "letters/y", "tier": "B", "from": "red", "to": "amber"}],
             "tier_disagreements": ["letters/x"], "print_disagreements": [], "fixtures": [{"fixture": "letters/y"}]}
        m = merge_summaries.merge([a, b])
        self.assertEqual(m["counts"]["A"], {"green": 3, "amber": 1, "red": 1, "missing": 1})
        self.assertEqual(m["counts"]["B"]["green"], 1)
        self.assertEqual(len(m["regressed"]) + len(m["improved"]), 2)
        self.assertEqual(m["tier_disagreements"], ["letters/x"])
        self.assertEqual([f["fixture"] for f in m["fixtures"]], ["letters/a", "letters/x", "letters/y"])

    def test_a_shard_without_a_summary_is_named(self):
        with tempfile.TemporaryDirectory() as d:
            good = os.path.join(d, "good.json")
            with open(good, "w") as f:
                json.dump({"counts": {"A": {"green": 1}}}, f)
            out = os.path.join(d, "out")
            subprocess.run([sys.executable, os.path.join(LAB, "merge_summaries.py"), out, good,
                            os.path.join(d, "absent.json")], check=True)
            md = open(os.path.join(out, "summary.md")).read()
            self.assertIn("absent.json", md)
            self.assertEqual(json.load(open(os.path.join(out, "summary.json")))["counts"]["A"]["green"], 1)


if __name__ == "__main__":
    unittest.main()
