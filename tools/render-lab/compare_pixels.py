"""Image primitives for compare.py: loading, ink masks, SSIM, colours, rule erasing."""

import numpy as np
from PIL import Image


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


def long_lines(mask, axis):
    """Indices of rows (axis=1) or columns (axis=0) of `mask` holding an
    unbroken line over half the image long, grouped into (first, last) runs
    of adjacent indices, in order."""
    m = mask if axis == 1 else mask.T
    hits = np.nonzero(longest_run(bridge(m), axis=1) > 0.5 * m.shape[1])[0]
    groups = []
    for i in hits:
        if groups and i == groups[-1][1] + 1:
            groups[-1][1] = i
        else:
            groups.append([i, i])
    return [tuple(g) for g in groups]


def lo_cell_box(ref):
    """The cells inside LibreOffice's printed sheet: (left, top, right,
    bottom), or None. With headings on and gridlines off (fixtures.py),
    Calc frames the print range and boxes each heading in black, so the
    long lines are, in order, the frame's top, the column headings' bottom
    and the frame's bottom (and the same across: frame, row headings,
    frame). The cells are what the second and last of each enclose."""
    dark = np.asarray(ref.convert("L")) < 128
    rows, cols = long_lines(dark, axis=1), long_lines(dark, axis=0)
    if len(rows) < 3 or len(cols) < 3:
        return None
    return cols[1][1] + 1, rows[1][1] + 1, cols[-1][0], rows[-1][0]


def our_cell_origin(ours):
    """Where our cells start: the width of the row-header band and the
    height of the column-header band, both drawn in one flat shade. A band
    ends at the first column (row) where that shade covers under half the
    pixels; labels cover far less than half their band. The shade is read
    a few px in from the corner, clear of the 1 px frame a Broadway
    capture can carry on its edge."""
    a = np.asarray(ours).astype(int)
    shade = a[4, 4]
    is_shade = np.abs(a - shade).sum(axis=2) <= 6

    def band(fraction):
        inside = np.nonzero(fraction >= 0.5)[0]
        if not len(inside):
            return 0
        past = np.nonzero(fraction[inside[0] :] < 0.5)[0]
        return int(inside[0] + past[0]) if len(past) else 0

    return band(is_shade.mean(axis=0)), band(is_shade.mean(axis=1))


def bridge(mask, gap=3):
    """`mask` with gaps of up to `gap` px along each row filled in: where a
    rule crosses another, the crossing pixels continue both ways and are
    not a thin line in either direction, so they would break the run."""
    w = mask.shape[1]
    left = np.zeros_like(mask)
    right = np.zeros_like(mask)
    for k in range(1, gap + 1):
        left[:, k:] |= mask[:, : w - k]
        right[:, : w - k] |= mask[:, k:]
    return mask | (left & right)


def longest_run(mask, axis):
    """Length of the longest run of consecutive True along `axis`, for each
    row (axis=1) or column (axis=0)."""
    m = mask if axis == 1 else mask.T
    best = np.zeros(m.shape[0], dtype=int)
    cur = np.zeros(m.shape[0], dtype=int)
    for j in range(m.shape[1]):
        cur = np.where(m[:, j], cur + 1, 0)
        best = np.maximum(best, cur)
    return best


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
    idx = np.asarray(q).ravel()
    px = np.asarray(img.convert("RGB"), dtype=int).reshape(-1, 3)
    counts = np.bincount(idx, minlength=n)
    total = counts.sum()
    out = []
    for i in range(n):
        if counts[i] / total < min_share:
            continue
        rgb = np.array(pal[i * 3 : i * 3 + 3])
        if rgb.max() - rgb.min() < 40:  # grey-ish
            continue
        # The colour the cluster's saturated pixels actually have, not the
        # cluster's mean: small coloured text shares a cluster with the
        # black text and grey anti-aliasing beside it, and their mean
        # (C00000 red text averaged to 905050) is in neither image.
        members = px[idx == i]
        members = members[members.max(axis=1) - members.min(axis=1) >= 40]
        out.append(np.median(members, axis=0).astype(int) if len(members) else rgb)
    return out


def color_groups(colors, near=60):
    """`colors` grouped so that shades within `near` of one another count
    as one colour. The anti-aliased edge of a single rotated fill quantises
    into several clusters a few levels apart; they are one colour to look
    for, not five, and each alone covers too little to be found."""
    groups = []
    for rgb in colors:
        for g in groups:
            if any(np.abs(rgb - other).sum() < near for other in g):
                g.append(rgb)
                break
        else:
            groups.append([rgb])
    return groups


def color_presence(ref, ours):
    wanted = color_groups(salient_colors(ref))
    if not wanted:
        return None
    # Full resolution: halving blends thin coloured strokes (red text)
    # into the white round them until the colour is gone.
    arr = np.asarray(ours.convert("RGB"), dtype=int).reshape(-1, 3)
    hit = 0
    for group in wanted:
        near = np.zeros(len(arr), dtype=bool)
        for rgb in group:
            near |= np.abs(arr - rgb).sum(axis=1) < 60
        if near.mean() > 0.0005:
            hit += 1
    return hit / len(wanted)


