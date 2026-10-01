"""Every ignored security advisory names its issue and a review date.

`deny.toml` ignores advisories that have no patched release or sit behind a
feature that does not ship. Each entry had a reason, but most named no
issue and none had a date, so nothing ever asked again whether the reason
still held: an exception became permanent by default (#1197). The
capability ledger's waiver rule already says a waiver "names an issue,
reason, scope and review date" — this applies it to the advisory gate.

The date is enforced, not decorative: once it passes, this test fails until
someone re-checks the upstream blocker and either removes the entry or sets
a new date with a reason. That is the point. An expired waiver should stop
a pull request, not sit quietly in a file.
"""

import datetime
import os
import re
import tomllib
import unittest

DENY = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "deny.toml")
REVIEW = re.compile(r"review by (\d{4}-\d{2}-\d{2})")
ISSUE = re.compile(r"#\d+")


def ignored():
    with open(DENY, "rb") as handle:
        entries = tomllib.load(handle).get("advisories", {}).get("ignore", [])
    assert entries, "deny.toml ignores no advisories; this check is vacuous"
    return entries


class EveryAdvisoryWaiverIsDated(unittest.TestCase):
    def test_every_waiver_names_an_issue_and_a_review_date(self):
        for entry in ignored():
            reason = entry.get("reason", "")
            with self.subTest(advisory=entry.get("id")):
                self.assertRegex(reason, ISSUE, f"{entry.get('id')} names no tracking issue")
                self.assertRegex(reason, REVIEW, f"{entry.get('id')} has no `review by YYYY-MM-DD`")

    def test_no_waiver_is_past_its_review_date(self):
        today = datetime.date.today()
        for entry in ignored():
            match = REVIEW.search(entry.get("reason", ""))
            if not match:
                continue  # reported by the test above
            with self.subTest(advisory=entry.get("id")):
                due = datetime.date.fromisoformat(match.group(1))
                self.assertGreaterEqual(
                    due, today,
                    f"{entry['id']} was due for review on {due}. Re-check the upstream "
                    "blocker named in its reason: remove the entry if it is fixed, "
                    "otherwise set a new date and say what you checked.",
                )


if __name__ == "__main__":
    unittest.main()
