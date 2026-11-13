"""lifafa/fork/with-suite-common.py: a copy of lifafa/ that builds on its own.

Lifafa links ../suite-common, which a Flatpak build does not copy into the
sandbox; the script brings the shared crates into the tree. These check that
it does so on a copy of the real files, and that it refuses a tree it does not
recognise rather than half-rewriting it.
"""

import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "lifafa" / "fork" / "with-suite-common.py"


class WithSuiteCommonTest(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir)
        shutil.copy(ROOT / "Cargo.toml", self.dir / "Cargo.toml")
        (self.dir / "lifafa").mkdir()
        shutil.copy(ROOT / "lifafa" / "Cargo.toml", self.dir / "lifafa" / "Cargo.toml")
        for crate in ("suite-common", "suite-common-core"):
            shutil.copytree(ROOT / crate, self.dir / crate, ignore=shutil.ignore_patterns("target"))
        self.tree = self.dir / "lifafa"

    def run_script(self):
        return subprocess.run([sys.executable, str(SCRIPT), str(self.tree)], capture_output=True, text=True)

    def test_brings_the_crates_in_and_points_lifafa_at_them(self):
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = tomllib.loads((self.tree / "Cargo.toml").read_text())
        self.assertEqual(manifest["dependencies"]["suite-common"], {"path": "suite-common"})
        self.assertEqual(manifest["workspace"]["exclude"], ["suite-common", "suite-common-core"])
        for crate in ("suite-common", "suite-common-core"):
            self.assertTrue((self.tree / crate / "src" / "lib.rs").is_file(), crate)
        # suite-common's own path to the core crate now resolves inside the tree.
        common = tomllib.loads((self.tree / "suite-common" / "Cargo.toml").read_text())
        self.assertEqual(common["dependencies"]["suite-common-core"], {"path": "../suite-common-core"})

    def test_writes_out_what_the_crates_inherit_from_the_root_workspace(self):
        self.assertEqual(self.run_script().returncode, 0)
        root_deps = tomllib.loads((self.dir / "Cargo.toml").read_text())["workspace"]["dependencies"]
        for crate in ("suite-common", "suite-common-core"):
            text = (self.tree / crate / "Cargo.toml").read_text()
            self.assertNotIn("workspace = true", text, crate)
            for name, spec in tomllib.loads(text).get("dependencies", {}).items():
                if name in root_deps and isinstance(spec, dict) and "path" not in spec:
                    self.assertEqual(spec, root_deps[name], f"{crate}: {name}")

    def test_points_a_direct_suite_common_core_dependency_at_its_copy_too(self):
        manifest = self.tree / "Cargo.toml"
        text = manifest.read_text().replace(
            'suite-common = { path = "../suite-common" }\n',
            'suite-common = { path = "../suite-common" }\nsuite-common-core = { path = "../suite-common-core" }\n')
        manifest.write_text(text)
        self.assertEqual(self.run_script().returncode, 0)
        deps = tomllib.loads(manifest.read_text())["dependencies"]
        self.assertEqual(deps["suite-common"], {"path": "suite-common"})
        self.assertEqual(deps["suite-common-core"], {"path": "suite-common-core"})

    def test_refuses_a_tree_that_does_not_link_suite_common_as_expected(self):
        manifest = self.tree / "Cargo.toml"
        manifest.write_text(manifest.read_text().replace('path = "../suite-common"', 'path = "elsewhere"'))
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("suite-common", result.stderr)

    def test_refuses_a_tree_outside_the_suite(self):
        (self.dir / "Cargo.toml").unlink()
        self.assertNotEqual(self.run_script().returncode, 0)


if __name__ == "__main__":
    unittest.main()
