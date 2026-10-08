#!/usr/bin/env python3
"""Score our renders against Microsoft Office, beside BetterOffice and
LibreOffice.

BetterOffice (Apache-2.0, https://github.com/xhayankhan/betteroffice)
publishes a corpus of DOCX/PPTX/XLSX documents with Office's own pages
(Word and PowerPoint 16.112 PDF exports at 150 DPI), and with each release
its renders' scores against them: its own, and LibreOffice 26.2's. This
renders the same documents in our apps (Tier A, as capture.py does) and
scores them the way that report does, so the three engines can be put side
by side document by document:

    SSIM   mean over pages of skimage's structural_similarity on Pillow
           grayscale (win 7, data range 255); a missing or extra page counts
           as 0 (sum over common pages / max(page counts)).

One difference: Office's pages are 150 DPI and ours are at the app's own
scale, so our page is resized (Lanczos) to Office's before scoring. Their
LibreOffice and BetterOffice pages are 150 DPI and never resized. What the
resize alone costs is recorded per document (`resize_ceiling`: Office's
page taken to our size and back); on the pilot it was 0.984-0.996.

The corpus documents carry no licence of their own, so they are fetched at
run time (sha256-verified, optionally cached), never committed.

Usage: office_bench.py <out-dir> [--format docx|pptx] [--samples ID ...]
                       [--limit N] [--cache DIR] [--summarise]
Writes <out-dir>/bench.json and <out-dir>/bench.md.
"""

import argparse
import glob
import hashlib
import json
import os
import sys
import tempfile
import time
import urllib.request

import numpy as np
from PIL import Image
from skimage.metrics import structural_similarity

sys.path.insert(0, os.path.dirname(__file__))
import capture  # noqa: E402

CORPUS = "https://corpus.betteroffice.dev"
BENCH = "https://benchmarks.betteroffice.dev"
APPS = {"docx": "letters", "pptx": "decks", "xlsx": "tables"}
# XLSX references are print ranges at the workbook's print scale; Tables
# shows the grid on screen, so a page-for-page score would measure the
# capture, not the rendering. Left out until Tables can print a range.
FORMATS = ("docx", "pptx")
# Two engines within this much SSIM of each other on a document are a tie.
TIE = 0.005


def get(url, limit=256 * 1024 * 1024):
    for attempt in range(4):
        try:
            # The corpus host refuses Python's default User-Agent.
            req = urllib.request.Request(url, headers={"User-Agent": "gtk-office-suite-render-lab/1"})
            with urllib.request.urlopen(req, timeout=120) as r:
                data = r.read(limit + 1)
            if len(data) > limit:
                raise ValueError(f"{url}: larger than {limit} bytes")
            return data
        except (OSError, ValueError):
            if attempt == 3:
                raise
            time.sleep(2 ** (attempt + 1))


def asset(entry, dest, cache):
    """`entry` ({url, sha256}) at `dest`, from the cache or fetched and
    checked against its hash."""
    sha = entry["sha256"]
    cached = os.path.join(cache, sha) if cache else None
    if cached and os.path.exists(cached):
        data = open(cached, "rb").read()
    else:
        data = get(entry["url"])
    if hashlib.sha256(data).hexdigest() != sha:
        raise ValueError(f"{entry['url']}: sha256 mismatch")
    if cached and not os.path.exists(cached):
        os.makedirs(cache, exist_ok=True)
        open(cached, "wb").write(data)
    open(dest, "wb").write(data)


def gray(path, size=None):
    with Image.open(path) as image:
        rgba = image.convert("RGBA")
    page = Image.alpha_composite(Image.new("RGBA", rgba.size, "white"), rgba).convert("RGB")
    if size and page.size != tuple(size):
        page = page.resize(tuple(size), Image.Resampling.LANCZOS)
    return np.asarray(page.convert("L"))


