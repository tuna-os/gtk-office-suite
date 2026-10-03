#!/usr/bin/env python3
"""Capture every surface of Letters, Tables and Decks for a design audit
against the GNOME HIG: each app's window (wide and narrow), its main menu,
and whatever each of its application actions opens (dialogs, popovers,
windows).

Runs in the feature tour's environment (Xvfb, a compositor, AT-SPI, a
private settings store): see hig_audit.sh. Actions are listed from the
running app over D-Bus (org.gtk.Actions.DescribeAll), so a new dialog is
audited without anyone adding it here. Each action runs in a fresh app, so
one that leaves a modal open or changes the document cannot spoil the
next; a screenshot is kept only when the action changed what is on screen.

Usage: hig_audit.py <output-dir> [app ...]
Writes <output-dir>/<app>/<surface>.png and <output-dir>/index.json.
"""

import json
import os
import re
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import feature_tour as ft  # noqa: E402

OUT = sys.argv[1] if len(sys.argv) > 1 else None
ONLY = sys.argv[2:]

APPS = {
    "letters": ("org.tunaos.letters", ft.letters),
    "tables": ("org.tunaos.tables", ft.tables),
    "decks": ("org.tunaos.decks", ft.decks),
}

# Actions that end the app, write files, or need a parameter the audit
# can't supply. Everything else is opened.
SKIP = re.compile(r"^(quit|close.*|save.*|test-.*|undo|redo|.*autosave.*|clear-recent-files|open-recent.*)$")
ONLY_ACTIONS = set(os.environ.get("HIG_ACTIONS", "").split())


def actions(app_id):
    """The app's enabled, parameterless application actions."""
    path = "/" + app_id.replace(".", "/")
    out = subprocess.run(
        ["gdbus", "call", "--session", "--dest", app_id, "--object-path", path,
         "--method", "org.gtk.Actions.DescribeAll"],
        capture_output=True, text=True, check=False).stdout
    with open(f"{OUT}/{app_id}.actions.txt", "w") as f:
        f.write(out)
    found = re.findall(r"'([\w.-]+)': \((true|false), (?:signature )?'([^']*)'", out)
    return sorted(name for name, enabled, sig in found if enabled == "true" and sig == "" and not SKIP.match(name))


def screen():
    import mss
    from PIL import Image
    with mss.MSS() as sct:
        raw = sct.grab(sct.monitors[1])
    return Image.frombytes("RGB", raw.size, raw.rgb)


def changed(a, b):
    """How much of the screen differs between two screenshots (0..1)."""
    from PIL import ImageChops
    diff = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 24 else 0)
    return diff.histogram()[255] / (a.width * a.height)


def save(img, app, name):
    from PIL import Image, ImageChops
    os.makedirs(f"{OUT}/{app}", exist_ok=True)
    root = Image.new("RGB", img.size, img.getpixel((img.width - 1, img.height - 1)))
    box = ImageChops.difference(img, root).convert("L").point(lambda v: 255 if v > 8 else 0).getbbox()
    (img.crop(box) if box else img).save(f"{OUT}/{app}/{name}.png", optimize=True)
    return f"{app}/{name}.png"


def main():
    if not OUT:
        sys.exit(__doc__)
    os.makedirs(OUT, exist_ok=True)
    index = []
    for app, (app_id, launch) in APPS.items():
        if ONLY and app not in ONLY:
            continue
        a = launch()
        names = actions(app_id)
        base = screen()
        index.append({"app": app, "surface": "window", "file": save(base, app, "window")})
        # The main menu, as a user opens it.
        try:
            ft.keys("F10", settle=1.0)
            index.append({"app": app, "surface": "main-menu", "file": save(screen(), app, "main-menu")})
        except Exception as e:  # noqa: BLE001
            print(f"{app}: main menu: {e}", file=sys.stderr)
        a.close()
        print(f"{app}: {len(names)} actions: {' '.join(names)}")
        for name in ([] if os.environ.get("HIG_LIST_ONLY") else [n for n in names if not ONLY_ACTIONS or n in ONLY_ACTIONS]):
            a = launch()
            before = screen()
            a.action(name, settle=2.0)
            after = screen()
            delta = changed(before, after)
            entry = {"app": app, "surface": name, "changed": round(delta, 4)}
            if delta > 0.002:
                entry["file"] = save(after, app, name)
            index.append(entry)
            print(f"{app}.{name}: {delta:.3f}")
            a.close()
            # A window the action opened that outlives the app's main
            # window (print dialogs run in-process, so close takes them).
    with open(f"{OUT}/index.json", "w") as f:
        json.dump(index, f, indent=1)


if __name__ == "__main__":
    main()
