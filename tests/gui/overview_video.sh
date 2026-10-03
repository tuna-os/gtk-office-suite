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

# Adwaita Sans, GNOME's typeface since GNOME 48, for the apps and the cards.
# It goes into the private data directory below as well: fontconfig looks
# in $XDG_DATA_HOME/fonts, and that directory replaces ~/.local/share.
FONTS="${ADWAITA_FONTS_DIR:-$HOME/.cache/gtk-office-suite/adwaita-fonts}"
if ! ls "$FONTS"/AdwaitaSans-*.ttf >/dev/null 2>&1; then
    mkdir -p "$FONTS"; tmp="$(mktemp -d)"
    curl -fsSL https://download.gnome.org/sources/adwaita-fonts/51/adwaita-fonts-51.0.tar.xz | tar xJ -C "$tmp"
    find "$tmp" -name 'AdwaitaSans-*.ttf' -exec cp {} "$FONTS/" \;
fi
export ADWAITA_FONTS_DIR="$FONTS"

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
mkdir -p "$CFG/data/fonts"; cp "$FONTS"/AdwaitaSans-*.ttf "$CFG/data/fonts/"; fc-cache -f "$CFG/data/fonts" >/dev/null
export OUT REPO PYTHON_BIN
dbus-run-session -- bash -c 'gsettings set org.gnome.desktop.interface toolkit-accessibility true
    gsettings set org.gnome.desktop.interface font-name "Adwaita Sans 11"
    # Larger windows than the tour'"'"'s screenshots use, so a page fits
    # across with room around it at 1080p.
    for app in letters tables decks; do
        gsettings set org.tunaos.$app window-width 1440
        gsettings set org.tunaos.$app window-height 900
    done
    exec "$PYTHON_BIN" "$REPO/tests/gui/overview_video.py" "$OUT" '"$*"
