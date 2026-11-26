#!/usr/bin/env python3
"""Count real-corpus documents that have a publisher PDF made by Office (#1534).

The render lab scores against LibreOffice, a second implementation of the
Office formats. Where a publisher also posts a PDF exported by Microsoft
Office, that PDF is Office's own render of the document for free. This is
the first step of #1534: for each document in real_corpus/manifest.json,
read its publication page, find the PDF links, download each candidate and
read its Producer. A PDF counts as made by Office when the Producer names
Microsoft Office (Word, Excel, PowerPoint or "Microsoft 365").

It only counts. It does not check that a PDF is the same revision as the
document (page count, text overlap, dates); that is the next step, and the
`candidates` it writes are what that step starts from.

Standard library only. Producer is read from the Info dictionary and the XMP
packet, looking inside Flate-compressed streams too (object streams hold the
Info dictionary in many PDFs). A PDF where neither names a producer is
reported as `unknown`.

Usage: publisher_pdfs.py [--app APP] [--out FILE] [--max-pdf-mb N]
Writes JSON to --out (default: stdout) and a summary table to stderr.
"""

import argparse
import json
import os
import re
import sys
import urllib.parse
import urllib.request
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
CORPUS = os.path.join(HERE, "real_corpus", "manifest.json")
UA = {"User-Agent": "gtk-office-suite render lab"}

OFFICE = re.compile(r"microsoft\W*(?:\W*office|word|excel|power\s*point|365)", re.I)
HREF = re.compile(r"""href\s*=\s*["']([^"']+)["']""", re.I)
PDF_URL = re.compile(r"\.pdf(?:$|[?#])", re.I)
INFO_PRODUCER = re.compile(rb"/Producer\s*(\((?:\\.|[^\\)])*\)|<[0-9A-Fa-f\s]*>)")
INFO_CREATOR = re.compile(rb"/Creator\s*(\((?:\\.|[^\\)])*\)|<[0-9A-Fa-f\s]*>)")
XMP = re.compile(rb"<(pdf:Producer|xmp:CreatorTool)>([^<]*)<")
STREAM = re.compile(rb"stream\r?\n(.*?)endstream", re.S)


def get(url, limit=None, timeout=60):
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=timeout) as r:
        data = r.read(limit + 1) if limit else r.read()
    if limit and len(data) > limit:
        raise ValueError(f"over {limit} bytes")
    return data


def pdf_links(page_url, html):
    """PDF links on a publication page, absolute and de-duplicated."""
    seen = []
    for href in HREF.findall(html):
        url = urllib.parse.urljoin(page_url, href.replace("&amp;", "&"))
        if PDF_URL.search(url) and url not in seen:
            seen.append(url)
    return seen


def _text(raw):
    raw = raw.strip()
    if raw.startswith(b"<"):
        hexed = re.sub(rb"\s", b"", raw[1:-1])
        if len(hexed) % 2:
            hexed += b"0"
        b = bytes.fromhex(hexed.decode())
        return b[2:].decode("utf-16-be", "replace") if b[:2] == b"\xfe\xff" else b.decode("latin-1")
    return re.sub(rb"\\(.)", rb"\1", raw[1:-1]).decode("latin-1")


def producers(pdf):
    """Producer / Creator strings found in the PDF, including compressed streams."""
    chunks = [pdf]
    for m in STREAM.finditer(pdf):
        try:
            chunks.append(zlib.decompress(m.group(1)))
        except zlib.error:
            pass
    found = []
    for c in chunks:
        for rx in (INFO_PRODUCER, INFO_CREATOR):
            found += [_text(m) for m in rx.findall(c)]
        found += [m[1].decode("utf-8", "replace") for m in XMP.findall(c)]
    return list(dict.fromkeys(f for f in found if f.strip()))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--app")
    ap.add_argument("--out")
    ap.add_argument("--max-pdf-mb", type=int, default=40)
    args = ap.parse_args()

    corpus = [e for e in json.load(open(CORPUS)) if not args.app or e["app"] == args.app]
    pages = {}
    results = []
    for e in corpus:
        row = {"app": e["app"], "feature": e["feature"], "page": e["page"], "candidates": [], "office": False}
        try:
            if e["page"] not in pages:
                pages[e["page"]] = get(e["page"]).decode("utf-8", "replace")
            links = pdf_links(e["page"], pages[e["page"]])
        except Exception as ex:
            row["error"] = f"page: {ex}"
            results.append(row)
            continue
        for url in links[:25]:
            cand = {"url": url}
            try:
                found = producers(get(url, args.max_pdf_mb << 20))
                cand["producer"] = found
                cand["office"] = any(OFFICE.search(f) for f in found)
                if not found:
                    cand["note"] = "unknown"
            except Exception as ex:
                cand["error"] = str(ex)
            row["candidates"].append(cand)
            row["office"] = row["office"] or cand.get("office", False)
        results.append(row)

    out = json.dumps(results, indent=1)
    if args.out:
        open(args.out, "w").write(out + "\n")
    else:
        print(out)
    print(f"{'app':8} {'documents':>9} {'with PDF link':>13} {'Office PDF':>10}", file=sys.stderr)
    for app in sorted({r["app"] for r in results}):
        rs = [r for r in results if r["app"] == app]
        print(f"{app:8} {len(rs):9} {sum(bool(r['candidates']) for r in rs):13} "
              f"{sum(r['office'] for r in rs):10}", file=sys.stderr)


if __name__ == "__main__":
    main()
