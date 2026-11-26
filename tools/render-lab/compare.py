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
    lost_lines LibreOffice's text lines (3+ words) of which half or more
               are missing from ours
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

Print agreement (A vs PDF, Letters): the same measure between the on-screen
pages and the app's own PDF export of them, rasterised at 96 dpi (P-<n>.png,
made by capture.py from print.pdf), both box-averaged 4x so the two
rasterisers' antialiasing doesn't count. Paper and screen come from one layout,
so they should be near-identical; a different page count scores 255.
Reported in summary.json (`print_agreement`, `print_disagreements`); it
does not change verdicts.

Usage:
    compare.py <fixtures-dir> <out-dir> [--baseline scorecard.json]

With --baseline, exits 1 if any fixture's verdict got worse (the ratchet).
"""

import argparse
import functools
import glob
import json
import os
import re
import shutil
import sys
from collections import Counter

import numpy as np
from PIL import Image, ImageOps

# Sibling modules: this file is also loaded by path (tests, export_compare).
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from compare_ocr import (  # noqa: F401  (re-exported: tests and export_compare use compare.<name>)
    LOST_LINE_MIN_WORDS,
    PX_TO_PT,
    blank_rules,
    is_printed,
    join_matches,
    mark_printed,
    match_words,
    norm_word,
    ocr_crop_text,
    ocr_pass,
    ocr_words,
    printed_words,
    rescue_unread,
    union_words,
)
from compare_pixels import (  # noqa: F401
    background,
    bridge,
    color_groups,
    color_presence,
    erase_keys,
    erase_rules,
    ink_mask,
    lo_cell_box,
    load,
    long_lines,
    longest_run,
    our_cell_origin,
    run_lengths,
    salient_colors,
    ssim,
)
from compare_report import TIER_AGREE, TIERS, VERDICT_RANK, fmt, ratchet, write_report  # noqa: F401

# Budgets. Deliberately explicit: moving one is a reviewed change.
RED_INK = 0.10  # we drew <10% of LibreOffice's ink: "not rendered"
RED_WORDS = 0.50
GREEN_WORDS = 0.90
GREEN_DISP_PT = 6.0
GREEN_COLORS = 0.90
GREEN_SSIM = 0.75
GREEN_SCALE = 0.10  # Tables: our grid within ±10% of LibreOffice's size
METRICS = ("ink", "words", "disp_pt", "colors", "ssim", "scale")
# How many of LibreOffice's printed pages a Tables sheet is matched against.
SHEET_CANDIDATES = 10
# The share of a printed page's words our opening sheet must hold for the
# page to be the sheet's own.
SHEET_CONTAINED = 0.5


def reference_words(lo_path, ref, app):
    """OCR reads of the reference page `ref` (rendered from `lo_path`),
    each marked with whether LibreOffice printed it (`mark_printed`)."""
    words = ocr_words(ref, app == "tables")
    return mark_printed(words, printed_words(os.path.splitext(lo_path)[0] + ".txt"))


# ── scoring ───────────────────────────────────────────────────────────────
# How much more of the sheet our capture must hold than LibreOffice's
# printed page shows before the page is cut out of it.
PAGE_WINDOW_OVERFLOW = 1.15
# Words both sides read once each, needed to place the page in ours.
PAGE_WINDOW_MIN_PAIRS = 4
PAGE_ROWS_SKEW = 1.15


def page_in_capture(ref, ours):
    """Where LibreOffice's printed page sits in our capture, from the words
    both read: (left, top, right, bottom, scale) in our pixels, or None.

    Each word read exactly once on each side (three letters or more) pairs
    the two. Their spread gives the scale between the renderings, the
    median offset then places the page's corners."""

    def once(img):
        seen = Counter()
        found = {}
        for text, x, y, *_ in ocr_pass(img.convert("L"), 2, "11"):
            if len(text) >= 3:
                seen[text] += 1
                found[text] = (x, y)
        return {t: xy for t, xy in found.items() if seen[t] == 1}

    if not shutil.which("tesseract"):
        return None
    r, o = once(ref), once(ours)
    pairs = [(r[t], o[t]) for t in r.keys() & o.keys()]
    if len(pairs) < PAGE_WINDOW_MIN_PAIRS:
        return None
    ratios = []
    for i, (ra, oa) in enumerate(pairs):
        for rb, ob in pairs[i + 1 :]:
            dr = np.hypot(rb[0] - ra[0], rb[1] - ra[1])
            if dr >= 20:
                ratios.append(np.hypot(ob[0] - oa[0], ob[1] - oa[1]) / dr)
    if not ratios:
        return None
    k = float(np.median(ratios))
    tx = float(np.median([oa[0] - k * ra[0] for ra, oa in pairs]))
    ty = float(np.median([oa[1] - k * ra[1] for ra, oa in pairs]))
    return tx, ty, tx + k * ref.width, ty + k * ref.height, k


