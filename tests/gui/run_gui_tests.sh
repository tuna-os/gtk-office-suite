#!/usr/bin/env bash
# Run GUI tests under Xvfb with AT-SPI and compiled GSettings schemas.
# Used by CI and for local runs. Usage:
#   tests/gui/run_gui_tests.sh test_smoke.py [extra pytest args...]
set -euo pipefail

cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

# GSettings schemas: the apps abort at startup without them.
SCHEMA_DIR="${GSETTINGS_SCHEMA_DIR:-/tmp/gtk-office-schemas}"
mkdir -p "$SCHEMA_DIR"
cp "$REPO_ROOT"/flatpak/*.gschema.xml "$SCHEMA_DIR/"
glib-compile-schemas "$SCHEMA_DIR"
export GSETTINGS_SCHEMA_DIR="$SCHEMA_DIR"

export GDK_BACKEND=x11
export GTK_A11Y=atspi
# dogtail's a11y check accepts this env var (GTK4 itself ignores GTK_MODULES).
export GTK_MODULES=gail:atk-bridge

# Start Xvfb if no display is available (CI); reuse existing DISPLAY otherwise.
if [ -z "${DISPLAY:-}" ]; then
    Xvfb :99 -screen 0 1920x1080x24 &
    XVFB_PID=$!
    trap 'kill $XVFB_PID 2>/dev/null || true' EXIT
    export DISPLAY=:99
    sleep 1
fi

# AT-SPI needs a session bus; dbus-run-session gives us a private one.
# dogtail refuses to start unless toolkit-accessibility is enabled in that session.
exec dbus-run-session -- bash -c '
    gsettings set org.gnome.desktop.interface toolkit-accessibility true
    exec /usr/bin/python3 -m pytest "$@" -v --tb=short
' _ "$@"
