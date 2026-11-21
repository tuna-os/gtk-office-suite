#!/usr/bin/env python3
"""Render every fixture in a manifest with LibreOffice: document -> PDF ->
one PNG per page/slide at 96 DPI (the density our apps draw at 100% zoom).

Output: <out>/<app>/<feature>/lo-<page>.png, and lo-<page>.txt beside it:
the page's text layer, which tells the scorer what LibreOffice printed so it
can set aside what OCR misreads in the reference (compare.reference_words).

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


def csv_filter(src):
    """Calc's CSV import filter for `src` when it is delimited text whatever
    its name says (a GOV.UK statistics download served as .xlsx is a Latin-1
    CSV), else None. Without it LibreOffice's type detection opens such a
    file in Writer, and the Tables reference is a page of monospace text.
    The delimiter and character set are chosen as Tables' reader chooses
    them (tables-core io::load_delimited_into_engine)."""
    with open(src, "rb") as f:
        head = f.read(65536)
    if head[:4] == b"PK\x03\x04" or head[:8] == bytes.fromhex("D0CF11E0A1B11AE1"):
        return None
    if any(b == 0 or (b < 0x20 and b not in b"\t\n\r\x0c") for b in head[:512]):
        return None  # binary: not a table of text
    try:
        text = head.decode("utf-8")
        charset = 76  # UTF-8
    except UnicodeDecodeError as e:
        if e.start >= len(head) - 3 and len(head) == 65536:
            # A multi-byte character cut by the read, not Latin-1.
            text = head[: e.start].decode("utf-8")
            charset = 76
        else:
            text = head.decode("latin-1")
            charset = 12  # ISO-8859-1
    if not text.strip() or text.lstrip().startswith("<"):
        return None  # empty or markup: not a table of text
    if src.lower().endswith(".csv"):
        delim = ","
    else:
        # The first record's delimiters outside quoted fields.
        counts, quoted = {",": 0, "\t": 0, ";": 0}, False
        for ch in text:
            if ch == '"':
                quoted = not quoted
            elif ch in "\r\n" and not quoted:
                break
            elif ch in counts and not quoted:
                counts[ch] += 1
        delim = max(counts, key=lambda d: (counts[d], d == ","))
        if not counts[delim]:
            delim = ","
    return f"Text - txt - csv (StarCalc):{ord(delim)},34,{charset},1"


def to_pdf(src, outdir, profile, app=None):
    # A private profile per run: a shared one serializes conversions and
    # carries state (recent files, first-start dialogs) between runs.
    infilter = csv_filter(src) if app == "tables" else None
    subprocess.run(
        [
            soffice(),
            f"-env:UserInstallation=file://{profile}",
            "--headless",
            "--norestore",
            *([f"--infilter={infilter}"] if infilter else []),
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
    for old in glob.glob(os.path.join(dest_dir, "lo-*.png")) + glob.glob(os.path.join(dest_dir, "lo-*.txt")):
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
        # Best effort: without it the scorer keeps every OCR read.
        subprocess.run(
            ["pdftotext", "-f", str(i), "-l", str(i), "-enc", "UTF-8", pdf, os.path.join(dest_dir, f"lo-{i}.txt")],
            capture_output=True,
        )
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
                pdf = to_pdf(os.path.join(args.fixtures, fx["file"]), tmp, profile, fx["app"])
                n = rasterize(pdf, dest)
                print(f"lo  {fx['app']}/{fx['feature']}: {n} page(s)")
            except Exception as e:  # keep going; the report shows the gap
                failures += 1
                print(f"lo  {fx['app']}/{fx['feature']}: FAILED {e}", file=sys.stderr)
    shutil.rmtree(profile, ignore_errors=True)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
