#!/usr/bin/env python3
"""Flathub's manifests, derived from the development ones (#1209).

    flathub_manifests.py <archive-url-or-path> <sha256> <out-dir>

The manifests in flatpak/ build from the checkout and let cargo fetch
crates while building (`--share=network`), which suits CI and is refused
by Flathub. Flathub's builds have no network and build from sources with
a known checksum. So each Flathub manifest is the development one with:

- the app module built from the release's locked source archive
  (tools/release/source_archive.sh: the tracked files and every crate
  vendored), named by URL, or by local path for a CI build of a revision;
- cargo run offline against that vendored copy, and the network grant
  dropped;
- everything else, the runtime, the permissions and the other modules,
  exactly as the development manifest has it, so the two cannot drift.

Every source in the result is locked: a URL with a sha256, or a local
path CI has just checked. `locked_sources_problems` says which is not.
"""

import copy
import glob
import json
import os
import sys

FLATPAK_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "flatpak")


def flathub(dev: dict, archive: str, sha256: str) -> dict:
    out = copy.deepcopy(dev)
    out.get("build-options", {}).pop("build-args", None)
    app_module = out["modules"][-1]
    source = {"type": "archive", "sha256": sha256}
    source["url" if "://" in archive else "path"] = archive
    app_module["sources"] = [source]
    commands = []
    for cmd in app_module["build-commands"]:
        # The archive holds no build output to clean, and cargo must not
        # reach for the network: the vendored crates are all there is.
        cmd = cmd.replace("cargo clean && ", "")
        cmd = cmd.replace("cargo build --locked", "cargo build --offline --locked")
        commands.append(cmd)
    app_module["build-commands"] = commands
    return out


def locked_sources_problems(manifest: dict) -> list:
    """The sources an offline, reproducible build could not trust."""
    problems = []
    if "--share=network" in manifest.get("build-options", {}).get("build-args", []):
        problems.append("the build is granted the network")
    for module in manifest["modules"]:
        for cmd in module.get("build-commands", []):
            if "cargo build" in cmd and "--offline" not in cmd:
                problems.append(f"{module['name']}: cargo builds online: {cmd}")
        for src in module.get("sources", []):
            if src.get("type") == "dir":
                problems.append(f"{module['name']}: builds from a directory, not a locked source")
            elif "url" in src and not (src.get("sha256") or src.get("commit")):
                problems.append(f"{module['name']}: {src['url']} has no sha256")
            elif "url" in src and "/master/" in src["url"]:
                problems.append(f"{module['name']}: {src['url']} names a branch, which moves")
    return problems


def main(argv: list) -> int:
    archive, sha256, out = argv[1:4]
    os.makedirs(out, exist_ok=True)
    failed = False
    for path in sorted(glob.glob(os.path.join(FLATPAK_DIR, "org.tunaos.*.json"))):
        manifest = flathub(json.load(open(path)), archive, sha256)
        for problem in locked_sources_problems(manifest):
            print(f"{os.path.basename(path)}: {problem}", file=sys.stderr)
            failed = True
        with open(os.path.join(out, os.path.basename(path)), "w") as f:
            json.dump(manifest, f, indent=2)
            f.write("\n")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
