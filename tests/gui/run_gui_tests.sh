#!/usr/bin/env bash
# Run GUI tests under Xvfb with AT-SPI and compiled GSettings schemas.
# Used by CI and for local runs. Usage:
#   tests/gui/run_gui_tests.sh test_smoke.py [extra pytest args...]
set -euo pipefail

cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

# GSettings schemas: the apps abort at startup without them.
#
# Per run, not the shared /tmp/gtk-office-schemas this used to use: two runs
# on one machine raced the copy and the compile, and an app that read a
# half-written gschemas.compiled aborted at startup with nothing in the
# journey log to explain it (#354). Set GSETTINGS_SCHEMA_DIR to reuse a
# prepared directory instead.
if [ -n "${GSETTINGS_SCHEMA_DIR:-}" ]; then
    SCHEMA_DIR="$GSETTINGS_SCHEMA_DIR"
    mkdir -p "$SCHEMA_DIR"
else
    SCHEMA_DIR="$(mktemp -d -t gtk-office-schemas-XXXXXXXX)"
    trap 'rm -rf "${SCHEMA_DIR:-}"' EXIT
fi
cp "$REPO_ROOT"/flatpak/*.gschema.xml "$SCHEMA_DIR/"
glib-compile-schemas "$SCHEMA_DIR"
export GSETTINGS_SCHEMA_DIR="$SCHEMA_DIR"

# How long setup waits for the display and the window manager to come up.
# Each of the waits below polls every 0.1s and breaks the moment its
# condition holds, so a warm machine is not slowed by a generous budget —
# but a cold CI runner genuinely takes longer than the 10s this used to
# allow, and it failed the run while Xvfb was still starting (it printed
# its own xkbcomp banner three seconds *after* setup gave up). A timeout
# that is only just long enough is a flake; size it for the slowest
# machine and let the predicate decide when to stop waiting.
GUI_TEST_READY_SECONDS="${GUI_TEST_READY_SECONDS:-60}"
GUI_TEST_READY_TICKS="$(( GUI_TEST_READY_SECONDS * 10 ))"

# Starting a server is a different cost from probing one that has already
# started, so the -displayfd handshake gets its own budget. They were one
# variable, and that made a harness self-test wrong rather than merely
# slow: InjectedSetupFailures shortens the budget so a deliberately broken
# readiness probe reports quickly, and on a cold runner Xvfb had not
# announced itself inside that same second — so setup failed at the
# handshake instead of the gate under test. Two budgets let a test starve
# one wait without starving the other.
GUI_TEST_XVFB_SECONDS="${GUI_TEST_XVFB_SECONDS:-60}"
GUI_TEST_XVFB_TICKS="$(( GUI_TEST_XVFB_SECONDS * 10 ))"

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
#
# The display number is allocated by Xvfb itself (-displayfd), not fixed at
# :99. With a fixed number, a second concurrent run did not fail — it
# quietly joined the first run's display:
#
#     run A owns :99
#     run B: Xvfb -> "Fatal server error: Server is already active for display 99"
#     run B proceeded on DISPLAY=:99
#     geometry seen by run B: 800x600   (A's size, not the 1920x1080 B asked for)
#
# Two runs' windows then shared a screen, xdotool activation fought over
# focus, and one run's synthetic keystrokes landed in the other's editor.
# -displayfd lets the X server pick a free number under its own locking and
# report it back, so there is nothing to race (#354).
if [ -z "${GUI_TEST_REUSE_DISPLAY:-}" ]; then
    # GUI_TEST_SCREEN_SIZE is both the Xvfb geometry and what the recorder
    # is told to capture, so a stress campaign's narrow/wide display
    # matrix (tests/gui/stress.py) only has to set one variable and the
    # video still matches the screen.
    export GUI_TEST_SCREEN_SIZE="${GUI_TEST_SCREEN_SIZE:-1920x1080}"
    DISPLAY_NUM_FILE="$(mktemp -t gtk-office-display-XXXXXXXX)"
    # An explicitly requested number is still honoured — the stress matrix
    # and interactive debugging both use it — and goes through the same
    # -displayfd handshake, so a taken number fails instead of quietly
    # sharing: Xvfb writes the number only once it is serving, and writes
    # nothing at all when the display is already active.
    Xvfb ${GUI_TEST_DISPLAY_NUM:+":${GUI_TEST_DISPLAY_NUM}"} \
        -displayfd 3 -screen "0" "${GUI_TEST_SCREEN_SIZE}x24" \
        3>"$DISPLAY_NUM_FILE" &
    XVFB_PID=$!
    trap 'kill ${XVFB_PID:-} 2>/dev/null || true; rm -rf "${SCHEMA_DIR:-}" "${DISPLAY_NUM_FILE:-}"' EXIT

    # Xvfb writes the number once it is ready to accept connections, so this
    # waits for readiness and the allocation in one step.
    XVFB_DISPLAY_NUM=""
    for _ in $(seq 1 "$GUI_TEST_XVFB_TICKS"); do
        XVFB_DISPLAY_NUM="$(tr -d '[:space:]' < "$DISPLAY_NUM_FILE")"
        [ -n "$XVFB_DISPLAY_NUM" ] && break
        # Our own server dying is the collision case: report it as such
        # rather than waiting out the full timeout.
        if ! kill -0 "${XVFB_PID}" 2>/dev/null; then
            break
        fi
        sleep 0.1
    done
    if ! kill -0 "${XVFB_PID}" 2>/dev/null; then
        echo "GUI setup failed: our Xvfb exited before it was ready." >&2
        if [ -n "${GUI_TEST_DISPLAY_NUM:-}" ]; then
            echo "Display ${GUI_TEST_DISPLAY_NUM} was requested explicitly; is it already in use?" >&2
        fi
        exit 1
    fi
    if [ -z "$XVFB_DISPLAY_NUM" ]; then
        echo "GUI setup failed: Xvfb never reported a display number within ${GUI_TEST_XVFB_SECONDS}s." >&2
        exit 1
    fi
    export DISPLAY=":${XVFB_DISPLAY_NUM}"
    # Worth a line in the log: which display a run used is the first thing
    # you want to know when two runs on one machine behave oddly.
    echo "GUI display: ${DISPLAY} (${GUI_TEST_SCREEN_SIZE})"
fi

# Wait for the display to actually answer, and fail the run if it never
# does. This was a blind `sleep 1`: an Xvfb that could not start — display
# number already taken, missing binary, no /tmp/.X11-unix — left DISPLAY
# pointing at nothing and the journeys failed later with a timeout that
# said nothing about the cause. #241 asks setup to fail when the display is
# unavailable, and to say so.
for _ in $(seq 1 "$GUI_TEST_READY_TICKS"); do
    if xdpyinfo -display "$DISPLAY" >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done
if ! xdpyinfo -display "$DISPLAY" >/dev/null 2>&1; then
    echo "GUI setup failed: no X display at ${DISPLAY} after ${GUI_TEST_READY_SECONDS}s." >&2
    if [ -z "${GUI_TEST_REUSE_DISPLAY:-}" ]; then
        echo "Xvfb did not come up; is display ${XVFB_DISPLAY_NUM} already in use?" >&2
    else
        echo "GUI_TEST_REUSE_DISPLAY is set, so DISPLAY was inherited rather than started." >&2
    fi
    exit 1
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
    trap 'kill ${MWM_PID:-} ${XVFB_PID:-} 2>/dev/null || true; rm -rf "${SCHEMA_DIR:-}" "${DISPLAY_NUM_FILE:-}"' EXIT
    # Wait for the WM to actually claim the display rather than sleeping a
    # second and hoping. matchbox advertises itself through
    # _NET_SUPPORTING_WM_CHECK; until that is set, a GTK toplevel can map
    # without ever receiving X input focus and every synthetic keystroke in
    # the journey goes nowhere (#354: bounded predicates, not sleeps).
    for _ in $(seq 1 "$GUI_TEST_READY_TICKS"); do
        if xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q "window id"; then
            break
        fi
        sleep 0.1
    done
    if ! xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q "window id"; then
        echo "GUI setup failed: matchbox did not claim ${DISPLAY} within ${GUI_TEST_READY_SECONDS}s." >&2
        exit 1
    fi
fi

# The interpreter is pinned to the system one because dogtail and the GTK
# introspection bindings are distro packages, not pip installs — but an
# environment whose /usr/bin/python3 is not the one those packages were
# built for (a toolbox, a rebased image) can point GUI_TEST_PYTHON at the
# matching interpreter instead of failing at import time.
PYTHON_BIN="${GUI_TEST_PYTHON:-/usr/bin/python3}"

# AT-SPI needs a session bus; dbus-run-session gives us a private one.
# dogtail refuses to start unless toolkit-accessibility is enabled in that session.
# The light/dark preference is a session setting, so it is set inside the
# private bus rather than exported: libadwaita reads it from the portal/
# settings daemon, not from the environment. "default" leaves the session
# as it comes, which is what every ordinary run wants.
export GUI_TEST_COLOR_SCHEME_RESOLVED="${GUI_TEST_COLOR_SCHEME:-default}"

# Not `exec`: exec replaces this shell, and with it the EXIT trap that
# kills the Xvfb and the window manager started above. Every run then
# leaked an X server and a matchbox process — invisible in a single run,
# and dozens of stranded servers after a stress campaign, each holding a
# display number the next run has to step around.
status=0
dbus-run-session -- bash -c '
    gsettings set org.gnome.desktop.interface toolkit-accessibility true
    if [ "$GUI_TEST_COLOR_SCHEME_RESOLVED" != "default" ]; then
        gsettings set org.gnome.desktop.interface color-scheme \
            "$GUI_TEST_COLOR_SCHEME_RESOLVED"
    fi
    exec "$0" -m pytest "$@" -v --tb=short
' "$PYTHON_BIN" "$@" || status=$?
exit "$status"
