#!/usr/bin/env python3
"""Read an Xvfb framebuffer dump (`Xvfb -fbdir`, XWD format, 24/32-bit
ZPixmap) with nothing but the standard library: no screenshot tool and no
Pillow are in the Flatpak builder image the CI job runs in.

    xwd_ink.py SCREEN.xwd [--png OUT.png]

Prints, as JSON, the size of the largest white rectangle found on screen
(the page Letters draws) and the dark "ink" pixels inside it: a page whose
text is not drawn has next to none. --png writes the screen out for the
job's artifacts.
"""
import json
import struct
import sys
import zlib


def read_xwd(path):
    data = open(path, "rb").read()
    header_size = struct.unpack(">I", data[0:4])[0]
    fields = struct.unpack(">25I", data[0:100])
    (_, _, fmt, depth, width, height, _, byte_order, _, _, _, bpp, bpl) = fields[:13]
    ncolors = fields[19]
    if fmt != 2 or bpp not in (24, 32):
        raise SystemExit(f"unsupported XWD: format {fmt}, {bpp} bits per pixel")
    start = header_size + ncolors * 12
    step = bpp // 8
    rows = []
    for y in range(height):
        row = data[start + y * bpl: start + y * bpl + width * step]
        if byte_order == 0:  # LSBFirst: B, G, R(, X)
            rows.append([(row[i + 2], row[i + 1], row[i]) for i in range(0, len(row), step)])
        else:
            off = step - 3
            rows.append([(row[i + off], row[i + off + 1], row[i + off + 2]) for i in range(0, len(row), step)])
    return width, height, rows


def page_box(width, height, rows):
    """The page: the widest run of near-white pixels with the grey backdrop
    on both sides of it, grown up and down while its edges stay white.
    The header bar and status bar are white too, but they run to the
    window's edge rather than out of grey."""
    def white(p):
        return min(p) >= 245

    # The view's backdrop: a neutral grey, light or (in the dark style)
    # dark, never the page's white.
    def backdrop(p):
        return max(p) <= 235 and max(p) - min(p) <= 24

    # A stretch of backdrop, not a one-pixel window border.
    def backdrop_at(row, x, step):
        return all(0 <= x + step * i < width and backdrop(row[x + step * i]) for i in range(8))

    best = None
    for y in range(0, height, 4):
        row = rows[y]
        x = 1
        while x < width:
            if white(row[x]) and backdrop_at(row, x - 1, -1):
                end = x
                while end < width and white(row[end]):
                    end += 1
                if backdrop_at(row, end, 1) and end - x >= 200:
                    if best is None or end - x > best[2] - best[0]:
                        best = (x, y, end, y)
                x = end
            x += 1
    if best is None:
        return None
    x0, y0, x1, _ = best
    edge = lambda y: white(rows[y][x0]) and white(rows[y][x1 - 1])
    top = y0
    while top > 0 and edge(top - 1):
        top -= 1
    bottom = y0
    while bottom + 1 < height and edge(bottom + 1):
        bottom += 1
    return x0, top, x1, bottom + 1


def write_png(path, width, height, rows):
    raw = b"".join(b"\x00" + bytes(c for p in row for c in p) for row in rows)
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 6)))
        f.write(chunk(b"IEND", b""))


def measure(path, png=None):
    """The page's box and its ink pixels in the framebuffer at `path`."""
    width, height, rows = read_xwd(path)
    if png:
        write_png(png, width, height, rows)
    box = page_box(width, height, rows)
    ink = 0
    if box:
        x0, y0, x1, y1 = box
        ink = sum(1 for y in range(y0, y1) for x in range(x0, x1) if max(rows[y][x]) < 128)
    return {"screen": [width, height], "page": box, "ink": ink}


def main():
    args = sys.argv[1:]
    png = args[args.index("--png") + 1] if "--png" in args else None
    print(json.dumps(measure(args[0], png)))


if __name__ == "__main__":
    main()
