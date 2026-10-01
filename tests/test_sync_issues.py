"""sync_issues.py turns render-lab verdicts into GitHub issues (#1200).

The real-document corpus files one issue per document that is not green,
under its own label and marker, so it never touches the fixtures' issues.
"""

import json
import os
import sys
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "tools", "render-lab"))

import sync_issues as si  # noqa: E402


def a_real_document():
    entry = json.load(open(si.REAL_CORPUS))[0]
    return entry, {
        "fixture": f"{entry['app']}/{entry['feature']}",
        "app": entry["app"],
        "feature": entry["feature"],
        "file": entry["file"],
        "expect": entry["expect"],
        "verdicts": {"A": "amber", "B": "red"},
        "metrics": {"A": {"verdict": "amber", "ink": 0.9}, "B": {"verdict": "red", "ink": 0.4}},
    }


class RealDocumentIssues(unittest.TestCase):
    def test_the_issue_names_the_document_its_licence_and_how_to_reproduce(self):
        entry, fx = a_real_document()
        text = si.body(fx, "https://example.invalid/run", "real")
        self.assertTrue(text.startswith(si.MARK_REAL.format(fx["fixture"])), "the marker that dedups it")
        self.assertIn(entry["page"], text)
        self.assertIn(entry["license"], text)
        self.assertIn("RENDER_LAB_CORPUS=real", text)
        self.assertIn("baseline-real.json", text)

    def test_its_marker_and_label_are_not_the_fixtures(self):
        _, fx = a_real_document()
        self.assertNotEqual(si.LABEL_REAL, si.LABEL)
        self.assertNotIn(si.MARK.format(fx["fixture"]), si.body(fx, None, "real"))

    def test_the_worst_tier_names_the_issue(self):
        _, fx = a_real_document()
        self.assertEqual(si.worst(fx["verdicts"]), "red")


if __name__ == "__main__":
    unittest.main()
