"""Draw the demo's roadmap-thread PDFs (data/demo/) by hand: base-14 fonts and
vector shapes, no dependencies. Run: python3 tools/gen-demo-pdfs.py
"""

W, H = 612, 792

def rgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i+2], 16) / 255 for i in (0, 2, 4))

class Page:
    def __init__(self):
        self.ops = []
    def fill(self, c):
        r, g, b = rgb(c); self.ops.append(f"{r:.3f} {g:.3f} {b:.3f} rg")
    def stroke(self, c):
        r, g, b = rgb(c); self.ops.append(f"{r:.3f} {g:.3f} {b:.3f} RG")
    # y measured from the top, like a layout
    def rect(self, x, y, w, h, c):
        self.fill(c); self.ops.append(f"{x} {H-y-h} {w} {h} re f")
    def rrect(self, x, y, w, h, r, c, stroke=None, dash=False, lw=1):
        k = 0.5523 * r
        X0, Y0, X1, Y1 = x, H - y - h, x + w, H - y
        p = [f"{X0+r} {Y0} m", f"{X1-r} {Y0} l", f"{X1-r+k} {Y0} {X1} {Y0+r-k} {X1} {Y0+r} c",
             f"{X1} {Y1-r} l", f"{X1} {Y1-r+k} {X1-r+k} {Y1} {X1-r} {Y1} c",
             f"{X0+r} {Y1} l", f"{X0+r-k} {Y1} {X0} {Y1-r+k} {X0} {Y1-r} c",
             f"{X0} {Y0+r} l", f"{X0} {Y0+r-k} {X0+r-k} {Y0} {X0+r} {Y0} c", "h"]
        self.ops.append("q")
        if dash: self.ops.append("[4 3] 0 d")
        self.ops.append(f"{lw} w")
        if c: self.fill(c)
        if stroke: self.stroke(stroke)
        self.ops += p
        self.ops.append("B" if (c and stroke) else ("f" if c else "S"))
        self.ops.append("Q")
    def line(self, x0, y0, x1, y1, c, lw=1, dash=None):
        self.ops.append("q"); self.stroke(c); self.ops.append(f"{lw} w")
        if dash: self.ops.append(f"[{dash}] 0 d")
        self.ops.append(f"{x0} {H-y0} m {x1} {H-y1} l S Q")
    def diamond(self, cx, cy, s, c):
        self.fill(c); cy = H - cy
        self.ops.append(f"{cx} {cy+s} m {cx+s} {cy} l {cx} {cy-s} l {cx-s} {cy} l h f")
    def circle(self, cx, cy, r, c):
        self.rrect(cx - r, cy - r, 2 * r, 2 * r, r, c)
    def text(self, x, y, s, size, c, bold=False, anchor="l"):
        font = "F2" if bold else "F1"
        wdt = text_width(s, size, bold)
        if anchor == "c": x -= wdt / 2
        elif anchor == "r": x -= wdt
        s = s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")
        self.fill(c)
        self.ops.append(f"BT /{font} {size} Tf {x:.1f} {H-y:.1f} Td ({s}) Tj ET")

# Helvetica widths (per 1000 em) for the characters we use; rough default otherwise.
HV = {' ':278,'·':278,',':278,'.':278,'-':333,':':278,'(':333,')':333,'/':278,'+':584,'0':556,'1':556,'2':556,'3':556,'4':556,'5':556,'6':556,'7':556,'8':556,'9':556,
 'A':667,'B':667,'C':722,'D':722,'E':667,'F':611,'G':778,'H':722,'I':278,'J':500,'K':667,'L':556,'M':833,'N':722,'O':778,'P':667,'Q':778,'R':722,'S':667,'T':611,'U':722,'V':667,'W':944,'X':667,'Y':667,'Z':611,
 'a':556,'b':556,'c':500,'d':556,'e':556,'f':278,'g':556,'h':556,'i':222,'j':222,'k':500,'l':222,'m':833,'n':556,'o':556,'p':556,'q':556,'r':333,'s':500,'t':278,'u':556,'v':500,'w':722,'x':500,'y':500,'z':500}
HB = dict(HV); HB.update({'a':556,'b':611,'c':556,'d':611,'e':556,'f':333,'g':611,'h':611,'i':278,'j':278,'k':556,'l':278,'m':889,'n':611,'o':611,'p':611,'q':611,'r':389,'s':556,'t':333,'u':611,'v':556,'w':778,'x':556,'y':556,'z':500,
 'A':722,'B':722,'C':722,'D':722,'E':667,'F':611,'G':778,'H':722,'I':278,'J':556,'K':722,'L':611,'M':833,'N':722,'O':778,'P':667,'Q':778,'R':722,'S':667,'T':611,'U':722,'V':667,'W':944,'X':667,'Y':667,'Z':611,':':333,'-':333})
