#!/usr/bin/env python3
"""Refuse an OCI image that is not the Flatpak it is about to be filed as.

    check_oci_ref.py <oci layout dir> <ref prefix>

The ref prefix is `app/<app id>/<arch>/`. The image's config carries the
Flatpak ref in its `org.flatpak.ref` label (flatpak build-bundle --oci
writes it); the label must start with the prefix. ghcr.io/tuna-os/tables
held Decks for x86_64 in v2.1.62 and v2.1.63, because three apps' images
shared one artifact name in the publish run and nothing looked inside them.
"""

import json
import sys
from pathlib import Path


def blob(layout: Path, digest: str) -> dict:
    algorithm, _, hexdigest = digest.partition(":")
    return json.loads((layout / "blobs" / algorithm / hexdigest).read_text())


def flatpak_ref(layout: Path) -> str:
    index = json.loads((layout / "index.json").read_text())
    manifests = index.get("manifests") or []
    if len(manifests) != 1:
        raise ValueError(f"{layout}: expected one manifest in index.json, found {len(manifests)}")
    manifest = blob(layout, manifests[0]["digest"])
    config = blob(layout, manifest["config"]["digest"])
    labels = (config.get("config") or {}).get("Labels") or {}
    ref = labels.get("org.flatpak.ref")
    if not ref:
        raise ValueError(f"{layout}: the image has no org.flatpak.ref label")
    return ref


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    layout, prefix = Path(argv[1]), argv[2]
    try:
        ref = flatpak_ref(layout)
    except (OSError, ValueError, KeyError) as error:
        print(f"::error::{error}", file=sys.stderr)
        return 1
    if not ref.startswith(prefix):
        print(f"::error::{layout} holds {ref}, not {prefix}*", file=sys.stderr)
        return 1
    print(f"{layout}: {ref}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
