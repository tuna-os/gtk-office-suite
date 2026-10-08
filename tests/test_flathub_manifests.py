"""Flathub's manifests (tools/release/flathub_manifests.py) are the
development ones built offline from the locked source archive: nothing
else differs, and every source they name is locked."""

import copy
import glob
import json
import os
import sys

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
