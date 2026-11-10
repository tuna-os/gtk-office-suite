#!/usr/bin/env bash
# MatrixRTC e2e: the real app joins a call, driven through its UI.
#
# Every other script here exercises the matrixrtc crate through the
# join_call example. This one runs the built Flatpak bundle against the same
# synapse + lk-jwt-service + LiveKit stack (compose.yml) and drives it the
# way a user would, through the accessibility tree (tests/gui/drive_app.py):
# log in with a password, finish the new-account session setup, open the
# room, press "Join Call" and the prescreen's "Join Call". A join_call bot is
# already in the call, and it is the bot's view that is asserted:
#   - the app's device appears in the bot's membership list and in room
#     state as a well-formed m.call.member event,
#   - the bot subscribes to a media track published by the app's device,
#   - pressing "Leave Call" removes the membership again,
# plus: the app's log has no panic, CRITICAL or app-logged ERROR.
#
# What it cannot assert — and why: the bot is not an Olm client (see the
# join_call docs: it sends its call keys in plaintext and cannot decrypt
# Olm-encrypted to-device messages), while the app sends its keys only
# Olm-encrypted, and only to devices that have published device keys
# (src/session/call/client_api.rs, encrypt_and_send_to_device). So the bot
# never gets the app's media key, and "key exchange happened" / "frames
# decrypted" are recorded as INFO from the bot's E2EE state, not gated. That
# needs a second Olm-capable peer, e.g. a second app instance.
#
# Usage: JOIN_CALL_BIN=... ./run-gui-call.sh path/to/Mandelbrot.flatpak
# Host:  flatpak, mutter, dbus, at-spi2-core, python3-gi + gir1.2-atspi-2.0,
#        gnome-keyring, libsecret-tools, xdg-desktop-portal-gnome, pulseaudio,
#        xdg-desktop-portal(-gtk), jq, curl.

set -uo pipefail
cd "$(dirname "$0")" || exit 2

HARNESS_NAME=gui-call
# shellcheck source=harness-lib.sh
. ./harness-lib.sh
# shellcheck source=../gui/headless-session.sh
. ../gui/headless-session.sh
DRIVE=$PWD/../gui/drive_app.py

if [ -z "${MANDELBROT_TEST_IN_DBUS:-}" ]; then
    # --- outside the private session: stack, users, room, install ----------
    JOIN_CALL_BIN=$(realpath "${JOIN_CALL_BIN:?set JOIN_CALL_BIN to the prebuilt join_call example}")
    [ -x "$JOIN_CALL_BIN" ] || die "join_call binary not found at $JOIN_CALL_BIN"
    BUNDLE=$(realpath "${1:?usage: $0 bundle.flatpak}")
    LOG_DIR=$PWD/logs/gui-call-$(date +%Y%m%d-%H%M%S)
    mkdir -p "$LOG_DIR"

    trap stack_down EXIT
    stack_up

    SUFFIX=$(date +%s)
    GUI_USER=gui-$SUFFIX
    GUI_JSON=$(register "$GUI_USER") || die "registering the app's user"
    BOT_JSON=$(register "bot-$SUFFIX") || die "registering the bot"
    GUI_ID=$(jq -r .user_id <<<"$GUI_JSON")
    GUI_TOKEN=$(jq -r .access_token <<<"$GUI_JSON")
    GUI_REG_DEV=$(jq -r .device_id <<<"$GUI_JSON")
    BOT_ID=$(jq -r .user_id <<<"$BOT_JSON")
    BOT_TOKEN=$(jq -r .access_token <<<"$BOT_JSON")
    BOT_DEV=$(jq -r .device_id <<<"$BOT_JSON")

    ROOM=$(create_room "$BOT_TOKEN" '{"name":"gui call test"}')
    [ -n "$ROOM" ] && [ "$ROOM" != null ] || die "creating the room"
    join_room "$GUI_TOKEN" "$ROOM" || die "joining the app's user to the room"
    log "room $ROOM: bot $BOT_ID, app user $GUI_ID"

    # The compose teardown runs after HOME moves; keep docker's own config.
    export DOCKER_CONFIG=${DOCKER_CONFIG:-$HOME/.docker}
    isolate_env "$(mktemp -d)"
    install_bundle "$BUNDLE" >"$LOG_DIR/install.log" 2>&1 ||
        { cat "$LOG_DIR/install.log"; die "installing the bundle"; }

    export JOIN_CALL_BIN LOG_DIR GUI_USER GUI_ID GUI_TOKEN GUI_REG_DEV \
        BOT_ID BOT_TOKEN BOT_DEV ROOM COMPOSE
    # The private session's script takes over the teardown.
    trap - EXIT
fi
in_dbus_session "$0" "$@"

# --- inside the private D-Bus session --------------------------------------
BOT_PID=""
cleanup() {
    [ -n "$BOT_PID" ] && kill "$BOT_PID" 2>/dev/null
    if [ -n "$APP_PID" ]; then
        python3 "$DRIVE" --dump-dir "$LOG_DIR/a11y" --timeout 5 dump >/dev/null 2>&1
    fi
    stop_app
    stop_audio
    stop_compositor
    stack_down
}
trap cleanup EXIT

