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

SCHEMA_DIR="${GSETTINGS_SCHEMA_DIR:-/tmp/gtk-office-schemas}"
mkdir -p "$SCHEMA_DIR"
cp "$REPO_ROOT"/flatpak/*.gschema.xml "$SCHEMA_DIR/"
glib-compile-schemas "$SCHEMA_DIR"
export GSETTINGS_SCHEMA_DIR="$SCHEMA_DIR"
export GDK_BACKEND=x11
export GTK_A11Y=atspi
export GTK_MODULES=gail:atk-bridge

Xvfb :96 -screen 0 1600x1000x24 &
XVFB_PID=$!
trap 'kill $XVFB_PID 2>/dev/null || true' EXIT
export DISPLAY=:96
sleep 1

dbus-run-session -- bash -c '
    gsettings set org.gnome.desktop.interface toolkit-accessibility true
    exec /usr/bin/python3 "'"$REPO_ROOT"'/tests/gui/walkthrough.py" "'"$OUTDIR"'"
'
