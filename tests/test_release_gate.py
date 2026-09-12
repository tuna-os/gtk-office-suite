"""Unit tests for scripts/release_gate.py — the release contract validator.

check_app validates the five release assets (manifest, desktop, schema,
metainfo, icon) for each of the three Flatpaks against a fixed contract.
Tests build a minimal fake flatpak/ tree in a temp dir and assert the
contract checks fire — no real Flatpak build needed.
"""

import json
import os
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'scripts'))

import release_gate as rg  # noqa: E402


def _make_app_dir(root, app, cargo=True):
    """Create a valid flatpak/ asset set for one app under root."""
    app_id = f"org.tunaos.{app}"
    d = root / "flatpak"
    d.mkdir(parents=True, exist_ok=True)
    icons = d / "icons"
    icons.mkdir(exist_ok=True)

    manifest = {
        "app-id": app_id,
        "runtime": "org.gnome.Platform",
        "runtime-version": "50",
        "sdk": "org.gnome.Sdk",
        "command": app,
        "modules": [
            {"name": app, "buildsystem": "simple", "sources": [
                {"type": "dir", "path": ".."},
            ]},
        ],
    }
    if cargo:
        manifest["modules"].append({
            "name": f"{app}-rs", "buildsystem": "simple",
            "build-commands": ["cargo build --locked"],
            "sources": [],
        })
    (d / f"{app_id}.json").write_text(json.dumps(manifest))

    desktop = f"[Desktop Entry]\nExec={app}\nIcon={app_id}\nMimeType=text/plain;\n"
    (d / f"{app_id}.desktop").write_text(desktop)

    schema = ('<schemalist><schema id="%s" path="/org/tunaos/%s/">'
              '<key name="recent-files" type="b"><default>true</default></key>'
              '</schema></schemalist>') % (app_id, app)
    (d / f"{app_id}.gschema.xml").write_text(schema)

    metainfo = ('<component type="desktop-application"><id>%s</id>'
                '<launchable type="desktop-id">%s.desktop</launchable>'
                '<screenshots><screenshot type="default"></screenshot></screenshots>'
                '<releases><release version="1.0" date="2026-08-01"/></releases>'
                '</component>') % (app_id, app)
    (d / f"{app_id}.metainfo.xml").write_text(metainfo)

    (icons / f"{app_id}.svg").write_text("<svg></svg>")
    return d


def _install(monkeypatch, tmp_path):
    """Point ROOT at tmp_path and return the flatpak dir."""
    monkeypatch.setattr(rg, "ROOT", tmp_path)
    return tmp_path / "flatpak"


def test_check_app_valid(tmp_path, monkeypatch):
    _install(monkeypatch, tmp_path)
    _make_app_dir(tmp_path, "letters")
    assets = rg.check_app("letters")
    assert len(assets) == 5


def test_check_app_missing_asset(tmp_path, monkeypatch):
    _install(monkeypatch, tmp_path)
    _make_app_dir(tmp_path, "letters")
    (tmp_path / "flatpak" / "icons" / "org.tunaos.letters.svg").unlink()
    with pytest.raises(AssertionError, match="missing release asset"):
        rg.check_app("letters")


def test_check_app_manifest_key_missing(tmp_path, monkeypatch):
    _install(monkeypatch, tmp_path)
    _make_app_dir(tmp_path, "letters")
    manifest_path = tmp_path / "flatpak" / "org.tunaos.letters.json"
    manifest = json.loads(manifest_path.read_text())
    del manifest["sdk"]
    manifest_path.write_text(json.dumps(manifest))
    with pytest.raises(AssertionError, match="manifest lacks sdk"):
        rg.check_app("letters")


def test_check_app_command_mismatch(tmp_path, monkeypatch):
    _install(monkeypatch, tmp_path)
    _make_app_dir(tmp_path, "letters")
    manifest_path = tmp_path / "flatpak" / "org.tunaos.letters.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["command"] = "other"
    manifest_path.write_text(json.dumps(manifest))
    with pytest.raises(AssertionError, match="app-id/command mismatch"):
        rg.check_app("letters")


def test_check_app_cargo_without_locked(tmp_path, monkeypatch):
    _install(monkeypatch, tmp_path)
    _make_app_dir(tmp_path, "letters", cargo=False)
    manifest_path = tmp_path / "flatpak" / "org.tunaos.letters.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["modules"].append({
        "name": "rust", "buildsystem": "simple",
        "build-commands": ["cargo build"],  # NOT --locked
        "sources": [],
    })
    manifest_path.write_text(json.dumps(manifest))
    with pytest.raises(AssertionError, match="--locked"):
        rg.check_app("letters")


def test_check_app_desktop_exec_mismatch(tmp_path, monkeypatch):
    _install(monkeypatch, tmp_path)
    _make_app_dir(tmp_path, "letters")
    desktop_path = tmp_path / "flatpak" / "org.tunaos.letters.desktop"
    desktop_path.write_text("[Desktop Entry]\nExec=wrong\nIcon=org.tunaos.letters\nMimeType=text/plain;\n")
    with pytest.raises(AssertionError, match="Exec/Icon mismatch"):
        rg.check_app("letters")


def test_check_app_desktop_no_mime(tmp_path, monkeypatch):
    _install(monkeypatch, tmp_path)
    _make_app_dir(tmp_path, "letters")
    desktop_path = tmp_path / "flatpak" / "org.tunaos.letters.desktop"
    desktop_path.write_text("[Desktop Entry]\nExec=letters\nIcon=org.tunaos.letters\n")
    with pytest.raises(AssertionError, match="MIME"):
        rg.check_app("letters")


