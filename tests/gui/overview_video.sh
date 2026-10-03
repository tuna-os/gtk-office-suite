#!/usr/bin/env bash
# Record the overview videos (overview_video.py) in the feature tour's
# environment: Xvfb with a compositor, AT-SPI, a private settings store and
# the demo documents. The apps run at 2x on a 3200x2000 display, so the
# windows stay sharp when the videos scale them to 1080p.
#
#   tests/gui/overview_video.sh <out-dir> [letters|tables|decks|suite ...]
#
# Needs ffmpeg, rsvg-convert and, for GNOME's typeface, Adwaita Sans
# (fetched into ~/.local/share/fonts when fontconfig has no copy).
set -euo pipefail
OUT="$(realpath -m "$1")"; shift
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
PYTHON_BIN="${GUI_TEST_PYTHON:-python3}"

if ! fc-list | grep -qi "Adwaita Sans"; then
    fonts="$HOME/.local/share/fonts"; mkdir -p "$fonts"
    tmp="$(mktemp -d)"
    curl -fsSL https://download.gnome.org/sources/adwaita-fonts/51/adwaita-fonts-51.0.tar.xz | tar xJ -C "$tmp"
    find "$tmp" -name 'AdwaitaSans-*.ttf' -exec cp {} "$fonts/" \;
    fc-cache -f >/dev/null
fi

SCHEMA_DIR="$(mktemp -d)"; cp "$REPO"/flatpak/*.gschema.xml "$SCHEMA_DIR/"; glib-compile-schemas "$SCHEMA_DIR"
export GSETTINGS_SCHEMA_DIR="$SCHEMA_DIR" GDK_BACKEND=x11 GTK_A11Y=atspi GDK_SCALE=2
DEMO_DIR="$(mktemp -d)"; export WALKTHROUGH_DEMO_DIR="$DEMO_DIR"
cp "$REPO/tests/gui/demo/quarterly-report.md" "$DEMO_DIR/"
(cd "$REPO" && cargo run -q -p tables-core --example make_demo_xlsx -- "$DEMO_DIR/demo.xlsx" \
    && cargo run -q -p decks-core --example make_demo_pptx -- "$DEMO_DIR/demo.pptx")

Xvfb :95 -screen 0 3200x2000x24 & XP=$!
export DISPLAY=:95; sleep 1
xcompmgr >/dev/null 2>&1 & CP=$!
trap 'kill $CP $XP 2>/dev/null || true' EXIT
CFG="$(mktemp -d)"; export XDG_CONFIG_HOME="$CFG" XDG_DATA_HOME="$CFG/data" XDG_STATE_HOME="$CFG/state" GSETTINGS_BACKEND=keyfile
export OUT REPO PYTHON_BIN
dbus-run-session -- bash -c 'gsettings set org.gnome.desktop.interface toolkit-accessibility true
    gsettings set org.gnome.desktop.interface font-name "Adwaita Sans 11"
    exec "$PYTHON_BIN" "$REPO/tests/gui/overview_video.py" "$OUT" '"$*"
