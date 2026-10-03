#!/usr/bin/env python3
"""Overview videos of Letters, Tables and Decks, in GNOME's style.

Records the feature tour (feature_tour.py) as it drives each app, and cuts
one segment per feature: from the moment the app's window is up to just
after the feature is fully on screen. Each segment shows the window with
rounded corners and a soft shadow, on a gradient in the app's accent
colour, captioned in a GNOME OSD-style pill. A title card (the app's icon,
name and summary from its metainfo) opens each video and a suite card
closes it, joined by crossfades. Type is Adwaita Sans, GNOME's typeface.

Everything shown is the running app, reached the way the tour reaches it;
nothing in a segment is drawn over the app but the caption.

Writes <out>/<app>.mp4 per app and <out>/gtk-office-suite.mp4, a shorter
trailer of three features per app. Runs in the feature tour's environment
(overview_video.sh sets it up at 2x scale, so the downscaled windows are
sharp).

Usage: overview_video.py <out-dir> [letters|tables|decks|suite ...]
"""

import os
import re
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import feature_tour as ft  # noqa: E402

REPO = ft.REPO
W, H, FPS = 1920, 1080, 30
FADE = 0.5            # crossfade between segments, seconds
LEAD = 5.0            # the most of a feature's lead-up a segment shows
HOLD = 2.5            # how long a finished feature stays on screen

APPS = {
    # name, accent (light, dark): GNOME's palette (blue, green, orange).
    "letters": ("Letters", ("#62a0ea", "#1a5fb4")),
    "tables": ("Tables", ("#57e389", "#26a269")),
    "decks": ("Decks", ("#ffa348", "#c64600")),
}
TRAILER = {
    "letters": ("letters-document", "letters-styles", "letters-track-changes"),
    "tables": ("tables-grid", "tables-insert-chart", "tables-conditional-format"),
    "decks": ("decks-editor", "decks-insert-shape", "decks-rehearse"),
}


# ── Type and art ─────────────────────────────────────────────────────

def font_path():
    """Adwaita Sans, else Cantarell, else whatever sans fontconfig has."""
    for family in ("Adwaita Sans", "Cantarell", "Sans"):
        out = subprocess.run(["fc-match", "-f", "%{file}", family], capture_output=True, text=True).stdout
        if out and (family == "Sans" or family.split()[0].lower() in os.path.basename(out).lower()):
            return out
    return None


FONT = font_path()


def font(size, weight=400):
    from PIL import ImageFont
    f = ImageFont.truetype(FONT, size)
    try:
        # Adwaita Sans has two axes, optical size and weight, in that
        # order; display sizes use its display optical size.
        values = {b"Weight": weight, b"Optical size": 32 if size >= 40 else 14}
        f.set_variation_by_axes([values.get(a["name"], a["default"]) for a in f.get_variation_axes()])
    except Exception:  # noqa: BLE001  a static font has no axes
        pass
    return f


def rgb(hex_colour):
    return tuple(int(hex_colour[i:i + 2], 16) for i in (1, 3, 5))


def gradient(light, dark, size=(W, H)):
    """A diagonal gradient with two soft glows, GNOME-wallpaper calm."""
    from PIL import Image, ImageDraw, ImageFilter
    w, h = size
    a, b = rgb(light), rgb(dark)
    img = Image.new("RGB", size)
    px = img.load()
    for y in range(h):
        for x in range(0, w, 4):
            t = min(1.0, max(0.0, (x / w) * 0.45 + (y / h) * 0.55))
            c = tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))
            for dx in range(4):
                if x + dx < w:
                    px[x + dx, y] = c
    glow = Image.new("RGBA", size, (0, 0, 0, 0))
    d = ImageDraw.Draw(glow)
    d.ellipse((-w * 0.15, -h * 0.35, w * 0.45, h * 0.55), fill=(255, 255, 255, 46))
    d.ellipse((w * 0.6, h * 0.45, w * 1.2, h * 1.35), fill=(0, 0, 0, 40))
    glow = glow.filter(ImageFilter.GaussianBlur(160))
    return Image.alpha_composite(img.convert("RGBA"), glow)


def icon(app, size):
    from PIL import Image
    path = tempfile.mktemp(suffix=".png")
    subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), "-o", path,
                    f"{REPO}/flatpak/icons/org.tunaos.{app}.svg"], check=True)
    return Image.open(path).convert("RGBA")