def window_to_page(ref, ours, scale):
    """Cut our capture to the part of the sheet one printed page shows.

    LibreOffice prints a sheet wider or longer than the paper over several
    pages, and the lab scores Tables against one of them (pick_sheet_page).
    Our capture is the grid on screen, so it holds more of the sheet than
    that page: bics-sectoral-consumption-estimates prints column A alone on
    page 1, column B being too wide to share it, while our screen shows
    columns A-F. Resampling the whole grid onto the page compared different
    cells (scale 6.7, no words found) and turned such sheets red in every
    tier. A fixed crop at the page's size does not work: sheets print scaled
    (fit to page, a print scale) and need not start at A1. So the page is
    found in our capture from the words both read (page_in_capture), and
    `scale` becomes the size ratio those words give. Only when our capture
    holds clearly more than that region (PAGE_WINDOW_OVERFLOW); otherwise,
    or when too few words pair, the capture is scored whole, as before."""
    found = page_in_capture(ref, ours)
    if not found:
        return ref, ours, scale
    left, top, right, bottom, k = found
    left, top = max(0, int(round(left))), max(0, int(round(top)))
    right, bottom = min(ours.width, int(round(right))), min(ours.height, int(round(bottom)))
    if right - left < 16 or bottom - top < 16:
        return ref, ours, scale
    if ours.width <= (right - left) * PAGE_WINDOW_OVERFLOW and ours.height <= (bottom - top) * PAGE_WINDOW_OVERFLOW:
        return ref, ours, scale
    return ref, ours.crop((left, top, right, bottom)), k


