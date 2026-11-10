#!/usr/bin/env python3
"""Release metadata for the GitHub Release of a Mandelbrot version.

Usage:
  release-notes.py type VERSION
      Print the AppStream release type (`stable`, `development`, ...).
  release-notes.py notes VERSION TAG ARCH=DIGEST [ARCH=DIGEST ...]
      Print the release notes as Markdown.

Both read the matching <release> entry of the AppStream metainfo, which
RELEASING.md requires for every release, so the GitHub Release and the app's
own release notes cannot drift apart. The notes list the OCI images published
for TAG with their immutable digests. A missing entry is an error.
"""

import sys
import textwrap
import xml.etree.ElementTree as ET
from pathlib import Path

METAINFO = (
    Path(__file__).resolve().parent.parent / "data" / "org.tunaos.mandelbrot.metainfo.xml.in.in"
)
IMAGE = "ghcr.io/tuna-os/mandelbrot"
REMOTE = "https://tunaos.org/flatpak/tuna-os.flatpakrepo"

NOTES = textwrap.dedent(
    """\
    {description}

    ## Install

    Mandelbrot is published on the TunaOS Flatpak remote. It needs the GNOME runtime from
    Flathub.

    ```sh
    flatpak remote-add --user --if-not-exists tuna-os {remote}
    flatpak install --user tuna-os org.tunaos.mandelbrot
    ```

    ## OCI images

    This release has no downloadable files. The Flatpak is published as OCI images; each digest
    identifies the exact image built from this tag.

    | Architecture | Tag | Pinned by digest |
    | --- | --- | --- |
    {rows}
    """
)


def find_release(version: str) -> ET.Element:
    releases = ET.parse(METAINFO).getroot().find("releases")
    for release in releases if releases is not None else ():
        if release.get("version") == version:
            return release
    sys.exit(f'error: no <release version="{version}"> entry in {METAINFO.name}')


def text_of(element: ET.Element) -> str:
    return " ".join("".join(element.itertext()).split())


def description_markdown(release: ET.Element) -> str:
    description = release.find("description")
    if description is None:
        return ""
    blocks = []
    for child in description:
        if child.tag == "p":
            blocks.append(text_of(child))
        elif child.tag in ("ul", "ol"):
            blocks.append("\n".join(f"* {text_of(item)}" for item in child if item.tag == "li"))
    return "\n\n".join(blocks)


def main(args: list[str]) -> None:
    if len(args) == 2 and args[0] == "type":
        print(find_release(args[1]).get("type", "stable"))
    elif len(args) >= 4 and args[0] == "notes":
        version, tag, *pairs = args[1:]
        rows = []
        for pair in pairs:
            arch, sep, digest = pair.partition("=")
            if not sep or not digest.startswith("sha256:"):
                sys.exit(f"error: expected ARCH=sha256:..., got {pair!r}")
            rows.append(f"| {arch} | `{IMAGE}:{tag}-{arch}` | `{IMAGE}@{digest}` |")
        description = description_markdown(find_release(version))
        print(NOTES.format(description=description, remote=REMOTE, rows="\n".join(rows)))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
