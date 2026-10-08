#!/usr/bin/env python3
"""Fetch the real-document corpus into a fixtures directory (#1200).

The single-feature fixtures (fixtures.py) find bugs; this corpus shows
whether users would notice. tools/render-lab/real_corpus/manifest.json
lists about 30 published documents per app, each with its source URL,
licence, publisher and sha256. The documents are not committed: the
decks alone are 26 MB of pictures. This downloads each one, refuses a file
whose sha256 has changed (the publisher replaced it, so its verdict would
no longer mean the same thing), and writes the fixtures manifest the rest
of the lab reads. A document that cannot be fetched is left out and
reported; once it has a baseline verdict, compare.py's ratchet reports it
as `missing`, so link rot shows instead of quietly shrinking the corpus.

CI caches the fetched files keyed on the corpus manifest, so a source that
disappears after the first fetch does not take the nightly down with it.

Usage: fetch_real_corpus.py <fixtures-dir> [--app APP] [--cache DIR]
"""

import argparse
import hashlib
import json
import os
import shutil
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
CORPUS = os.path.join(HERE, "real_corpus", "manifest.json")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def fetch(entry, dest, cache):
    """Put the document at `dest`, from the cache or its source; True if it
    is there with the recorded sha256."""
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    cached = os.path.join(cache, entry["sha256"]) if cache else None
    if cached and os.path.exists(cached) and sha256(cached) == entry["sha256"]:
        shutil.copy(cached, dest)
        return True
    req = urllib.request.Request(entry["url"], headers={"User-Agent": "gtk-office-suite render lab"})
    with urllib.request.urlopen(req, timeout=120) as r, open(dest + ".part", "wb") as out:
        shutil.copyfileobj(r, out)
    got = sha256(dest + ".part")
    if got != entry["sha256"]:
        os.remove(dest + ".part")
        print(f"real {entry['app']}/{entry['feature']}: sha256 changed at the source ({got[:12]}…), skipped",
              file=sys.stderr)
        return False
    os.replace(dest + ".part", dest)
    if cached:
        os.makedirs(cache, exist_ok=True)
        shutil.copy(dest, cached)
    return True


def shard(entries, spec):
    """The entries shard "K/N" takes: every Nth from the Kth, in corpus
    order. The ratchet judges only the documents in a run's manifest, so
    N shards together judge the whole corpus, each document once."""
    index, _, count = spec.partition("/")
    index, count = int(index), int(count)
    if not 0 <= index < count:
        raise SystemExit(f"--shard {spec}: want K/N with 0 <= K < N")
    return entries[index::count]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("fixtures")
    ap.add_argument("--app")
    ap.add_argument("--cache", help="directory of fetched documents, named by sha256")
    ap.add_argument("--shard", help="K/N: only every Nth document from the Kth, so N CI jobs share an app's corpus")
    args = ap.parse_args()

    corpus = json.load(open(CORPUS))
    corpus = [e for e in corpus if not args.app or e["app"] == args.app]
    if args.shard:
        corpus = shard(corpus, args.shard)
    manifest, failures = [], 0
    for entry in corpus:
        dest = os.path.join(args.fixtures, entry["file"])
        try:
            ok = fetch(entry, dest, args.cache)
        except Exception as e:  # keep going; the ratchet reports what is missing
            ok = False
            print(f"real {entry['app']}/{entry['feature']}: {e}", file=sys.stderr)
        if not ok:
            failures += 1
            continue
        manifest.append({k: entry[k] for k in ("app", "feature", "file", "expect", "needs")})
    os.makedirs(args.fixtures, exist_ok=True)
    with open(os.path.join(args.fixtures, "manifest.json"), "w") as f:
        json.dump(manifest, f, indent=2)
    print(f"{len(manifest)} real documents -> {args.fixtures} ({failures} not fetched)")


if __name__ == "__main__":
    main()