def align(app, lo_path, ours_path, page_window=False):
    """Put both renderings in one coordinate space; returns (ref, ours,
    scale) where scale is our size / LibreOffice's for the same content
    (None where the page box fixes it).

    Letters pages and Decks slides share a page box with LibreOffice's PDF,
    so ours is resampled onto the reference. A Tables capture is the header
    band plus the used range (the app crops to it); LibreOffice's printed
    page is cropped to its ink, which is the same headings + used range.
    How much bigger or smaller our grid is overall is reported as `scale`.
    Then both are cut to their cells (headings are drawn differently on
    paper and on screen, see below) and ours is resampled onto
    LibreOffice's, so cells line up and every metric compares layout within
    the grid. For an unruled reference and an unclipped capture, compare
    ink bounding boxes after removing the screen's header bands; neither
    side then has a frame defining the used range's whitespace.
    """
    ref_raw = load(lo_path)
    if app != "tables":
        return ref_raw, load(ours_path, ref_raw.size), None
    ys_r, xs_r = np.nonzero(ink_mask(np.asarray(ref_raw)))
    ref = ref_raw.crop((xs_r.min(), ys_r.min(), xs_r.max() + 1, ys_r.max() + 1)) if len(xs_r) else ref_raw
    ours = load(ours_path)
    scale = (ours.width / ref.width + ours.height / ref.height) / 2
    # Headings are furniture and each side draws its own: Calc prints them
    # boxed in black, we shade them as a screen does. So only the cells are
    # compared: LibreOffice's inside its frame and heading boxes, ours from
    # the end of our header bands, resampled onto LibreOffice's.
    box = lo_cell_box(ref)
    ox, oy = our_cell_origin(ours)
    if box and 0 < ox < ours.width and 0 < oy < ours.height:
        ref = ref.crop(box)
        ours = ours.crop((ox, oy, ours.width, ours.height))
    elif box is None and 0 < ox < ours.width and 0 < oy < ours.height:
        # Published sheets need not print headings. The reference above is
        # then an ink bounding box, not a framed used range. Remove our
        # screen headings and crop to ink too, or their labels and the
        # unused cell padding get stretched into the document (#1348).
        cells = ours.crop((ox, oy, ours.width, ours.height))
        ys, xs = np.nonzero(ink_mask(np.asarray(cells)))
        dark = np.asarray(ref.convert("L")) < 128
        ruled = long_lines(dark, axis=1) or long_lines(dark, axis=0)
        # Partial borders are not a heading frame, and a viewport cut
        # through text need not contain the printed page's whole range.
        # Neither gives us corresponding ink boxes to align.
        complete = len(xs) and xs.max() < cells.width - 1 and ys.max() < cells.height - 1
        # When an unruled, unheaded sheet's column is wider than both the
        # printed page and the screen viewport, each render clips the column
        # at its own right edge (#1339). Strip our screen headings, compare
        # the shared width from the cell origin, and measure scale along the
        # unclipped vertical axis.
        ys_c, xs_c = np.nonzero(np.asarray(cells.convert("L")) < 200)
        v_complete = len(ys_c) and ys_c.max() < cells.height - 1
        ref_h_clipped = len(xs_r) and xs_r.max() >= ref_raw.width - 4
        ours_h_clipped = len(xs_c) and xs_c.max() >= cells.width - 4 and ours.width >= 960
        if complete and not ruled:
            ours = cells.crop((xs.min(), ys.min(), xs.max() + 1, ys.max() + 1))
            scale = (ours.width / ref.width + ours.height / ref.height) / 2
        elif v_complete and not ruled and ref_h_clipped and ours_h_clipped:
            ours_ink = cells.crop((xs_c.min(), ys_c.min(), xs_c.max() + 1, ys_c.max() + 1))
            shared_w = min(ref.width, ours_ink.width)
            ref = ref.crop((0, 0, shared_w, ref.height))
            ours = ours_ink.crop((0, 0, shared_w, ours_ink.height))
            scale = ours.height / ref.height
    if page_window:
        before = ours.size
        ref, ours, scale = window_to_page(ref, ours, scale)
        if ours.size == before:
            ours, scale = trim_to_page_rows(ref, ours, scale)
    ours = ours.resize(ref.size, Image.LANCZOS)
    # A white margin, wider than background()'s 4 px inset, so the
    # background is read from the page and not from a cell's fill or
    # border at the crop's edge.
    pad = lambda im: ImageOps.expand(im, border=8, fill=(255, 255, 255))
    return pad(ref), pad(ours), scale


def trim_to_page_rows(ref, ours, scale):
    """Fallback for window_to_page when too few words pair (#1347).

    A long sheet prints over several pages; the page scored is the first,
    which starts at the sheet's top, while our capture is continuous and
    holds more rows. Grids scale uniformly, so the rows the page shows are
    the first ref.height * (ours.width / ref.width) pixels of ours. Applied
    only when the vertical ratio exceeds the horizontal by PAGE_ROWS_SKEW,
    so a capture that already matches is left whole."""
    if not ref.width or not ref.height:
        return ours, scale
    k = ours.width / ref.width
    rows = int(round(ref.height * k))
    if rows < 16 or ours.height <= rows * PAGE_ROWS_SKEW:
        return ours, scale
    return ours.crop((0, 0, ours.width, rows)), k


