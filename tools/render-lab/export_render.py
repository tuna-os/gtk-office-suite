#!/usr/bin/env python3
"""Export our own PDF for every opted-in fixture and rasterize it like the
LibreOffice reference, so the two PDFs compare pixel for pixel.

For each manifest entry with `"export": true` (docx/pptx only;
docs/EXPORT-PARITY-SPEC.md), the app's headless `--export-pdf` hook writes
`export.pdf` into the fixture's output directory, and the same
`pdftoppm -r 96` invocation as lo_render.py rasterizes it to `ours-<n>.png`.

Usage: export_render.py <fixtures-dir> <out-dir> [--app APP]
Needs the binaries in target/debug (or $RENDER_LAB_BIN); runs headless
under Xvfb like capture.py's Tier A.
"""

import argparse
import glob
import json
import os
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import capture

# Apps with a headless --export-pdf hook (spec item 1). xlsx is out of
# scope, so Tables has no hook and an export:true tables fixture is a
# manifest bug, reported loudly rather than silently skipped.
EXPORT_APPS = ("letters", "decks")
DPI = 96


def rasterize_ours(pdf, dest_dir):
    """The same pdftoppm -r 96 invocation as lo_render.py, naming ours-<n>."""
    os.makedirs(dest_dir, exist_ok=True)
    for old in glob.glob(os.path.join(dest_dir, "ours-*.png")):
        os.remove(old)
    subprocess.run(
        ["pdftoppm", "-r", str(DPI), "-png", pdf, os.path.join(dest_dir, "ours")],
        check=True,
        capture_output=True,
    )
    # pdftoppm zero-pads by page count (ours-1 / ours-01); normalize.
    pages = sorted(glob.glob(os.path.join(dest_dir, "ours-*.png")))
    for i, p in enumerate(pages, start=1):
        os.rename(p, os.path.join(dest_dir, f"ours-{i}.tmp"))
    for i in range(1, len(pages) + 1):
        os.rename(os.path.join(dest_dir, f"ours-{i}.tmp"), os.path.join(dest_dir, f"ours-{i}.png"))
    return len(pages)


def export_one(app, doc, dest, env):
    """Run the app's --export-pdf hook under Xvfb; returns page count or None."""
    for old in glob.glob(os.path.join(dest, "ours-*.png")) + [os.path.join(dest, "export.pdf")]:
        if os.path.exists(old):
            os.remove(old)
    pdf = os.path.join(dest, "export.pdf")
    # Same window seeding as Tier A, so the settled layout the exporter
    # draws is the one the screenshots judge.
    capture.seed_settings(env["HOME"], extra=2 * capture.SOLID_CSD_BORDER)
    w, h = capture.window(app)
    xvfb = capture.start(
        ["Xvfb", ":71", "-screen", "0", f"{w + 100}x{h + 100}x24", "-nolisten", "tcp"], env, subprocess.DEVNULL
    )
    time.sleep(1)
    e = dict(env, DISPLAY=":71", GDK_BACKEND="x11", GSK_RENDERER="cairo")
    pages = None
    with open(os.path.join(dest, "export.log"), "w") as log:
        p = capture.start([os.path.join(capture.BIN, app), doc, "--export-pdf", pdf], e, log)
        try:
            p.wait(capture.TIMEOUT)
        except subprocess.TimeoutExpired:
            log.write("render-lab: export timed out\n")
        finally:
            capture.stop(p)
            capture.stop(xvfb)
    if os.path.exists(pdf):
        try:
            pages = rasterize_ours(pdf, dest)
        except subprocess.CalledProcessError as ex:
            print(f"export {app}: pdftoppm failed: {ex}", file=sys.stderr)
            pages = None
    return pages


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("fixtures")
    ap.add_argument("out")
    ap.add_argument("--app")
    args = ap.parse_args()

    manifest = json.load(open(os.path.join(args.fixtures, "manifest.json")))
    wanted = [fx for fx in manifest if fx.get("export")]
    if args.app:
        wanted = [fx for fx in wanted if fx["app"] == args.app]
    bad = [f"{fx['app']}/{fx['feature']}" for fx in wanted if fx["app"] not in EXPORT_APPS]
    if bad:
        sys.exit(f"export_render: export:true on an app with no --export-pdf hook (xlsx is out of scope): {bad}")

    home = tempfile.mkdtemp(prefix="render-lab-export-")
    env = capture.base_env(home)
    failures = 0
    for fx in wanted:
        key = f"{fx['app']}/{fx['feature']}"
        dest = os.path.join(args.out, fx["app"], fx["feature"])
        os.makedirs(dest, exist_ok=True)
        try:
            n = export_one(fx["app"], os.path.join(args.fixtures, fx["file"]), dest, env)
            if n:
                print(f"export {key}: {n} page(s)")
            else:
                failures += 1
                print(f"export {key}: FAILED (no PDF; see export.log)", file=sys.stderr)
        except Exception as ex:  # keep going; the report shows the gap
            failures += 1
            print(f"export {key}: FAILED {ex}", file=sys.stderr)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
