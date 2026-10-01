#!/usr/bin/env bash
# Capture the README/docs walkthrough screenshots under Xvfb, then the
# per-feature screenshots for docs/features/ (feature_tour.py).
# Usage: tests/gui/capture_walkthrough.sh <output-dir>
# Requires built binaries (cargo build --bin letters --bin tables --bin decks).
#
#   FEATURE_TOUR_DIR    where the feature screenshots go (docs/features/img)
#   FEATURE_TOUR_ONLY   space-separated stop-name prefixes to capture only
#                       those, e.g. "letters-toc tables-"
#   WALKTHROUGH=0       skip the README walkthrough, run only the tour
#   GUI_TEST_PYTHON     the Python with dogtail (default /usr/bin/python3)
set -euo pipefail

OUTDIR="${1:?usage: capture_walkthrough.sh <output-dir>}"
mkdir -p "$OUTDIR"
OUTDIR="$(cd "$OUTDIR" && pwd)"

cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

# A private per-run directory unless one is given, as in run_gui_tests.sh:
# a fixed /tmp name can be created first by another user (#819).
SCHEMA_TMP=""
if [ -n "${GSETTINGS_SCHEMA_DIR:-}" ]; then
    SCHEMA_DIR="$GSETTINGS_SCHEMA_DIR"
    mkdir -p "$SCHEMA_DIR"
else
    SCHEMA_TMP="$(mktemp -d -t gtk-office-schemas-XXXXXXXX)"
    SCHEMA_DIR="$SCHEMA_TMP"
fi
cp "$REPO_ROOT"/flatpak/*.gschema.xml "$SCHEMA_DIR/"
glib-compile-schemas "$SCHEMA_DIR"
export GSETTINGS_SCHEMA_DIR="$SCHEMA_DIR"
export GDK_BACKEND=x11
export GTK_A11Y=atspi
export GTK_MODULES=gail:atk-bridge

# Demo documents: the markdown one is checked in; the binary formats are
# generated from the core crates so they always match the current code.
DEMO_DIR="${WALKTHROUGH_DEMO_DIR:-$(mktemp -d)}"
export WALKTHROUGH_DEMO_DIR="$DEMO_DIR"
cp "$REPO_ROOT/tests/gui/demo/quarterly-report.md" "$DEMO_DIR/"
(cd "$REPO_ROOT" && cargo run -q -p tables-core --example make_demo_xlsx -- "$DEMO_DIR/demo.xlsx")
(cd "$REPO_ROOT" && cargo run -q -p decks-core --example make_demo_pptx -- "$DEMO_DIR/demo.pptx")

Xvfb :96 -screen 0 1600x1000x24 &
XVFB_PID=$!
trap 'kill $XVFB_PID 2>/dev/null || true; [ -z "$SCHEMA_TMP" ] || rm -rf "$SCHEMA_TMP"' EXIT
export DISPLAY=:96
sleep 1

# A compositor, so a popover's rounded corners and shadow are drawn over
# the window behind it: on a bare X server they come out as a solid black
# box. Optional, so a machine without one still captures (uglier) shots.
if command -v xcompmgr >/dev/null; then
    xcompmgr >/dev/null 2>&1 &
    COMPOSITOR_PID=$!
    trap 'kill ${COMPOSITOR_PID:-} $XVFB_PID 2>/dev/null || true; [ -z "$SCHEMA_TMP" ] || rm -rf "$SCHEMA_TMP"' EXIT
else
    echo "xcompmgr not found: popovers will show black corners" >&2
fi

# Settings of their own, so every app starts at its schema defaults
# (window size, toolbar shown, light style) whatever this machine's user
# last chose.
CONFIG_TMP="$(mktemp -d -t gtk-office-capture-config-XXXXXXXX)"
export XDG_CONFIG_HOME="$CONFIG_TMP" XDG_DATA_HOME="$CONFIG_TMP/data" XDG_STATE_HOME="$CONFIG_TMP/state"
export GSETTINGS_BACKEND=keyfile

PYTHON_BIN="${GUI_TEST_PYTHON:-/usr/bin/python3}"
TOUR_DIR="${FEATURE_TOUR_DIR:-$REPO_ROOT/docs/features/img}"
mkdir -p "$TOUR_DIR"
export PYTHON_BIN OUTDIR TOUR_DIR REPO_ROOT
export WALKTHROUGH="${WALKTHROUGH:-1}" FEATURE_TOUR_ONLY="${FEATURE_TOUR_ONLY:-}"

# The tour's exit status is how many of its stops failed: a feature that
# could not be reached costs the run, not just its own screenshot.
dbus-run-session -- bash -c '
    gsettings set org.gnome.desktop.interface toolkit-accessibility true
    if [ "$WALKTHROUGH" != 0 ]; then
        "$PYTHON_BIN" "$REPO_ROOT/tests/gui/walkthrough.py" "$OUTDIR" || exit 1
    fi
    # shellcheck disable=SC2086
    exec "$PYTHON_BIN" "$REPO_ROOT/tests/gui/feature_tour.py" "$TOUR_DIR" $FEATURE_TOUR_ONLY
'