def compare_page(app, ref, ours, ref_words):
    """`ref_words` is reference_words(...) of `ref`, cached by the caller."""
    ref_ink = ink_mask(np.asarray(ref)).sum()
    ink = float(ink_mask(np.asarray(ours)).sum() / ref_ink) if ref_ink else None
    ours_words = ocr_words(ours, app == "tables") if ref_words is not None else None
    reread = None
    if ref_words is not None and ours_words is not None:
        reread = functools.partial(rescue_unread, ref.convert("L"), ours.convert("L"))
    words, disp, lost = match_words(ref_words, ours_words, reread=reread)
    return {
        "ink": ink,
        "words": words,
        "lost_lines": lost,
        "disp_pt": disp,
        "colors": color_presence(ref, ours),
        "ssim": ssim(ref, ours),
        # What LibreOffice printed: misreads of its page don't make it a
        # page with text to recall.
        "ref_words": sum(1 for w in ref_words if len(w) < 6 or w[5]) if ref_words is not None else None,
    }


def wrap_structure(lo_path, ours_path):
    """Measure the single-cell wrap fixture before alignment resizes rows.

    Count separated bands of text pixels, independent of OCR and of where
    Excel/Calc break words. The column heading provides a single-line height
    control: row 1 must grow to at least 2.5 heading heights for three lines.
    Unknown framing or clipped text fails closed.
    """
    measurements = {}
    for name, path in (("reference", lo_path), ("ours", ours_path)):
        img = load(path)
        if name == "reference":
            ys, xs = np.nonzero(ink_mask(np.asarray(img)))
            if not len(xs):
                return {"passed": False, **measurements}
            img = img.crop((xs.min(), ys.min(), xs.max() + 1, ys.max() + 1))
            box = lo_cell_box(img)
        else:
            ox, oy = our_cell_origin(img)
            box = (ox, oy, img.width, img.height) if ox and oy else None
        if box is None:
            return {"passed": False, **measurements}
        cell = img.crop(box)
        ink = erase_rules(np.asarray(cell.convert("L"))) < 128
        rows = np.flatnonzero(ink.sum(axis=1) >= 2)
        # Tiny internal gaps in glyphs (e.g. dots) are part of one line.
        bands = []
        for y in rows:
            if bands and y - bands[-1][1] <= 3:
                bands[-1][1] = int(y)
            else:
                bands.append([int(y), int(y)])
        measurements[name] = {
            "lines": len(bands),
            "row_height": cell.height,
            "heading_height": int(box[1]),
            "unclipped": bool(len(rows) and rows[0] > 0 and rows[-1] < cell.height - 1),
        }
    passed = all(
        m["lines"] == 3 and m["row_height"] >= 2.5 * m["heading_height"] and m["unclipped"]
        for m in measurements.values()
    )
    return {"passed": passed, **measurements}


def verdict(m):
    if m is None:
        return "missing"
    if m["ink"] is not None and m["ink"] < RED_INK:
        return "red"
    if m["words"] is not None and (m["ref_words"] or 0) >= 3 and m["words"] < RED_WORDS:
        return "red"
    # Only the isolated wrap fixture supplies this evidence. Its accepted
    # Excel/Calc word-break difference makes SSIM unsuitable; keep SSIM in
    # the report and require the independent check even if SSIM is high.
    structure = m.get("wrap_structure")
    ok = [structure["passed"] if structure is not None else m["ssim"] >= GREEN_SSIM]
    # As for red: a reference with under three words has no text to recall,
    # only what OCR makes of shapes (a star read as "006"), so it does not
    # decide green either.
    if m["words"] is not None and (m.get("ref_words") or 0) >= 3:
        ok.append(m["words"] >= GREEN_WORDS)
    if m.get("lost_lines") is not None:
        ok.append(m["lost_lines"] == 0)
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


def page_bag(path):
    """A page's words as a multiset, from one quick OCR pass: enough to tell
    which page shows a sheet, not to score it (None without tesseract)."""
    if not shutil.which("tesseract"):
        return None
    return Counter(text for text, *_ in ocr_pass(load(path).convert("L"), 2, "6"))


