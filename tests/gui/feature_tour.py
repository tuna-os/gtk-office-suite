#!/usr/bin/env python3
"""Drive Letters, Tables and Decks through their features and capture one
screenshot per feature for the feature overviews in docs/features/.

Runs in the same Xvfb + AT-SPI environment as walkthrough.py
(tests/gui/capture_walkthrough.sh sets it up, generates the demo documents
and calls this after the walkthrough). Every screenshot is of the running
app: features are reached the way a user reaches them, through the app's
own actions, keyboard shortcuts and buttons, never by drawing anything.

Each stop is independent: one that fails is reported and the tour goes on,
so a broken feature costs its own screenshot, not everyone else's. The
exit status is the number of stops that failed, and the list of stops
(STOPS, filled in by @stop) is what tests/test_feature_docs.py holds the
documentation to.

Usage: feature_tour.py <output-dir> [stop-name-prefix ...]
"""

import os
import subprocess
import sys
import time
import traceback

OUT = sys.argv[1] if len(sys.argv) > 1 else None
ONLY = sys.argv[2:]
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEMO = os.environ.get("WALKTHROUGH_DEMO_DIR", f"{REPO}/tests/gui/demo")
TARGET = os.environ.get("CARGO_TARGET_DIR", f"{REPO}/target")

# Windows open at the origin of the bare Xvfb display at their default
# 960x680 size; the docs show the window, not the empty screen around it.
WINDOW = (0, 0, 962, 682)

# Every stop, in tour order: (app, name, caption). The docs embed
# docs/features/img/<name>.png for each, and the test holds them to it.
STOPS = []

# The apps this tour started, so a stop that fails can close its own app
# without killing anything by name (tests/test_gui_harness.py).
RUNNING = []


def stop(app, name, caption):
    def register(fn):
        STOPS.append((app, name, caption, fn))
        return fn
    return register


# ── Helpers ──────────────────────────────────────────────────────────

def shot(name):
    """The app's window as it is on screen, cropped to the window. The
    root window around it is one flat colour (black on a bare X server,
    grey under the compositor), and the window opens away from the bottom
    right corner, so everything that differs from that corner's colour is
    the window, with any popover or dialog over it."""
    import mss
    from PIL import Image, ImageChops
    path = f"{OUT}/{name}.png"
    with mss.MSS() as sct:
        sct.shot(output=path)
    img = Image.open(path).convert("RGB")
    root = Image.new("RGB", img.size, img.getpixel((img.width - 1, img.height - 1)))
    box = ImageChops.difference(img, root).convert("L").point(lambda v: 255 if v > 8 else 0).getbbox() or WINDOW
    img.crop(box).save(path, optimize=True)
    print(f"captured {name}.png ({box[2] - box[0]}x{box[3] - box[1]})")


def wait_for(predicate, timeout=10.0, what="a condition"):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            value = predicate()
        except Exception:
            value = None
        if value:
            return value
        time.sleep(0.2)
    raise TimeoutError(f"timed out waiting for {what}")


