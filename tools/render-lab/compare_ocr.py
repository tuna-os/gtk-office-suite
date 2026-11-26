"""OCR passes and word matching for compare.py."""

import os
import re
import shutil
import subprocess

import numpy as np
from compare_pixels import erase_rules
from PIL import Image

PX_TO_PT = 72.0 / 96.0
# A line of LibreOffice's text is lost when at least this many words long
# and half or more of them are missing from ours. `words` is a page-wide
# fraction, so a header reading "Page  of" instead of "Page 2 of 5" (two
# words out of ~500 on the page) scored 99% and was green; per line it is
# half of its words gone. Three words: a two-word line losing one word is
# too often an OCR split ("8pt text" read as one token) to judge.
LOST_LINE_MIN_WORDS = 3


OCR_TIMEOUT_S = float(os.environ.get("RENDER_LAB_OCR_TIMEOUT", "60"))


def ocr_pass(g, k, psm):
    """One tesseract TSV run over greyscale `g` upscaled by `k`. Coordinates
    come back in 1x pixels so passes at several scales union directly."""
    import tempfile

    import observation

    words = []
    with tempfile.NamedTemporaryFile(suffix=".png") as tmp:
        g.resize((g.width * k, g.height * k), Image.LANCZOS).save(tmp.name)
        # Bounded and checked: a hang, nonzero exit or malformed TSV raises
        # observation.ObservationError instead of reading as "no words".
        tsv, _attempt = observation.run_tesseract(tmp.name, psm=psm, timeout=OCR_TIMEOUT_S)
    for line in tsv.splitlines()[1:]:
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
        left, top, w, h = (int(v) / k for v in f[6:10])
        # Tesseract's own line: (block, paragraph, line). The box (in the
        # input image's pixels) locates the word for the magnifier fallback
        # in match_words.
        words.append((text, left + w / 2, top + h / 2, tuple(f[2:5]), (left, top, w, h)))
    return words


def union_words(first, second, tol=6.0):
    """`first` plus the words of `second` with no same-text word within
    `tol` 1x px: a second OCR scale may only add reads, never take them.
    Words carry their box (in 1x px) for the magnifier fallback."""
    out = list(first)
    for text, x, y, line, box in second:
        if not any(t == text and abs(x - x2) <= tol and abs(y - y2) <= tol for t, x2, y2, _, _ in out):
            out.append((text, x, y, line, box))
    return out


def ocr_words(img, sparse=False):
    """OCR word boxes. `sparse` is for Tables' grids of short values, where
    page-layout analysis (psm 3) drops most of them. They are read as one
    uniform block of text (psm 6): sparse-text mode (psm 11) dropped whole
    cells of short numbers from LibreOffice's own reference (5 of the 10
    values in tables/conditional, 14 of 25 words in tables/chart) and, just
    as arbitrarily, different ones from ours, so it measured which blobs
    tesseract happened to find rather than what was drawn."""
    if not shutil.which("tesseract"):
        return None
    # Upscale: 96 DPI body text is below tesseract's comfortable size, and a
    # spreadsheet's 10-11 pt cell text more so; at 2x it dropped row numbers
    # and short values like "1/2" from both images, which is noise in a
    # fixture with only a handful of words.
    g = img.convert("L")
    if sparse:
        g = Image.fromarray(erase_rules(np.asarray(g), np.asarray(img.convert("RGB"))))
    if not sparse:
        return ocr_pass(g, 2, "3")
    # Sparse grids read at 4x and 6x unioned: a larger scale reads some
    # tiny tokens the base scale mis-segments (chart-scatter's "7"s), but
    # on its own it goes blind elsewhere (number-formats' "###" rows read
    # 3 of 3, a vacuous green), so neither scale may replace the other.
    # The union only adds reads, which is what the second scale is for.
    return union_words(ocr_pass(g, 4, "6"), ocr_pass(g, 6, "6"))


# A word the full-page read could not match gets one closer look: the word's
# own box plus this pad, magnified RESCUE_K times and read as a single word
# (psm 8). At 96 DPI a spreadsheet digit is ~7 px tall; the page-wide pass
# segments it by its neighbours (a "7" beside a gridline reads "ri"), while
# the crop reads the glyphs it actually holds.
RESCUE_PAD = 4

RESCUE_K = 3


def ocr_crop_text(grey):
    """Normalized text of one word-sized greyscale crop, or "": the same
    confidence floor and normalization as ocr_words."""
    import tempfile

    with tempfile.NamedTemporaryFile(suffix=".png") as tmp:
        grey.resize((grey.width * RESCUE_K, grey.height * RESCUE_K), Image.LANCZOS).save(tmp.name)
        r = subprocess.run(["tesseract", tmp.name, "-", "--psm", "8", "tsv"], capture_output=True, text=True)
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
        return text
    return ""