def pick_sheet_page(lo_pages, ours_path):
    """The printed page, among `lo_pages`, that starts our opening sheet:
    the first whose words are mostly (SHEET_CONTAINED) ours. A sheet too
    long for one page prints over several, each held by our capture, and
    the capture starts at the sheet's top. Failing that, the page with the
    best overlap for both sizes (2 * shared / (ours + page)): a raw count
    of shared words picks a page of guidance prose, which shares common
    words with any sheet. The first page without OCR. Picking costs one
    quick read per page: scoring every candidate in full (two OCR scales and
    a closer look at each unmatched word) ran the Tables job past its
    timeout."""
    ours = page_bag(ours_path)
    if not ours:
        return lo_pages[0]
    n_ours = sum(ours.values())
    best, best_f1 = lo_pages[0], -1.0
    for lo in lo_pages:
        bag = page_bag(lo) or Counter()
        n_lo = sum(bag.values())
        shared = sum((ours & bag).values())
        if n_lo and shared / n_lo >= SHEET_CONTAINED:
            return lo
        f1 = 2 * shared / (n_ours + n_lo)
        if f1 > best_f1:
            best, best_f1 = lo, f1
    return best


def score_fixture(app, d, tier, ref_words_cache, feature=None):
    lo_pages = sorted(glob.glob(os.path.join(d, "lo-[0-9]*.png")), key=page_no)
    ours_pages = sorted(glob.glob(os.path.join(d, f"{tier}-[0-9]*.png")), key=page_no)
    if not lo_pages or not ours_pages:
        return None
    # Tier B can only see the pages on screen (capture.py stops at the
    # viewport), so it is scored on those; Tier A judges the rest. Tables
    # has no pages on screen: its Tier A dump is the grid's viewport too
    # (tables/src/window.rs writes A-1 only), so a sheet LibreOffice prints
    # on nine pages scored eight of them as missing. The real corpus showed
    # it (#1200): every Tables document red in A, amber in B, from the same
    # picture.
    partial = tier == "B" or app == "tables"
    lo_page = None
    page_window = False
    if app == "tables" and len(ours_pages) == 1 and len(lo_pages) > 1:
        # Tables opens the workbook's active sheet (as Excel and Calc do),
        # but LibreOffice prints every sheet from the first: a workbook
        # that opens on its third sheet was scored against its cover page
        # (#1512). Score the opening sheet against the printed page that
        # shows it: the one, among the first pages, that shares the most
        # words with it (then the most structure).
        page_window = True
        best = pick_sheet_page(lo_pages[:SHEET_CANDIDATES], ours_pages[0])
        lo_page = page_no(best)
        lo_pages = [best]
    if partial:
        lo_pages = lo_pages[: len(ours_pages)]
    per_page = []
    for i, lo in enumerate(lo_pages):
        if i >= len(ours_pages):
            per_page.append(
                {
                    "ink": 0.0,
                    "words": 0.0,
                    "lost_lines": None,
                    "disp_pt": None,
                    "colors": 0.0,
                    "ssim": 0.0,
                    "ref_words": None,
                }
            )
            continue
        ref, ours, scale = align(app, lo, ours_pages[i], page_window)
        key = (lo, ref.size)
        if key not in ref_words_cache:
            ref_words_cache[key] = reference_words(lo, ref, app)
        per_page.append(dict(compare_page(app, ref, ours, ref_words_cache[key]), scale=scale))
    m = {k: mean([p.get(k) for p in per_page]) for k in METRICS}
    m["ref_words"] = sum(p["ref_words"] or 0 for p in per_page)
    lost = [p["lost_lines"] for p in per_page if p.get("lost_lines") is not None]
    m["lost_lines"] = sum(lost) if lost else None
    m["pages_lo"], m["pages_ours"] = len(lo_pages), len(ours_pages)
    m["page_count_match"] = partial or len(lo_pages) == len(ours_pages)
    if lo_page is not None:
        m["lo_page"] = lo_page
    if app == "tables" and feature == "wrap-text":
        m["wrap_structure"] = (
            wrap_structure(lo_pages[0], ours_pages[0]) if len(lo_pages) == len(ours_pages) == 1 else {"passed": False}
        )
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


