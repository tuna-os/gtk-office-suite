---
name: vision-check
description: Run vision-based GUI verification on Flatpak-built office suite apps using Gemini or Lemonade VLM. Launches the app, takes screenshots, sends them to a vision model for structural pass/fail assertions, and reports results.
---

# Vision Check — GUI Verification for the Hanthor Office Suite

Uses a Vision Language Model (Gemini 2.5 Flash or Gemma-4-31B via Lemonade) to verify the rendered state of the three office apps (Letters, Decks, Tables). Tests are written in Python using `dogtail` for AT-SPI automation and the `assertVision()` framework in `tests/gui/framework/base.py`.

## Prerequisites

- Flatpak-built apps installed (run `just flatpak-build` from the project root)
- Python 3.13+ with `dogtail`, `mss`, `Pillow`, `requests`
- A Gemini API key set via `GEMINI_API_KEY` environment variable (or use the Lemonade fallback)
- An X11/Wayland display (Xvfb works for headless CI)

## Quick Start

```bash
# Build and install Flatpaks (run from the gtk-office-suite checkout)
cd "$(git rev-parse --show-toplevel)"
flatpak run org.flatpak.Builder --force-clean --install --user --ccache \
  --disable-rofiles-fuse build-dir flatpak/org.tunaos.letters.json

# Run all GUI tests with Gemini backend
cd tests/gui
VLM_BACKEND=gemini python3 -m pytest test_letters.py -v --tb=short
```

## How It Works

Each test method in `test_letters.py`, `test_decks.py`, and `test_tables.py`:

1. Launches the Flatpak app via `flatpak run org.tunaos.<app>`
2. Uses `dogtail` to find widgets by their AT-SPI accessible names
3. Performs an action (click button, type text, press shortcut)
4. Takes a screenshot via `mss` cropped to the app window
5. Sends the screenshot + structured assertions to the VLM
6. Parses the "Result: Pass." / "Result: Fail." responses
7. Asserts all checks pass

## Available Test Suites

| File | Tests | What It Covers |
|------|-------|----------------|
| `test_letters.py` | 31 | Word processor features |
| `test_decks.py` | 18 | Presentation features |
| `test_tables.py` | 21 | Spreadsheet features |
| `test_design_gnome_hig.py` | 4 | Design reviews per app |
| `test_gnome_hig_compliance.py` | 26 | Rigorous GNOME HIG audits |
| **Total** | **100** | |

## VLM Backends

Set `VLM_BACKEND` environment variable to choose:

| Backend | Env Value | Model | Speed | Cost |
|---------|-----------|-------|-------|------|
| Gemini (recommended) | `gemini` | `gemini-2.5-flash` | ~4s | Free tier (1500 req/day) |
| Lemonade (local) | `lemonade` | `Gemma-4-31B-it-GGUF` | ~12s | Free (self-hosted) |

## Adwaita Icon Verification

The toolbar icons must use only icons from the Adwaita symbolic icon theme.
Valid icons are in `/usr/share/icons/Adwaita/symbolic/actions/`.

To check if an icon exists:
```bash
find /usr/share/icons/Adwaita -name "*icon-name*"
```

Known valid formatting icons:
- `format-text-bold-symbolic`, `format-text-italic-symbolic`, `format-text-underline-symbolic`
- `format-text-strikethrough-symbolic`, `color-select-symbolic`
- `view-list-bullet-symbolic`, `view-list-ordered-symbolic`
- `format-justify-left-symbolic`, `format-justify-center-symbolic`
- `format-justify-right-symbolic`, `format-justify-fill-symbolic`
- `insert-link-symbolic`, `view-continuous-symbolic`, `view-dual-symbolic`

## Example: Running a Single Vision Check

```python
import base64, json, requests
from io import BytesIO
from PIL import Image

img = Image.open("screenshot.png")
img.thumbnail((800, 600), Image.LANCZOS)
buf = BytesIO()
img.save(buf, format="JPEG", quality=70)
b64 = base64.b64encode(buf.getvalue()).decode()

resp = requests.post(
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent"
    "?key=YOUR_API_KEY",
    json={"contents": [{"parts": [
        {"inline_data": {"mime_type": "image/jpeg", "data": b64}},
        {"text": "Does this screenshot show a text editor with bold/italic/underline icons? Yes/no."},
    ]}]},
    timeout=30,
)
print(resp.json()["candidates"][0]["content"]["parts"][0]["text"])
```

## Assertion Format

The VLM expects assertion checks in this format:
```json
[
  {"id": "check-name", "assertion": "Description of what should be visible"}
]
```

And responds with:
```
Result: Pass. Evidence: ...
Result: Fail. Evidence: ...
```
