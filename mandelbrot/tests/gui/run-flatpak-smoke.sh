#!/usr/bin/env bash
# Launch smoke test of the real Flatpak bundle.
#
# Installs the bundle CI built (and the GNOME runtime it targets) into a
# throwaway per-user installation, starts it on a headless Wayland display
# with a private D-Bus session and fresh HOME/XDG dirs, and asserts that:
#   - the app activated (its log has `Application::activate`),
#   - it is still running after $ALIVE_S seconds,
#   - its log has no `panicked`, no `CRITICAL` and no app-logged `ERROR` line
#     (a keyring, compositor and portals are running, so none is expected).
#
# This is what catches a runtime or dependency bump that compiles and passes
# the unit tests but no longer starts: a missing shared library or GSettings
# schema, a GTK/libadwaita API that now asserts, a panic during startup.
#
# Usage: tests/gui/run-flatpak-smoke.sh path/to/Mandelbrot.flatpak
# Env:   ALIVE_S  seconds the app must survive (default 20)
#        OUT      evidence directory (default ./smoke-logs)

set -uo pipefail

HARNESS_NAME=flatpak-smoke
# shellcheck source-path=SCRIPTDIR source=headless-session.sh
. "$(dirname "$0")/headless-session.sh"

if [ -z "${MANDELBROT_TEST_IN_DBUS:-}" ]; then
    BUNDLE=$(realpath "${1:?usage: $0 bundle.flatpak}")
    OUT=$(realpath -m "${OUT:-smoke-logs}")
    mkdir -p "$OUT"
    export BUNDLE OUT
    isolate_env "$(mktemp -d)"
    install_bundle "$BUNDLE" || gui_die "installing the bundle"
fi
in_dbus_session "$0" "$@"

ALIVE_S=${ALIVE_S:-20}
APP_LOG=$OUT/app.log
trap 'stop_app; stop_compositor' EXIT

start_compositor "$OUT/mutter.log"
gui_log "compositor up on $WAYLAND_DISPLAY"
start_secret_portal

start_app "$APP_LOG"
gui_log "app started (pid $APP_PID); waiting ${ALIVE_S}s"
sleep "$ALIVE_S"

failed=0
if kill -0 "$APP_PID" 2>/dev/null; then
    gui_log "PASS alive after ${ALIVE_S}s"
else
    wait "$APP_PID"
    gui_log "FAIL the app exited early with status $?"
    failed=1
fi
if grep -q "Application::activate" "$APP_LOG"; then
    gui_log "PASS the application activated"
else
    gui_log "FAIL no 'Application::activate' in the log"
    failed=1
fi
FATAL=$(fatal_log_lines "$APP_LOG")
if [ -z "$FATAL" ]; then
    gui_log "PASS no panic, CRITICAL or app ERROR in the log"
else
    gui_log "FAIL the log has panics or criticals:"
    printf '%s\n' "$FATAL"
    failed=1
fi

stop_app
gui_log "app log: $APP_LOG"
echo "----- app log -----"
cat "$APP_LOG"
exit "$failed"
