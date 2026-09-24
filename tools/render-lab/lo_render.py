#!/usr/bin/env python3
"""Render every fixture in a manifest with LibreOffice: document -> PDF ->
one PNG per page/slide at 96 DPI (the density our apps draw at 100% zoom).

Output: <out>/<app>/<feature>/lo-<page>.png

Usage: lo_render.py <fixtures-dir> <out-dir> [--app APP]
"""

import argparse
import glob
import json
import os
import shutil
import subprocess
import sys
import tempfile

DPI = 96


def soffice():
    for name in ("soffice", "libreoffice"):
        path = shutil.which(name)
        if path:
            return path
    sys.exit("lo_render: LibreOffice (soffice) not found")


def to_pdf(src, outdir, profile):
    # A private profile per run: a shared one serializes conversions and
    # carries state (recent files, first-start dialogs) between runs.
    subprocess.run(
        [
            soffice(),
            f"-env:UserInstallation=file://{profile}",
            "--headless",
            "--norestore",
            "--convert-to",
            "pdf",
            "--outdir",
            outdir,
            src,
        ],
        check=True,
        capture_output=True,
        timeout=180,
    )
    pdf = os.path.join(outdir, os.path.splitext(os.path.basename(src))[0] + ".pdf")
    if not os.path.exists(pdf):
        raise RuntimeError(f"no PDF produced for {src}")
    return pdf


def rasterize(pdf, dest_dir):
    os.makedirs(dest_dir, exist_ok=True)
    for old in glob.glob(os.path.join(dest_dir, "lo-*.png")):
        os.remove(old)
    subprocess.run(
        ["pdftoppm", "-r", str(DPI), "-png", pdf, os.path.join(dest_dir, "lo")],
        check=True,
        capture_output=True,
    )
    # pdftoppm zero-pads by page count (lo-1 / lo-01); normalize to lo-<n>.
    pages = sorted(glob.glob(os.path.join(dest_dir, "lo-*.png")))
    for i, p in enumerate(pages, start=1):
        os.rename(p, os.path.join(dest_dir, f"lo-{i}.tmp"))
    for i in range(1, len(pages) + 1):
        os.rename(os.path.join(dest_dir, f"lo-{i}.tmp"), os.path.join(dest_dir, f"lo-{i}.png"))
    return len(pages)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("fixtures")
    ap.add_argument("out")
    ap.add_argument("--app")
    args = ap.parse_args()

    manifest = json.load(open(os.path.join(args.fixtures, "manifest.json")))
    profile = tempfile.mkdtemp(prefix="render-lab-lo-")
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        for fx in manifest:
            if args.app and fx["app"] != args.app:
                continue
            dest = os.path.join(args.out, fx["app"], fx["feature"])
            try:
                pdf = to_pdf(os.path.join(args.fixtures, fx["file"]), tmp, profile)
                n = rasterize(pdf, dest)
                print(f"lo  {fx['app']}/{fx['feature']}: {n} page(s)")
            except Exception as e:  # keep going; the report shows the gap
                failures += 1
                print(f"lo  {fx['app']}/{fx['feature']}: FAILED {e}", file=sys.stderr)
    shutil.rmtree(profile, ignore_errors=True)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