# A row or column of a rescue crop inked along this much of its length is a
# rule (a cell border, a frame), not part of a glyph: a word's strokes span
# its x-height or cap height, which the crop's padding keeps well under it.
# Ink is anything off-white: grid lines are drawn light (217 on 255).
RESCUE_RULE_SPAN = 0.9

RESCUE_RULE_INK = 245


def blank_rules(grey):
    """`grey`, a rescue crop, with its rules (rows and columns inked along
    RESCUE_RULE_SPAN of the crop) painted white. A grid line beside a lone
    digit stops tesseract's single-word mode reading the digit at all
    (tables/chart-pie in Tier C, #1199), and a rule is never the text being
    rescued. erase_rules can't do this: a crop is shorter than RULE_MIN, and
    a light grid line is not ink by its measure."""
    arr = np.asarray(grey).copy()
    ink = arr < RESCUE_RULE_INK
    rows, cols = ink.mean(axis=1) >= RESCUE_RULE_SPAN, ink.mean(axis=0) >= RESCUE_RULE_SPAN
    arr[rows, :] = 255
    arr[:, cols] = 255
    return Image.fromarray(arr)


def rescue_unread(ref_grey, ours_grey, box):
    """The magnified crop of `box` (in either image's pixels) reads as the
    same non-empty word on both sides, or None. The box comes from the
    reference's own word, so both crops show the same location: agreement
    means both renderings draw the same text there, read closely. It only
    ever adds a match the page-wide pass missed; no budget changes."""
    l, t, w, h = box
    area = (
        max(0, int(l - RESCUE_PAD)),
        max(0, int(t - RESCUE_PAD)),
        min(ref_grey.width, int(l + w + RESCUE_PAD)),
        min(ref_grey.height, int(t + h + RESCUE_PAD)),
    )
    ref_text = ocr_crop_text(blank_rules(ref_grey.crop(area)))
    if not ref_text:
        return None
    ours_text = ocr_crop_text(blank_rules(ours_grey.crop(area)))
    return ref_text if ours_text == ref_text else None


# How many neighbouring reads may join to one read on the other side, how
# long their joined text must be, and how far apart (1x px, the corners of
# the run and the token) they may stand.
JOIN_MAX = 6

JOIN_MIN_CHARS = 6

JOIN_TOL = 40.0

# The share of the longer text the shorter must cover when they differ by
# a letter or two OCR lost at an edge ("ousing" for "housing").
JOIN_COVER = 0.85