def score(reference, ours):
    """The report's scoring: per-page SSIM, penalised for missing or extra
    pages. `ours` is resized to each reference page."""
    pages, ceiling = [], []
    for ref, mine in zip(reference, ours):
        expected = gray(ref)
        size = (expected.shape[1], expected.shape[0])
        rendered = gray(mine, size=size)
        window = min(7, min(expected.shape) if min(expected.shape) % 2 else min(expected.shape) - 1)
        pages.append(float(structural_similarity(expected, rendered, data_range=255, win_size=window)))
        # What the resize alone costs: Office's own page taken down to our
        # size and back. A perfect render of ours scores at most this.
        with Image.open(mine) as m:
            small = m.size
        back = np.asarray(Image.fromarray(expected).resize(small, Image.Resampling.LANCZOS).resize(size, Image.Resampling.LANCZOS))
        ceiling.append(float(structural_similarity(expected, back, data_range=255, win_size=window)))
    count = max(len(reference), len(ours))
    return {
        "reference_pages": len(reference),
        "actual_pages": len(ours),
        "common_page_ssim": sum(pages) / len(pages) if pages else 0.0,
        "penalized_ssim": sum(pages) / count if count else 0.0,
        "resize_ceiling": sum(ceiling) / len(ceiling) if ceiling else None,
        "pages": pages,
    }


def theirs(sample):
    """BetterOffice's (the published commit's) and LibreOffice's scores
    from the report, by channel."""
    out = {}
    for c in sample.get("comparisons", []):
        name = {"commit": "betteroffice", "libreoffice": "libreoffice"}.get(c.get("channel"))
        if name and c.get("status") == "ok":
            out[name] = {k: c.get(k) for k in ("reference_pages", "actual_pages", "penalized_ssim")}
    return out


def winner(row):
    ours = row.get("ours", {}).get("penalized_ssim")
    bo = row.get("betteroffice", {}).get("penalized_ssim")
    if ours is None or bo is None:
        return None
    if abs(ours - bo) <= TIE:
        return "tie"
    return "ours" if ours > bo else "betteroffice"


def verdict(closer):
    """Whether BetterOffice is worth reading for this format: it is where
    it comes closer to Office than we do on more documents than we beat
    it on; otherwise we are already as close, and it can be dropped."""
    if closer["betteroffice"] > closer["ours"]:
        return "reference"
    return "drop"


def summarise(rows):
    out = {}
    for fmt in FORMATS:
        sub = [r for r in rows if r["format"] == fmt]
        if not sub:
            continue
        entry = {"documents": len(sub)}
        for engine in ("ours", "betteroffice", "libreoffice"):
            scored = [r[engine] for r in sub if engine in r]
            entry[engine] = {
                "scored": len(scored),
                "mean_ssim": round(sum(s["penalized_ssim"] for s in scored) / len(scored), 4) if scored else None,
                "exact_pages": sum(1 for s in scored if s["actual_pages"] == s["reference_pages"]),
            }
        wins = [winner(r) for r in sub]
        entry["closer_to_office"] = c = {k: wins.count(k) for k in ("ours", "betteroffice", "tie")}
        entry["verdict"] = verdict(c)
        out[fmt] = entry
    return out