def text_width(s, size, bold):
    t = HB if bold else HV
    return sum(t.get(ch, 556) for ch in s) * size / 1000

def pdf(page, title):
    content = "\n".join(page.ops).encode("latin-1")
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {W} {H}] /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> /Contents 4 0 R >>".encode(),
        b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>",
        f"<< /Title ({title}) /Producer (Hylki demo) >>".encode(),
    ]
    out = bytearray(b"%PDF-1.4\n")
    offs = []
    for i, o in enumerate(objs):
        offs.append(len(out))
        out += f"{i+1} 0 obj\n".encode() + o + b"\nendobj\n"
    x = len(out)
    out += f"xref\n0 {len(objs)+1}\n0000000000 65535 f \n".encode()
    for o in offs: out += f"{o:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objs)+1} /Root 1 0 R /Info 7 0 R >>\nstartxref\n{x}\n%%EOF\n".encode()
    return bytes(out)

INK, MUTED, FAINT, RULE, BG = "#1e1e24", "#6b6b76", "#9a9aa3", "#e4e4e8", "#f6f6f8"
BLUE, GREEN, AMBER, PURPLE, RED, TEAL = "#3584e4", "#2ec27e", "#e5a50a", "#9141ac", "#e01b24", "#1a9e8f"

def header(p, kicker, title, sub):
    p.rect(0, 0, W, 8, BLUE)
    p.text(56, 118, kicker, 9, BLUE, bold=True)
    p.text(56, 150, title, 28, INK, bold=True)
    p.text(56, 172, sub, 11, MUTED)
    p.line(56, 192, W - 56, 192, RULE)

def footer(p, left, right):
    p.line(56, 736, W - 56, 736, RULE)
    p.text(56, 754, left, 8, FAINT)
    p.text(W - 56, 754, right, 8, FAINT, anchor="r")

# ---- Sophie's draft roadmap -------------------------------------------------
def roadmap():
    p = Page()
    header(p, "STUDIO  ·  PLANNING", "Q3 Roadmap", "Draft for Thursday's review  ·  July to September")
    # timeline grid: label column then 13 weeks
    x0, x1, top = 176, W - 56, 214
    wk = (x1 - x0) / 13
    p.rrect(56, top, W - 112, 30, 6, BG)
    for i, m in enumerate(["July", "August", "September"]):
        p.text(x0 + (i * 13 / 3 + 13 / 6) * wk, top + 19, m, 9, MUTED, bold=True, anchor="c")
    p.text(68, top + 19, "Workstream", 9, MUTED, bold=True)
    rows = [
        ("Research", "Interviews, audit", 0, 3, TEAL),
        ("Design system", "Tokens, components", 1.5, 5, PURPLE),
        ("Migration", "Data + accounts", 4, 5.5, AMBER),
        ("Reader redesign", "Cards, dark mode", 4.5, 4.5, BLUE),
        ("Beta", "Staged rollout", 9, 2.5, GREEN),
        ("Release", "3.0 launch", 11.5, 1.5, RED),
    ]
    rh = 44
    gy0 = top + 38
    for i in range(14):
        x = x0 + i * wk
        p.line(x, gy0, x, gy0 + rh * len(rows), RULE if i % 13 else "#d4d4da", lw=0.6 if i % 13 else 0.8,
               dash=None if i in (0, 13) or i % 13 == 0 else "1 2")
    for m in (13 / 3, 26 / 3):
        p.line(x0 + m * wk, gy0 - 8, x0 + m * wk, gy0 + rh * len(rows), "#d4d4da", lw=0.8)
    for r, (name, note, s, d, c) in enumerate(rows):
        y = gy0 + r * rh
        if r:
            p.line(56, y, W - 56, y, RULE, lw=0.5)
        p.text(68, y + 20, name, 10.5, INK, bold=True)
        p.text(68, y + 33, note, 8.5, MUTED)
        bx, bw = x0 + s * wk + 2, d * wk - 4
        if name == "Migration":
            p.rrect(bx, y + 12, bw, 20, 10, "#fdf0cc", stroke=AMBER, dash=True, lw=1.2)
            p.text(bx + bw / 2, y + 25.5, "Sequencing TBD", 8.5, "#8a6300", bold=True, anchor="c")
        else:
            p.rrect(bx, y + 12, bw, 20, 10, c)
    gy1 = gy0 + rh * len(rows)
    # milestones
    for w, label, c in [(9, "Beta", GREEN), (11.5, "Freeze", RED)]:
        x = x0 + w * wk
        p.line(x, gy0, x, gy1 + 6, c, lw=1, dash="3 2")
        p.diamond(x, gy1 + 12, 5, c)
        p.text(x, gy1 + 30, label, 8.5, c, bold=True, anchor="c")
    # open questions
    y = gy1 + 64
    p.text(56, y, "Open questions", 13, INK, bold=True)
    qs = [
        "Can the first two migration steps run in parallel with the design system work?",
        "Does QA need the staging environment before the migration starts?",
        "Is a week of slack before the freeze enough for the beta feedback round?",
    ]
    for i, q in enumerate(qs):
        yy = y + 24 + i * 22
        p.circle(62, yy - 3.5, 3, AMBER)
        p.text(74, yy, q, 10, INK)
    footer(p, "Draft 2  ·  Sophie Turner", "Studio  ·  Q3 planning")
    return pdf(p, "Q3 Roadmap (draft)")

