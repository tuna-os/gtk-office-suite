#!/usr/bin/env python3
"""Flathub's manifests, derived from the development ones (#1209).

    flathub_manifests.py <archive-url-or-path> <sha256> <out-dir>
    flathub_manifests.py letters <tag> <flatpak-cargo-generator.py> <out-dir>

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
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

FLATPAK_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "flatpak")
GIT_VENDOR = "cargo/git-vendor"
GIT_VENDORED_SOURCES = "git-vendored-sources"


def separate_git_crates(sources: list) -> list:
    """Vendor the generator's git crates apart from its crates.io ones.

    The generator puts both in one directory source, `cargo/vendor`. A lock
    holding the same name and version from crates.io and from git (v2.1.0:
    rdocx-oxml 0.1.2) then resolves the git package to the registry copy,
    cargo wants to write that copy's checksum into the lock, and the
    `--locked` build refuses. A second directory source for the git crates
    keeps the two apart.
    """
    vendor, mkdir = "cargo/vendor/", {"type": "shell", "commands": [f"mkdir -p {GIT_VENDOR}"]}
    out, names = [], set()
    for source in sources:
        source = copy.deepcopy(source)
        if source["type"] == "shell":
            for i, command in enumerate(source["commands"]):
                if command.startswith('cp -r --reflink=auto "flatpak-cargo/git/') and f'"{vendor}' in command:
                    source["commands"][i] = command.replace(f'"{vendor}', f'"{GIT_VENDOR}/')
                    names.add(command.rsplit(f'"{vendor}', 1)[1].rstrip('"'))
                    if mkdir not in out:
                        out.append(mkdir)
        elif source["type"] == "inline" and source["dest"].startswith(vendor) \
                and source["dest"].removeprefix(vendor) in names:
            source["dest"] = f"{GIT_VENDOR}/{source['dest'].removeprefix(vendor)}"
        elif source["type"] == "inline" and source["dest"] == "cargo" and source["dest-filename"] == "config":
            tables = tomllib.loads(source["contents"])["source"]
            for table in tables.values():
                if "git" in table:
                    table["replace-with"] = GIT_VENDORED_SOURCES
            if any("git" in table for table in tables.values()):
                tables[GIT_VENDORED_SOURCES] = {"directory": GIT_VENDOR}
            source["contents"] = "\n".join(
                f"[source.{json.dumps(name)}]\n"
                + "".join(f"{key} = {json.dumps(value)}\n" for key, value in table.items())
                for name, table in tables.items())
        out.append(source)
    return out


def letters_submission(tag: str, generator: str, out: str) -> None:
    """Prepare Letters from a tag's lock, never the working tree's lock.

    Run with the Python environment that has the official
    flatpak-cargo-generator.py's dependencies installed.
    """
    repo = Path(FLATPAK_DIR).resolve().parent
    commit = subprocess.check_output(
        ["git", "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}"], cwd=repo, text=True).strip()
    lock = subprocess.check_output(["git", "show", f"{commit}:Cargo.lock"], cwd=repo)
    output = Path(out).resolve()
    output.mkdir(parents=True, exist_ok=True)
    (output / "letters-Cargo.lock").write_bytes(lock)
    generator_path = Path(generator).resolve()
    with tempfile.TemporaryDirectory() as tmp:
        lock_path = Path(tmp) / "Cargo.lock"
        lock_path.write_bytes(lock)
        subprocess.run([sys.executable, str(generator_path), str(lock_path),
                        "-o", str(output / "cargo-sources.json")], check=True)
    sources_path = output / "cargo-sources.json"
    sources_path.write_text(json.dumps(separate_git_crates(json.loads(sources_path.read_text())), indent=4))
    dev = json.loads((repo / "flatpak/org.tunaos.letters.json").read_text())
    manifest = flathub(dev, "unused", "unused")
    module = manifest["modules"][-1]
    module["sources"] = [{"type": "git", "url": "https://github.com/tuna-os/gtk-office-suite.git",
                          "tag": tag, "commit": commit}, "cargo-sources.json"]
    manifest["build-options"].setdefault("env", {})["CARGO_HOME"] = f"/run/build/{module['name']}/cargo"
    (output / "org.tunaos.letters.json").write_text(json.dumps(manifest, indent=2) + "\n")
    provenance = {"tag": tag, "commit": commit, "cargo-lock-sha256": hashlib.sha256(lock).hexdigest(),
                  "generator-sha256": hashlib.sha256(generator_path.read_bytes()).hexdigest()}
    (output / "letters-source.json").write_text(json.dumps(provenance, indent=2) + "\n")


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
    # Archive mode remains the candidate gate; submission mode prepares the
    # checked-in Letters manifest and source list for the owner to submit.
    if len(argv) == 5 and argv[1] == "letters":
        letters_submission(*argv[2:])
        return 0
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
