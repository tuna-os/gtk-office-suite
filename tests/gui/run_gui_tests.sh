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

# Always use our own private Xvfb, never an inherited DISPLAY. On a
# machine with a real logged-in desktop (e.g. via distrobox, which
# forwards the host's X/Xwayland socket into the container by default),
# $DISPLAY is *already set* to that real session — the old
# `[ -z "$DISPLAY" ]` check silently skipped Xvfb in that case and these
# tests ran their launched app windows, keyboard input, and window
# activation against whatever was actually on someone's screen. Test
# isolation requires a display nobody else is using, full stop; set
# GUI_TEST_REUSE_DISPLAY=1 only if you specifically want to watch the
# tests run on your own X session for debugging.
if [ -z "${GUI_TEST_REUSE_DISPLAY:-}" ]; then
    XVFB_DISPLAY_NUM="${GUI_TEST_DISPLAY_NUM:-99}"
    Xvfb ":${XVFB_DISPLAY_NUM}" -screen 0 1920x1080x24 &
    XVFB_PID=$!
    trap 'kill ${XVFB_PID:-} 2>/dev/null || true' EXIT
    export DISPLAY=":${XVFB_DISPLAY_NUM}"
    sleep 1
fi

# A window manager is required for GTK4 toplevels to receive X input focus
# under Xvfb — without one, AT-SPI's synthetic keyboard/mouse events
# (dogtail.rawinput) are accepted without error but never reach the app
# (confirmed via direct XTest probing: xdotool key delivery only works
# once a WM is present and the target window is explicitly activated —
# see framework/base.py's _activate_window). matchbox is minimal and
# needs no config; skip it entirely when reusing a real desktop, which
# already has one.
if [ -z "${GUI_TEST_REUSE_DISPLAY:-}" ] && command -v matchbox-window-manager >/dev/null 2>&1; then
    matchbox-window-manager -use_titlebar no &
    MWM_PID=$!
    trap 'kill ${MWM_PID:-} ${XVFB_PID:-} 2>/dev/null || true' EXIT
    sleep 1
fi

# AT-SPI needs a session bus; dbus-run-session gives us a private one.
# dogtail refuses to start unless toolkit-accessibility is enabled in that session.
exec dbus-run-session -- bash -c '
    gsettings set org.gnome.desktop.interface toolkit-accessibility true
    exec /usr/bin/python3 -m pytest "$@" -v --tb=short
' _ "$@"
