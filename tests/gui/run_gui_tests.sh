#!/usr/bin/env bash
# Run GUI tests under Xvfb with AT-SPI and compiled GSettings schemas.
# Used by CI and for local runs. Usage:
#   tests/gui/run_gui_tests.sh test_smoke.py [extra pytest args...]
set -euo pipefail

cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

# Each run owns its schemas and settings, even when several jobs run at once.
TEST_ROOT=$(mktemp -d -t gtk-office-gui.XXXXXXXX)
trap 'rm -rf -- "$TEST_ROOT"' EXIT
SCHEMA_DIR="$TEST_ROOT/schemas"
mkdir -p "$SCHEMA_DIR"
cp "$REPO_ROOT"/flatpak/*.gschema.xml "$SCHEMA_DIR/"
glib-compile-schemas "$SCHEMA_DIR"
export GSETTINGS_SCHEMA_DIR="$SCHEMA_DIR"

export GDK_BACKEND=x11
export GTK_A11Y=atspi
# dogtail's a11y check accepts this env var (GTK4 itself ignores GTK_MODULES).
export GTK_MODULES=gail:atk-bridge
export XDG_CONFIG_HOME="$TEST_ROOT/config"
export XDG_DATA_HOME="$TEST_ROOT/data"
export XDG_STATE_HOME="$TEST_ROOT/state"
export XDG_CACHE_HOME="$TEST_ROOT/cache"
export GSETTINGS_BACKEND=keyfile
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_CACHE_HOME"

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
display_runner=()
if [ "${GUI_TEST_REUSE_DISPLAY:-0}" != "1" ]; then
    for dependency in xvfb-run matchbox-window-manager xdotool; do
        command -v "$dependency" >/dev/null || { echo "Missing GUI dependency: $dependency" >&2; exit 1; }
    done
    # Matchbox sizes app windows to this screen. Matrix widths are logical
    # pixels; scaling the screen as well exercises the same layout at 1x/2x.
    screen_geometry=1920x1080
    if [ -n "${GUI_TEST_WIDTH:-}" ]; then
        width="$GUI_TEST_WIDTH"
        scale="${GUI_TEST_SCALE:-1}"
        [[ "$width" =~ ^[1-9][0-9]{0,3}$ && "$scale" =~ ^[12]$ ]] || {
            echo "Invalid GUI_TEST_WIDTH or GUI_TEST_SCALE" >&2; exit 1;
        }
        screen_geometry="$((10#$width * scale))x$((900 * scale))"
    fi
    display_runner=(xvfb-run -a -s "-screen 0 ${screen_geometry}x24")
fi

# A window manager is required for GTK4 toplevels to receive X input focus
# under Xvfb — without one, AT-SPI's synthetic keyboard/mouse events
# (dogtail.rawinput) are accepted without error but never reach the app
# (confirmed via direct XTest probing: xdotool key delivery only works
# once a WM is present and the target window is explicitly activated —
# see framework/base.py's _activate_window). matchbox is minimal and
# needs no config; skip it entirely when reusing a real desktop, which
# already has one.
# AT-SPI needs a session bus; dbus-run-session gives us a private one.
# dogtail refuses to start unless toolkit-accessibility is enabled in that session.
# Keep the outer shell alive so its EXIT cleanup runs on pytest failures too.
"${display_runner[@]}" dbus-run-session -- bash -c '
    set -euo pipefail
    if [ "${GUI_TEST_REUSE_DISPLAY:-0}" != "1" ]; then
        matchbox-window-manager -use_titlebar no &
        wm_pid=$!
        trap '\''kill "$wm_pid" 2>/dev/null || :; wait "$wm_pid" 2>/dev/null || :'\'' EXIT
    fi
    gsettings set org.gnome.desktop.interface toolkit-accessibility true
    gsettings set org.gnome.desktop.interface enable-animations false
    /usr/bin/python3 -m pytest "$@" -v --tb=short
' _ "$@"
