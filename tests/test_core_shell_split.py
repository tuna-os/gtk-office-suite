"""The core/shell split is the repository's central architecture rule, and
nothing checked it.

ADR-0001 decides it and `AGENTS.md` calls it "the one that matters": each app
has a GTK-free core crate owning its model, logic and file I/O, and the app
binaries are shells that wire signals and render. Core crates "must be
testable without GTK headers". That property is what lets ~90 unit tests run
in milliseconds without a display, and it is the reason the logic that used
to live in widget code (and shipped three launch-blocking bugs unnoticed) is
reachable by a test at all.

The rule was enforced by review alone. A single `gtk4 = "0.11"` line added to
`letters-core/Cargo.toml` would satisfy clippy, compile, pass every existing
test, and silently end the property: from then on the crate needs GTK headers
to build, its tests need a display, and `SUITE_GTK_TESTS=skip` would start
hiding them. Nothing in the tree would have said so.

This is the sixth instance of one shape in this repository -- a requirement
restated in prose that nothing derives (see test_ci_test_list.py for four of
them and test_workspace_test_lanes.py for the fifth). Closed the same way:
derive the requirement from the thing it describes, so a new crate is covered
the moment it exists.

What is checked, and why each is separate:

- C1  No core crate declares a GTK-stack dependency. This is the direct
      reading of "GTK-free", and the cheapest thing to get wrong.
- C2  No core crate's `src/` imports a GTK-stack crate. A dependency can be
      declared for one feature and used somewhere it should not be, so the
      source is checked independently of the manifest.
- C3  No core crate reaches a GTK-stack crate with its *default* features,
      as reported by `cargo tree`. "Testable without GTK headers" is a
      property of the built graph, not of the direct deps -- a pure-looking
      crate that pulls `pango` through a helper still needs the headers.
      `Cargo.lock` cannot answer this: it records an optional dependency
      edge unconditionally, with no marker distinguishing it from a
      mandatory one, so reading the lock reports `letters-core -> cairo-rs`
      for a feature that is off by default. `cargo tree` resolves features,
      which is the question being asked. Skipped when cargo is unavailable,
      because a check that cannot run must say so rather than pass.
- C4  The core crate list is derived from the workspace, not restated here,
      so adding `slides-core` without this test knowing is not possible.

The one documented exception is `letters-core`'s `render` feature (ADR-0010):
Pango is the production text shaper and the crate may use it *behind that
optional feature*, because text layout belongs to Pango and never to us
(ADR-0001). The exception is encoded as exactly that -- optional, named, and
confined to one crate -- so a GTK dependency appearing anywhere else, or a
non-optional one appearing here, still fails.
"""

from __future__ import annotations

import os
import re
import subprocess
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# The GTK stack as it actually appears in Cargo.lock, not a guess: every
# crate whose presence means "this needs GTK/GNOME headers to build".
GTK_STACK = (
    "gtk4", "gtk4-sys", "gtk4-macros",
    "libadwaita", "libadwaita-sys",
    "gdk4", "gdk4-sys", "gdk-pixbuf", "gdk-pixbuf-sys",
    "gsk4", "gsk4-sys",
    "pango", "pango-sys", "pangocairo", "pangocairo-sys",
    "cairo-rs", "graphene-rs",
)

# ADR-0010: Pango is the production shaper for letters-core's RenderTree, and
# may be used behind the optional `render` feature. Crate -> the features that
# are permitted to carry a GTK-stack dependency.
DOCUMENTED_OPTIONAL = {"letters-core": {"render"}}

# A core crate is GTK-free by contract. Derived from the workspace member
# list (C4) rather than named: a `*-core` crate is a core crate, plus
# suite-export, which ADR-0001 places on the same side of the split.
CORE_SUFFIX = "-core"
EXTRA_CORE = ("suite-export",)


def workspace_members() -> list[str]:
    """Every workspace member, read from the root manifest."""
    with open(os.path.join(REPO_ROOT, "Cargo.toml"), encoding="utf-8") as handle:
        text = handle.read()
    block = re.search(r"members\s*=\s*\[(.*?)\]", text, re.S)
    assert block, "root Cargo.toml has no workspace members list"
    members = re.findall(r'"([^"]+)"', block.group(1))
    assert members, "workspace member list is empty, so this check is vacuous"
    return members