def summary(app):
    text = open(f"{REPO}/flatpak/org.tunaos.{app}.metainfo.xml").read()
    m = re.search(r"<summary>([^<]+)</summary>", text)
    return m.group(1).strip() if m else ""


def text_centre(draw, y, text, f, fill):
    w = draw.textlength(text, font=f)
    draw.text(((W - w) / 2, y), text, font=f, fill=fill)


def title_card(app, path):
    from PIL import ImageDraw
    name, (light, dark) = APPS[app]
    img = gradient(light, dark)
    img.alpha_composite(icon(app, 300), ((W - 300) // 2, 230))
    d = ImageDraw.Draw(img)
    text_centre(d, 580, name, font(120, 800), (255, 255, 255, 255))
    text_centre(d, 735, summary(app), font(46, 500), (255, 255, 255, 225))
    img.convert("RGB").save(path)


def suite_card(path, heading, subheading):
    from PIL import ImageDraw
    img = gradient("#3d3846", "#241f31")
    for i, app in enumerate(APPS):
        img.alpha_composite(icon(app, 200), (W // 2 - 340 + i * 240 - 100 + 100, 250))
    d = ImageDraw.Draw(img)
    text_centre(d, 520, heading, font(96, 800), (255, 255, 255, 255))
    text_centre(d, 655, subheading, font(44, 500), (255, 255, 255, 220))
    text_centre(d, 780, "Letters · Tables · Decks  —  built with GTK 4 and libadwaita", font(36, 400), (255, 255, 255, 170))
    text_centre(d, 840, "github.com/tuna-os/gtk-office-suite  ·  Pre-alpha", font(32, 400), (255, 255, 255, 140))
    img.convert("RGB").save(path)


def caption_pill(text, path):
    """A GNOME OSD/toast: a dark rounded bubble, white Adwaita Sans."""
    from PIL import Image, ImageDraw
    f = font(34, 500)
    probe = ImageDraw.Draw(Image.new("RGBA", (1, 1)))
    words, lines, line = text.split(), [], ""
    for word in words:
        trial = f"{line} {word}".strip()
        if probe.textlength(trial, font=f) > 1500 and line:
            lines.append(line)
            line = word
        else:
            line = trial
    lines.append(line)
    tw = max(probe.textlength(l, font=f) for l in lines)
    pad_x, pad_y, gap = 40, 20, 10
    lh = 42
    bw, bh = int(tw + pad_x * 2), int(pad_y * 2 + lh * len(lines) + gap * (len(lines) - 1))
    img = Image.new("RGBA", (bw, bh), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((0, 0, bw - 1, bh - 1), radius=min(bh // 2, 28), fill=(30, 30, 30, 225))
    for i, l in enumerate(lines):
        lw = d.textlength(l, font=f)
        d.text(((bw - lw) / 2, pad_y + i * (lh + gap) - 2), l, font=f, fill=(255, 255, 255, 255))
    img.save(path)
    return bw, bh


def badge(app, path):
    """The app's icon and name, top left, as GNOME's top bar shows the
    focused app."""
    from PIL import Image, ImageDraw
    name = APPS[app][0]
    f = font(34, 700)
    tw = int(ImageDraw.Draw(Image.new("RGBA", (1, 1))).textlength(name, font=f))
    img = Image.new("RGBA", (64 + 16 + tw, 64), (0, 0, 0, 0))
    img.alpha_composite(icon(app, 64), (0, 0))
    ImageDraw.Draw(img).text((80, 10), name, font=f, fill=(255, 255, 255, 240))
    img.save(path)


def window_frame(w, h, mask_path, shadow_path, radius=14):
    """The rounded-corner mask for a window of w×h, and its shadow (the
    shadow image is the canvas-sized layer, offset and blurred)."""
    from PIL import Image, ImageDraw, ImageFilter
    mask = Image.new("L", (w, h), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, w - 1, h - 1), radius=radius, fill=255)
    mask.save(mask_path)
    pad = 80
    shadow = Image.new("RGBA", (w + pad * 2, h + pad * 2), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle((pad, pad + 14, pad + w, pad + h + 14), radius=radius, fill=(0, 0, 0, 120))
    shadow.filter(ImageFilter.GaussianBlur(28)).save(shadow_path)
    return pad


# ── Recording ────────────────────────────────────────────────────────

class Recorder:
    """ffmpeg grabbing the whole X display, with the clock of its first
    frame, so tour events can be placed on the recording."""

    def __init__(self, path):
        self.path = path
        size = subprocess.run(["xdpyinfo"], capture_output=True, text=True).stdout
        m = re.search(r"dimensions:\s+(\d+)x(\d+)", size)
        self.size = (int(m.group(1)), int(m.group(2)))
        self.proc = subprocess.Popen(
            ["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-framerate", str(FPS),
             "-video_size", f"{self.size[0]}x{self.size[1]}", "-i", os.environ["DISPLAY"],
             "-c:v", "libx264", "-preset", "ultrafast", "-crf", "14", "-pix_fmt", "yuv420p", path],
            stdin=subprocess.PIPE)
        # The first frame's moment: the file has data once ffmpeg is grabbing.
        end = time.monotonic() + 10
        while time.monotonic() < end and (not os.path.exists(path) or os.path.getsize(path) < 1024):
            time.sleep(0.05)
        self.t0 = time.monotonic() - 0.2

    def stop(self):
        self.proc.communicate(b"q", timeout=60)


def tour(app, stops, rec):
    """Run `app`'s stops, returning one segment per feature that reached
    the screen: (name, caption, start, end, crop box), times relative to
    the recording."""
    marks = {}
    original_init = ft.App.__init__

    def init(self, *a, **k):
        original_init(self, *a, **k)
        marks["ready"] = time.monotonic()

    def shot(name):
        import mss
        from PIL import Image, ImageChops
        with mss.MSS() as sct:
            raw = sct.grab(sct.monitors[1])
        img = Image.frombytes("RGB", raw.size, raw.rgb)
        root = Image.new("RGB", img.size, img.getpixel((img.width - 1, img.height - 1)))
        box = ImageChops.difference(img, root).convert("L").point(lambda v: 255 if v > 8 else 0).getbbox()
        marks["shot"] = (time.monotonic(), box)
        time.sleep(HOLD)

    ft.App.__init__, ft.shot = init, shot
    segments = []
    try:
        for (_app, name, caption, fn) in stops:
            marks.clear()
            try:
                fn()
            except Exception as e:  # noqa: BLE001
                print(f"overview: {name} failed ({e}); left out", file=sys.stderr)
                for proc in ft.RUNNING:
                    if proc.poll() is None:
                        proc.kill()
                ft.RUNNING.clear()
                time.sleep(1)
                continue
            if "shot" not in marks or not marks["shot"][1]:
                continue
            t_shot, box = marks["shot"]
            start = max(marks.get("ready", t_shot - LEAD), t_shot - LEAD) - rec.t0
            segments.append((name, caption, max(0.0, start), t_shot + HOLD - 0.3 - rec.t0, box))
    finally:
        ft.App.__init__ = original_init
    return segments


# ── Composition ──────────────────────────────────────────────────────

def even(n):
    return int(n) // 2 * 2


def render_segment(app, raw, seg, out, work):
    name, caption, start, end, box = seg
    x0, y0, x1, y1 = box
    cw, ch = even(x1 - x0), even(y1 - y0)
    # The window fits the canvas above the caption.
    scale = min(1560 / cw, 820 / ch)
    sw, sh = even(cw * scale), even(ch * scale)
    wx, wy = (W - sw) // 2, 120 + (820 - sh) // 2
    bg, mask, shadow, cap, bdg = (f"{work}/{name}-{k}.png" for k in ("bg", "mask", "shadow", "cap", "badge"))
    gradient(*APPS[app][1]).convert("RGB").save(bg)
    pad = window_frame(sw, sh, mask, shadow)
    bw, bh = caption_pill(caption, cap)
    badge(app, bdg)
    dur = end - start
    filt = (
        f"[0:v]crop={cw}:{ch}:{x0}:{y0},scale={sw}:{sh}:flags=lanczos,format=rgba[win];"
        f"[2:v]format=gray[m];[win][m]alphamerge[wr];"
        f"[1:v][3:v]overlay={wx - pad}:{wy - pad}[b1];"
        f"[b1][wr]overlay={wx}:{wy}[b2];"
        f"[b2][5:v]overlay=48:36[b3];"
        f"[4:v]format=rgba,fade=t=in:st=0.3:d=0.4:alpha=1[c];"
        f"[b3][c]overlay=(W-{bw})/2:{H - bh - 48}:format=auto,format=yuv420p[v]"
    )
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-ss", f"{start:.2f}", "-t", f"{dur:.2f}", "-i", raw,
         "-loop", "1", "-i", bg, "-loop", "1", "-i", mask, "-loop", "1", "-i", shadow,
         "-loop", "1", "-i", cap, "-loop", "1", "-i", bdg,
         "-filter_complex", filt, "-map", "[v]", "-t", f"{dur:.2f}", "-r", str(FPS),
         "-c:v", "libx264", "-preset", "medium", "-crf", "18", out], check=True)
    return dur


def still(png, seconds, out, zoom=True):
    """A card as a clip, with a slow push-in."""
    z = "zoompan=z='min(zoom+0.0006,1.04)':d=1:x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':s=1920x1080:fps=30," if zoom else ""
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-loop", "1", "-i", png, "-t", f"{seconds}",
                    "-vf", f"scale=2112:1188,{z}format=yuv420p", "-r", str(FPS),
                    "-c:v", "libx264", "-preset", "medium", "-crf", "18", out], check=True)
    return seconds


def join(clips, out):
    """Clips joined with crossfades, faded in from and out to black."""
    inputs, filt, last, offset = [], [], "[0:v]", 0.0
    for c, _ in clips:
        inputs += ["-i", c]
    for i in range(1, len(clips)):
        offset += clips[i - 1][1] - FADE
        filt.append(f"{last}[{i}:v]xfade=transition=fade:duration={FADE}:offset={offset:.2f}[x{i}]")
        last = f"[x{i}]"
    total = sum(d for _, d in clips) - FADE * (len(clips) - 1)
    filt.append(f"{last}fade=t=in:d=0.6,fade=t=out:st={total - 0.8:.2f}:d=0.8,format=yuv420p[v]")
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", *inputs, "-filter_complex", ";".join(filt),
                    "-map", "[v]", "-c:v", "libx264", "-preset", "slow", "-crf", "20",
                    "-movflags", "+faststart", out], check=True)
    print(f"overview: {out} ({total:.0f}s)")


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out = os.path.abspath(sys.argv[1])
    wanted = sys.argv[2:] or [*APPS, "suite"]
    os.makedirs(out, exist_ok=True)
    work = tempfile.mkdtemp(prefix="overview-")
    ft.OUT = work
    rendered = {}
    for app in APPS:
        if app not in wanted and "suite" not in wanted:
            continue
        stops = [s for s in ft.STOPS if s[0] == app]
        raw = f"{work}/{app}-raw.mkv"
        rec = Recorder(raw)
        try:
            segments = tour(app, stops, rec)
        finally:
            rec.stop()
        clips = []
        for seg in segments:
            clip = f"{work}/{seg[0]}.mp4"
            clips.append((clip, render_segment(app, raw, seg, clip, work)))
        rendered[app] = {seg[0]: c for seg, c in zip(segments, clips)}
        if app in wanted:
            card = f"{work}/{app}-title.png"
            title_card(app, card)
            outro = f"{work}/outro.png"
            suite_card(outro, "GTK Office Suite", "A word processor, a spreadsheet and a presentation app for GNOME")
            join([(f"{work}/{app}-title.mp4", still(card, 3.5, f"{work}/{app}-title.mp4")), *clips,
                  (f"{work}/{app}-outro.mp4", still(outro, 4.0, f"{work}/{app}-outro.mp4"))],
                 f"{out}/{app}.mp4")
    if "suite" in wanted:
        intro = f"{work}/suite-intro.png"
        suite_card(intro, "GTK Office Suite", "A word processor, a spreadsheet and a presentation app for GNOME")
        clips = [(f"{work}/suite-intro.mp4", still(intro, 4.0, f"{work}/suite-intro.mp4"))]
        for app, names in TRAILER.items():
            card = f"{work}/{app}-title.png"
            title_card(app, card)
            clips.append((f"{work}/{app}-t2.mp4", still(card, 2.5, f"{work}/{app}-t2.mp4")))
            clips += [rendered[app][n] for n in names if n in rendered.get(app, {})]
        join(clips, f"{out}/gtk-office-suite.mp4")


if __name__ == "__main__":
    main()