# ---- Marcus's two timelines ---------------------------------------------------
def timeline():
    p = Page()
    header(p, "MIGRATION PHASE", "Timeline options", "Sequential vs. parallel, against the 21 August release freeze")
    x0, x1 = 176, W - 56
    days = 49  # 3 July .. 21 August
    dx = (x1 - x0) / days
    def panel(y, tag, title, sub, bars, end, good):
        p.rrect(56, y, W - 112, 176, 10, BG)
        p.rrect(68, y + 14, 22, 22, 6, BLUE if good else MUTED)
        p.text(79, y + 29, tag, 11, "#ffffff", bold=True, anchor="c")
        p.text(100, y + 25, title, 13, INK, bold=True)
        p.text(100, y + 40, sub, 9, MUTED)
        if good:
            p.rrect(W - 56 - 100, y + 14, 88, 20, 10, "#d8f3e4")
            p.text(W - 56 - 56, y + 28, "Recommended", 8.5, "#1b7a4b", bold=True, anchor="c")
        gy = y + 62
        for i in range(0, days + 1, 7):
            p.line(x0 + i * dx, gy - 4, x0 + i * dx, gy + 3 * 28, RULE, lw=0.6, dash="1 2")
        for i, (label, s, d, c) in enumerate(bars):
            yy = gy + i * 28
            p.text(68, yy + 15, label, 9.5, INK, bold=True)
            p.rrect(x0 + s * dx, yy + 3, d * dx, 17, 8.5, c)
        fx = x0 + days * dx
        p.line(fx, gy - 10, fx, gy + 3 * 28 + 2, RED, lw=1.4, dash="3 2")
        ex = x0 + end * dx
        p.diamond(ex, gy + 3 * 28 + 8, 4.5, INK)
        if good:
            p.rrect(ex + 8, gy + 3 * 28 + 4, fx - ex - 10, 8, 4, "#d8f3e4")
            p.text((ex + fx) / 2, gy + 3 * 28 + 24, "1 week slack", 8.5, "#1b7a4b", bold=True, anchor="c")
            p.text(ex - 8, gy + 3 * 28 + 24, "Lands 14 Aug", 8.5, INK, bold=True, anchor="r")
        else:
            p.text(ex - 8, gy + 3 * 28 + 24, "Lands 21 Aug, no slack", 8.5, RED, bold=True, anchor="r")
    # week axis
    ay = 212
    for i, lab in enumerate(["3 Jul", "10 Jul", "17 Jul", "24 Jul", "31 Jul", "7 Aug", "14 Aug", "21 Aug"]):
        p.text(x0 + i * 7 * dx, ay, lab, 8, MUTED, bold=True, anchor="c" if i < 7 else "r")
    p.text(x0 + days * dx, ay - 12, "FREEZE", 7.5, RED, bold=True, anchor="r")
    panel(226, "A", "Sequential", "Each step waits for the one before", [
        ("Schema move", 0, 14, AMBER), ("Account sync", 14, 17, AMBER), ("Cut-over", 31, 18, AMBER)], 49, False)
    panel(418, "B", "Parallel first two steps", "Schema move and account sync overlap", [
        ("Schema move", 0, 14, BLUE), ("Account sync", 3, 17, BLUE), ("Cut-over", 20, 22, BLUE)], 42, True)
    y = 624
    p.text(56, y, "Needs", 13, INK, bold=True)
    for i, q in enumerate(["Staging environment one week earlier (QA, Priya)", "Ops sign-off on the overlap window (Marcus)"]):
        yy = y + 24 + i * 22
        p.circle(62, yy - 3.5, 3, BLUE)
        p.text(74, yy, q, 10, INK)
    footer(p, "Marcus Chen  ·  for the Q3 roadmap review", "Option B pulls the migration in by one week")
    return pdf(p, "Migration timeline options")

import os
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "data", "demo")
open(OUT + "/q3-roadmap-draft.pdf", "wb").write(roadmap())
open(OUT + "/migration-timeline.pdf", "wb").write(timeline())
