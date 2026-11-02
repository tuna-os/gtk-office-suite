"""Flathub's manifests (tools/release/flathub_manifests.py) are the
development ones built offline from the locked source archive: nothing
else differs, and every source they name is locked."""

import copy
import glob
import hashlib
import json
import os
import sys
import tomllib
from pathlib import Path
from urllib.parse import urlparse

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "tools", "release"))

import flathub_manifests as fm  # noqa: E402

DEV = sorted(glob.glob(os.path.join(REPO, "flatpak", "org.tunaos.*.json")))


def test_every_app_has_a_manifest():
    assert [os.path.basename(p) for p in DEV] == [
        "org.tunaos.decks.json", "org.tunaos.letters.json", "org.tunaos.tables.json"]


def test_the_flathub_manifests_are_offline_and_locked():
    for path in DEV:
        manifest = fm.flathub(json.load(open(path)), "https://example.org/src.tar.gz", "ab" * 32)
        assert fm.locked_sources_problems(manifest) == [], path
        app = manifest["modules"][-1]
        assert app["sources"] == [{"type": "archive", "sha256": "ab" * 32, "url": "https://example.org/src.tar.gz"}]
        assert any("cargo build --offline --locked --release" in c for c in app["build-commands"])


def test_only_the_app_source_and_its_cargo_change():
    for path in DEV:
        dev = json.load(open(path))
        flat = fm.flathub(dev, "/tmp/src.tar.gz", "ab" * 32)
        assert flat["modules"][-1]["sources"] == [{"type": "archive", "sha256": "ab" * 32, "path": "/tmp/src.tar.gz"}]
        a, b = copy.deepcopy(dev), copy.deepcopy(flat)
        for m in (a, b):
            m.get("build-options", {}).pop("build-args", None)
            m["modules"][-1].pop("sources")
            m["modules"][-1]["build-commands"] = [
                c.replace("cargo clean && ", "").replace(" --offline", "") for c in m["modules"][-1]["build-commands"]]
        assert a == b, path


def test_the_development_manifests_fetch_nothing_that_moves():
    # A pinned sha256 on a branch URL breaks the build the day the branch
    # moves: the dictionaries were fetched from LibreOffice's master.
    for path in DEV:
        for module in json.load(open(path))["modules"]:
            for src in module.get("sources", []):
                if "url" in src:
                    assert "/master/" not in src["url"] and "/main/" not in src["url"], src["url"]
                    assert src.get("sha256"), src["url"]


def test_a_branch_url_or_an_online_build_is_reported():
    manifest = {"build-options": {"build-args": ["--share=network"]}, "modules": [
        {"name": "dict", "sources": [{"type": "file", "url": "https://x/master/a", "sha256": "1"}]},
        {"name": "app", "build-commands": ["cargo build --locked"], "sources": [{"type": "dir", "path": ".."}]}]}
    assert fm.locked_sources_problems(manifest) == [
        "the build is granted the network",
        "dict: https://x/master/a names a branch, which moves",
        "app: cargo builds online: cargo build --locked",
        "app: builds from a directory, not a locked source",
    ]


def test_letters_submission_covers_the_tagged_lock():
    root = Path(REPO) / "flathub"
    lock_bytes = (root / "letters-Cargo.lock").read_bytes()
    lock = tomllib.loads(lock_bytes.decode())
    provenance = json.loads((root / "letters-source.json").read_text())
    assert hashlib.sha256(lock_bytes).hexdigest() == provenance["cargo-lock-sha256"]
    manifest = json.loads((root / "org.tunaos.letters.json").read_text())
    app = manifest["modules"][-1]
    assert app["sources"] == [
        {"type": "git", "url": "https://github.com/tuna-os/gtk-office-suite.git",
         "tag": provenance["tag"], "commit": provenance["commit"]}, "cargo-sources.json"]
    assert len(provenance["commit"]) == 40
    assert "--share=network" not in manifest["build-options"].get("build-args", [])
    assert "cargo build --offline --locked --release -p letters" in app["build-commands"]
    assert manifest["build-options"]["env"]["CARGO_HOME"] == "/run/build/letters-rust/cargo"
    sources = json.loads((root / "cargo-sources.json").read_text())
    archives = {s["url"]: s["sha256"] for s in sources if s["type"] == "archive"}
    git_pins = {(s["url"], s["commit"]) for s in sources if s["type"] == "git"}
    for package in lock["package"]:
        source = package.get("source", "")
        if source.startswith("registry+"):
            name, version = package["name"], package["version"]
            url = f"https://static.crates.io/crates/{name}/{name}-{version}.crate"
            assert archives[url] == package["checksum"]
        elif source.startswith("git+"):
            parsed = urlparse(source.removeprefix("git+"))
            url = f"{parsed.scheme}://{parsed.netloc}{parsed.path.removesuffix('.git')}".lower()
            assert (url, parsed.fragment) in git_pins
    assert len(archives) == sum(p.get("source", "").startswith("registry+") for p in lock["package"])


def test_ci_builds_the_committed_submission_on_both_architectures():
    import yaml

    workflow = yaml.safe_load((Path(REPO) / ".github/workflows/release-gate.yml").read_text())
    job = workflow["jobs"]["offline-flatpak"]
    assert {row["arch"] for row in job["strategy"]["matrix"]["include"]} == {"x86_64", "aarch64"}
    builds = [s for s in job["steps"] if s.get("with", {}).get("manifest-path") == "flathub/org.tunaos.letters.json"]
    assert len(builds) == 1
    assert "if" not in builds[0] and not builds[0].get("continue-on-error")
    assert builds[0]["with"]["cache"] is False


def test_submission_reads_the_tag_lock_instead_of_the_checkout(tmp_path, monkeypatch):
    calls = []
    lock = b'version = 4\n'

    def git_output(args, **kwargs):
        calls.append(args)
        return "a" * 40 if args[1] == "rev-parse" else lock

    def generator_run(args, **kwargs):
        assert Path(args[2]).read_bytes() == lock
        Path(args[-1]).write_text("[]\n")

    monkeypatch.setattr(fm.subprocess, "check_output", git_output)
    monkeypatch.setattr(fm.subprocess, "run", generator_run)
    generator = tmp_path / "generator.py"
    generator.write_text("# test generator\n")
    out = tmp_path / "submission"
    fm.letters_submission("v-test", str(generator), str(out))
    assert calls[0][-1] == "refs/tags/v-test^{commit}"
    assert calls[1][-1] == f"{'a' * 40}:Cargo.lock"
    assert (out / "letters-Cargo.lock").read_bytes() == lock
    provenance = json.loads((out / "letters-source.json").read_text())
    assert provenance["generator-sha256"] == hashlib.sha256(generator.read_bytes()).hexdigest()
