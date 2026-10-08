#!/usr/bin/env python3
"""Assemble a release's evidence bundle (release-evidence.yml, #1209).

    evidence.py <revision> <tag> <in-dir> <out-dir>

release.md asks each release to publish its machine-readable capability
matrix, JUnit, independent-reader results, visual and accessibility
evidence and performance measurements. The release-evidence workflow
gathers them into <in-dir>, one sub-directory per kind; this copies them
into <out-dir> and writes evidence.json beside them: the revision and tag
they were produced at, and every file with its size and SHA-256, so a
reader can tell the bundle is the one CI made and that nothing is missing.

A kind with no files is listed as missing rather than left out, and the
script exits 1, so a release is not published with a gap it does not show.
"""

import hashlib
import json
import os
import shutil
import sys

# Each kind release.md names, and what fills it.
KINDS = {
    "capabilities": "the capability ledger and the scorecard drawn from it",
    "junit": "JUnit from the workspace tests and release-critical journeys at the revision",
    "independent-readers": "LibreOffice round trips (soffice_oracle) and the LibreOffice-authored corpus (lo_parity)",
    "visual": "the visual matrix: every app in four states, light, dark and high contrast, scales 1 and 2",
    "performance": "the performance budgets' measured p50 and p95 against each budget",
}


def sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 16), b""):
            h.update(block)
    return h.hexdigest()


def assemble(revision: str, tag: str, src: str, out: str) -> dict:
    manifest = {"revision": revision, "tag": tag, "kinds": {}, "missing": []}
    for kind, what in KINDS.items():
        files = []
        root = os.path.join(src, kind)
        for dirpath, _, names in sorted(os.walk(root)):
            for name in sorted(names):
                path = os.path.join(dirpath, name)
                rel = os.path.relpath(path, src)
                dest = os.path.join(out, rel)
                os.makedirs(os.path.dirname(dest), exist_ok=True)
                shutil.copyfile(path, dest)
                files.append({"path": rel, "bytes": os.path.getsize(path), "sha256": sha256(path)})
        manifest["kinds"][kind] = {"what": what, "files": files}
        if not files:
            manifest["missing"].append(kind)
    with open(os.path.join(out, "evidence.json"), "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")
    return manifest


def main(argv: list) -> int:
    revision, tag, src, out = argv[1:5]
    os.makedirs(out, exist_ok=True)
    manifest = assemble(revision, tag, src, out)
    for kind in KINDS:
        n = len(manifest["kinds"][kind]["files"])
        print(f"{kind}: {n} file(s)" if n else f"{kind}: MISSING")
    return 1 if manifest["missing"] else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