# ── OCR ───────────────────────────────────────────────────────────────────
# A rule is a run of ink at least this long (px at 96 DPI) and at most
# RULE_THICK thick. No glyph of spreadsheet-sized text has a stroke that
# long, so what it removes is gridlines, borders and chart axes.
RULE_MIN = 24

RULE_THICK = 4


def run_lengths(mask):
    """For each True pixel of `mask`, the length of the horizontal run of
    True it is part of (0 elsewhere)."""
    out = np.zeros(mask.shape, dtype=np.int32)
    for r in range(mask.shape[0]):
        row = mask[r]
        if not row.any():
            continue
        edges = np.flatnonzero(np.diff(np.concatenate(([0], row.astype(np.int8), [0]))))
        for a, b in zip(edges[::2], edges[1::2]):
            out[r, a:b] = b - a
    return out


def erase_rules(g, rgb=None):
    """`g` (greyscale, uint8 array) with its thin straight lines (grid-,
    border- and axis lines) painted white.

    Tesseract reads a line that runs past or through short text as part of
    it: in a chart, LibreOffice's y-axis labels touch the first vertical
    gridline and came back as "gs", "al", "tot" instead of 9, 8, 7, and our
    own as "ee" and "40", so the word score measured where gridlines met
    digits, not the digits (render lab tables/chart-scatter).

    A pixel is a rule's when the ink it is in runs RULE_MIN or more one way
    and RULE_THICK or less the other. Lengths are counted through crossing
    lines, so a gridline between two others is still one long run; a
    filled area (a cell's fill, a bar, a pie slice) is thick both ways and
    stays, with any text on it."""
    ink = g < 200
    across = run_lengths(ink)
    down = run_lengths(ink.T).T
    flat = (across >= RULE_MIN) & (down <= RULE_THICK)
    tall = (down >= RULE_MIN) & (across <= RULE_THICK)
    # Where two rules cross, the ink is long both ways, like a fill. A
    # crossing is what joins them: within RULE_THICK of it, the same row
    # goes on as a flat rule and the same column as a tall one. A fill's
    # rows and columns are thick, so its edges and corners never are.
    near_flat = np.zeros_like(flat)
    near_tall = np.zeros_like(tall)
    for d in range(1, RULE_THICK + 1):
        near_flat[:, d:] |= flat[:, :-d]
        near_flat[:, :-d] |= flat[:, d:]
        near_tall[d:, :] |= tall[:-d, :]
        near_tall[:-d, :] |= tall[d:, :]
    cross = (across >= RULE_MIN) & (down >= RULE_MIN) & near_flat & near_tall
    rule = flat | tall | cross
    out = g.copy()
    out[rule] = 255
    if rgb is not None:
        erase_keys(out, ink & ~rule, rgb)
    return out


def erase_keys(out, ink, rgb):
    """Paint white, in `out`, each solid blob of `ink` KEY_MIN to KEY_MAX px
    across: a chart's legend keys and data-point markers. Tesseract reads
    one beside a word as letters ("mqqi" for a key and "Q1", "oo" for a
    marker), which scores where keys sit, not what the text says. Only a
    coloured (saturated), square or round, solid blob counts: text is
    black or grey, or thin-stroked if coloured, and never that solid."""
    rgbf = rgb.astype(np.float32)
    # HSV saturation measures chroma relative to brightness. Dividing by
    # 255 instead misses dark colours such as the purple Q4 legend key
    # (128, 100, 162), which OCR then joins to the label as "ma".
    saturation = (rgbf.max(axis=2) - rgbf.min(axis=2)) / np.maximum(rgbf.max(axis=2), 1)
    h, w = ink.shape
    seen = np.zeros_like(ink)
    for y, x in zip(*np.nonzero(ink)):
        if seen[y, x]:
            continue
        stack, pixels = [(y, x)], []
        seen[y, x] = True
        while stack:
            cy, cx = stack.pop()
            pixels.append((cy, cx))
            if len(pixels) > KEY_MAX * KEY_MAX:
                break
            for ny, nx in ((cy + 1, cx), (cy - 1, cx), (cy, cx + 1), (cy, cx - 1)):
                if 0 <= ny < h and 0 <= nx < w and ink[ny, nx] and not seen[ny, nx]:
                    seen[ny, nx] = True
                    stack.append((ny, nx))
        if stack:
            continue  # bigger than a key: a fill, a bar, a slice
        ys = [p[0] for p in pixels]
        xs = [p[1] for p in pixels]
        bh, bw = max(ys) - min(ys) + 1, max(xs) - min(xs) + 1
        square = 0.8 <= bw / bh <= 1.25
        coloured = np.mean([saturation[p] for p in pixels]) > 0.25
        if (
            KEY_MIN <= bh <= KEY_MAX
            and KEY_MIN <= bw <= KEY_MAX
            and square
            and coloured
            and len(pixels) >= 0.7 * bh * bw
        ):
            for py, px in pixels:
                out[py, px] = 255


# Legend keys are 8 px squares; markers 7-10 px circles (a circle fills
# 79% of its box).
KEY_MIN = 5

KEY_MAX = 14