def join_matches(ref, ours, dist, taken):
    """Pair the same text read as one word on one side and several on the
    other, which `match_words`' same-text pairing cannot: tesseract splits
    an e-mail address at its punctuation on one rendering and not on the
    other, so the affordable-housing data dictionary's identical
    `housing.statistics@communities.gov.uk` read as four words on
    LibreOffice's page ("h ousing statistics communitiesgovuk") and one on
    ours, and the line was counted lost.

    A run of up to JOIN_MAX unpaired, adjacent words of one line on one side
    whose texts join to an unpaired word of the other, starting within
    JOIN_TOL of it, is paired with it: each reference word of the run, or
    the one reference word, takes the distance between the run's and the
    word's top-left corners. Updates `dist` (ref index -> distance) and
    `taken` (ours indices) in place. The texts may differ by what OCR
    dropped at an edge (the same page read "ousing" there): the shorter
    inside the longer, covering JOIN_COVER of it."""

    def same(a, b):
        if a == b:
            return True
        short, long_ = (a, b) if len(a) <= len(b) else (b, a)
        return len(short) >= 8 and short in long_ and len(short) >= JOIN_COVER * len(long_)

    def lines(words, free):
        by_line = {}
        for k in free:
            by_line.setdefault(words[k][3], []).append(k)
        return [sorted(ks, key=lambda k: words[k][4][0]) for ks in by_line.values()]

    def corner(w):
        return w[4][0], w[4][1]

    def near(a, b):
        return ((a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2) ** 0.5

    def runs(words, free):
        for ks in lines(words, free):
            for n in range(2, JOIN_MAX + 1):
                for start in range(len(ks) - n + 1):
                    run = ks[start : start + n]
                    text = "".join(words[k][0] for k in run)
                    if len(text) >= JOIN_MIN_CHARS:
                        yield run, text

    # Several reference words, one of ours.
    free_ref = [i for i in range(len(ref)) if i not in dist]
    free_ours = [j for j in range(len(ours)) if j not in taken]
    for run, text in sorted(runs(ref, free_ref), key=lambda r: -len(r[0])):
        if any(i in dist for i in run):
            continue
        at = corner(ref[run[0]])
        best = min(
            ((near(at, corner(ours[j])), j) for j in free_ours if j not in taken and same(ours[j][0], text)),
            default=None,
        )
        if best and best[0] <= JOIN_TOL:
            for i in run:
                dist[i] = best[0]
            taken.add(best[1])
    # One reference word, several of ours.
    free_ref = [i for i in range(len(ref)) if i not in dist]
    free_ours = [j for j in range(len(ours)) if j not in taken]
    for run, text in sorted(runs(ours, free_ours), key=lambda r: -len(r[0])):
        if any(j in taken for j in run):
            continue
        at = corner(ours[run[0]])
        best = min(
            ((near(at, corner(ref[i])), i) for i in free_ref if i not in dist and same(ref[i][0], text)), default=None
        )
        if best and best[0] <= JOIN_TOL:
            dist[best[1]] = best[0]
            taken.update(run)


def match_words(ref, ours, reread=None):
    """(fraction of `ref`'s words found in `ours`, median displacement in
    points, lost lines: `ref` lines of LOST_LINE_MIN_WORDS or more words
    with half or more of them not found).

    Same-text pairs are taken nearest first over the whole page, not in
    reading order: in reading order one word OCR missed on our side let
    its line's twin take the next line's copy, and every later repeat of
    that word ("text" on each line of letters/font-sizes) then took the
    line below its own, a cascade that read as 14pt of displacement on a
    page that agrees to a pixel.

    A reference word marked unprinted (`mark_printed`) that nothing on our
    side matches is left out altogether: a misread of the reference is
    never held against us. Matched, it counts like any other.

    `reread`, when given, is called with an unmatched reference word's box
    and returns the word both sides read there on a closer look, or None;
    a rescued word counts as found where it stands (displacement 0)."""
    if ref is None or ours is None or not ref:
        return None, None, None
    pairs = sorted(
        (((w[1] - o[1]) ** 2 + (w[2] - o[2]) ** 2) ** 0.5, i, j)
        for i, w in enumerate(ref)
        for j, o in enumerate(ours)
        if o[0] == w[0]
    )
    dist, taken = {}, set()
    for d, i, j in pairs:
        if i not in dist and j not in taken:
            dist[i] = d
            taken.add(j)
    join_matches(ref, ours, dist, taken)
    found, counted, disp = 0, 0, []
    per_line = {}
    for i, w in enumerate(ref):
        line, box = w[3], w[4]
        hit = i in dist
        rescued = not hit and reread is not None and reread(box) is not None
        if not (hit or rescued) and len(w) > 5 and not w[5]:
            continue
        counted += 1
        total, hits = per_line.get(line, (0, 0))
        per_line[line] = (total + 1, hits + (hit or rescued))
        if hit:
            found += 1
            disp.append(dist[i])
        elif rescued:
            found += 1
            disp.append(0.0)
    median = float(np.median(disp)) * PX_TO_PT if disp else None
    lost = sum(1 for total, hits in per_line.values() if total >= LOST_LINE_MIN_WORDS and 2 * hits <= total)
    return (found / counted if counted else None), median, lost


# A word tesseract reads off LibreOffice's own picture that LibreOffice did
# not print may be a misread of the reference ("8pt" as "spt", a bullet as
# "e"), which no rendering of ours can match. It may also be text inside a
# picture (a logo's "Government of Canada"), which ours draws too. So an
# unprinted read is marked, not dropped: match_words leaves it out only
# where nothing on our side matches it. The reference's text layer
# (lo-<n>.txt, lo_render.py) says what was printed.
# The layer runs a raised or lowered digit into its word ("mc2", "h2o")
# where OCR reads the letters alone, so printed words are also cut where
# letters meet digits. A read may join neighbours ("8pttext") or, across a
# hyphen or a tight kern, cut a word: up to TRUTH_JOIN printed pieces
# joined, or TRUTH_PIECE_MIN or more characters of one word, still count.
TRUTH_JOIN = 4

TRUTH_PIECE_MIN = 4


def norm_word(w):
    return re.sub(r"[^0-9a-z]", "", w.lower())


def printed_words(txt_path):
    """The words of a reference page's text layer, normalized as OCR reads
    are, in order; None when the layer is missing (an older reference)."""
    try:
        with open(txt_path, encoding="utf-8", errors="replace") as f:
            raw = f.read()
    except OSError:
        return None
    return [w for w in (norm_word(t) for t in raw.split()) if w]


def is_printed(text, printed, joined):
    if text in joined:
        return True
    return len(text) >= TRUTH_PIECE_MIN and any(text in w for w in printed)


def mark_printed(words, printed):
    """`words` (OCR reads of the reference), each with a sixth field: did
    LibreOffice print it. A page whose text layer is empty (a scan, text
    drawn as curves) or missing marks every read printed: there is nothing
    to check them against."""
    if words is None:
        return None
    if not printed:
        return [(*w[:5], True) for w in words]
    pieces = [p for w in printed for p in re.findall(r"[0-9]+|[a-z]+", w)]
    joined = {"".join(pieces[i : i + n]) for n in range(1, TRUTH_JOIN + 1) for i in range(len(pieces) - n + 1)}
    return [(*w[:5], is_printed(w[0], printed, joined)) for w in words]