def test_check_app_schema_id_mismatch(tmp_path, monkeypatch):
    _install(monkeypatch, tmp_path)
    _make_app_dir(tmp_path, "letters")
    schema_path = tmp_path / "flatpak" / "org.tunaos.letters.gschema.xml"
    schema_path.write_text('<schemalist><schema id="org.tunaos.wrong"></schema></schemalist>')
    with pytest.raises(AssertionError, match="schema id mismatch"):
        rg.check_app("letters")


def test_check_app_metainfo_no_launchable(tmp_path, monkeypatch):
    _install(monkeypatch, tmp_path)
    _make_app_dir(tmp_path, "letters")
    meta_path = tmp_path / "flatpak" / "org.tunaos.letters.metainfo.xml"
    meta_path.write_text('<component type="desktop-application"><id>org.tunaos.letters</id></component>')
    with pytest.raises(AssertionError, match="launchable"):
        rg.check_app("letters")


def test_check_app_metainfo_no_release(tmp_path, monkeypatch):
    _install(monkeypatch, tmp_path)
    _make_app_dir(tmp_path, "letters")
    meta_path = tmp_path / "flatpak" / "org.tunaos.letters.metainfo.xml"
    meta_path.write_text(
        '<component type="desktop-application"><id>org.tunaos.letters</id>'
        '<launchable type="desktop-id">org.tunaos.letters.desktop</launchable>'
        '<screenshots><screenshot type="default"></screenshot></screenshots>'
        '</component>')
    with pytest.raises(AssertionError, match="screenshot or release"):
        rg.check_app("letters")


def test_module_size_ceiling_exceeded(tmp_path, monkeypatch):
    _install(monkeypatch, tmp_path)
    for app in rg.APPS:
        _make_app_dir(tmp_path, app)
    (tmp_path / "po").mkdir(exist_ok=True)
    (tmp_path / "po" / "POTFILES").write_text("")
    (tmp_path / "po" / "gtk-office-suite.pot").write_text("")
    (tmp_path / "Cargo.lock").write_text("")

    letters_win = tmp_path / "letters" / "src" / "window.rs"
    letters_win.parent.mkdir(parents=True, exist_ok=True)
    letters_win.write_text("\n" * 1900)

    with pytest.raises(AssertionError, match="exceeds maximum line ceiling"):
        rg.main()



# ── The gate's trigger must cover the files it polices ──────────────────
#
# This is the third instance of one defect: a hand-written path filter in
# YAML that omits the code it exists to check. The oracle's filter left out
# suite-common-core (tests/test_oracle_triggers.py); the journey selector
# could pick too few apps (tests/test_journey_selection.py); and this gate
# enforces per-module line ceilings while `release-gate.yml` triggers on
# flatpak/, flathub/, po/, Cargo.lock and itself — none of which are the
# files whose lines it counts.
#
# That is not hypothetical. `tables/src/window.rs` crossed its 2300-line
# ceiling at 5c7f436 and sat over it on main until #594 brought it back to
# 2226, because no pull request that touched it ever ran the gate. Fixing
# the count without fixing the trigger leaves the next breach just as
# silent, so the required list is derived from the ceilings themselves
# rather than copied alongside them.

WORKFLOW = Path(__file__).resolve().parent.parent / ".github" / "workflows" / "release-gate.yml"
# `module_ceilings` is a local in main(), so it is read from the source
# rather than imported. Strict on purpose: a parse that found nothing would
# make every assertion below hold vacuously, which is the failure this
# whole area keeps producing.
CEILING_ENTRY = re.compile(r'^\s*"([^"]+)":\s*(\d+),', re.M)
PATTERN_LINE = re.compile(r"^\s*-\s*'([^']+)'\s*$", re.M)


def ceiling_paths():
    """The repository paths whose line counts the gate enforces."""
    source = Path(rg.__file__).read_text()
    block = source.split("module_ceilings = {", 1)
    assert len(block) == 2, "release_gate.py no longer declares module_ceilings"
    body = block[1].split("}", 1)[0]
    found = CEILING_ENTRY.findall(body)
    assert found, "module_ceilings parsed as empty, so this test would check nothing"
    return {path for path, _limit in found}


def trigger_paths():
    """The `paths:` filter under the workflow's pull_request trigger."""
    text = WORKFLOW.read_text()
    head, _, _rest = text.partition("\n  push:")
    assert "pull_request:" in head, f"{WORKFLOW} has no pull_request trigger to read"
    found = PATTERN_LINE.findall(head)
    assert found, f"{WORKFLOW}'s pull_request trigger declares no paths"
    return found


def _covered(path: str, patterns: list) -> bool:
    """Whether any filter pattern would select this path.

    Only the glob forms the filter actually uses are honoured — an exact
    name, or a `dir/**` prefix. Matching more loosely here would let a
    pattern appear to cover a file it does not.
    """
    for pattern in patterns:
        if pattern == path:
            return True
        if pattern.endswith("/**") and path.startswith(pattern[:-2]):
            return True
    return False


def test_every_ceiling_file_triggers_the_gate():
    patterns = trigger_paths()
    missing = sorted(path for path in ceiling_paths() if not _covered(path, patterns))
    assert not missing, (
        "release-gate.yml does not run when these files change, yet the gate "
        f"enforces a line ceiling on each: {', '.join(missing)}. A ceiling "
        "nothing triggers on is how tables/src/window.rs sat 43 lines over "
        "its limit on main without anything noticing."
    )


def test_the_filter_is_actually_read():
    """A parser that silently found nothing would pass the test above."""
    patterns = trigger_paths()
    assert len(patterns) > 3, patterns
    assert any(p.startswith("flatpak") for p in patterns), patterns


def test_the_ceilings_are_actually_read():
    paths = ceiling_paths()
    assert "tables/src/window.rs" in paths, paths
    assert len(paths) >= 4, paths
