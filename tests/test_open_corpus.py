import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "open_corpus", Path(__file__).resolve().parents[1] / "interop/open_corpus.py")
corpus = importlib.util.module_from_spec(spec)
spec.loader.exec_module(corpus)


class CorpusContractTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "sample.txt").write_bytes(b"sample")
        self.item = dict(id="sample", app="letters", expected="open",
                         path="sample.txt", sha256=hashlib.sha256(b"sample").hexdigest(),
                         authoring="project", license="GPL-3.0-or-later")

    def load(self, fixtures=None):
        path = self.root / "manifest.json"
        path.write_text(json.dumps(dict(schema_version=1, fixtures=fixtures or [self.item])))
        return corpus.load_manifest(path)

    def test_valid_file(self):
        self.assertEqual(len(self.load()["fixtures"]), 1)

    def test_modified_file_rejected(self):
        (self.root / "sample.txt").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            self.load()

    def test_duplicate_ids_rejected(self):
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            self.load([self.item, self.item])

    def test_path_escape_rejected_before_read(self):
        self.item["path"] = "../outside.txt"
        with self.assertRaisesRegex(ValueError, "escapes"):
            self.load()

    def test_missing_provenance_rejected(self):
        del self.item["authoring"]
        with self.assertRaisesRegex(ValueError, "provenance"):
            self.load()

    def test_invalid_expected_outcome_rejected(self):
        self.item["expected"] = "skip"
        with self.assertRaisesRegex(ValueError, "contract"):
            self.load()