def core_crates() -> list[str]:
    """The GTK-free side of the split, derived from the workspace (C4)."""
    members = workspace_members()
    found = [m for m in members if m.endswith(CORE_SUFFIX) or m in EXTRA_CORE]
    assert found, (
        "no core crates found in the workspace; if the naming convention "
        "changed, this test must change with it rather than pass vacuously"
    )
    return found


def manifest(crate: str) -> str:
    with open(os.path.join(REPO_ROOT, crate, "Cargo.toml"), encoding="utf-8") as handle:
        return handle.read()


def dependency_lines(text: str) -> list[tuple[str, str]]:
    """(crate, line) for each dependency entry in any [dependencies] table.

    Covers [dependencies], [dev-dependencies] and [build-dependencies], and
    the target-specific forms, because a GTK dependency in any of them makes
    the crate need the headers.
    """
    lines: list[tuple[str, str]] = []
    in_deps = False
    for raw in text.split("\n"):
        line = raw.strip()
        if line.startswith("["):
            in_deps = line.rstrip("]").endswith(
                ("dependencies", "dev-dependencies", "build-dependencies")
            )
            continue
        if not in_deps or not line or line.startswith("#"):
            continue
        name = re.match(r"([A-Za-z0-9_-]+)\s*[=\.]", line)
        if name:
            lines.append((name.group(1), line))
    return lines


