#!/usr/bin/env python3
"""Compare our captures against LibreOffice's rendering of the same fixture
and write report.html + scorecard.json.

Layout expected under <out>/<app>/<feature>/:
    lo-<n>.png        LibreOffice reference, page/slide n (lo_render.py)
    A-<n>.png         Tier A: in-app offscreen render
    B-<n>.png         Tier B: Broadway, screenshotted by headless Chromium
    X-<n>.png         Tier X: Xvfb capture
    C-<n>.png         Tier C: VM capture

Metrics per page (see docs/RENDER-PARITY-ROADMAP.md, "Metrics"):
    ink        our non-background pixels / LibreOffice's (0 => not rendered)
    words      fraction of LibreOffice's OCR words found in ours
    scale      Tables only: our grid's size / LibreOffice's for the same cells
    disp_pt    median displacement of matched word centres, in points
    colors     fraction of LibreOffice's salient colours present in ours
    ssim       structural similarity, grayscale, 4x downsampled

Tier agreement (A vs B, not against LibreOffice): mean absolute grey-level
difference between what the app drew for itself (S-<n>.png where the app
writes an on-screen copy, else A-<n>.png) and what reached the browser
(B-<n>.png). Both come from the same app and the same window size, so they
should be near-identical; a disagreement means pixels were lost or moved
between GTK and a real client, which is its own bug class.

Usage:
    compare.py <fixtures-dir> <out-dir> [--baseline scorecard.json]

With --baseline, exits 1 if any fixture's verdict got worse (the ratchet).
"""

import argparse
import glob
import html
import json
import os
import re
import shutil
import subprocess
import sys

import numpy as np
from PIL import Image, ImageOps

TIERS = ("A", "B", "X", "C")
VERDICT_RANK = {"missing": 0, "red": 1, "amber": 2, "green": 3}
PX_TO_PT = 72.0 / 96.0

# Budgets. Deliberately explicit: moving one is a reviewed change.
RED_INK = 0.10  # we drew <10% of LibreOffice's ink: "not rendered"
RED_WORDS = 0.50
GREEN_WORDS = 0.90
GREEN_DISP_PT = 6.0
GREEN_COLORS = 0.90
GREEN_SSIM = 0.75
GREEN_SCALE = 0.10  # Tables: our grid within ±10% of LibreOffice's size
METRICS = ("ink", "words", "disp_pt", "colors", "ssim", "scale")
TIER_AGREE = 3.0  # grey levels; measured 0.0-0.3 when the tiers agree


# ── image helpers ─────────────────────────────────────────────────────────
def load(path, size=None):
    img = Image.open(path).convert("RGB")
    if size and img.size != size:
        img = img.resize(size, Image.LANCZOS)
    return img


def background(arr):
    """Most common colour along a ring just inside the image border: the
    page/slide/grid background, whatever colour it is. Inset by a few px
    because a crop can carry a 1 px frame line on its edge (the Decks
    canvas draws one round the slide), which would otherwise win."""
    i = 4 if min(arr.shape[:2]) > 16 else 0
    border = np.concatenate([arr[i], arr[-1 - i], arr[:, i], arr[:, -1 - i]])
    vals, counts = np.unique(border.reshape(-1, 3), axis=0, return_counts=True)
    return vals[counts.argmax()].astype(int)


def ink_mask(arr):
    bg = background(arr)
    return np.abs(arr.astype(int) - bg).sum(axis=2) > 60


def strip_rules(img):
    """Paint out ruled lines (rows/columns that are mostly ink). Used for
    Tables: LibreOffice prints black gridlines and we draw faint ones, so
    with the rules left in, ink and OCR would measure gridline weight
    instead of cell content (tesseract also reads rules as `|`)."""
    arr = np.asarray(img).copy()
    m = ink_mask(arr)
    bg = background(arr)
    arr[m.mean(axis=1) > 0.5, :] = bg
    arr[:, m.mean(axis=0) > 0.5] = bg
    return Image.fromarray(arr)


