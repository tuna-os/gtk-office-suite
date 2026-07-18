#!/usr/bin/env bash
# Launch one app under Xvfb and capture a screenshot for visual validation.
# Usage: tests/gui/screenshot_app.sh <letters|tables|decks> <out.png> [settle-seconds]
# Requires the binary to be built (cargo build --bin <app>).
set -euo pipefail

APP="${1:?usage: screenshot_app.sh <letters|tables|decks> <out.png> [settle-seconds]}"
OUT="${2:?output png path required}"
SETTLE="${3:-3}"

cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"
BIN="$REPO_ROOT/target/debug/$APP"
[ -x "$BIN" ] || { echo "Binary not built: $BIN (run cargo build --bin $APP)"; exit 1; }

SCHEMA_DIR="${GSETTINGS_SCHEMA_DIR:-/tmp/gtk-office-schemas}"
mkdir -p "$SCHEMA_DIR"
cp "$REPO_ROOT"/flatpak/*.gschema.xml "$SCHEMA_DIR/"
glib-compile-schemas "$SCHEMA_DIR"
export GSETTINGS_SCHEMA_DIR="$SCHEMA_DIR"
export GDK_BACKEND=x11

# Dedicated display so we never clash with a concurrent test run on :99.
export DISPLAY=:98
Xvfb :98 -screen 0 1600x1000x24 &
XVFB_PID=$!
cleanup() { kill "$XVFB_PID" 2>/dev/null || true; }
trap cleanup EXIT
sleep 1

dbus-run-session -- bash -c "
    \"$BIN\" &
    APP_PID=\$!
    sleep $SETTLE
    /usr/bin/python3 \"$REPO_ROOT/tests/gui/take_screenshot_xvfb.py\" \"$OUT\"
    kill \$APP_PID 2>/dev/null || true
"
echo "Captured $OUT"
