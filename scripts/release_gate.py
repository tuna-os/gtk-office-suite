#!/usr/bin/env python3
"""Dependency-free release contract checks for the three Flatpaks.

This deliberately checks source metadata, not a locally installed desktop.
The CI job runs AppStream/desktop/schema validators as a second layer when
those tools are available in the runner image.
"""
from __future__ import annotations

import configparser
import hashlib
import json
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
APPS = ("letters", "tables", "decks")
REQUIRED_MANIFEST_KEYS = ("app-id", "runtime", "runtime-version", "sdk", "command", "modules")


def fail(message: str) -> None:
    raise AssertionError(message)


def check_app(app: str) -> list[Path]:
    app_id = f"org.tunaos.{app}"
    manifest_path = ROOT / "flatpak" / f"{app_id}.json"
    desktop_path = ROOT / "flatpak" / f"{app_id}.desktop"
    schema_path = ROOT / "flatpak" / f"{app_id}.gschema.xml"
    metainfo_path = ROOT / "flatpak" / f"{app_id}.metainfo.xml"
    icon_path = ROOT / "flatpak" / "icons" / f"{app_id}.svg"
    for path in (manifest_path, desktop_path, schema_path, metainfo_path, icon_path):
        if not path.is_file():
            fail(f"{app}: missing release asset {path.relative_to(ROOT)}")

    manifest = json.loads(manifest_path.read_text())
    for key in REQUIRED_MANIFEST_KEYS:
        if key not in manifest:
            fail(f"{app}: manifest lacks {key}")
    if manifest["app-id"] != app_id or manifest["command"] != app:
        fail(f"{app}: manifest app-id/command mismatch")
    if not any(source.get("type") == "dir" and source.get("path") == ".."
               for module in manifest["modules"] for source in module.get("sources", [])):
        fail(f"{app}: manifest has no reproducible repository source")
    commands = [command for module in manifest["modules"] for command in module.get("build-commands", [])]
    if any("cargo build" in command for command in commands) and not any("cargo build --locked" in command for command in commands):
        fail(f"{app}: Rust build does not enforce Cargo.lock with --locked")
    flatpak_text = desktop_path.read_text()
    desktop = configparser.ConfigParser(interpolation=None, strict=False)
    desktop.read_string(flatpak_text)
    entry = desktop["Desktop Entry"]
    if entry.get("exec") != app or entry.get("icon") != app_id:
        fail(f"{app}: desktop Exec/Icon mismatch")
    if not entry.get("mimetype"):
        fail(f"{app}: desktop entry has no MIME associations")

    schema = ET.parse(schema_path).getroot()
    schemas = [node for node in schema if node.tag == "schema"]
    if len(schemas) != 1 or schemas[0].get("id") != app_id:
        fail(f"{app}: schema id mismatch")
    if not any(key.get("name") == "recent-files" for key in schemas[0]):
        fail(f"{app}: recent-files privacy policy is not packaged")

    component = ET.parse(metainfo_path).getroot()
    if component.findtext("id") != app_id:
        fail(f"{app}: metainfo id mismatch")
    launchable = component.find("launchable")
    if launchable is None or launchable.get("type") != "desktop-id":
        fail(f"{app}: metainfo has no desktop launchable")
    if component.find("screenshots/screenshot") is None or component.find("releases/release") is None:
        fail(f"{app}: metainfo lacks screenshot or release data")
    return [manifest_path, desktop_path, schema_path, metainfo_path, icon_path]


def main() -> int:
    assets = [path for app in APPS for path in check_app(app)]
    potfiles = ROOT / "po" / "POTFILES"
    if not potfiles.is_file():
        fail("po/POTFILES is missing")
    for line in potfiles.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#") and not (ROOT / line).is_file():
            fail(f"po/POTFILES names missing source {line}")
    lock = ROOT / "Cargo.lock"
    if not lock.is_file():
        fail("Cargo.lock is required for a reproducible release")
    assets += [potfiles, ROOT / "po" / "gtk-office-suite.pot", lock]
    digest = hashlib.sha256()
    for path in sorted(assets):
        data = path.read_bytes()
        digest.update(str(path.relative_to(ROOT)).encode())
        digest.update(b"\0")
        digest.update(data)
    print(f"release contract: {len(APPS)} apps, {len(assets)} inputs, manifest digest {digest.hexdigest()}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, json.JSONDecodeError, ET.ParseError) as exc:
        print(f"release contract failed: {exc}", file=sys.stderr)
        raise SystemExit(1)
