#!/usr/bin/env bash
# Capture the README/docs walkthrough screenshots under Xvfb.
# Usage: tests/gui/capture_walkthrough.sh <output-dir>
# Requires built binaries (cargo build --bin letters --bin tables --bin decks).
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

dbus-run-session -- bash -c '
    gsettings set org.gnome.desktop.interface toolkit-accessibility true
    exec /usr/bin/python3 "'"$REPO_ROOT"'/tests/gui/walkthrough.py" "'"$OUTDIR"'"
'
