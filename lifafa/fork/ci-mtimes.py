#!/usr/bin/env python3
"""Keep the timestamps of unchanged sources across cached builds.

    fork/ci-mtimes.py record TREE RECORD   # on the host, before flatpak-builder
    fork/ci-mtimes.py apply BUILDDIR VENDOR  # in each Rust module, before cargo

fork-release.yml keeps cargo's target directories between runs. cargo decides
that a crate is stale when a source file, or a directory a build script names
in `rerun-if-changed`, is newer than its last build. A fresh checkout stamps
every file with the checkout's time, and flatpak-builder's copy into the build
sandbox gives every directory the copy's time, so without this every crate
with such a build script, and everything depending on it, would be rebuilt on
every run.

`record` runs on the checkout. RECORD (JSON, kept in the same cache as the
target directories) maps each file to its SHA-256 and modification time at the
last build. A file whose content is unchanged gets that time back; a changed
or new file keeps the checkout's, which is later than any build cargo
recorded. Each directory's time is then the latest of the files under it, or
now if a file under it was removed since the last build, and that map is
written to fork/.ci-dir-mtimes.json for `apply`. RECORD is rewritten for the
next run.

`apply` runs inside the sandbox, where the copy kept the files' times: it
gives the tree's directories their times from that map, and every entry of the
vendored crates (VENDOR, whose content is fixed by each crate's version and
checksum) one fixed time. Without the map, as in a local build, it does
nothing.
"""
import hashlib
import json
import os
import sys
import time
from pathlib import Path

# What flatpak-builder does not copy into the build (the `skip` list
# tools/local-manifest.sh writes), and the state directory the record lives in.
SKIP = {
    ".git",
    ".flatpak-builder",
    "dist",
    "target",
    "packaging/out",
    "native/tuta-unofficial-sdk/target",
    "native/tuta-unofficial-sdk/upstream",
}
DIR_MAP = "fork/.ci-dir-mtimes.json"
# Older than any build; a vendored crate's content never changes under the
# same version, so its timestamps carry no information.
VENDOR_MTIME_NS = 315532800 * 10**9  # 1980-01-01


def files(tree):
    for root, dirs, names in os.walk(tree):
        rel_root = os.path.relpath(root, tree)
        dirs[:] = [d for d in dirs if os.path.normpath(os.path.join(rel_root, d)) not in SKIP]
        for name in names:
            rel = os.path.normpath(os.path.join(rel_root, name))
            path = os.path.join(root, name)
            if rel != DIR_MAP and not os.path.islink(path):
                yield rel, path


def digest(path):
    with open(path, "rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def ancestors(rel):
    """The directories containing `rel`, innermost first, ending with "."."""
    while rel != ".":
        rel = os.path.dirname(rel) or "."
        yield rel


def record(tree, record_path):
    record_path = Path(record_path)
    try:
        previous = json.loads(record_path.read_text())
    except FileNotFoundError:
        previous = {}
    current = {}
    dirs = {}
    for rel, path in files(tree):
        sha = digest(path)
        mtime = os.stat(path).st_mtime_ns
        old = previous.get(rel)
        if old is not None and old[0] == sha:
            mtime = old[1]
            os.utime(path, ns=(mtime, mtime))
        current[rel] = [sha, mtime]
        for parent in ancestors(rel):
            dirs[parent] = max(dirs.get(parent, 0), mtime)
    # A removed file leaves nothing newer behind, so every directory that held
    # it and is still there counts as changed now.
    now = time.time_ns()
    removed = set(previous) - set(current)
    for rel in removed:
        for parent in ancestors(rel):
            if parent in dirs:
                dirs[parent] = now
    (Path(tree) / DIR_MAP).write_text(json.dumps(dirs))
    record_path.parent.mkdir(parents=True, exist_ok=True)
    record_path.write_text(json.dumps(current))
    unchanged = sum(1 for rel, (sha, _) in current.items() if previous.get(rel, [None])[0] == sha)
    print(f"ci-mtimes: {unchanged} of {len(current)} files unchanged since the last build, "
          f"{len(removed)} removed")


def apply(builddir, vendor):
    try:
        dirs = json.loads((Path(builddir) / DIR_MAP).read_text())
    except FileNotFoundError:
        return
    for rel, mtime in dirs.items():
        path = os.path.join(builddir, rel)
        if os.path.isdir(path) and not os.path.islink(path):
            os.utime(path, ns=(mtime, mtime))
    if os.path.isdir(vendor):
        for root, subdirs, names in os.walk(vendor, topdown=False):
            for name in names + subdirs:
                os.utime(os.path.join(root, name), ns=(VENDOR_MTIME_NS,) * 2, follow_symlinks=False)
        os.utime(vendor, ns=(VENDOR_MTIME_NS,) * 2)
    print(f"ci-mtimes: restored {len(dirs)} directory times under {builddir}")


if __name__ == "__main__":
    if len(sys.argv) != 4 or sys.argv[1] not in ("record", "apply"):
        sys.exit(__doc__)
    {"record": record, "apply": apply}[sys.argv[1]](*sys.argv[2:])