start_compositor "$LOG_DIR/mutter.log"
start_secret_portal
start_audio "$LOG_DIR/pulseaudio.log"

APP_LOG=$LOG_DIR/app.log
start_app "$APP_LOG"
log "app started (pid $APP_PID)"

# --- log in through the UI ---------------------------------------------------
if python3 "$DRIVE" --dump-dir "$LOG_DIR/a11y" --timeout 180 login \
    --homeserver "$HS" --username "$GUI_USER" --password testpassword \
    --done-names "Main Menu" "gui call test" >"$LOG_DIR/drive-login.log" 2>&1; then
    result PASS gui-login "logged in and finished session setup through the UI"
else
    cat "$LOG_DIR/drive-login.log"
    result FAIL gui-login "see drive-login.log and the a11y/ tree dumps"
    summary
    exit 1
fi

# The app's own device: the one that is not the registration device.
GUI_DEV=$(curl -sf "$HS/_matrix/client/v3/devices" -H "Authorization: Bearer $GUI_TOKEN" |
    jq -r --arg reg "$GUI_REG_DEV" '[.devices[] | select(.device_id != $reg)][0].device_id // empty')
[ -n "$GUI_DEV" ] || die "cannot find the device the app logged in with"
GUI_MEMBER="$GUI_ID:$GUI_DEV"
log "app device: $GUI_MEMBER"

# --- the bot starts the call -------------------------------------------------
# It goes first so that the call's active focus comes from its membership:
# the app takes its preferred foci from https://<server_name>/.well-known,
# which this loopback stack does not serve.
BOT_LOG=$LOG_DIR/bot.log
"$JOIN_CALL_BIN" "$HS" "$BOT_ID" "$BOT_DEV" "$ROOM" --focus "$JWT" \
    <<<"$BOT_TOKEN" >"$BOT_LOG" 2>&1 &
BOT_PID=$!
wait_log "$BOT_LOG" "connected as" 60 || die "the bot did not connect to the SFU"
wait_memberships "$BOT_TOKEN" "$ROOM" 1 30 || die "the bot's membership did not appear"

# --- the app joins through the UI ------------------------------------------
# Open the room the way a matrix.to link would: GApplication forwards the
# second launch's URI to the running instance.
flatpak --user run "$APP_ID" "https://matrix.to/#/$ROOM" >"$LOG_DIR/open-room.log" 2>&1
if python3 "$DRIVE" --dump-dir "$LOG_DIR/a11y" --timeout 90 join-call \
    >"$LOG_DIR/drive-join.log" 2>&1; then
    result PASS gui-join "pressed Join Call twice; the call view shows Leave Call"
else
    cat "$LOG_DIR/drive-join.log"
    result FAIL gui-join "see drive-join.log and the a11y/ tree dumps"
fi

if wait_log "$BOT_LOG" "memberships changed: .*\"$GUI_MEMBER\"" 60; then
    result PASS bot-sees-membership "the bot's session lists $GUI_MEMBER"
else
    result FAIL bot-sees-membership "$GUI_MEMBER never appeared in the bot's memberships"
fi

STATE=$(member_events "$BOT_TOKEN" "$ROOM")
echo "$STATE" >"$LOG_DIR/member-state.json"
if jq -e --arg dev "$GUI_DEV" '[.[] | select(.content.device_id == $dev) | .content |
        select(.application == "m.call" and .focus_active.type == "livekit")] | length == 1' \
    <<<"$STATE" >/dev/null; then
    result PASS app-membership-wellformed "one m.call.member for $GUI_DEV with a livekit focus"
else
    result FAIL app-membership-wellformed "see member-state.json"
fi

if wait_log "$BOT_LOG" "subscribed to track .* of $GUI_MEMBER\$" 60; then
    result PASS bot-subscribed-app-track "the app published media the bot subscribed to"
else
    result FAIL bot-subscribed-app-track "no track of $GUI_MEMBER reached the bot"
fi

sleep 10
E2EE=$(grep "e2ee state of $GUI_MEMBER:" "$BOT_LOG" | tail -1 | sed 's/.*: //')
result INFO bot-e2ee-state-of-app "${E2EE:-none} (the bot cannot receive Olm-encrypted keys; see the header)"
MIC=$(grep -m1 "Failed to publish the microphone" "$APP_LOG" || true)
[ -n "$MIC" ] && result INFO app-microphone "$MIC"

# --- the app leaves through the UI ------------------------------------------
if python3 "$DRIVE" --dump-dir "$LOG_DIR/a11y" --timeout 30 leave-call \
    >"$LOG_DIR/drive-leave.log" 2>&1 &&
    wait_memberships "$BOT_TOKEN" "$ROOM" 1 30; then
    result PASS gui-leave "Leave Call removed the app's membership"
else
    cat "$LOG_DIR/drive-leave.log"
    result FAIL gui-leave "the app's membership is still there (or Leave Call was not found)"
fi

FATAL=$(fatal_log_lines "$APP_LOG")
if [ -z "$FATAL" ]; then
    result PASS app-log-clean "no panic, CRITICAL or app ERROR in the app log"
else
    result FAIL app-log-clean "$(head -3 <<<"$FATAL" | tr '\n' ' ')"
fi

summary
