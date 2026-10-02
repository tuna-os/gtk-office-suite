"""conformance/validate_fixtures.py: every fixture says where it came from
and what it proves (#1275). The repository's own manifests pass, and each
rule fails on the case it exists for."""

import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "conformance"))

import validate_fixtures as vf  # noqa: E402

PARITY = """# Parity

## Tables (spreadsheet)

### Tier 1

| Feature | Status | Proven by | Render |
|---|---|---|---|
| ODS / CSV / TSV import | ✅ | I1 | ✅ tables/values |
"""


def entry(**over):
    e = {
        "author": {"app": "LibreOffice Calc", "version": "24.2"},
        "format": "ods",
        "features": ["tables/ods-csv-tsv-import"],
        "semantics": ["opens with A1 = Item"],
        "permitted_losses": [],
        "used_by": ["uses.py"],
    }
    e.update(over)
    return e


class FixtureManifests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        (self.root / "docs").mkdir()
        (self.root / "docs" / "PARITY.md").write_text(PARITY)
        (self.root / "set").mkdir()
        (self.root / "set" / "a.ods").write_bytes(b"x")
        (self.root / "uses.py").write_text("open('set/a.ods')\n")

    def manifest(self, fixtures, not_fixtures=None):
        body = {"schema": 1, "fixtures": fixtures}
        if not_fixtures is not None:
            body["not_fixtures"] = not_fixtures
        (self.root / "set" / "fixtures.json").write_text(json.dumps(body))

    def errors(self):
        return vf.validate(self.root, sets=["set"])

    def assertRule(self, rule):
        errs = self.errors()
        self.assertTrue(any(e.startswith(rule) for e in errs), f"expected {rule}, got {errs}")

    def test_the_repository_manifests_pass(self):
        self.assertEqual(vf.validate(), [])

    def test_a_complete_manifest_passes(self):
        self.manifest({"a.ods": entry()})
        self.assertEqual(self.errors(), [])

    def test_a_render_fixture_id_is_a_known_feature(self):
        self.manifest({"a.ods": entry(features=["tables/values"])})
        self.assertEqual(self.errors(), [])

    def test_f1_a_set_without_a_manifest(self):
        self.assertRule("F1")

    def test_f2_a_file_without_an_entry(self):
        (self.root / "set" / "b.ods").write_bytes(b"y")
        self.manifest({"a.ods": entry()})
        self.assertRule("F2")

    def test_f2_an_entry_without_a_file(self):
        self.manifest({"a.ods": entry(), "gone.ods": entry()})
        self.assertRule("F2")

    def test_a_listed_non_fixture_needs_no_entry(self):
        (self.root / "set" / "README.md").write_text("about")
        self.manifest({"a.ods": entry()}, {"README.md": "describes the set"})
        self.assertEqual(self.errors(), [])

    def test_f3_a_missing_field(self):
        e = entry()
        del e["semantics"]
        self.manifest({"a.ods": e})
        self.assertRule("F3")

    def test_f3_an_author_without_a_version_key(self):
        self.manifest({"a.ods": entry(author={"app": "LibreOffice"})})
        self.assertRule("F3")

    def test_f4_an_unknown_feature(self):
        self.manifest({"a.ods": entry(features=["tables/pivot-wizardry"])})
        self.assertRule("F4")

    def test_f5_a_permitted_loss_without_an_issue(self):
        self.manifest({"a.ods": entry(permitted_losses=[{"what": "charts"}])})
        self.assertRule("F5")

    def test_f6_a_user_that_never_names_the_fixture(self):
        (self.root / "uses.py").write_text("nothing here\n")
        self.manifest({"a.ods": entry()})
        self.assertRule("F6")

    def test_slugs_follow_the_parity_feature_text(self):
        self.assertEqual(vf.slug("ODS / CSV / TSV import"), "ods-csv-tsv-import")
        self.assertEqual(vf.slug("**LO-authored parity corpus for Decks**"), "lo-authored-parity-corpus-for-decks")


if __name__ == "__main__":
    unittest.main()