def markdown(summary, rows, commit):
    lines = [
        "# Against Microsoft Office: ours, BetterOffice, LibreOffice",
        "",
        f"BetterOffice and LibreOffice scores from BetterOffice's report for `{commit[:12]}`; "
        "ours rendered here (Tier A, resized to Office's page). Mean penalised SSIM, higher is closer.",
        "",
        "| | ours | BetterOffice | LibreOffice | closer to Office: ours / BetterOffice / tie | BetterOffice as reference |",
        "|---|---|---|---|---|---|",
    ]
    def cell(e, k):
        return f"{e[k]['mean_ssim']} ({e[k]['exact_pages']}/{e[k]['scored']} exact pages)" if e[k]["scored"] else "—"

    def val(r, k):
        if k in r:
            return f"{r[k]['penalized_ssim']:.3f}"
        return r.get("error", "—")[:40] if k == "ours" else "—"

    for fmt, e in summary.items():
        c = e["closer_to_office"]
        lines.append(f"| {fmt} | {cell(e, 'ours')} | {cell(e, 'betteroffice')} | {cell(e, 'libreoffice')} | {c['ours']} / {c['betteroffice']} / {c['tie']} | {e['verdict']} |")
    lines += ["", "| document | format | ours | BetterOffice | LibreOffice | closer |", "|---|---|---|---|---|---|"]
    for r in sorted(rows, key=lambda r: (r["format"], r["id"])):
        lines.append(f"| {r['id']} | {r['format']} | {val(r, 'ours')} | {val(r, 'betteroffice')} | {val(r, 'libreoffice')} | {winner(r) or '—'} |")
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--format", choices=FORMATS, action="append")
    ap.add_argument("--samples", nargs="*")
    ap.add_argument("--limit", type=int)
    ap.add_argument("--cache")
    ap.add_argument("--summarise", action="store_true", help="only re-summarise <out-dir>/bench.json")
    args = ap.parse_args()
    formats = args.format or list(FORMATS)
    os.makedirs(args.out, exist_ok=True)
    if args.summarise:
        done = json.load(open(os.path.join(args.out, "bench.json")))
        write(args.out, done["commit"], done["rows"])
        return

    latest = json.loads(get(f"{BENCH}/renders/latest.json"))
    report = json.loads(get(f"{BENCH}/{latest['report']}"))
    samples = [s for s in report["samples"] if s["format"] in formats]
    if args.samples:
        samples = [s for s in samples if s["id"] in args.samples]
    if args.limit:
        samples = samples[: args.limit]

    home = tempfile.mkdtemp(prefix="office-bench-home-")
    env = capture.base_env(home)
    rows = []
    for n, sample in enumerate(samples, 1):
        sid, fmt = sample["id"], sample["format"]
        row = {"id": sid, "format": fmt, **theirs(sample)}
        work = os.path.join(args.out, fmt, sid)
        try:
            meta = json.loads(get(sample["metadata_url"]))
            os.makedirs(os.path.join(work, "reference"), exist_ok=True)
            doc = os.path.join(work, f"source.{fmt}")
            asset(meta["source"], doc, args.cache)
            reference = []
            for i, page in enumerate(meta["reference_pages"], 1):
                path = os.path.join(work, "reference", f"page_{i:04d}.png")
                asset(page, path, args.cache)
                reference.append(path)
            ours_dir = os.path.join(work, "ours")
            os.makedirs(ours_dir, exist_ok=True)
            capture.tier_a(APPS[fmt], os.path.abspath(doc), os.path.abspath(ours_dir), env)
            ours = sorted(glob.glob(os.path.join(ours_dir, "A-*.png")), key=lambda p: int(p.rsplit("-", 1)[1][:-4]))
            if not ours:
                raise RuntimeError("no pages rendered")
            row["ours"] = score(reference, ours)
        except Exception as e:  # one document's failure is recorded, not fatal
            row["error"] = f"{type(e).__name__}: {e}"
        rows.append(row)
        o = row.get("ours", {}).get("penalized_ssim")
        print(f"[{n}/{len(samples)}] {fmt} {sid}: ours={o if o is None else round(o, 3)} "
              f"bo={row.get('betteroffice', {}).get('penalized_ssim')} lo={row.get('libreoffice', {}).get('penalized_ssim')}"
              + (f" ({row['error']})" if "error" in row else ""), flush=True)
        json.dump({"commit": report["commit"], "rows": rows}, open(os.path.join(args.out, "bench.json"), "w"), indent=1)

    write(args.out, report["commit"], rows)


def write(out, commit, rows):
    summary = summarise(rows)
    json.dump({"commit": commit, "summary": summary, "rows": rows}, open(os.path.join(out, "bench.json"), "w"), indent=1)
    open(os.path.join(out, "bench.md"), "w").write(markdown(summary, rows, commit))
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