def cargo_tree(crate: str, features: list[str] | None = None) -> str | None:
    """`cargo tree` for one crate's normal deps, or None when cargo is absent.

    Feature resolution is the whole point: the default feature set is what
    `cargo test -p <crate>` builds, and therefore what decides whether the
    crate needs GTK headers.
    """
    command = ["cargo", "tree", "-p", crate, "-e", "normal"]
    if features:
        command += ["--features", ",".join(features)]
    try:
        done = subprocess.run(
            command, cwd=REPO_ROOT, capture_output=True, text=True, timeout=600
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if done.returncode != 0:
        return None
    return done.stdout


TREE_NODE = re.compile(
    r"(?:^|[^A-Za-z0-9_-])(" + "|".join(re.escape(c) for c in GTK_STACK) + r") v[0-9]"
)


def gtk_nodes(tree: str) -> set[str]:
    """GTK-stack crates appearing as nodes in a `cargo tree` rendering."""
    return {match.group(1) for match in TREE_NODE.finditer(tree)}


class CoreCratesAreGtkFree(unittest.TestCase):
    def test_c4_the_core_crate_list_comes_from_the_workspace(self):
        """The list this test polices is derived, not restated."""
        crates = core_crates()
        for crate in crates:
            with self.subTest(crate=crate):
                self.assertTrue(
                    os.path.isdir(os.path.join(REPO_ROOT, crate)),
                    f"{crate} is a workspace member with no directory",
                )
        # The three app shells are the other side of the split; if one of them
        # ever matched, the derivation would be wrong.
        for shell in ("letters", "tables", "decks"):
            with self.subTest(shell=shell):
                self.assertNotIn(
                    shell, crates, f"{shell} is an app shell, not a core crate"
                )

    def test_c1_no_core_crate_declares_a_gtk_dependency(self):
        for crate in core_crates():
            permitted = DOCUMENTED_OPTIONAL.get(crate, set())
            for name, line in dependency_lines(manifest(crate)):
                if name not in GTK_STACK:
                    continue
                with self.subTest(crate=crate, dependency=name):
                    self.assertTrue(
                        permitted,
                        f"{crate}/Cargo.toml declares {name}, but {crate} is a "
                        "GTK-free core crate (ADR-0001). Logic that needs GTK "
                        "belongs in the app shell or suite-common.",
                    )
                    # Where an exception is documented, it is still only an
                    # exception when the dependency is optional.
                    self.assertIn(
                        "optional = true",
                        line,
                        f"{crate} may depend on {name} only behind an optional "
                        f"feature ({', '.join(sorted(permitted))}); this "
                        "declaration is unconditional, so the crate now needs "
                        "GTK headers to build at all.",
                    )

    def test_c2_no_core_crate_source_imports_gtk(self):
        pattern = re.compile(
            r"^\s*use\s+(" + "|".join(re.escape(c.replace("-", "_")) for c in GTK_STACK) + r")\b",
            re.M,
        )
        for crate in core_crates():
            permitted = DOCUMENTED_OPTIONAL.get(crate, set())
            src = os.path.join(REPO_ROOT, crate, "src")
            for folder, _dirs, files in os.walk(src):
                for filename in files:
                    if not filename.endswith(".rs"):
                        continue
                    full = os.path.join(folder, filename)
                    with open(full, encoding="utf-8", errors="replace") as handle:
                        body = handle.read()
                    hits = pattern.findall(body)
                    if not hits:
                        continue
                    rel = os.path.relpath(full, REPO_ROOT)
                    with self.subTest(file=rel):
                        self.assertTrue(
                            permitted,
                            f"{rel} imports {', '.join(sorted(set(hits)))}, but "
                            f"{crate} is a GTK-free core crate (ADR-0001).",
                        )
                        # The import must be reachable only with the feature on.
                        gated = any(
                            f'feature = "{feature}"' in body for feature in permitted
                        ) or any(
                            f'feature = "{feature}"' in _module_gate(crate, rel)
                            for feature in permitted
                        )
                        self.assertTrue(
                            gated,
                            f"{rel} imports a GTK-stack crate without a "
                            f"cfg(feature = ...) gate for "
                            f"{' or '.join(sorted(permitted))}; an ungated "
                            "import makes the whole crate need GTK headers.",
                        )

    def test_c3_no_core_crate_needs_gtk_with_default_features(self):
        probe = cargo_tree(core_crates()[0])
        if probe is None:
            self.skipTest(
                "cargo is unavailable, so feature resolution cannot be "
                "checked; C1 and C2 still ran against the manifests and "
                "sources"
            )
        for crate in core_crates():
            tree = cargo_tree(crate)
            with self.subTest(crate=crate):
                self.assertIsNotNone(tree, f"cargo tree failed for {crate}")
                found = gtk_nodes(tree or "")
                self.assertEqual(
                    found,
                    set(),
                    f"{crate} pulls {', '.join(sorted(found))} with its default "
                    "features, so `cargo test -p " + crate + "` now needs GTK "
                    "headers and a display. ADR-0001 requires core crates to be "
                    "testable without them. If this is the documented Pango "
                    "exception, the dependency must be optional and off by "
                    "default.",
                )

    def test_c3_the_documented_pango_exception_is_still_opt_in(self):
        """The exception must remain an exception.

        If `render` ever became a default feature, C3 above would catch it --
        but this says so directly, so the failure names the cause instead of
        looking like an unrelated regression.
        """
        for crate, features in DOCUMENTED_OPTIONAL.items():
            text = manifest(crate)
            block = re.search(r"\[features\](.*?)(\n\[|\Z)", text, re.S)
            self.assertIsNotNone(block, f"{crate} declares no [features] table")
            default = re.search(r"^default\s*=\s*\[(.*?)\]", block.group(1), re.M | re.S)
            enabled = re.findall(r'"([^"]+)"', default.group(1)) if default else []
            for feature in sorted(features):
                with self.subTest(crate=crate, feature=feature):
                    self.assertNotIn(
                        feature,
                        enabled,
                        f"{crate}'s `{feature}` feature carries a GTK-stack "
                        "dependency and must not be in `default`; enabling it "
                        "by default makes the crate need GTK headers for every "
                        "build and test.",
                    )

        if cargo_tree(next(iter(DOCUMENTED_OPTIONAL))) is None:
            return
        # And the feature really is what carries Pango -- so the exception is
        # documenting something true, not describing a dependency that moved.
        for crate, features in DOCUMENTED_OPTIONAL.items():
            tree = cargo_tree(crate, features=sorted(features))
            if tree is None:
                continue
            with self.subTest(crate=crate, features=sorted(features)):
                self.assertNotEqual(
                    gtk_nodes(tree),
                    set(),
                    f"{crate}'s {', '.join(sorted(features))} feature no longer "
                    "pulls any GTK-stack crate. If Pango is gone, drop the "
                    "exception from DOCUMENTED_OPTIONAL rather than leaving a "
                    "permission nothing needs.",
                )


def _module_gate(crate: str, rel: str) -> str:
    """The `mod` declaration site for a file, where a cfg gate may live.

    `layout/pango.rs` is declared by `layout/mod.rs`, so a
    `#[cfg(feature = "render")] pub mod pango;` there is what gates the
    file's imports. Returns the parent module's text, or "" when absent.
    """
    folder = os.path.dirname(os.path.join(REPO_ROOT, rel))
    for parent in ("mod.rs", "lib.rs"):
        candidate = os.path.join(folder, parent)
        if os.path.isfile(candidate):
            with open(candidate, encoding="utf-8", errors="replace") as handle:
                return handle.read()
    return ""


if __name__ == "__main__":
    unittest.main()
