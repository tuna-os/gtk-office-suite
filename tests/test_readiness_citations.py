"""A readiness document that cites a test by name must cite one that exists.

These documents carry their evidence as test names — "held by
`test_no_retry_option_is_offered`", "rejected by
`test_an_expired_waiver_is_rejected`". That is the right way to write them:
a named test can be run by a reader who doubts the claim, where prose cannot.

It also decays silently. Rename or delete the test and the sentence still
reads as evidence, because nothing connects the name to the tree — the same
shape as the roadmap's line counts, which sat wrong for a month, and as
`#441`'s own rule that claims must be validated against collected tests
rather than against source strings. So the names are resolved here.

A name resolves as a test file anywhere under `tests/` (the GUI journeys live
in `tests/gui/`), a Python function (`def <name>`) or a Rust function
(`fn <name>`). Anything else is either a typo or a claim about a test nobody
wrote.
"""

import os
import re
import subprocess
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOCS = os.path.join(REPO_ROOT, "docs", "readiness-2026-09")
# `test_a_skipped_test_cannot_support_a_verified_claim` — long enough to be a
# real identifier rather than a prose fragment, and always written with the
# test_ prefix these suites use.
NAME = re.compile(r"\b(test_[a-z0-9_]{4,})\b")


def cited():
    """Every test name the readiness documents cite, with its document."""
    found = {}
    for filename in sorted(os.listdir(DOCS)):
        if not filename.endswith(".md"):
            continue
        with open(os.path.join(DOCS, filename), encoding="utf-8") as handle:
            for name in NAME.findall(handle.read()):
                found.setdefault(name, set()).add(filename)
    assert found, f"{DOCS} cites no test names, so this check is vacuous"
    return found


def resolves(name):
    """Whether a cited name is a test file or a function in the tree."""
    for root, _dirs, files in os.walk(os.path.join(REPO_ROOT, "tests")):
        if f"{name}.py" in files:
            return True
    # grep rather than a Python walk: the Rust tree is large, and a cited name
    # may live in any crate's tests/ or src/.
    for pattern in (f"def {name}", f"fn {name}"):
        found = subprocess.run(
            ["grep", "-rqF", pattern, "--include=*.py", "--include=*.rs", "."],
            cwd=REPO_ROOT,
            capture_output=True,
        )
        if found.returncode == 0:
            return True
    return False


class ReadinessDocumentsCiteRealTests(unittest.TestCase):
    def test_every_cited_test_name_exists(self):
        for name, documents in sorted(cited().items()):
            with self.subTest(name=name):
                self.assertTrue(
                    resolves(name),
                    f"{', '.join(sorted(documents))} cites {name}, which is "
                    "neither a test file nor a function in this tree — a "
                    "renamed test leaves the sentence reading as evidence",
                )


if __name__ == "__main__":
    unittest.main()