def ssim(a, b):
    """Mean SSIM over 8x8 windows (Wang et al. 2004), grayscale, after a 4x
    downsample, so anti-aliasing and hinting noise don't dominate."""

    def prep(img):
        g = img.convert("L")
        g = g.resize((max(8, g.width // 4), max(8, g.height // 4)), Image.BOX)
        return np.asarray(g, dtype=np.float64)

    x, y = prep(a), prep(b)
    k = 8

    def box(m):
        c = np.cumsum(np.cumsum(np.pad(m, ((1, 0), (1, 0))), 0), 1)
        return (c[k:, k:] - c[:-k, k:] - c[k:, :-k] + c[:-k, :-k]) / (k * k)

    mx, my = box(x), box(y)
    vx = box(x * x) - mx * mx
    vy = box(y * y) - my * my
    cxy = box(x * y) - mx * my
    c1, c2 = (0.01 * 255) ** 2, (0.03 * 255) ** 2
    s = ((2 * mx * my + c1) * (2 * cxy + c2)) / ((mx * mx + my * my + c1) * (vx + vy + c2))
    return float(s.mean())


def salient_colors(img, n=8, min_share=0.003):
    """Distinct saturated colours covering a meaningful share of the image:
    fills, coloured text, shapes. Greys are excluded (text/gridlines are
    covered by the word metric)."""
    q = img.quantize(colors=n, method=Image.MEDIANCUT)
    pal = q.getpalette()[: n * 3]
    counts = np.bincount(np.asarray(q).ravel(), minlength=n)
    total = counts.sum()
    out = []
    for i in range(n):
        if counts[i] / total < min_share:
            continue
        rgb = np.array(pal[i * 3 : i * 3 + 3])
        if rgb.max() - rgb.min() < 40:  # grey-ish
            continue
        out.append(rgb)
    return out


def color_presence(ref, ours):
    wanted = salient_colors(ref)
    if not wanted:
        return None
    arr = np.asarray(ours.resize((ours.width // 2, ours.height // 2)), dtype=int).reshape(-1, 3)
    hit = 0
    for rgb in wanted:
        d = np.abs(arr - rgb).sum(axis=1)
        if (d < 60).mean() > 0.0005:
            hit += 1
    return hit / len(wanted)


# ── OCR ───────────────────────────────────────────────────────────────────
def ocr_words(img, sparse=False):
    """OCR word boxes. `sparse` (tesseract --psm 11) suits grids of short
    values, where page-layout analysis (psm 3) drops most of them."""
    if not shutil.which("tesseract"):
        return None
    # Upscale 2x: 96 DPI body text is below tesseract's comfortable size.
    import tempfile

    with tempfile.NamedTemporaryFile(suffix=".png") as tmp:
        g = img.convert("L")
        g.resize((g.width * 2, g.height * 2), Image.LANCZOS).save(tmp.name)
        psm = "11" if sparse else "3"
        r = subprocess.run(["tesseract", tmp.name, "-", "--psm", psm, "tsv"], capture_output=True, text=True)
    words = []
    for line in r.stdout.splitlines()[1:]:
        f = line.split("\t")
        if len(f) < 12 or not f[11].strip():
            continue
        try:
            conf = float(f[10])
        except ValueError:
            continue
        text = re.sub(r"[^0-9a-z]", "", f[11].lower())
        if conf < 30 or not text:
            continue
        left, top, w, h = (int(v) / 2 for v in f[6:10])
        words.append((text, left + w / 2, top + h / 2))
    return words


def match_words(ref, ours):
    if ref is None or ours is None or not ref:
        return None, None
    pool = list(ours)
    found, disp = 0, []
    for text, x, y in ref:
        best, bi = None, None
        for i, (t2, x2, y2) in enumerate(pool):
            if t2 == text:
                d = ((x - x2) ** 2 + (y - y2) ** 2) ** 0.5
                if best is None or d < best:
                    best, bi = d, i
        if bi is not None:
            found += 1
            disp.append(best)
            pool.pop(bi)
    median = float(np.median(disp)) * PX_TO_PT if disp else None
    return found / len(ref), median


# ── scoring ───────────────────────────────────────────────────────────────
def align(app, lo_path, ours_path):
    """Put both renderings in one coordinate space; returns (ref, ours,
    scale) where scale is our size / LibreOffice's for the same content
    (None where the page box fixes it).

    Letters pages and Decks slides share a page box with LibreOffice's PDF,
    so ours is resampled onto the reference. A Tables capture is the header
    band plus the used range (the app crops to it); LibreOffice's printed
    page is cropped to its ink, which is the same headings + used range.
    Ours is resampled onto that, so cells line up and the metrics compare
    layout within the grid. How much bigger or smaller our grid is overall
    is reported separately as `scale` rather than hidden in every metric.
    """
    ref = load(lo_path)
    if app != "tables":
        return ref, load(ours_path, ref.size), None
    ys, xs = np.nonzero(ink_mask(np.asarray(ref)))
    if len(xs):
        ref = ref.crop((xs.min(), ys.min(), xs.max() + 1, ys.max() + 1))
    ours = load(ours_path)
    scale = (ours.width / ref.width + ours.height / ref.height) / 2
    ours = ours.resize(ref.size, Image.LANCZOS)
    # A white margin: cropped to its ink, LibreOffice's border *is* the
    # outer gridline, and background() reads the border.
    # Wider than background()'s 4 px inset, or the ring lands on the rule.
    pad = lambda im: ImageOps.expand(im, border=8, fill=(255, 255, 255))
    return pad(ref), pad(ours), scale


def compare_page(app, ref, ours, ref_words):
    """`ref_words` is ocr_words(content(app, ref)), cached by the caller."""
    rc, oc = content(app, ref), content(app, ours)
    ra, oa = np.asarray(rc), np.asarray(oc)
    ref_ink = ink_mask(ra).sum()
    ink = float(ink_mask(oa).sum() / ref_ink) if ref_ink else None
    words, disp = match_words(ref_words, ocr_words(oc, app == "tables") if ref_words is not None else None)
    return {
        "ink": ink,
        "words": words,
        "disp_pt": disp,
        "colors": color_presence(ref, ours),
        "ssim": ssim(ref, ours),
        "ref_words": len(ref_words) if ref_words is not None else None,
    }


def content(app, img):
    """What ink and OCR look at: the page itself, or for Tables the cell
    contents without the grid rules."""
    return strip_rules(img) if app == "tables" else img


def verdict(m):
    if m is None:
        return "missing"
    if m["ink"] is not None and m["ink"] < RED_INK:
        return "red"
    if m["words"] is not None and (m["ref_words"] or 0) >= 3 and m["words"] < RED_WORDS:
        return "red"
    ok = [m["ssim"] >= GREEN_SSIM]
    if m["words"] is not None:
        ok.append(m["words"] >= GREEN_WORDS)
    if m["disp_pt"] is not None:
        ok.append(m["disp_pt"] <= GREEN_DISP_PT)
    if m["colors"] is not None:
        ok.append(m["colors"] >= GREEN_COLORS)
    if m.get("scale") is not None:
        ok.append(abs(m["scale"] - 1.0) <= GREEN_SCALE)
    if not m.get("page_count_match", True):
        ok.append(False)
    return "green" if all(ok) else "amber"


def mean(vals):
    vals = [v for v in vals if v is not None]
    return float(np.mean(vals)) if vals else None


def score_fixture(app, d, tier, ref_words_cache):
    lo_pages = sorted(glob.glob(os.path.join(d, "lo-[0-9]*.png")), key=page_no)
    ours_pages = sorted(glob.glob(os.path.join(d, f"{tier}-[0-9]*.png")), key=page_no)
    if not lo_pages or not ours_pages:
        return None
    # Tier B can only see the pages on screen (capture.py stops at the
    # viewport), so it is scored on those; Tier A judges the rest.
    partial = tier == "B"
    if partial:
        lo_pages = lo_pages[: len(ours_pages)]
    per_page = []
    for i, lo in enumerate(lo_pages):
        if i >= len(ours_pages):
            per_page.append({"ink": 0.0, "words": 0.0, "disp_pt": None, "colors": 0.0, "ssim": 0.0, "ref_words": None})
            continue
        ref, ours, scale = align(app, lo, ours_pages[i])
        key = (lo, ref.size)
        if key not in ref_words_cache:
            ref_words_cache[key] = ocr_words(content(app, ref), app == "tables")
        per_page.append(dict(compare_page(app, ref, ours, ref_words_cache[key]), scale=scale))
    m = {k: mean([p.get(k) for p in per_page]) for k in METRICS}
    m["ref_words"] = sum(p["ref_words"] or 0 for p in per_page)
    m["pages_lo"], m["pages_ours"] = len(lo_pages), len(ours_pages)
    m["page_count_match"] = partial or len(lo_pages) == len(ours_pages)
    m["verdict"] = verdict(m)
    return m


def tier_agreement(d):
    """Worst per-page A-vs-B difference in grey levels, or None."""
    worst = None
    for b in glob.glob(os.path.join(d, "B-[0-9]*.png")):
        n = page_no(b)
        a = next((p for p in (os.path.join(d, f"S-{n}.png"), os.path.join(d, f"A-{n}.png")) if os.path.exists(p)), None)
        if a is None:
            continue
        bi = Image.open(b).convert("L")
        ai = Image.open(a).convert("L")
        if ai.size != bi.size:
            ai = ai.resize(bi.size, Image.LANCZOS)
        mad = float(np.abs(np.asarray(ai, dtype=float) - np.asarray(bi, dtype=float)).mean())
        worst = mad if worst is None else max(worst, mad)
    return worst


def page_no(path):
    return int(re.search(r"-(\d+)\.png$", path).group(1))


def diff_overlay(app, lo_path, ours_path, dest):
    ref, ours, _ = align(app, lo_path, ours_path)
    a, b = np.asarray(ref, dtype=int), np.asarray(ours, dtype=int)
    gray = np.asarray(ref.convert("L"), dtype=np.uint8)
    out = np.stack([gray // 3 + 170] * 3, axis=2).astype(np.uint8)
    only_ref = ink_mask(a) & ~ink_mask(b)
    only_ours = ink_mask(b) & ~ink_mask(a)
    out[only_ref] = (220, 30, 30)  # LibreOffice drew it, we didn't
    out[only_ours] = (30, 90, 230)  # we drew it, LibreOffice didn't
    Image.fromarray(out).save(dest)


# ── report ────────────────────────────────────────────────────────────────
def fmt(v, spec="{:.2f}"):
    return "–" if v is None else spec.format(v)


def write_report(out, manifest, results, agreement):
    rows = []
    totals = {t: {k: 0 for k in VERDICT_RANK} for t in TIERS}
    for fx in manifest:
        key = f"{fx['app']}/{fx['feature']}"
        d = os.path.join(out, fx["app"], fx["feature"])
        r = results.get(key, {})
        for t in TIERS:
            totals[t][(r.get(t) or {"verdict": "missing"})["verdict"]] += 1
        imgs = []
        lo1 = os.path.join(d, "lo-1.png")
        if os.path.exists(lo1):
            imgs.append(("LibreOffice", os.path.relpath(lo1, out)))
        for t in TIERS:
            p = os.path.join(d, f"{t}-1.png")
            if os.path.exists(p):
                imgs.append((f"Tier {t}", os.path.relpath(p, out)))
                if os.path.exists(lo1):
                    dp = os.path.join(d, f"diff-{t}-1.png")
                    diff_overlay(fx["app"], lo1, p, dp)
                    imgs.append((f"diff {t}", os.path.relpath(dp, out)))
        cells = "".join(
            f'<figure><a href="{html.escape(src)}"><img loading="lazy" src="{html.escape(src)}"></a>'
            f"<figcaption>{html.escape(label)}</figcaption></figure>"
            for label, src in imgs
        )
        tiers = ""
        for t in TIERS:
            m = r.get(t)
            if not m:
                continue
            tiers += (
                f'<tr><td>{t}</td><td><span class="v {m["verdict"]}">{m["verdict"]}</span></td>'
                f'<td>{fmt(m["ink"])}</td><td>{fmt(m["words"], "{:.0%}")}</td>'
                f'<td>{fmt(m["disp_pt"], "{:.1f}pt")}</td><td>{fmt(m["colors"], "{:.0%}")}</td>'
                f'<td>{fmt(m["ssim"])}</td><td>{fmt(m.get("scale"), "{:.2f}×")}</td><td>{m["pages_ours"]}/{m["pages_lo"]}</td></tr>'
            )
        worst = min(
            [(r[t]["verdict"]) for t in TIERS if r.get(t)] or ["missing"],
            key=lambda v: VERDICT_RANK[v],
        )
        agree = agreement.get(key)
        agree_html = (
            ""
            if agree is None
            else f'<p class="expect">Tier A↔B: {agree:.2f} grey levels '
            + ('(agree)' if agree <= TIER_AGREE else '<b class="red">DISAGREE: pixels changed between GTK and the browser</b>')
            + "</p>"
        )
        rows.append(
            f'<section class="fx" data-app="{fx["app"]}" data-verdict="{worst}">'
            f'<h2><span class="v {worst}">{worst}</span> {html.escape(key)}</h2>'
            f'<p class="expect">Expect: {html.escape(fx["expect"])}</p>{agree_html}'
            f'<table><tr><th>tier</th><th>verdict</th><th>ink</th><th>words</th><th>Δpos</th>'
            f"<th>colours</th><th>SSIM</th><th>scale</th><th>pages</th></tr>{tiers or '<tr><td colspan=9>no captures</td></tr>'}</table>"
            f'<div class="imgs">{cells}</div></section>'
        )
    summary = "".join(
        f"<tr><td>Tier {t}</td>" + "".join(f'<td class="{k}">{totals[t][k]}</td>' for k in ("green", "amber", "red", "missing")) + "</tr>"
        for t in TIERS
        if any(totals[t][k] for k in ("green", "amber", "red"))
    )
    page = f"""<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Render Parity Report</title>
<style>
:root{{--bg:#fafafa;--fg:#1d1d1f;--muted:#666;--card:#fff;--line:#ddd;
--green:#1a7f37;--amber:#b35900;--red:#c62828;--missing:#777}}
@media (prefers-color-scheme:dark){{:root{{--bg:#1b1b1d;--fg:#eee;--muted:#aaa;--card:#26262a;--line:#3a3a40;
--green:#4ac26b;--amber:#f0a040;--red:#ff6b6b;--missing:#999}}}}
body{{background:var(--bg);color:var(--fg);font:14px/1.45 system-ui,sans-serif;margin:0;padding:16px;max-width:1400px;margin-inline:auto}}
h1{{font-size:22px}} h2{{font-size:16px;margin:0 0 4px}}
.fx{{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:12px;margin:12px 0}}
.expect{{color:var(--muted);margin:0 0 8px}}
table{{border-collapse:collapse;font-variant-numeric:tabular-nums}} td,th{{padding:2px 10px;text-align:left;border-bottom:1px solid var(--line)}}
.v{{font-weight:600;text-transform:uppercase;font-size:12px}}
.green{{color:var(--green)}}.amber{{color:var(--amber)}}.red{{color:var(--red)}}.missing{{color:var(--missing)}}
.imgs{{display:flex;gap:8px;overflow-x:auto;margin-top:8px}}
figure{{margin:0;flex:0 0 auto}} figure img{{max-height:300px;max-width:420px;border:1px solid var(--line);background:#fff}}
figcaption{{color:var(--muted);font-size:12px}}
.filters button{{margin-right:6px}}
</style></head><body>
<h1>Render parity: our apps vs LibreOffice</h1>
<p>Each fixture isolates one feature. Diff overlay: <b class="red">red</b> = LibreOffice drew it and we didn't,
<b style="color:#1e5ae6">blue</b> = we drew something LibreOffice didn't. Budgets: docs/RENDER-PARITY-ROADMAP.md.</p>
<table><tr><th></th><th class="green">green</th><th class="amber">amber</th><th class="red">red</th><th class="missing">missing</th></tr>{summary}</table>
<p class="filters">Show: <button data-f="all">all</button><button data-f="letters">letters</button><button data-f="tables">tables</button>
<button data-f="decks">decks</button><button data-f="red">red only</button></p>
{''.join(rows)}
<script>
document.querySelectorAll('.filters button').forEach(b=>b.onclick=()=>{{const f=b.dataset.f;
document.querySelectorAll('.fx').forEach(s=>{{s.style.display=(f==='all'||s.dataset.app===f||s.dataset.verdict===f)?'':'none'}})}});
</script></body></html>"""
    with open(os.path.join(out, "report.html"), "w") as f:
        f.write(page)
    return totals


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("fixtures")
    ap.add_argument("out")
    ap.add_argument("--baseline")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--app")
    args = ap.parse_args()

    manifest = json.load(open(os.path.join(args.fixtures, "manifest.json")))
    if args.app:
        manifest = [fx for fx in manifest if fx["app"] == args.app]
    results, cache, agreement = {}, {}, {}
    for fx in manifest:
        key = f"{fx['app']}/{fx['feature']}"
        d = os.path.join(args.out, fx["app"], fx["feature"])
        results[key] = {t: score_fixture(fx["app"], d, t, cache) for t in TIERS}
        agreement[key] = tier_agreement(d)
        line = "  ".join(f"{t}:{(results[key][t] or {'verdict': '-'})['verdict']}" for t in TIERS)
        if agreement[key] is not None:
            line += f"  A~B:{agreement[key]:.2f}"
        print(f"{key:32} {line}")

    totals = write_report(args.out, manifest, results, agreement)
    card = {
        k: {t: {"verdict": m["verdict"], **{x: m.get(x) for x in METRICS}} for t, m in v.items() if m}
        for k, v in results.items()
    }
    with open(os.path.join(args.out, "scorecard.json"), "w") as f:
        json.dump(card, f, indent=2, sort_keys=True)
    for t in TIERS:
        if any(totals[t][k] for k in ("green", "amber", "red")):
            print(f"Tier {t}: " + ", ".join(f"{totals[t][k]} {k}" for k in VERDICT_RANK))

    ratchet(args, manifest, card, agreement)


def ratchet(args, manifest, card, agreement):
    """Compare verdicts with the committed baseline, write the machine-
    readable summary (summary.json, summary.md) that CI, PR comments and
    the hive consume, and exit non-zero on any change the PR didn't lock in.

    The baseline stores verdicts only, not metrics: verdicts are stable
    across runs, raw floats are not. Two ways to fail:
      regressed  a fixture's verdict got worse than the baseline;
      stale      it got better but the baseline wasn't updated, so the
                 gain isn't locked in (re-run with --update-baseline and
                 commit tools/render-lab/baseline.json).
    """
    now = {k: {t: m["verdict"] for t, m in v.items()} for k, v in card.items()}
    base = {}
    if args.baseline and os.path.exists(args.baseline):
        base = json.load(open(args.baseline))
    regressed, improved = [], []
    for fx in manifest:
        key = f"{fx['app']}/{fx['feature']}"
        for t in TIERS:
            b = base.get(key, {}).get(t)
            n = now.get(key, {}).get(t)
            if n is None:
                continue
            if b is None:
                if base:  # a baseline exists but doesn't know this fixture yet
                    improved.append({"fixture": key, "tier": t, "from": "missing", "to": n})
                continue
            if VERDICT_RANK[n] < VERDICT_RANK[b]:
                regressed.append({"fixture": key, "tier": t, "from": b, "to": n})
            elif VERDICT_RANK[n] > VERDICT_RANK[b]:
                improved.append({"fixture": key, "tier": t, "from": b, "to": n})

    counts = {t: {v: 0 for v in VERDICT_RANK} for t in TIERS}
    for tiers in now.values():
        for t, v in tiers.items():
            counts[t][v] += 1
    counts = {t: c for t, c in counts.items() if sum(c.values())}
    fixtures = [
        {
            "fixture": f"{fx['app']}/{fx['feature']}",
            "app": fx["app"],
            "feature": fx["feature"],
            "file": fx["file"],
            "expect": fx["expect"],
            "needs": fx.get("needs"),
            "verdicts": now.get(f"{fx['app']}/{fx['feature']}", {}),
            "metrics": card.get(f"{fx['app']}/{fx['feature']}", {}),
            "tier_agreement": agreement.get(f"{fx['app']}/{fx['feature']}"),
        }
        for fx in manifest
    ]
    disagree = sorted(k for k, v in agreement.items() if v is not None and v > TIER_AGREE)
    summary = {
        "counts": counts,
        "regressed": regressed,
        "improved": improved,
        "tier_disagreements": disagree,
        "fixtures": fixtures,
    }
    with open(os.path.join(args.out, "summary.json"), "w") as f:
        json.dump(summary, f, indent=2, sort_keys=True)

    icon = {"green": "🟢", "amber": "🟠", "red": "🔴", "missing": "⚪"}
    md = ["## Render parity vs LibreOffice", ""]
    md.append("| tier | " + " | ".join(f"{icon[v]} {v}" for v in ("green", "amber", "red", "missing")) + " |")
    md.append("|---|---|---|---|---|")
    for t, c in counts.items():
        md.append(f"| {t} | " + " | ".join(str(c[v]) for v in ("green", "amber", "red", "missing")) + " |")
    for title, rows in (("Regressed", regressed), ("Improved", improved)):
        if rows:
            md += ["", f"**{title}:**", ""]
            md += [f"- `{r['fixture']}` tier {r['tier']}: {icon[r['from']]} {r['from']} → {icon[r['to']]} {r['to']}" for r in rows]
    if not regressed and not improved:
        md += ["", "No verdict changes against the committed baseline."]
    if disagree:
        md += ["", f"**Tier A/B disagree** (> {TIER_AGREE:g} grey levels between the app's own render and the browser):", ""]
        md += [f"- `{k}`: {agreement[k]:.1f}" for k in disagree]
    with open(os.path.join(args.out, "summary.md"), "w") as f:
        f.write("\n".join(md) + "\n")

    if args.update_baseline:
        # Merge, so a single-app run (--app) doesn't drop the other apps.
        with open(args.baseline, "w") as f:
            json.dump({**base, **now}, f, indent=2, sort_keys=True)
            f.write("\n")
        print(f"baseline written: {args.baseline}")
        return
    if not args.baseline:
        return
    if regressed:
        print("RATCHET FAILED: rendering got worse:\n  " + "\n  ".join(f"{r['fixture']} {r['tier']}: {r['from']} -> {r['to']}" for r in regressed), file=sys.stderr)
    if improved:
        print("RATCHET STALE: improved or new fixtures are not in the baseline; lock them in with\n  tools/render-lab/run.sh --update-baseline\nand commit tools/render-lab/baseline.json:\n  " + "\n  ".join(f"{r['fixture']} {r['tier']}: {r['from']} -> {r['to']}" for r in improved), file=sys.stderr)
    if regressed or improved:
        sys.exit(1)

if __name__ == "__main__":
    main()