class App:
    """One running app, launched fresh with its own settings."""

    def __init__(self, name, app_id, document=None, env=None):
        self.name, self.app_id = name, app_id
        argv = [f"{TARGET}/debug/{name}"] + ([document] if document else [])
        self.proc = subprocess.Popen(argv, env={**os.environ, **(env or {})})
        RUNNING.append(self.proc)
        from dogtail import tree
        self.node = wait_for(lambda: tree.root.application(name), 30, f"{name} on the accessibility bus")
        wait_for(lambda: self.node.findChild(lambda n: n.roleName == "frame", retry=False, requireResult=False),
                 30, f"{name}'s window")
        time.sleep(1.5)

    def action(self, action, settle=1.0):
        subprocess.run(["gapplication", "action", self.app_id, action], check=False)
        time.sleep(settle)

    def find(self, name, role=None, timeout=10.0):
        from dogtail import tree
        match = lambda n: n.name == name and n.showing and (role is None or n.roleName == role)
        return wait_for(lambda: tree.root.findChild(match, retry=False, requireResult=False),
                        timeout, f"{role or 'node'} {name!r}")

    def press(self, name, role="push button", settle=1.0):
        self.find(name, role).do_action(0)
        time.sleep(settle)

    def close(self):
        self.proc.terminate()
        try:
            self.proc.wait(10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
        time.sleep(1)


def keys(*combos, settle=0.3):
    from dogtail import rawinput
    for combo in combos:
        rawinput.keyCombo(combo)
        time.sleep(settle)


def type_text(text, settle=0.5):
    from dogtail import rawinput
    rawinput.typeText(text)
    time.sleep(settle)


def letters(document=f"{DEMO}/quarterly-report.md", env=None):
    return App("letters", "org.tunaos.letters", document, env)


def tables(document=f"{DEMO}/demo.xlsx", env=None):
    return App("tables", "org.tunaos.tables", document, env)


def decks(document=f"{DEMO}/demo.pptx", env=None):
    return App("decks", "org.tunaos.decks", document, env)


# ── Letters ──────────────────────────────────────────────────────────

@stop("letters", "letters-document", "A document in Print Layout: headings, emphasis, lists, a block quote and a link")
def _(_=None):
    app = letters()
    shot("letters-document")
    app.close()


@stop("letters", "letters-styles", "The paragraph-style picker, each style drawn as it will look on the page")
def _(_=None):
    app = letters()
    keys("<Control>Home")
    app.press("Paragraph Style", "toggle button", settle=1.5)
    shot("letters-styles")
    app.close()


@stop("letters", "letters-outline", "The outline sidebar lists the headings; choosing one moves to it")
def _(_=None):
    app = letters()
    app.action("toggle-outline", settle=1.5)
    shot("letters-outline")
    app.close()


@stop("letters", "letters-palette", "Ctrl+K: every command, searchable by name, with its shortcut")
def _(_=None):
    app = letters()
    keys("<Control>k", settle=1.5)
    type_text("list", settle=1.0)
    shot("letters-palette")
    app.close()


@stop("letters", "letters-find", "Find and Replace, with every match highlighted in the document")
def _(_=None):
    app = letters()
    app.action("find", settle=1.0)
    type_text("quarter", settle=1.5)
    shot("letters-find")
    app.close()


@stop("letters", "letters-track-changes", "Track Changes: insertions and deletions marked by author, reviewed in the sidebar")
def _(_=None):
    app = letters()
    app.action("track-changes", settle=1.0)
    # Strike the last word ("audit") and type its replacement.
    keys("<Control>End", "<Control><Shift>Left", "Delete", settle=0.5)
    type_text("review before year end", settle=0.5)
    app.action("toggle-changes", settle=1.5)
    shot("letters-track-changes")
    app.close()


@stop("letters", "letters-comments", "Comments anchored to text, with replies and Resolve, in the Comments sidebar")
def _(_=None):
    app = letters()
    keys("<Control>Home", "Down", "Down", "Down", "Down", "<Control>Right", "<Control>Right", "<Control><Shift>Right")
    app.action("add-comment", settle=1.0)
    type_text("Is this the audited figure?", settle=0.5)
    app.press("Comment", settle=1.5)
    app.action("show-comments", settle=1.5)
    shot("letters-comments")
    app.close()


@stop("letters", "letters-toc", "A table of contents generated from the headings, with page numbers")
def _(_=None):
    app = letters()
    keys("<Control>Home")
    app.action("insert-toc", settle=2.0)
    shot("letters-toc")
    app.close()


@stop("letters", "letters-smart-chips", "Smart chips: \"@\" offers dates, people and links as inline objects")
def _(_=None):
    app = letters()
    keys("<Control>End", "Return")
    type_text("Review on @", settle=1.5)
    shot("letters-smart-chips")
    app.close()


@stop("letters", "letters-insert-table", "Insert Table puts a table at the caret; its cells are edited in place")
def _(_=None):
    app = letters()
    # Two Returns: the first starts a list item, the second ends the list.
    keys("<Control>End", "Return", "Return")
    app.action("insert-table", settle=1.5)
    type_text("Region", settle=0.5)
    shot("letters-insert-table")
    app.close()


@stop("letters", "letters-headers", "Headers and footers, with page-number fields")
def _(_=None):
    app = letters()
    app.action("edit-headers", settle=1.5)
    shot("letters-headers")
    app.close()


@stop("letters", "letters-dark", "Dark style follows the desktop; the page stays paper-white")
def _(_=None):
    app = letters(env={"ADW_DEBUG_COLOR_SCHEME": "prefer-dark"})
    shot("letters-dark")
    app.close()


# ── Tables ───────────────────────────────────────────────────────────

def goto(cell):
    """Ctrl+G to a cell, then Escape to hand the keyboard to the grid."""
    keys("<Control>g", settle=0.4)
    type_text(cell, settle=0.2)
    keys("Return", settle=0.4)
    keys("Escape", settle=0.4)


@stop("tables", "tables-grid", "A workbook with formulas; selecting a range shows its Sum, Average and Count")
def _(_=None):
    app = tables()
    goto("C6")
    for _ in range(4):
        keys("<Shift>Up", settle=0.2)
    time.sleep(1.0)
    shot("tables-grid")
    app.close()


@stop("tables", "tables-formula-autocomplete", "Typing a function name offers the functions it could be, with their signatures")
def _(_=None):
    app = tables()
    keys("<Control>g", settle=0.4)
    type_text("E2", settle=0.2)
    keys("Return", settle=0.4)
    type_text("=AV", settle=1.5)
    shot("tables-formula-autocomplete")
    app.close()


@stop("tables", "tables-format-inspector", "The Format sidebar: font, fill, borders, alignment and number format for the selection")
def _(_=None):
    app = tables()
    goto("B2")
    keys("<Shift>Down", "<Shift>Down", "<Shift>Right")
    app.press("Format", "toggle button", settle=1.5)
    shot("tables-format-inspector")
    app.close()


@stop("tables", "tables-column-menu", "Alt+Down on a cell opens its column's menu: sort, and filter by value")
def _(_=None):
    app = tables()
    goto("A3")
    keys("<Alt>Down", settle=1.5)
    shot("tables-column-menu")
    app.close()


@stop("tables", "tables-insert-chart", "Insert Chart: the active cell's column against the labels in column A, previewed in each kind")
def _(_=None):
    app = tables()
    goto("C2")
    app.action("insert-chart", settle=2.0)
    shot("tables-insert-chart")
    app.close()


@stop("tables", "tables-conditional-format", "Conditional formatting: colour cells by a rule on their value")
def _(_=None):
    app = tables()
    goto("D2")
    keys("<Shift>Down", "<Shift>Down", "<Shift>Down")
    app.action("conditional-format", settle=1.5)
    shot("tables-conditional-format")
    app.close()


@stop("tables", "tables-number-format", "Number formats: built-in kinds or a custom format code, previewed on the cell")
def _(_=None):
    app = tables()
    goto("D2")
    app.action("edit-number-format", settle=1.5)
    shot("tables-number-format")
    app.close()


@stop("tables", "tables-define-name", "Named ranges: name a range, then jump to it or use it in formulas")
def _(_=None):
    app = tables()
    goto("B2")
    keys("<Shift>Down", "<Shift>Down", "<Shift>Down")
    app.action("define-name", settle=1.5)
    type_text("Q1Sales", settle=0.5)
    shot("tables-define-name")
    app.close()


@stop("tables", "tables-note", "Notes on cells, shown on hover and read out by screen readers")
def _(_=None):
    app = tables()
    goto("C3")
    app.action("edit-note", settle=1.5)
    type_text("Includes the late June order.", settle=0.5)
    shot("tables-note")
    app.close()


@stop("tables", "tables-sheets", "Several sheets: add, rename, reorder and delete them from the sheet bar, all undoable")
def _(_=None):
    app = tables()
    app.action("add-sheet", settle=1.0)
    app.action("add-sheet", settle=1.0)
    from dogtail import tree
    switcher = wait_for(lambda: tree.root.application("tables").findChild(
        lambda n: n.roleName == "combo box" and n.showing, retry=False, requireResult=False), 10, "the sheet switcher")
    switcher.findChild(lambda n: n.roleName == "toggle button", retry=False).do_action(0)
    time.sleep(1.5)
    shot("tables-sheets")
    app.close()


@stop("tables", "tables-dark", "Tables in the dark style")
def _(_=None):
    app = tables(env={"ADW_DEBUG_COLOR_SCHEME": "prefer-dark"})
    goto("B2")
    keys("<Shift>Down", "<Shift>Down", "<Shift>Down", settle=0.5)
    shot("tables-dark")
    app.close()


# ── Decks ────────────────────────────────────────────────────────────

def go_to_slide(app, index):
    subprocess.run(["gapplication", "action", app.app_id, "go-to-slide", f"uint32 {index}"], check=False)
    time.sleep(1.0)


@stop("decks", "decks-editor", "The editor: slide thumbnails, the slide, speaker notes, and the presenter controls")
def _(_=None):
    app = decks()
    go_to_slide(app, 1)
    shot("decks-editor")
    app.close()


@stop("decks", "decks-format", "The Format sidebar for a slide: its layout, background and transition")
def _(_=None):
    app = decks()
    go_to_slide(app, 1)
    app.press("Format", "toggle button", settle=1.5)
    shot("decks-format")
    app.close()


@stop("decks", "decks-insert-shape", "Insert Shape: a searchable library, each shape drawn as it will appear")
def _(_=None):
    app = decks()
    go_to_slide(app, 1)
    app.press("Insert Shape", "toggle button", settle=1.5)
    shot("decks-insert-shape")
    app.close()


@stop("decks", "decks-insert-chart", "Insert Chart: each kind drawn with sample data before it is chosen")
def _(_=None):
    app = decks()
    go_to_slide(app, 2)
    app.press("Insert Chart", "toggle button", settle=1.5)
    shot("decks-insert-chart")
    app.close()


@stop("decks", "decks-chart", "A chart on a slide, its type and data edited in the Format sidebar")
def _(_=None):
    app = decks()
    go_to_slide(app, 2)
    app.press("Add Slide", settle=1.0)
    app.press("Insert Chart", "toggle button", settle=1.0)
    app.press("Pie", settle=1.5)
    app.press("Format", "toggle button", settle=1.5)
    shot("decks-chart")
    app.close()


@stop("decks", "decks-notes", "Speaker notes under each slide, saved with the deck")
def _(_=None):
    app = decks()
    go_to_slide(app, 1)
    app.action("focus-notes", settle=0.8)
    keys("<Control>End")
    type_text(" Mention the June enterprise deals first.", settle=1.0)
    shot("decks-notes")
    app.close()


@stop("decks", "decks-master", "Edit Master: shapes and styles on the master appear on every slide that uses it")
def _(_=None):
    app = decks()
    app.action("edit-master", settle=2.0)
    # A decoration on the master, so there is something to see on it.
    app.press("Insert Shape", "toggle button", settle=1.0)
    app.press("Rounded Rectangle", settle=1.5)
    shot("decks-master")
    app.close()


@stop("decks", "decks-templates", "New From Template: a theme chooser, each theme previewed")
def _(_=None):
    app = decks()
    app.action("new-from-template", settle=2.0)
    shot("decks-templates")
    app.close()


@stop("decks", "decks-rehearse", "Rehearse: the presenter view with the current and next slide, notes and a clock")
def _(_=None):
    app = decks()
    go_to_slide(app, 1)
    app.action("rehearse", settle=3.0)
    shot("decks-rehearse")
    keys("Escape", settle=1.0)
    app.close()


@stop("decks", "decks-dark", "Decks in the dark style")
def _(_=None):
    app = decks(env={"ADW_DEBUG_COLOR_SCHEME": "prefer-dark"})
    go_to_slide(app, 1)
    shot("decks-dark")
    app.close()


def main():
    if not OUT:
        sys.exit(__doc__)
    os.makedirs(OUT, exist_ok=True)
    failed, ran = [], 0
    for app, name, caption, fn in STOPS:
        if ONLY and not any(name.startswith(p) for p in ONLY):
            continue
        ran += 1
        try:
            fn()
        except Exception:
            failed.append(name)
            print(f"FAILED {name}", file=sys.stderr)
            traceback.print_exc()
            for proc in RUNNING:
                if proc.poll() is None:
                    proc.kill()
                    proc.wait()
            RUNNING.clear()
            time.sleep(1)
    print(f"feature tour: {ran - len(failed)} captured, {len(failed)} failed {failed}")
    sys.exit(len(failed))


if __name__ == "__main__":
    main()
