"""Which Cargo profile the GUI journeys look for the app binaries in.

The release soak builds release binaries; the journeys used to look only in
`target/debug`, so every soak attempt failed in setUp with "Binary not found".
"""

import importlib.util
import os
import unittest
from unittest import mock

_PATH = os.path.join(os.path.dirname(__file__), "gui", "framework", "binary_paths.py")
_spec = importlib.util.spec_from_file_location("binary_paths", _PATH)
binary_paths = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(binary_paths)


class BinaryPaths(unittest.TestCase):
    def test_journeys_look_in_debug_by_default(self):
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("GUI_TEST_PROFILE", None)
            self.assertEqual(binary_paths.binary_path("/t", "letters"), "/t/debug/letters")

    def test_the_soak_names_the_release_profile(self):
        with mock.patch.dict(os.environ, {"GUI_TEST_PROFILE": "release"}):
            self.assertEqual(binary_paths.binary_path("/t", "tables"), "/t/release/tables")

    def test_a_blank_profile_is_the_default(self):
        with mock.patch.dict(os.environ, {"GUI_TEST_PROFILE": "  "}):
            self.assertEqual(binary_paths.binary_path("/t", "decks"), "/t/debug/decks")

    def test_the_soak_workflow_sets_the_profile_for_both_campaigns(self):
        text = open(os.path.join(os.path.dirname(__file__), "..", ".github", "workflows", "release-soak.yml")).read()
        self.assertGreaterEqual(text.count("GUI_TEST_PROFILE: release"), 2)
        self.assertIn("cargo build --release", text)

    def test_stress_reads_the_same_variable(self):
        text = open(os.path.join(os.path.dirname(__file__), "gui", "stress.py")).read()
        self.assertIn("GUI_TEST_PROFILE", text)


if __name__ == "__main__":
    unittest.main()
