#!/usr/bin/env python3
"""Validate the versioned office corpus contract and package structure."""

import json
import os
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).parent.parent
CORPUS = ROOT / "interop" / "corpus.json"
FORMATS = {"docx", "odt", "xlsx", "ods", "pptx", "odp"}
DIRECTIONS = {"our-write-oracle-rewrite", "oracle-authored"}
POLICY = {"must_preserve", "opaque_pass_through", "warn_on_loss", "forbidden_silent_loss"}
OOXML = {"docx", "xlsx", "pptx"}
ODF = {"odt", "ods", "odp"}
NS_REL = "http://schemas.openxmlformats.org/package/2006/relationships"
NS_CT = "http://schemas.openxmlformats.org/package/2006/content-types"
NS_MANIFEST = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"


def fail(message):
    raise ValueError(message)


def xml(path):
    try:
        return ET.parse(path).getroot()
    except (ET.ParseError, OSError) as exc:
        fail(f"{path}: invalid XML: {exc}")


def package_check(item):
    package = ROOT / "interop" / item["package_dir"]
    if not package.is_dir():
        fail(f"{item['id']}: missing package directory {package}")
    files = {p.relative_to(package).as_posix() for p in package.rglob("*") if p.is_file()}
    if item["format"] in OOXML:
        if "[Content_Types].xml" not in files or "_rels/.rels" not in files:
            fail(f"{item['id']}: OOXML package needs [Content_Types].xml and _rels/.rels")
        types = xml(package / "[Content_Types].xml")
        overrides = {e.attrib.get("PartName", "").lstrip("/") for e in types.findall(f"{{{NS_CT}}}Override")}
        for part in overrides:
            if part not in files:
                fail(f"{item['id']}: content-type override targets missing {part}")
        rels = xml(package / "_rels/.rels")
        for rel in rels.findall(f"{{{NS_REL}}}Relationship"):
            target = (package / rel.attrib["Target"]).resolve()
            if not target.is_file() or package.resolve() not in target.parents:
                fail(f"{item['id']}: relationship targets missing/outside package: {rel.attrib['Target']}")
    else:
        if "mimetype" not in files or "META-INF/manifest.xml" not in files:
            fail(f"{item['id']}: ODF package needs mimetype and META-INF/manifest.xml")
        expected = {"odt":"application/vnd.oasis.opendocument.text", "ods":"application/vnd.oasis.opendocument.spreadsheet", "odp":"application/vnd.oasis.opendocument.presentation"}[item["format"]]
        if (package / "mimetype").read_text().strip() != expected:
            fail(f"{item['id']}: mimetype is not {expected}")
        manifest = xml(package / "META-INF/manifest.xml")
        for entry in manifest.findall(f"{{{NS_MANIFEST}}}file-entry"):
            name = entry.attrib.get(f"{{{NS_MANIFEST}}}full-path")
            if name and name != "/" and name not in files:
                fail(f"{item['id']}: manifest entry targets missing {name}")


def main():
    data = json.loads(CORPUS.read_text())
    if data.get("schema_version") != 1:
        fail("unsupported corpus schema")
    if set(data.get("loss_budgets", {})) != POLICY:
        fail("loss_budgets must define exactly the four policy categories")
    if any(not isinstance(v, list) or not v for v in data["loss_budgets"].values()):
        fail("every loss budget category must have at least one rule")
    fixtures = data.get("fixtures", [])
    if len(fixtures) < 6:
        fail("corpus must contain all six office formats")
    seen = set()
    for item in fixtures:
        required = {"id", "format", "direction", "authoring", "provenance", "license", "scenario", "package_dir", "semantics"}
        missing = required - set(item)
        if missing:
            fail(f"{item.get('id', '<unknown>')}: missing {sorted(missing)}")
        if item["format"] not in FORMATS or item["direction"] not in DIRECTIONS:
            fail(f"{item['id']}: unsupported format or direction")
        if not item["authoring"].get("suite") or not item["authoring"].get("version"):
            fail(f"{item['id']}: authoring suite and version are required")
        if not item["provenance"] or not item["license"] or not item["scenario"] or not item["semantics"]:
            fail(f"{item['id']}: provenance, license, scenario, and semantics are required")
        seen.add((item["format"], item["direction"]))
        package_check(item)
    expected = {(fmt, direction) for fmt in FORMATS for direction in DIRECTIONS}
    missing = expected - seen
    if missing:
        fail(f"missing format/direction fixtures: {sorted(missing)}")
    binary = os.environ.get("ONLYOFFICE_BIN")
    if binary:
        artifacts = [ROOT / item["artifact"] for item in fixtures if item.get("artifact")]
        if not artifacts:
            print("OnlyOffice lane: skipped (corpus has no binary artifacts yet)")
        else:
            with tempfile.TemporaryDirectory() as out:
                for artifact in artifacts:
                    result = subprocess.run([binary, "--headless", "--convert-to", "pdf", "--outdir", out, str(artifact)], capture_output=True, text=True)
                    if result.returncode:
                        fail(f"OnlyOffice failed for {artifact}: {result.stderr.strip()}")
            print(f"OnlyOffice lane: passed ({len(artifacts)} artifacts)")
    else:
        print("OnlyOffice lane: skipped (set ONLYOFFICE_BIN to enable)")
    print(f"Corpus valid: {len(fixtures)} fixtures, {len(seen)} format/direction pairs")


if __name__ == "__main__":
    try:
        main()
    except ValueError as exc:
        print(f"corpus validation failed: {exc}", file=sys.stderr)
        sys.exit(1)
