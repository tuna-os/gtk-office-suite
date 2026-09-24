#!/usr/bin/env python3
"""Capture what our apps draw for each fixture.

Tier A  app under Xvfb; its test-render-dump action writes A-<n>.png from
        GTK's own render nodes (suite_common::render_dump).
Tier B  app under gtk4-broadwayd; headless Chromium screenshots the
        Broadway page and the document area is cropped using geom.json.

Usage: capture.py <fixtures-dir> <out-dir> [--app APP] [--tier A|B]...
Needs the binaries in target/debug (or $RENDER_LAB_BIN) and compiled
GSettings schemas (made on the fly from flatpak/*.gschema.xml).
"""

import argparse
import glob
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
BIN = os.environ.get("RENDER_LAB_BIN", os.path.join(REPO, "target", "debug"))
APP_IDS = {"letters": "org.tunaos.letters", "tables": "org.tunaos.tables", "decks": "org.tunaos.decks"}
# Big enough that a whole A4/Letter page at the app's fit-width scale and a
# 16:9 slide are fully on screen.
WINDOW = (1100, 1700)
TIMEOUT = 60
SOLID_CSD_BORDER = 5  # px per side, GTK 4.14 on X11 without compositing
# Tier B's crop must match the app's own render (A-1.png from the same
# process) this closely, in mean absolute grey levels, or it is rejected.
MAX_LOCATE_DIFF = 4.0


def schemas_dir():
    d = os.path.join(tempfile.gettempdir(), "render-lab-schemas")
    os.makedirs(d, exist_ok=True)
    for f in glob.glob(os.path.join(REPO, "flatpak", "*.gschema.xml")):
        shutil.copy(f, d)
    subprocess.run(["glib-compile-schemas", d], check=True)
    return d


def base_env(home):
    env = dict(os.environ)
    env.update(
        GSETTINGS_SCHEMA_DIR=schemas_dir(),
        GSETTINGS_BACKEND="keyfile",
        GTK_OFFICE_TEST_MODE="1",
        HOME=home,
        XDG_CONFIG_HOME=os.path.join(home, "config"),
        XDG_DATA_HOME=os.path.join(home, "data"),
        XDG_CACHE_HOME=os.path.join(home, "cache"),
        XDG_STATE_HOME=os.path.join(home, "state"),
        GTK_A11Y="none",
        ADW_DISABLE_PORTAL="1",
        NO_AT_BRIDGE="1",
        LANG="C.UTF-8",
    )
    return env


def seed_settings(home, maximized=False, extra=0):
    """Window size comes from each app's GSettings. Seed a window tall
    enough for a whole A4/Letter page at the apps' fit-width scale, so the
    capture is never cut off by the viewport.

    Broadway (Tier B) needs `maximized`: GDK's Broadway backend starts with
    a hard-coded 1024x768 monitor and the app presents its window before
    broadwayd's real screen size arrives, so a plain default size is clamped
    to 1024x768 and never grows back. Maximized toplevels are the one case
    GDK resizes when the screen size arrives (gdkdisplay-broadway.c,
    _gdk_broadway_display_size_changed), and the browser viewport is exactly
    WINDOW, so both tiers lay the document out in the same 1100x1700."""
    d = os.path.join(home, "config", "glib-2.0", "settings")
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, "keyfile"), "w") as f:
        for app in APP_IDS.values():
            path = "/".join(app.split("."))
            f.write(
                f"[{path}]\nwindow-width={WINDOW[0] + extra}\nwindow-height={WINDOW[1] + extra}\n"
                f"window-maximized={'true' if maximized else 'false'}\n\n"
            )