def print_agreement(d):
    """Worst per-page difference in grey levels between the app's on-screen
    pages (A-<n>) and its own PDF export of them rasterised at the same
    96 dpi (P-<n>), or None when the app wrote no PDF. A page-count mismatch
    counts as total disagreement (255): paper and screen must be one layout.
    Informational; it does not change any verdict."""
    prints = glob.glob(os.path.join(d, "P-[0-9]*.png"))
    if not prints:
        return None
    screens = glob.glob(os.path.join(d, "A-[0-9]*.png"))
    if len(prints) != len(screens):
        return 255.0
    worst = 0.0
    for p in prints:
        a = os.path.join(d, f"A-{page_no(p)}.png")
        pi, ai = Image.open(p).convert("L"), Image.open(a).convert("L")
        # Two rasterisers (GTK's cairo, poppler) antialias glyph edges
        # differently; a moved line or word survives a 4x box average, edge
        # shading does not (the same 4x the SSIM metric uses).
        size = (pi.size[0] // 4, pi.size[1] // 4)
        pi, ai = pi.resize(size, Image.BOX), ai.resize(size, Image.BOX)
        worst = max(worst, float(np.abs(np.asarray(ai, dtype=float) - np.asarray(pi, dtype=float)).mean()))
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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("fixtures")
    ap.add_argument("out")
    ap.add_argument("--baseline")
    ap.add_argument("--update-baseline", action="store_true")
    # Repeatable: the combine job in render-parity.yml compares only the apps
    # a pull request reaches (`--app letters --app decks`).
    ap.add_argument("--app", action="append")
    args = ap.parse_args()

    manifest = json.load(open(os.path.join(args.fixtures, "manifest.json")))
    if args.app:
        manifest = [fx for fx in manifest if fx["app"] in args.app]
    results, cache, agreement, printed = {}, {}, {}, {}
    for fx in manifest:
        key = f"{fx['app']}/{fx['feature']}"
        d = os.path.join(args.out, fx["app"], fx["feature"])
        results[key] = {t: score_fixture(fx["app"], d, t, cache, fx["feature"]) for t in TIERS}
        agreement[key] = tier_agreement(d)
        printed[key] = print_agreement(d)
        line = "  ".join(f"{t}:{(results[key][t] or {'verdict': '-'})['verdict']}" for t in TIERS)
        if agreement[key] is not None:
            line += f"  A~B:{agreement[key]:.2f}"
        if printed[key] is not None:
            line += f"  A~PDF:{printed[key]:.2f}"
        print(f"{key:32} {line}")

    totals = write_report(args.out, manifest, results, agreement, diff_overlay)
    # Nothing captured at all is a broken pipeline, not a clean result: the
    # ratchet skips fixtures with no verdict, so an empty run would pass.
    if manifest and not any(results[k].get(t) for k in results for t in TIERS):
        print("compare: no captures found for any fixture; the lab produced nothing to judge", file=sys.stderr)
        sys.exit(2)
    card = {
        k: {
            t: {
                "verdict": m["verdict"],
                "lost_lines": m.get("lost_lines"),
                **{x: m.get(x) for x in METRICS},
                **({"wrap_structure": m["wrap_structure"]} if "wrap_structure" in m else {}),
            }
            for t, m in v.items()
            if m
        }
        for k, v in results.items()
    }
    with open(os.path.join(args.out, "scorecard.json"), "w") as f:
        json.dump(card, f, indent=2, sort_keys=True)
    for t in TIERS:
        if any(totals[t][k] for k in ("green", "amber", "red")):
            print(f"Tier {t}: " + ", ".join(f"{totals[t][k]} {k}" for k in VERDICT_RANK))

    ratchet(args, manifest, card, agreement, printed)


if __name__ == "__main__":
    main()
