#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Make a copy of lifafa/ build on its own, with the suite's shared crates.

    fork/with-suite-common.py <lifafa tree>

Lifafa links `suite-common` (INTEGRATION-PLAN C1) by the path
`../suite-common`, a sibling directory in gtk-office-suite. A Flatpak build
copies only the app's own tree into the sandbox, so that path does not exist
there (docs/CUTOVER-LIFAFA-MANDELBROT.md, step 2). This copies
`suite-common` and `suite-common-core` into the tree and points Lifafa at the
copies, in the throwaway checkout a release builds from; the repository's
own Cargo.toml is never touched.

The copies are excluded from Lifafa's workspace, so their dev-dependencies
stay out of Cargo.lock and `--locked` still holds; and the root workspace's
`{ workspace = true }` dependencies they inherit are written out, since the
root workspace is not in the copy. Lifafa's Cargo.lock already lists every
crate they use, so the vendored cargo-sources.json made from it covers them.
"""

import re
import shutil
import sys
import tomllib
from pathlib import Path

CRATES = ("suite-common", "suite-common-core")


def fail(message: str) -> None:
    sys.exit(f"with-suite-common: {message}")


def inline_workspace_deps(manifest: Path, workspace_deps: dict) -> None:
    """Replace `name = { workspace = true }` with the root workspace's spec."""
    text = manifest.read_text()

    def spec(match: re.Match) -> str:
        name = match.group(1)
        if name not in workspace_deps:
            fail(f"{manifest}: {name} inherits from the workspace, which does not define it")
        value = workspace_deps[name]
        if isinstance(value, str):
            return f'{name} = "{value}"'
        parts = []
        for key, item in value.items():
            if isinstance(item, list):
                rendered = "[" + ", ".join(f'"{v}"' for v in item) + "]"
            elif isinstance(item, bool):
                rendered = "true" if item else "false"
            else:
                rendered = f'"{item}"'
            parts.append(f"{key} = {rendered}")
        return f"{name} = {{ {', '.join(parts)} }}"

    text = re.sub(r"^([A-Za-z0-9_-]+) = \{ workspace = true \}$", spec, text, flags=re.M)
    if re.search(r"workspace\s*=\s*true|\.workspace\s*=", text):
        fail(f"{manifest}: an inherited key is left that this script does not write out")
    manifest.write_text(text)


def main() -> None:
    if len(sys.argv) != 2:
        fail("usage: with-suite-common.py <lifafa tree>")
    tree = Path(sys.argv[1]).resolve()
    suite = tree.parent
    root_manifest = suite / "Cargo.toml"
    if not root_manifest.is_file():
        fail(f"{root_manifest}: not found; run this on lifafa/ inside gtk-office-suite")
    workspace_deps = tomllib.loads(root_manifest.read_text()).get("workspace", {}).get("dependencies", {})

    for crate in CRATES:
        source, dest = suite / crate, tree / crate
        if not source.is_dir():
            fail(f"{source}: not found")
        if dest.exists():
            shutil.rmtree(dest)
        shutil.copytree(source, dest, ignore=shutil.ignore_patterns("target"))
        inline_workspace_deps(dest / "Cargo.toml", workspace_deps)

    manifest = tree / "Cargo.toml"
    text = manifest.read_text()
    # suite-common always; suite-common-core too where Lifafa uses its
    # GTK-free models directly.
    text, n = re.subn(r'^(suite-common(?:-core)?) = \{ path = "\.\./\1" \}$',
                      r'\1 = { path = "\1" }', text, flags=re.M)
    if not re.search(r'^suite-common = \{ path = "suite-common" \}$', text, flags=re.M):
        fail(f'{manifest}: expected `suite-common = {{ path = "../suite-common" }}`')
    exclude = ", ".join(f'"{c}"' for c in CRATES)
    text, n = re.subn(r"^\[workspace\]$", f"[workspace]\nexclude = [{exclude}]", text, count=1, flags=re.M)
    if n != 1:
        fail(f"{manifest}: expected a `[workspace]` table")
    manifest.write_text(text)
    print(f"with-suite-common: {tree} now carries {', '.join(CRATES)}")


if __name__ == "__main__":
    main()