def locate(template_path, screen_path, size=None, hint=None):
    """Top-left of `template` inside `screen` and its mean absolute error
    in grey levels, or None if the template doesn't fit.

    Tier B uses the app's own render (written by the same Broadway process)
    as the template. `hint` is where geom.json says the page is: the window
    is maximized at the browser's origin, so that is normally exact, and
    it is tried first (with a few px of slack). Otherwise the page is
    searched for by FFT cross-correlation of zero-mean grayscale at half
    resolution, refined at full resolution. Correlation alone is unreliable
    for near-uniform templates (a plain dark slide matched 73 px off), so
    whichever candidate has the lower pixel error wins."""
    import numpy as np
    from PIL import Image

    def gray(p, k, centre=True):
        g = Image.open(p).convert("L")
        if size and p == template_path:
            # Match at on-screen size (a no-op when the template is S-1).
            g = g.resize((round(size[0]), round(size[1])))
        g = g.resize((max(1, g.width // k), max(1, g.height // k)))
        a = np.asarray(g, dtype=np.float64)
        return a - a.mean() if centre else a

    # Score on raw grey levels: zero-mean images from different crops have
    # different means, which would add a constant to the score.
    tf, sf = gray(template_path, 1, False), gray(screen_path, 1, False)
    if tf.shape[0] > sf.shape[0] or tf.shape[1] > sf.shape[1]:
        return None
    best = [None, None]

    def around(x, y, r):
        for dy in range(-r, r + 1):
            for dx in range(-r, r + 1):
                X, Y = x + dx, y + dy
                if X < 0 or Y < 0 or Y + tf.shape[0] > sf.shape[0] or X + tf.shape[1] > sf.shape[1]:
                    continue
                d = np.abs(sf[Y : Y + tf.shape[0], X : X + tf.shape[1]] - tf).mean()
                if best[0] is None or d < best[0]:
                    best[0], best[1] = d, (X, Y)

    if hint is not None:
        around(round(hint[0]), round(hint[1]), 3)
        if best[0] is not None and best[0] < 1.0:
            return best[1], best[0]

    k = 2
    t, sc = gray(template_path, k), gray(screen_path, k)
    shape = (sc.shape[0] + t.shape[0], sc.shape[1] + t.shape[1])
    corr = np.fft.irfft2(np.fft.rfft2(sc, shape) * np.conj(np.fft.rfft2(t, shape)), shape)
    corr = corr[: sc.shape[0] - t.shape[0] + 1, : sc.shape[1] - t.shape[1] + 1]
    y, x = np.unravel_index(np.argmax(corr), corr.shape)
    around(x * k, y * k, k)
    return best[1], best[0]


def start(argv, env, log):
    return subprocess.Popen(argv, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)


def stop(proc):
    if proc and proc.poll() is None:
        os.killpg(proc.pid, signal.SIGTERM)
        try:
            proc.wait(5)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)


def app_argv(app, doc):
    return [os.path.join(BIN, app), doc]


def tier_a(app, doc, dest, env):
    # Under Xvfb with no window manager GTK draws "solid CSD": a 5 px
    # resize border *inside* the requested window size (a WM doesn't remove
    # it; neither does GTK_CSD=0). Ask for that much more, so the window's
    # content is WINDOW exactly, as in Tier B. Otherwise a landscape Letters
    # page, fitted to width, differs by 1% in scale between the tiers. The
    # A-vs-B agreement metric catches it if this ever stops being true.
    seed_settings(env["HOME"], extra=2 * SOLID_CSD_BORDER)
    for old in glob.glob(os.path.join(dest, "A-*.png")) + glob.glob(os.path.join(dest, "S-*.png")):
        os.remove(old)
    xvfb = start(["Xvfb", ":71", "-screen", "0", f"{WINDOW[0] + 100}x{WINDOW[1] + 100}x24", "-nolisten", "tcp"], env, subprocess.DEVNULL)
    time.sleep(1)
    e = dict(env, DISPLAY=":71", GDK_BACKEND="x11", GSK_RENDERER="cairo", GTK_OFFICE_RENDER_DUMP=dest)
    with open(os.path.join(dest, "A.log"), "w") as log:
        p = start(app_argv(app, doc), e, log)
        try:
            p.wait(TIMEOUT)
        except subprocess.TimeoutExpired:
            log.write("render-lab: timed out\n")
        finally:
            stop(p)
            stop(xvfb)
    return len(glob.glob(os.path.join(dest, "A-*.png")))


def tier_b(app, doc, dest, env, browser):
    from PIL import Image

    seed_settings(env["HOME"], maximized=True)
    for old in glob.glob(os.path.join(dest, "B-*.png")):  # includes B-template.png
        os.remove(old)
    gdir = tempfile.mkdtemp()
    port = 8080 + 5
    broadwayd = shutil.which("gtk4-broadwayd") or "broadwayd"
    bw = start([broadwayd, "--port", str(port), ":5"], env, subprocess.DEVNULL)
    time.sleep(1)
    e = dict(env, GDK_BACKEND="broadway", BROADWAY_DISPLAY=":5", GTK_OFFICE_RENDER_DUMP=gdir, GTK_OFFICE_RENDER_HOLD="1")
    shots = 0
    p = None
    with open(os.path.join(dest, "B.log"), "w") as log:
        # Connect the browser *before* the app starts: until a client is
        # attached, broadwayd reports a 1024x768 screen and GTK clamps the
        # window to it, so the page would be cut off at the bottom.
        page = browser.new_page(viewport={"width": WINDOW[0], "height": WINDOW[1]})
        try:
            page.goto(f"http://127.0.0.1:{port}/")
            page.wait_for_load_state("networkidle")
            time.sleep(0.5)
            p = start(app_argv(app, doc), e, log)
            geom_path = os.path.join(gdir, "geom.json")
            deadline = time.time() + TIMEOUT
            while not os.path.exists(geom_path) and time.time() < deadline:
                time.sleep(0.5)
            if not os.path.exists(geom_path):
                log.write("render-lab: app never wrote geom.json\n")
                return 0
            time.sleep(1.5)  # let the last frame reach the browser
            full = os.path.join(dest, "B-full.png")
            page.screenshot(path=full, full_page=True)
            geoms = json.load(open(geom_path))
            # The on-screen template (S-1, Decks) or, where the app's Tier A
            # is already the on-screen region (Letters, Tables), A-1.
            screen, template = os.path.join(gdir, "S-1.png"), os.path.join(gdir, "A-1.png")
            deadline = time.time() + 10
            while not os.path.exists(template) and time.time() < deadline:
                time.sleep(0.5)
            if os.path.exists(screen):
                template = screen
            found = locate(template, full, geoms[0][2:], geoms[0][:2]) if os.path.exists(template) else None
            if found is None:
                log.write("render-lab: could not locate the page in the Broadway screenshot\n")
                return 0
            found, mad = found
            log.write(f"render-lab: page found at {found}, mean abs diff {mad:.2f} grey levels\n")
            if mad > MAX_LOCATE_DIFF:
                # The best match is not the page (it is cut off, covered, or
                # never drawn); cropping there would score the wrong pixels.
                log.write("render-lab: no good match for the page; Tier B not captured (template kept as B-template.png)\n")
                shutil.copy(template, os.path.join(dest, "B-template.png"))
                return 0
            img = Image.open(full)
            x0, y0, w0, h0 = geoms[0]
            if found[0] + w0 > img.width + 1 or found[1] + h0 > img.height + 1:
                log.write(f"render-lab: page {geoms[0]} does not fit the screenshot {img.size}\n")
                return 0
            for i, (x, y, w, h) in enumerate(geoms, start=1):
                left, top = found[0] + (x - x0), found[1] + (y - y0)
                box = tuple(round(v) for v in (left, top, left + w, top + h))
                if box[2] > img.width + 1 or box[3] > img.height + 1:
                    # Scrolled out of view: a browser shows only what is on
                    # screen, so Tier B scores the visible pages and Tier A
                    # judges the rest (and the page count).
                    log.write(f"render-lab: pages {i}-{len(geoms)} are below the viewport; not captured\n")
                    break
                img.crop(box).save(os.path.join(dest, f"B-{i}.png"))
                shots += 1
        finally:
            page.close()
            stop(p)
            stop(bw)
            shutil.rmtree(gdir, ignore_errors=True)
    return shots


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("fixtures")
    ap.add_argument("out")
    ap.add_argument("--app")
    ap.add_argument("--tier", action="append", choices=["A", "B"])
    args = ap.parse_args()
    tiers = args.tier or ["A", "B"]

    manifest = json.load(open(os.path.join(args.fixtures, "manifest.json")))
    home = tempfile.mkdtemp(prefix="render-lab-home-")
    env = base_env(home)

    pw = browser = None
    if "B" in tiers:
        try:
            from playwright.sync_api import sync_playwright

            pw = sync_playwright().start()
            browser = pw.chromium.launch()
        except Exception as e:
            print(f"Tier B unavailable ({e}); skipping", file=sys.stderr)
            tiers = [t for t in tiers if t != "B"]

    for fx in manifest:
        if args.app and fx["app"] != args.app:
            continue
        doc = os.path.abspath(os.path.join(args.fixtures, fx["file"]))
        dest = os.path.abspath(os.path.join(args.out, fx["app"], fx["feature"]))
        os.makedirs(dest, exist_ok=True)
        res = []
        if "A" in tiers:
            res.append(f"A={tier_a(fx['app'], doc, dest, env)}")
        if "B" in tiers:
            res.append(f"B={tier_b(fx['app'], doc, dest, env, browser)}")
        print(f"cap {fx['app']}/{fx['feature']}: {' '.join(res)} page(s)")

    if browser:
        browser.close()
        pw.stop()
    shutil.rmtree(home, ignore_errors=True)


if __name__ == "__main__":
    main()
