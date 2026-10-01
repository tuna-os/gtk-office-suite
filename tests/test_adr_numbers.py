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


if __name__ == "__main__":
    unittest.main()
