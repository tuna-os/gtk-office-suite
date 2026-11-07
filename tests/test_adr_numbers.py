"""Every ADR has a number of its own (#1205, letters-fidelity.md).

Two ADRs were 0004 and two were 0006, so "ADR 0004" in a code comment or a
readiness document could mean either decision. This keeps the numbers
unique and each file's heading in step with its file name.
"""

import os
import re
import unittest

ADR_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "docs", "adr")
NAME = re.compile(r"^(\d{4})-[a-z0-9-]+\.md$")


class AdrNumbers(unittest.TestCase):
    def adrs(self):
        found = [(m.group(1), name) for name in sorted(os.listdir(ADR_DIR)) if (m := NAME.match(name))]
        self.assertGreater(len(found), 5, "no ADRs found; the scan is wrong")
        return found

    def test_no_two_adrs_share_a_number(self):
        seen = {}
        for number, name in self.adrs():
            with self.subTest(name):
                self.assertNotIn(number, seen, f"ADR {number} is both {seen.get(number)} and {name}")
            seen[number] = name

    def test_each_heading_names_its_own_number(self):
        for number, name in self.adrs():
            with open(os.path.join(ADR_DIR, name)) as f:
                heading = f.readline()
            with self.subTest(name):
                self.assertRegex(heading, rf"^# ADR[ -]{number}\b", f"{name} is headed {heading.strip()!r}")

    def test_letters_review_exclusion_is_historical_not_current(self):
        root = os.path.dirname(ADR_DIR)
        with open(os.path.join(ADR_DIR, "0003-later-tier-scope.md"), encoding="utf-8") as f:
            adr = f.read()
        with open(os.path.join(root, "LETTERS-REVIEW-WORKFLOWS.md"), encoding="utf-8") as f:
            workflows = f.read()
        with open(os.path.join(root, "PARITY.md"), encoding="utf-8") as f:
            parity = f.read()
        section = adr.split("## 2. ", 1)[1].split("## 3. ", 1)[0]
        self.assertIn("Historical decision", section)
        self.assertIn("not current Letters review support", section)
        self.assertIn("../LETTERS-REVIEW-WORKFLOWS.md", section)
        self.assertIn("original comments and tracked-changes exclusions", workflows)
        self.assertIn("historical for review", parity)
        self.assertNotIn("Endnotes/comments/track changes out per ADR 0003", parity)


if __name__ == "__main__":
    unittest.main()
