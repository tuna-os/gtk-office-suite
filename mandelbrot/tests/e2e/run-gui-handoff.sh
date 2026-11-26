#!/usr/bin/env bash
# Hand-off e2e: another suite app's entry points into the real Mandelbrot app.
#
# Letters, Tables, Decks and Lifafa reach Mandelbrot through GApplication
# actions (docs/INTEGRATION-PLAN.md A3, A4). This runs the built Flatpak
# bundle against the local synapse stack (compose.yml), logs in through the UI
# (tests/gui/drive_app.py), then calls the actions the way those apps do, with
# `gapplication`, and checks the result in the accessibility tree:
#   - `open-room` with a room ID shows that room,
#   - `share-file` with a file and that room shows the file in the composer's
#     confirmation (nothing is sent until a person confirms),
#   - `open-room` with something that is not a room shows the "Not a Matrix
#     room" toast instead of doing nothing.
#
# Usage: ./run-gui-handoff.sh path/to/Mandelbrot.flatpak
# Host:  as for run-gui-call.sh (minus pulseaudio and the join_call binary).

set -uo pipefail
cd "$(dirname "$0")" || exit 2

HARNESS_NAME=gui-handoff
# shellcheck source=harness-lib.sh
. ./harness-lib.sh
# shellcheck source=../gui/headless-session.sh
. ../gui/headless-session.sh
DRIVE=$PWD/../gui/drive_app.py
ROOM_NAME="hand-off room"

if [ -z "${MANDELBROT_TEST_IN_DBUS:-}" ]; then
    BUNDLE=$(realpath "${1:?usage: $0 bundle.flatpak}")
    LOG_DIR=$PWD/logs/gui-handoff-$(date +%Y%m%d-%H%M%S)
    mkdir -p "$LOG_DIR"

    trap stack_down EXIT
    stack_up

    SUFFIX=$(date +%s)
    GUI_USER=gui-$SUFFIX
    GUI_JSON=$(register "$GUI_USER") || die "registering the app's user"
    GUI_TOKEN=$(jq -r .access_token <<<"$GUI_JSON")
    ROOM=$(create_room "$GUI_TOKEN" "{\"name\":\"$ROOM_NAME\"}")
    [ -n "$ROOM" ] && [ "$ROOM" != null ] || die "creating the room"

    SHARE_DIR=$(mktemp -d)
    SHARE_FILE=$SHARE_DIR/handoff-report.txt
    echo "shared from another suite app" >"$SHARE_FILE"

    export DOCKER_CONFIG=${DOCKER_CONFIG:-$HOME/.docker}
    isolate_env "$(mktemp -d)"
    install_bundle "$BUNDLE" >"$LOG_DIR/install.log" 2>&1 ||
        { cat "$LOG_DIR/install.log"; die "installing the bundle"; }

    export LOG_DIR GUI_USER ROOM SHARE_FILE COMPOSE
    trap - EXIT
fi
in_dbus_session "$0" "$@"

# --- inside the private D-Bus session --------------------------------------
cleanup() {
    python3 "$DRIVE" --dump-dir "$LOG_DIR/a11y" --timeout 5 dump >/dev/null 2>&1
    stop_app
    stop_compositor
    stack_down
}
trap cleanup EXIT

start_compositor "$LOG_DIR/mutter.log"
start_secret_portal

APP_LOG=$LOG_DIR/app.log
start_app "$APP_LOG"

if python3 "$DRIVE" --dump-dir "$LOG_DIR/a11y" --timeout 180 login \
    --homeserver "$HS" --username "$GUI_USER" --password testpassword \
    --done-names "Main Menu" "$ROOM_NAME" >"$LOG_DIR/drive-login.log" 2>&1; then
    result PASS gui-login "logged in and finished session setup through the UI"
else
    cat "$LOG_DIR/drive-login.log"
    result FAIL gui-login "see drive-login.log and the a11y/ tree dumps"
    summary
    exit 1
fi

# The action, called as the other apps call it. Actions are activated over
# D-Bus on the running instance; the parameter is a GVariant text string.
action() {
    flatpak --user run --command=gapplication "$APP_ID" action "$APP_ID" "$@"
}

if action open-room "'$ROOM'" >"$LOG_DIR/open-room.log" 2>&1 &&
    python3 "$DRIVE" --dump-dir "$LOG_DIR/a11y" --timeout 30 wait-name "$ROOM_NAME" \
        >"$LOG_DIR/drive-open-room.log" 2>&1; then
    result PASS open-room "open-room '$ROOM' shows the room"
else
    result FAIL open-room "see open-room.log and drive-open-room.log"
fi

if action share-file "('$SHARE_FILE', '$ROOM')" >"$LOG_DIR/share-file.log" 2>&1 &&
    python3 "$DRIVE" --dump-dir "$LOG_DIR/a11y" --timeout 30 wait-name "$(basename "$SHARE_FILE")" \
        >"$LOG_DIR/drive-share-file.log" 2>&1; then
    result PASS share-file "share-file shows the file in the room's composer"
else
    result FAIL share-file "see share-file.log and drive-share-file.log"
fi

if action open-room "'not a room'" >"$LOG_DIR/bad-target.log" 2>&1 &&
    python3 "$DRIVE" --dump-dir "$LOG_DIR/a11y" --timeout 15 wait-name "Not a Matrix room" \
        >"$LOG_DIR/drive-bad-target.log" 2>&1; then
    result PASS bad-target "an unusable target is reported, not ignored"
else
    result FAIL bad-target "see bad-target.log and drive-bad-target.log"
fi

FATAL=$(fatal_log_lines "$APP_LOG")
if [ -z "$FATAL" ]; then
    result PASS app-log-clean "no panic, CRITICAL or app ERROR in the app log"
else
    result FAIL app-log-clean "$(head -3 <<<"$FATAL" | tr '\n' ' ')"
fi

summary
