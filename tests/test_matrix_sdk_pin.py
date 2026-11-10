"""Mandelbrot and the collaboration sidecar stay on one matrix-sdk (ADR 0013, #1973).

Both resolve matrix-sdk in their own lockfile, because the product lockfile
carries no network stack (tests/test_collab_defaults.py). The SQLite store
schema is versioned with the crate, so a store written by one program has to be
readable by the other, and that holds only while the two stay on the same
release. A bump of one is a bump of both, in one change.
"""

import os
import re
import tomllib
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def locked_version(lock_path, crate):
    with open(os.path.join(ROOT, lock_path), "rb") as f:
        packages = tomllib.load(f)["package"]
    versions = sorted({p["version"] for p in packages if p["name"] == crate})
    return versions


def minor(version):
    major, minor_, _ = version.split(".")
    return f"{major}.{minor_}"


class MatrixSdkPin(unittest.TestCase):
    def test_each_lockfile_resolves_one_matrix_sdk(self):
        for lock in ("mandelbrot/Cargo.lock", "collab/Cargo.lock"):
            with self.subTest(lock):
                self.assertEqual(len(locked_version(lock, "matrix-sdk")), 1, f"{lock}: expected exactly one matrix-sdk")

    def test_mandelbrot_and_the_sidecar_agree_on_the_release(self):
        mandelbrot = locked_version("mandelbrot/Cargo.lock", "matrix-sdk")
        collab = locked_version("collab/Cargo.lock", "matrix-sdk")
        self.assertEqual(mandelbrot, collab, "matrix-sdk differs between mandelbrot/ and collab/ (docs/adr/0013-one-matrix-account.md)")

    def test_the_binding_pin_names_the_locked_release(self):
        with open(os.path.join(ROOT, "collab/dcp-matrix/Cargo.toml"), encoding="utf-8") as f:
            text = f.read()
        m = re.search(r'matrix-sdk\s*=\s*\{[^}]*version\s*=\s*"=?(\d+\.\d+\.\d+)"', text)
        self.assertIsNotNone(m, "dcp-matrix no longer pins matrix-sdk by version")
        (locked,) = locked_version("collab/Cargo.lock", "matrix-sdk")
        self.assertEqual(minor(m.group(1)), minor(locked))


if __name__ == "__main__":
    unittest.main()
