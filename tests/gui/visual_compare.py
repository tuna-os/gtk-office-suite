"""Compare a visual-matrix screenshot with its expected image (#1284).

A cell is one app in one state under one display config, named
`<app>-<state>` inside `<config>`, e.g. `w800-dark-s2/tables-selection`.
Expected images live in `tests/gui/visual/expected/<config>/<cell>.png`,
stored at half resolution: a regression big enough to matter survives the
downscale, and the matrix's 216 images then fit in a few megabytes. The
screenshot is downscaled the same way before it is compared.

Two pixels differ when any channel differs by more than TOLERANCE (out of
255), which absorbs antialiasing noise; a cell fails when the fraction of
differing pixels exceeds its threshold, the default or the cell's entry in
`visual/thresholds.json`. A failed cell keeps expected, actual and a diff
(differing pixels in red over a faded copy of the actual image) in the
output directory, with the app's state snapshot beside them.
"""

import json
import os
import shutil

GUI_DIR = os.path.dirname(os.path.abspath(__file__))
VISUAL_DIR = os.path.join(GUI_DIR, "visual")
EXPECTED_DIR = os.path.join(VISUAL_DIR, "expected")
THRESHOLDS = os.path.join(VISUAL_DIR, "thresholds.json")
TOLERANCE = 32
DOWNSCALE = 2

WIDTHS = {400: "400x800", 800: "800x600", 1280: "1280x1024"}
THEMES = ("light", "dark", "hc")
SCALES = (1, 2)
APPS = ("letters", "tables", "decks")
STATES = ("editor", "selection", "dialog", "error")


def config_env(width, theme, scale):
    """The harness environment for one display config."""
    env = {
        "GUI_TEST_SCREEN_SIZE": WIDTHS[width],
        "GUI_TEST_COLOR_SCHEME": "prefer-dark" if theme == "dark" else "prefer-light",
        "GDK_SCALE": str(scale),
    }
    if theme == "hc":
        env["GUI_TEST_HIGH_CONTRAST"] = "1"
    return env


def configs():
    """Every display config: (name, env)."""
    return [
        (f"w{w}-{theme}-s{scale}", config_env(w, theme, scale))
        for w in WIDTHS for theme in THEMES for scale in SCALES
    ]


def config_name(env=None):
    """The name of the config the harness is running under."""
    env = os.environ if env is None else env
    width = int(env.get("GUI_TEST_SCREEN_SIZE", "1920x1080").split("x")[0])
    if env.get("GUI_TEST_HIGH_CONTRAST") == "1":
        theme = "hc"
    elif env.get("GUI_TEST_COLOR_SCHEME") == "prefer-dark":
        theme = "dark"
    else:
        theme = "light"
    return f"w{width}-{theme}-s{env.get('GDK_SCALE', '1')}"


def cells():
    return [f"{app}-{state}" for app in APPS for state in STATES]


def expected_path(config, cell):
    return os.path.join(EXPECTED_DIR, config, f"{cell}.png")


def out_dir(config):
    root = os.environ.get("GUI_VISUAL_OUT") or os.path.join(VISUAL_DIR, "out")
    return os.path.join(root, config)


def threshold(config, cell):
    with open(THRESHOLDS) as f:
        table = json.load(f)
    return table.get("cells", {}).get(f"{config}/{cell}", table["default"])


def _reduced(path):
    from PIL import Image
    img = Image.open(path).convert("RGB")
    return img.resize((max(1, img.width // DOWNSCALE), max(1, img.height // DOWNSCALE)), Image.Resampling.BOX)


def check(config, cell, actual):
    """Compare (or, with GUI_VISUAL_UPDATE=1, record) one cell. Returns a
    result dict: ok, message, differing fraction and the evidence paths."""
    from PIL import Image, ImageChops
    want = expected_path(config, cell)
    got = _reduced(actual)
    if os.environ.get("GUI_VISUAL_UPDATE") == "1":
        os.makedirs(os.path.dirname(want), exist_ok=True)
        got.save(want, optimize=True)
        return {"ok": True, "message": "expected image written", "expected": want, "actual": actual}
    if not os.path.exists(want):
        return {"ok": False, "message": f"no expected image ({os.path.relpath(want, GUI_DIR)}); "
                                        "run with GUI_VISUAL_UPDATE=1 to record one", "actual": actual}
    exp = Image.open(want).convert("RGB")
    out = os.path.dirname(actual)
    kept = os.path.join(out, f"{cell}.expected.png")
    shutil.copyfile(want, kept)
    if exp.size != got.size:
        return {"ok": False, "message": f"size {got.size} != expected {exp.size}", "expected": kept, "actual": actual}
    diff = ImageChops.difference(exp, got)
    r, g, b = diff.split()
    worst = ImageChops.lighter(ImageChops.lighter(r, g), b)
    mask = worst.point(lambda v: 255 if v > TOLERANCE else 0)
    differing = sum(1 for v in mask.getdata() if v)
    fraction = differing / (got.width * got.height)
    limit = threshold(config, cell)
    result = {"ok": fraction <= limit, "fraction": round(fraction, 6), "threshold": limit,
              "expected": kept, "actual": actual}
    if differing:
        faded = Image.blend(got, Image.new("RGB", got.size, (255, 255, 255)), 0.7)
        faded.paste((255, 0, 0), mask=mask)
        result["diff"] = os.path.join(out, f"{cell}.diff.png")
        faded.save(result["diff"])
    result["message"] = f"{fraction:.4%} of pixels differ (threshold {limit:.4%})"
    return result
