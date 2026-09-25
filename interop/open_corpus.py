"""Validate the executable corpus independently of its generator and GUI."""

import hashlib
import json
from pathlib import Path


def load_manifest(path):
    path = Path(path).resolve()
    data = json.loads(path.read_text())
    if data.get("schema_version") != 1 or not data.get("fixtures"):
        raise ValueError("Executable corpus is missing fixtures or has an unknown schema")
    seen = set()
    for item in data["fixtures"]:
        if item["id"] in seen:
            raise ValueError(f"Duplicate fixture ID: {item['id']}")
        seen.add(item["id"])
        if item["app"] not in ("letters", "tables", "decks") or item["expected"] not in ("open", "error"):
            raise ValueError(f"Invalid fixture contract: {item['id']}")
        if not item.get("authoring") or not item.get("license"):
            raise ValueError(f"Missing provenance: {item['id']}")
        fixture = (path.parent / item["path"]).resolve()
        if not fixture.is_relative_to(path.parent):
            raise ValueError(f"Fixture escapes corpus directory: {item['id']}")
        if hashlib.sha256(fixture.read_bytes()).hexdigest() != item["sha256"]:
            raise ValueError(f"Fixture hash mismatch: {item['id']}")
    return data
