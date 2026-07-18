#!/usr/bin/env python3
"""Drive each app through a small scripted walkthrough and capture the
screenshots embedded in README.md / docs/screenshots/.

Runs inside the same Xvfb + AT-SPI environment as the smoke tests
(tests/gui/capture_walkthrough.sh sets that up). Interaction is via
GioActions (gapplication) and keyboard only — AT-SPI extents are not
reliable for canvas widgets, so no coordinate clicking except the one
Decks object-select click, whose geometry is deterministic under the
fixed 960x680 default window on a bare Xvfb display.

Usage: walkthrough.py <output-dir>
"""

import subprocess
import sys
import time

OUT = sys.argv[1]
REPO = subprocess.run(
    ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True
).stdout.strip()

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


def launch(app):
    proc = subprocess.Popen([f"{REPO}/target/debug/{app}"])
    time.sleep(4)
    return proc


# ── Tables: data, a formula, and a live range selection ──────────────
tables = launch("tables")
app_action("org.tunaos.tables-rust", "new-document")
time.sleep(1.5)


def put(ref, value):
    rawinput.keyCombo("<Control>g")
    time.sleep(0.2)
    rawinput.typeText(ref)
    rawinput.keyCombo("Return")
    time.sleep(0.3)
    rawinput.typeText(value)
    rawinput.keyCombo("Return")
    time.sleep(0.3)


put("A1", "Region")
put("B1", "Revenue")
put("A2", "North")
put("B2", "1200")
put("A3", "South")
put("B3", "950")
put("A4", "East")
put("B4", "1430")
put("B5", "=SUM(B2:B4)")
for _ in range(3):
    rawinput.keyCombo("<Shift>Up")
time.sleep(1.0)
shot("tables")
tables.terminate()
time.sleep(1)

# ── Letters: styled document, then the command palette ───────────────
letters = launch("letters")
rawinput.keyCombo("<Control>n")
time.sleep(2)
rawinput.typeText("Quarterly Report")
rawinput.keyCombo("<Control>a")
time.sleep(0.3)
app_action("org.tunaos.letters-rust", "style-h1")
time.sleep(0.3)
rawinput.keyCombo("End")
rawinput.keyCombo("Return")
rawinput.typeText("Revenue grew in every region this quarter.")
time.sleep(1.0)
shot("letters")
rawinput.keyCombo("<Control>k")
time.sleep(1.5)
shot("letters-palette")
letters.terminate()
time.sleep(1)

# ── Decks: objects, inspector, presenter pill ────────────────────────
decks = launch("decks")
app_action("org.tunaos.decks-rust", "new-document")
time.sleep(1.5)
app_action("org.tunaos.decks-rust", "add-text-box")
app_action("org.tunaos.decks-rust", "add-shape")
time.sleep(1.0)
# Select the shape so the inspector shows real values (default Rect at
# slide 200,200 → this screen point under the default window layout).
rawinput.click(417, 372)
time.sleep(1.0)
shot("decks")
decks.terminate()
print("walkthrough complete")
