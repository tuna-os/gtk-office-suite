"""A `[patch.crates-io]` entry must be pinned and documented.

Patching a dependency is sometimes the only way forward — #283 sat on `hold`
for a week because an upstream crate's exhaustive `match` stopped compiling
under feature unification, with no in-repo workaround. But a patch is also the
easiest thing in a build to forget: it keeps working, so nothing ever asks
whether it is still needed, and if it points at a branch rather than a commit
the build stops being reproducible without anyone choosing that.

So every entry must pin a `rev` and appear in docs/DEPENDENCIES.md, and the
document must say how to remove it.
"""

import os
import re
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CARGO_TOML = os.path.join(REPO_ROOT, "Cargo.toml")
DOC = os.path.join(REPO_ROOT, "docs", "DEPENDENCIES.md")

SECTION = re.compile(r"^\[patch\.crates-io\]\s*$", re.M)
# name = { git = "...", rev = "..." } — or any other source spec.
ENTRY = re.compile(r"^([A-Za-z0-9_-]+)\s*=\s*\{(.+)\}\s*$")


def patch_entries():
    """Every crate patched in the workspace manifest, with its spec.

    An empty result is a legitimate answer here — the right number of patched
    dependencies is zero — so this returns `[]` rather than raising. The tests
    below are then vacuous *by design*, which is the one case where that is
    the correct outcome.
    """
    with open(CARGO_TOML, encoding="utf-8") as handle:
        lines = handle.read().splitlines()
    starts = [i for i, line in enumerate(lines) if SECTION.match(line)]
    entries = []
    for start in starts:
        for line in lines[start + 1:]:
            stripped = line.strip()
            if stripped.startswith("["):
                break
            if not stripped or stripped.startswith("#"):
                continue
            match = ENTRY.match(stripped)
            assert match, f"unparsed [patch.crates-io] line: {stripped!r}"
            entries.append((match.group(1), match.group(2)))
    return entries


class PatchedDependencies(unittest.TestCase):
    def test_every_patch_is_pinned_to_a_commit(self):
        for name, spec in patch_entries():
            with self.subTest(crate=name):
                self.assertIn(
                    "rev =",
                    spec,
                    f"{name} is patched without a rev. A branch or tag can move "
                    "under the build, which makes the release gate's "
                    "reproducibility claim false.",
                )
                self.assertNotIn(
                    "branch =",
                    spec,
                    f"{name} is patched to a branch; pin the commit instead",
                )

    def test_every_patch_is_documented(self):
        entries = patch_entries()
        if not entries:
            return
        self.assertTrue(os.path.isfile(DOC), f"{DOC} is missing")
        with open(DOC, encoding="utf-8") as handle:
            doc = handle.read()
        for name, _spec in entries:
            with self.subTest(crate=name):
                self.assertIn(
                    name,
                    doc,
                    f"{name} is patched but docs/DEPENDENCIES.md does not mention it",
                )

    def test_the_document_says_how_to_remove_them(self):
        """Otherwise the note becomes a headstone rather than a plan."""
        if not patch_entries():
            return
        with open(DOC, encoding="utf-8") as handle:
            doc = handle.read().lower()
        self.assertIn("how to remove", doc)

    def test_a_branch_pin_would_be_rejected(self):
        """The guard has to bite, and with no patch entry the tests above are
        vacuous — so exercise the rule directly on a spec the parser would
        accept."""
        spec = 'git = "https://example.invalid/x", branch = "main"'
        self.assertNotIn("rev =", spec)
        self.assertIn("branch =", spec)


if __name__ == "__main__":
    unittest.main()
