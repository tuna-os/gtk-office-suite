#!/usr/bin/env python3
"""Drive each app through a small scripted walkthrough and capture the
screenshots embedded in README.md / docs/screenshots/.

Runs inside the same Xvfb + AT-SPI environment as the smoke tests
(tests/gui/capture_walkthrough.sh sets that up, and pre-generates the
demo documents in $WALKTHROUGH_DEMO_DIR). Interaction is via GioActions
(gapplication) and keyboard only — AT-SPI extents are not reliable for
canvas widgets, so no coordinate clicking except the one Decks
object-select click, whose geometry is deterministic under the fixed
960x680 default window on a bare Xvfb display.

Usage: walkthrough.py <output-dir>
"""

import os
import subprocess
import sys
import time

OUT = sys.argv[1]
REPO = subprocess.run(
    ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True
).stdout.strip()
DEMO = os.environ.get("WALKTHROUGH_DEMO_DIR", f"{REPO}/tests/gui/demo")

from dogtail import rawinput  # noqa: E402  (needs DISPLAY at import)
import mss  # noqa: E402
from PIL import Image  # noqa: E402

# Windows open at the origin of the bare Xvfb display at their default
# 960x680 size; crop the dead screen area off for the docs.
WINDOW = (0, 0, 962, 682)


def shot(name):
    path = f"{OUT}/{name}.png"
    with mss.MSS() as sct:
        sct.shot(output=path)
    img = Image.open(path)
    img.crop(WINDOW).save(path)
    print(f"captured {name}.png")


def app_action(app_id, action):
    subprocess.run(["gapplication", "action", app_id, action], check=False)


def launch(app, document=None):
    argv = [f"{REPO}/target/debug/{app}"]
    if document:
        argv.append(document)
    proc = subprocess.Popen(argv)
    time.sleep(4)
    return proc


# ── Tables: demo workbook with formulas + a live range selection ─────
tables = launch("tables", f"{DEMO}/demo.xlsx")
time.sleep(1.5)
# Jump to the Q2 column and select the numeric range so the status area
# shows Sum/Avg/Count.
rawinput.keyCombo("<Control>g")
time.sleep(0.3)
rawinput.typeText("C6")
rawinput.keyCombo("Return")
time.sleep(0.5)
for _ in range(4):
    rawinput.keyCombo("<Shift>Up")
time.sleep(1.0)
shot("tables")
tables.terminate()
time.sleep(1)

# ── Letters: demo report, then the command palette ───────────────────
letters = launch("letters", f"{DEMO}/quarterly-report.md")
time.sleep(2)
shot("letters")
rawinput.keyCombo("<Control>k")
time.sleep(1.5)
shot("letters-palette")
letters.terminate()
time.sleep(1)

# ── Decks: demo deck with real slides, notes, inspector ──────────────
decks = launch("decks", f"{DEMO}/demo.pptx")
time.sleep(2)
shot("decks")
decks.terminate()
print("walkthrough complete")
