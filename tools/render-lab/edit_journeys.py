#!/usr/bin/env python3
"""Add each app's edited-and-saved document to the fixture manifest (#1201).

A fixture proves we draw what we read. This proves we write what we draw:
tests/gui/test_edit_render.py edits a new document through the real GUI
(type, bold, a bullet / a cell format / a shape), saves it with Save As
and reopens it. Run here with EDIT_RENDER_OUT pointing into the fixtures
directory, each saved file joins manifest.json as `<app>/edited-journey`,
so the rest of the lab treats it like any fixture: LibreOffice renders the
saved file (lo_render.py), our app opens it (capture.py), and compare.py
scores and ratchets the pair.

A journey that fails leaves its fixture out of the manifest; the ratchet
then reports the baselined verdicts as gone to `missing`, which fails the
lab, so a broken save path cannot pass by producing no file.

The report job, which compares every app's captures without re-running
the labs, uses --manifest-only: the entries alone, for compare.py to look
up the captures each lab uploaded.

Usage: edit_journeys.py <fixtures-dir> [--app APP] [--manifest-only]
"""

import argparse
import json
import os
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

JOURNEYS = {
    "letters": ("LettersEditRenderJourney", "docx",
                "A paragraph ending in two bold words, then a bulleted item, as typed and saved by Letters"),
    "tables": ("TablesEditRenderJourney", "xlsx",
               "Item/Amount header in bold over two rows (Paper 12, Ink 30), as entered and saved by Tables"),
    "decks": ("DecksEditRenderJourney", "pptx",
              "One slide with an added text box and an added shape, as saved by Decks"),
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("fixtures")
    ap.add_argument("--app")
    ap.add_argument("--manifest-only", action="store_true")
    args = ap.parse_args()

    fixtures = os.path.abspath(args.fixtures)
    manifest_path = os.path.join(fixtures, "manifest.json")
    manifest = json.load(open(manifest_path))
    manifest = [fx for fx in manifest if fx["feature"] != "edited-journey"]
    out = os.path.join(fixtures, "edited")
    failures = 0
    for app, (journey, ext, expect) in JOURNEYS.items():
        if args.app and app != args.app:
            continue
        saved = os.path.join(out, app, f"edited-journey.{ext}")
        if not args.manifest_only:
            if os.path.exists(saved):
                os.remove(saved)
            env = dict(os.environ, EDIT_RENDER_OUT=out)
            run = subprocess.run(
                [os.path.join(REPO, "tests/gui/run_gui_tests.sh"), "test_edit_render.py", "-k", journey],
                env=env, cwd=REPO,
            )
            if run.returncode != 0 or not os.path.exists(saved):
                failures += 1
                print(f"edit {app}: journey failed; no edited-journey fixture", file=sys.stderr)
                continue
        manifest.append({
            "app": app,
            "feature": "edited-journey",
            "file": os.path.relpath(saved, fixtures),
            "expect": expect,
            "needs": None,
        })
        print(f"edit {app}: {os.path.relpath(saved, fixtures)}")
    with open(manifest_path, "w") as f:
        json.dump(manifest, f, indent=2)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
