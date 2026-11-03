#!/usr/bin/env python3
"""Validate the examples next to this file against the DCP schemas.

Usage: python3 docs/collab-spec/validate.py
Requires the `jsonschema` package. Exit status is 1 if any example is invalid
or any negative case is accepted.
"""
import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parent
schemas = {p.name: json.loads(p.read_text()) for p in (ROOT / "schemas").glob("*.json")}
registry = Registry().with_resources(
    (name, Resource.from_contents(schema)) for name, schema in schemas.items()
)

POSITIVE = {
    "meta.json": "meta.schema.json",
    "version.json": "version.schema.json",
    "version-durable-stream.json": "version.schema.json",
    "version-blake3.json": "version.schema.json",
    "lease-held.json": "lease.schema.json",
    "lease-released.json": "lease.schema.json",
    "lease-held-empty-document.json": "lease.schema.json",
    "lease-held-wopi.json": "lease.schema.json",
    "update.json": "update.schema.json",
}


def check(schema_name: str, doc: dict) -> list[str]:
    validator = Draft202012Validator(schemas[schema_name], registry=registry)
    return [e.message for e in validator.iter_errors(doc)]


def mutated(name: str, change) -> dict:
    doc = json.loads((ROOT / "examples" / name).read_text())
    change(doc)
    return doc


NEGATIVE = [
    ("meta without L1", "meta.json", "meta.schema.json", lambda d: d.update(levels=["L0"])),
    ("meta L2 without algorithms", "meta.json", "meta.schema.json", lambda d: d.pop("algorithms")),
    ("version with bad hash", "version.json", "version.schema.json", lambda d: d["file"].update(sha256="xyz")),
    ("version without parents", "version.json", "version.schema.json", lambda d: d.pop("parents")),
    ("version, plain-http webdav", "version.json", "version.schema.json",
     lambda d: d["file"].update(locator={"type": "webdav", "href": "http://x/y"})),
    ("held lease without token", "lease-held.json", "lease.schema.json", lambda d: d.pop("token")),
    ("held lease, 2 hour duration", "lease-held.json", "lease.schema.json", lambda d: d.update(duration_ms=7_200_000)),
    ("held lease without renewal", "lease-held.json", "lease.schema.json", lambda d: d.pop("renewal")),
    ("held lease, negative renewal", "lease-held.json", "lease.schema.json", lambda d: d.update(renewal=-1)),
    ("held lease, fractional renewal", "lease-held.json", "lease.schema.json", lambda d: d.update(renewal=1.5)),
    ("held lease, string renewal", "lease-held.json", "lease.schema.json", lambda d: d.update(renewal="2")),
    ("held lease, empty base_version (use omission, not a sentinel)", "lease-held.json", "lease.schema.json",
     lambda d: d.update(base_version="")),
    ("held lease, unknown field", "lease-held.json", "lease.schema.json", lambda d: d.update(epoch=1)),
    ("held lease, 257-character token", "lease-held.json", "lease.schema.json", lambda d: d.update(token="t" * 257)),
    ("update over 34000 bytes", "update.json", "update.schema.json",
     lambda d: d["payload"].update(size=34001)),
    ("update with bad algorithm id", "update.json", "update.schema.json", lambda d: d.update(algorithm="Loro")),
    # Locators added in draft 1 update 2 (8.4, 8.5): additive enum values.
    ("durable-stream locator, plain-http href", "version-durable-stream.json", "version.schema.json",
     lambda d: d["file"]["locator"].update(href="http://streams.example.org/dcp/x/f/y")),
    ("durable-stream locator without href", "version-durable-stream.json", "version.schema.json",
     lambda d: d["file"]["locator"].pop("href")),
    ("durable-stream locator, hash instead of href", "version-durable-stream.json", "version.schema.json",
     lambda d: d["file"].update(locator={"type": "durable-stream", "hash": "0" * 64})),
    ("blake3 locator without hash", "version-blake3.json", "version.schema.json",
     lambda d: d["file"]["locator"].pop("hash")),
    ("blake3 locator, 63-digit hash", "version-blake3.json", "version.schema.json",
     lambda d: d["file"]["locator"].update(hash="a" * 63)),
    ("blake3 locator, 65-digit hash", "version-blake3.json", "version.schema.json",
     lambda d: d["file"]["locator"].update(hash="a" * 65)),
    ("blake3 locator, upper-case hash", "version-blake3.json", "version.schema.json",
     lambda d: d["file"]["locator"].update(hash="A" * 64)),
    ("blake3 locator, non-hex hash", "version-blake3.json", "version.schema.json",
     lambda d: d["file"]["locator"].update(hash="g" * 64)),
    ("blake3 locator, href instead of hash", "version-blake3.json", "version.schema.json",
     lambda d: d["file"].update(locator={"type": "blake3", "href": "https://example.org/x"})),
    ("unknown locator type (a reader of draft 1 update 2 rejects it too)", "version.json", "version.schema.json",
     lambda d: d["file"].update(locator={"type": "ipfs", "cid": "bafy"})),
    ("blake3 version, sha256 still required", "version-blake3.json", "version.schema.json",
     lambda d: d["file"].pop("sha256")),
]

# Draft 0 objects ("dcp": 0) must be rejected by every draft 1 schema.
for _example, _schema in POSITIVE.items():
    NEGATIVE.append((f"{_example} with dcp 0", _example, _schema, lambda d: d.update(dcp=0)))

failed = False
for example, schema in POSITIVE.items():
    errors = check(schema, json.loads((ROOT / "examples" / example).read_text()))
    if errors:
        failed = True
        print(f"FAIL  {example} should be valid: {errors[0]}")
    else:
        print(f"ok    {example}")
for label, example, schema, change in NEGATIVE:
    if not check(schema, mutated(example, change)):
        failed = True
        print(f"FAIL  negative case accepted: {label}")
    else:
        print(f"ok    rejects: {label}")
sys.exit(1 if failed else 0)
