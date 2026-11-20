"""Mandelbrot and the collaboration sidecar stay on one matrix-sdk (ADR 0013, #1973).

Both resolve matrix-sdk in their own lockfile, because the product lockfile
carries no network stack (tests/test_collab_defaults.py). The SQLite store
schema is versioned with the crate, so a store written by one program has to be
readable by the other, and that holds only while the two stay on the same
release. A bump of one is a bump of both, in one change.
"""

import json
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


class RenovateHoldsRuma(unittest.TestCase):
    """ruma moves with matrix-sdk, never ahead of it.

    matrix-sdk pins one ruma release line, so a lone ruma bump cannot resolve
    and Renovate's pull request for it is red on every Mandelbrot job. The
    bound in renovate.json has to name the line the lockfile is on; moving
    ruma and matrix-sdk together means raising it in the same change.
    """

    def test_renovate_holds_ruma_on_the_locked_release_line(self):
        (locked,) = locked_version("mandelbrot/Cargo.lock", "ruma")
        major, minor_, _ = locked.split(".")
        expected = f"<{major}.{int(minor_) + 1}"
        with open(os.path.join(ROOT, "renovate.json"), encoding="utf-8") as f:
            rules = json.load(f)["packageRules"]
        bounds = [
            r["allowedVersions"]
            for r in rules
            if "ruma" in r.get("matchPackageNames", []) and "allowedVersions" in r
        ]
        self.assertEqual(bounds, [expected], f"renovate.json must hold ruma at {expected} while mandelbrot/Cargo.lock has {locked}")


MANDELBROT = os.path.join(ROOT, "mandelbrot")


def mandelbrot_manifests():
    """Every Cargo package directory under mandelbrot/, relative to it."""
    found = []
    for dirpath, dirnames, filenames in os.walk(MANDELBROT):
        dirnames[:] = [d for d in dirnames if d != "target" and not d.startswith(".")]
        if "Cargo.toml" in filenames:
            found.append(os.path.relpath(dirpath, MANDELBROT))
    return sorted(found)


def mandelbrot_workspace():
    """The root manifest's members: itself, its listed members, and its path
    dependencies inside the directory (cargo's implicit membership), minus
    anything it excludes."""
    with open(os.path.join(MANDELBROT, "Cargo.toml"), "rb") as f:
        root = tomllib.load(f)
    workspace = root["workspace"]
    excluded = {os.path.normpath(e) for e in workspace.get("exclude", [])}
    members = {"."} | {os.path.normpath(m) for m in workspace.get("members", [])}
    for table in ("dependencies", "dev-dependencies", "build-dependencies"):
        for dep in root.get(table, {}).values():
            if isinstance(dep, dict) and "path" in dep:
                members.add(os.path.normpath(dep["path"]))
    return members - excluded, excluded


class MandelbrotIsOneWorkspace(unittest.TestCase):
    """mandelbrot/Cargo.toml is a workspace root (the suite's root excludes the
    directory), so cargo claims every package beneath it. A nested package
    the root neither depends on nor excludes does not build at all: that was
    hooks/checks, the pre-commit binary meson builds for the development
    profile. A member that keeps its own Cargo.lock keeps a file cargo never
    reads: matrixrtc/Cargo.lock drifted from the lockfile that ships while
    Renovate went on updating it."""

    def test_every_nested_package_is_a_member_or_excluded(self):
        members, excluded = mandelbrot_workspace()
        for package in mandelbrot_manifests():
            with self.subTest(package):
                own_root = False
                if package not in members:
                    with open(os.path.join(MANDELBROT, package, "Cargo.toml"), "rb") as f:
                        own_root = "workspace" in tomllib.load(f)
                self.assertTrue(
                    package in members or package in excluded or own_root,
                    f"mandelbrot/{package} is inside mandelbrot's workspace but neither a member nor in "
                    "[workspace].exclude, so cargo refuses to build it",
                )

    def test_members_resolve_from_the_root_lockfile_alone(self):
        members, _ = mandelbrot_workspace()
        for package in sorted(members - {"."}):
            with self.subTest(package):
                self.assertFalse(
                    os.path.exists(os.path.join(MANDELBROT, package, "Cargo.lock")),
                    f"mandelbrot/{package}/Cargo.lock is never read: {package} is a workspace member "
                    "and resolves from mandelbrot/Cargo.lock",
                )


if __name__ == "__main__":
    unittest.main()
