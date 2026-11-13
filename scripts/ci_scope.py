#!/usr/bin/env python3
"""What does a pull request's change reach? (ci.yml `test` and `flatpak`, gui-tests.yml `smoke`)

The heavy lanes — the workspace's clippy and nextest run, three Flatpak builds
and the GUI journeys — started on every pull request head, including heads
that change only prose, another product's lane or a workflow they do not run.
The hosted runner pool is the scarce resource (a burst of hive pushes queued
PRs for 30-80 minutes), so those heads now pass the heavy steps in seconds.

The checks still report on every pull request: the caller decides per *step*,
never with a job-level `if:` or a `paths:` filter, which is how
conformance/required_checks.py wants every required check to behave (a skipped
or filtered job leaves a required check pending, or counts as passing).

A path is *inert* — it cannot change anything the heavy lanes judge — when it is:

  - under `lifafa/` or `mandelbrot/`: their own workspaces and lock files, built
    and tested by lifafa.yml and mandelbrot.yml; the root workspace excludes
    them (Cargo.toml `exclude`) and nothing here links them;
  - under `.github/`, except the two workflows that run the heavy lanes
    (a workflow change is tested by the python lane, which stays on);
  - `renovate.json` or `codecov.yml`;
  - prose (`docs/`, `skills/`, `runbooks/`, a top-level `*.md`) that no source the
    heavy lanes build or run mentions. Derived, not listed: `include_str!`
    of docs/FORMATS.md, the GUI harness's screenshot directory and the like
    keep those files non-inert because their path, a parent directory of it
    or their file name occurs in the sources. `docs/collab-spec/` always counts
    (its schemas are compiled into the collab crates).

Everything else, and anything this cannot classify, is not inert: "unsure"
means run it, never skip it (the rule tests/gui/select_journeys.sh follows).

Flatpak builds `cargo build -p <app>` from the whole repository, so a change
confined to another app's own crates cannot change this app's Flatpak. Which
crates an app links comes from the Cargo manifests (real_scope.workspace_crates),
as render-real.yml does. Cargo.lock, Cargo.toml, flatpak/, third_party/, po/,
data/ and anything outside a workspace crate are shared, so they reach every app.

Usage (prints key=value lines for $GITHUB_OUTPUT):
  git diff --no-renames --name-only HEAD^1 HEAD > changed.txt
  python3 scripts/ci_scope.py --changed changed.txt
      heavy=true|false             the workspace lanes (test, smoke) have work
      flatpak=["letters", ...]     the apps whose Flatpak can change
  python3 scripts/ci_scope.py --all                       # not a pull request
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path, PurePosixPath

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "tools" / "render-lab"))
import real_scope  # noqa: E402  (the same manifest walk render-real.yml scopes with)

APPS = real_scope.APPS
OTHER_WORKSPACES = ("lifafa/", "mandelbrot/")
# The workflows whose heavy lanes this scopes: editing one is not inert.
HEAVY_WORKFLOWS = (".github/workflows/ci.yml", ".github/workflows/gui-tests.yml")
INERT_FILES = ("renovate.json", "codecov.yml")
PROSE_DIRS = ("docs/", "skills/", "runbooks/")
# Never inert even when nothing quotes them.
PROSE_COMPILED = ("docs/collab-spec/",)
# Where a prose file's path would be written if a heavy lane read it.
SOURCES = (
    "letters", "letters-core", "tables", "tables-core", "decks", "decks-core",
    "suite-common", "suite-common-core", "suite-export", "suite-collab", "suite-convert",
    "flatpak", "scripts", "conformance", "tools", "tests/gui", "third_party", "po", "data",
    ".config", "Cargo.toml",
)


def is_prose(path: str) -> bool:
    if path.startswith(PROSE_DIRS):
        return True
    return "/" not in path and path.endswith(".md")


def mentioned(path: str, root: Path = REPO_ROOT) -> bool:
    """Does a source the heavy lanes build or run name this file, a directory
    above it (depth 2 and up: `docs/features`, not `docs`) or its file name?"""
    parts = PurePosixPath(path).parts
    needles = {path, parts[-1]}
    needles.update("/".join(parts[:n]) for n in range(2, len(parts)))
    existing = [s for s in SOURCES if (root / s).exists()]
    for needle in needles:
        found = subprocess.run(
            ["git", "grep", "-q", "-F", "-e", needle, "--", *existing],
            cwd=root, capture_output=True,
        )
        if found.returncode == 0:
            return True
        if found.returncode != 1:
            raise RuntimeError(f"git grep failed for {needle!r}: {found.stderr.decode()}")
    return False


def is_inert(path: str, root: Path = REPO_ROOT) -> bool:
    if path.startswith(OTHER_WORKSPACES):
        return True
    if path.startswith(".github/"):
        return path not in HEAVY_WORKFLOWS
    if path in INERT_FILES:
        return True
    if path.startswith(PROSE_COMPILED):
        return False
    if is_prose(path):
        return not mentioned(path, root)
    return False


def apps_reached(changed: list, root: Path = REPO_ROOT) -> list:
    """The apps whose Flatpak can change: the crates each links, plus whatever
    is not a crate of this workspace at all, minus the inert paths."""
    crates = {app: real_scope.workspace_crates(app, root) for app in APPS}
    members = {c for linked in crates.values() for c in linked}
    # A path outside every crate an app links (shared files, tools/, a crate
    # no app links such as suite-convert) reaches every app: unsure means build.
    reached = set()
    for path in changed:
        if is_inert(path, root):
            continue
        owner = path.split("/", 1)[0] if "/" in path else None
        if owner in members:
            reached.update(app for app in APPS if owner in crates[app])
        else:
            return list(APPS)
    return [app for app in APPS if app in reached]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--changed", help="file listing the changed paths, one per line")
    source.add_argument("--all", action="store_true", help="not a pull request: everything runs")
    args = parser.parse_args()
    if args.all:
        heavy, flatpak = True, list(APPS)
    else:
        changed = [line.strip() for line in Path(args.changed).read_text().splitlines() if line.strip()]
        heavy = not all(is_inert(path) for path in changed)
        flatpak = apps_reached(changed) if heavy else []
    print(f"heavy={json.dumps(heavy)}")
    print(f"flatpak={json.dumps(flatpak, separators=(',', ':'))}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
