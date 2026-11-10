#!/usr/bin/env bash
# MatrixRTC interop e2e TEST 2: our native client <-> Element Call (SPA).
#
# Runs ONE scenario against ONE Element Call build/mode and records a
# conformance evidence bundle in logs/. Assertions are soft (recorded as
# RESULT lines) because the goal is a conformance report, not a gate; the
# exit code is 1 if any RESULT is FAIL.
#
# Usage:
#   ./run-interop-ec.sh <scenario> <mode>
#     scenario: ec-first   Element Call creates + joins the call, then our
#                          client joins the same room
#               us-first   our client creates the room + joins the call,
#                          Element Call joins via room link
#     mode:     legacy | compatibility | matrix_2_0   (EC matrix_rtc_mode)
#
# Env:
#   EC_IMAGE       default ghcr.io/element-hq/element-call:latest (stable)
#   PW_IMAGE       playwright container image (must match PW_VERSION)
#   PW_VERSION     playwright-core version to install in the container
#   JOIN_CALL_BIN  path to the prebuilt join_call example (required)
#   KEEP_STACK=1   don't tear down the backend stack afterwards
#
# The backend stack (compose.yml) is started if not already running and
# torn down at the end unless KEEP_STACK=1. The Element Call and playwright
# containers are always removed.

set -uo pipefail
cd "$(dirname "$0")"

SCENARIO=${1:?usage: run-interop-ec.sh <ec-first|us-first> <legacy|compatibility|matrix_2_0>}
MODE=${2:?missing mode}

HS=http://127.0.0.1:8008
JWT=http://127.0.0.1:6080
EC_URL=http://127.0.0.1:8080
EC_IMAGE=${EC_IMAGE:-ghcr.io/element-hq/element-call:latest}
PW_VERSION=${PW_VERSION:-1.54.0}
PW_IMAGE=${PW_IMAGE:-mcr.microsoft.com/playwright:v${PW_VERSION}-noble}
JOIN_CALL_BIN=${JOIN_CALL_BIN:?set JOIN_CALL_BIN to the prebuilt join_call example}
REG_SECRET="test_shared_secret_for_local_dev_only"

LOG_DIR=$PWD/logs/ec-$SCENARIO-$MODE-$(date +%Y%m%d-%H%M%S)
mkdir -p "$LOG_DIR"
RESULTS=()

log() { printf '[interop-ec] %s\n' "$*"; }
result() { # PASS/FAIL <name> <details>
    RESULTS+=("$1 $2 ${3:-}")
    log "RESULT $1 $2 ${3:-}"
}
die() { log "FATAL: $*"; exit 2; }

CLIENT_PID=""
cleanup() {
    [ -n "$CLIENT_PID" ] && kill "$CLIENT_PID" 2>/dev/null
    touch "$LOG_DIR/stop" 2>/dev/null
    podman rm -f mandelbrot-ec mandelbrot-ec-driver >/dev/null 2>&1
    if [ "${KEEP_STACK:-0}" != 1 ]; then
        podman-compose -f compose.yml down -v >/dev/null 2>&1
    fi
}
trap cleanup EXIT

# --- backend stack -------------------------------------------------------
if ! curl -sf "$HS/_matrix/client/versions" >/dev/null; then
    log "starting the backend stack"
    mkdir -p synapse_tmp
    podman-compose -f compose.yml up -d >/dev/null 2>&1
    for i in $(seq 1 180); do
        curl -sf "$HS/_matrix/client/versions" >/dev/null && break
        [ "$i" = 180 ] && die "synapse did not come up"
        sleep 1
    done
fi
curl -sf http://127.0.0.1:7880/ >/dev/null || die "livekit not reachable"

# --- Element Call --------------------------------------------------------
# mode "default": leave matrix_rtc_mode unset, i.e. whatever the build
# defaults to (what a real deployment would do).
if [ "$MODE" = default ]; then
    jq 'del(.matrix_rtc_mode)' element-call/config-template.json >"$LOG_DIR/ec-config.json.tmp"
else
    sed "s/@MODE@/$MODE/" element-call/config-template.json >"$LOG_DIR/ec-config.json.tmp"
fi
# DEVICE_EVENTS=0 turns off feature_use_device_session_member_events, i.e.
# EC falls back to legacy user-keyed membership state events. This is the
# format we must be able to READ from other clients.
if [ "${DEVICE_EVENTS:-1}" = 0 ]; then
    jq '.features.feature_use_device_session_member_events = false' \
        "$LOG_DIR/ec-config.json.tmp" >"$LOG_DIR/ec-config.json"
else
    mv "$LOG_DIR/ec-config.json.tmp" "$LOG_DIR/ec-config.json"
fi
rm -f "$LOG_DIR/ec-config.json.tmp"
podman rm -f mandelbrot-ec >/dev/null 2>&1
podman run -d --name mandelbrot-ec --network host \
    -v "$LOG_DIR/ec-config.json:/app/config.json:Z" "$EC_IMAGE" >/dev/null ||
    die "failed to start element-call"
for i in $(seq 1 30); do
    curl -sf "$EC_URL/config.json" >/dev/null && break
    [ "$i" = 30 ] && die "element-call did not come up"
    sleep 1
done
EC_VERSION=$(podman image inspect "$EC_IMAGE" \
    --format '{{index .Labels "org.opencontainers.image.version"}}' 2>/dev/null)
log "element-call $EC_VERSION ($EC_IMAGE) in mode $MODE at $EC_URL"

# --- users ----------------------------------------------------------------
SUFFIX=$(date +%s)
register() {
    curl -sf -X POST "$HS/_matrix/client/v3/register" \
        -H 'Content-Type: application/json' \
        -d "{\"username\":\"$1\",\"password\":\"testpassword\",\"auth\":{\"type\":\"m.login.dummy\"}}"
}
register_admin() { # shared-secret admin registration
    local user=$1 nonce mac
    nonce=$(curl -sf "$HS/_synapse/admin/v1/register" | jq -r .nonce)
    mac=$(printf '%s\0%s\0%s\0admin' "$nonce" "$user" "testpassword" |
        openssl dgst -sha1 -hmac "$REG_SECRET" | awk '{print $NF}')
    curl -sf -X POST "$HS/_synapse/admin/v1/register" -H 'Content-Type: application/json' \
        -d "{\"nonce\":\"$nonce\",\"username\":\"$user\",\"password\":\"testpassword\",\"admin\":true,\"mac\":\"$mac\"}"
}
OURS=$(register "mandelbrot-$SUFFIX") || die "registering our user"
OUR_ID=$(jq -r .user_id <<<"$OURS")
OUR_TOKEN=$(jq -r .access_token <<<"$OURS")
OUR_DEV=$(jq -r .device_id <<<"$OURS")
ADMIN=$(register_admin "admin-$SUFFIX") || die "registering admin"
ADMIN_TOKEN=$(jq -r .access_token <<<"$ADMIN")
log "our client: $OUR_ID ($OUR_DEV)"

admin_state() { # room_id
    curl -sf "$HS/_synapse/admin/v1/rooms/$1/state" \
        -H "Authorization: Bearer $ADMIN_TOKEN" | jq .state
}
member_events() { # room_id -> both legacy + sticky member event types
    admin_state "$1" | jq '[.[] | select(
        .type == "org.matrix.msc3401.call.member" or
        .type == "m.call.member" or
        .type == "org.matrix.msc4143.rtc.member")]'
}

# --- start the two participants ------------------------------------------
start_driver() { # scenario room_id
    podman rm -f mandelbrot-ec-driver >/dev/null 2>&1
    podman run -d --name mandelbrot-ec-driver --network host \
        -v "$PWD/element-call:/work:ro,Z" -v "$LOG_DIR:/out:Z" \
        -e EC_URL="$EC_URL" -e SCENARIO="$1" -e ROOM_ID="${2:-}" \
        -e DISPLAY_NAME="EC-Peer" -e DURATION=600 -e OUT_DIR=/out \
        "$PW_IMAGE" bash -lc \
        "mkdir -p /tmp/w && cd /tmp/w && cp /work/ec-driver.mjs . &&
         npm init -y >/dev/null 2>&1 &&
         (node -e \"require.resolve('playwright-core')\" 2>/dev/null ||
          npm i --no-audit --no-fund playwright-core@$PW_VERSION >/dev/null 2>&1) &&
         NODE_PATH=\$(npm root -g) node ec-driver.mjs" >/dev/null ||
        die "failed to start driver"
    (podman logs -f mandelbrot-ec-driver >"$LOG_DIR/driver.log" 2>&1 &)
}
wait_driver_event() { # regex timeout_s -> matching line
    local i line
    for i in $(seq 1 "$2"); do
        line=$(grep -E "$1" "$LOG_DIR/driver.log" 2>/dev/null | head -1)
        [ -n "$line" ] && { echo "$line"; return 0; }
        grep -q "EVENT error" "$LOG_DIR/driver.log" 2>/dev/null && {
            log "driver error: $(grep 'EVENT error' "$LOG_DIR/driver.log" | head -1)"
            return 1
        }
        sleep 1
    done
    return 1
}
start_our_client() { # room_id
    "$JOIN_CALL_BIN" "$HS" "$OUR_ID" "$OUR_DEV" "$1" \
        --focus "$JWT" <<<"$OUR_TOKEN" >>"$LOG_DIR/ours.log" 2>&1 &
    CLIENT_PID=$!
}
wait_tiles() { # count timeout_s
    local i
    for i in $(seq 1 "$2"); do
        [ "$(grep "EVENT tiles" "$LOG_DIR/driver.log" 2>/dev/null | tail -1 | awk '{print $3}')" = "$1" ] && return 0
        sleep 1
    done
    return 1
}

ROOM=""
if [ "$SCENARIO" = ec-first ]; then
    log "starting Element Call driver (EC creates the call)"
    start_driver create
    LINE=$(wait_driver_event "EVENT createRoom" 120) || die "EC did not create a room"
    ROOM=$(awk '{print $3}' <<<"$LINE")
    wait_driver_event "EVENT joined" 120 >/dev/null || die "EC did not join its call"
    log "EC created and joined room $ROOM"
    member_events "$ROOM" >"$LOG_DIR/state-ec-only.json"
    admin_state "$ROOM" >"$LOG_DIR/state-full-initial.json"

    JOIN=$(curl -s -X POST "$HS/_matrix/client/v3/join/$ROOM" \
        -H "Authorization: Bearer $OUR_TOKEN" -H 'Content-Type: application/json' -d '{}')
    if jq -e .room_id <<<"$JOIN" >/dev/null 2>&1; then
        result PASS room-join "our user can join the EC-created room"
    else
        result FAIL room-join "$(jq -c . <<<"$JOIN")"
        die "cannot join EC room; join response: $JOIN"
    fi
    start_our_client "$ROOM"
else
    log "creating the room ourselves (ENCRYPTED=${ENCRYPTED:-0})"
    # ENCRYPTED=1 adds m.room.encryption, which flips Element Call from
    # E2eeType.NONE to E2eeType.PER_PARTICIPANT (see EC
    # src/e2ee/sharedKeyManagement.ts useRoomEncryptionSystem) and therefore
    # to Olm-encrypted to-device key transport.
    CREATE_BODY='{"preset":"public_chat","name":"matrixrtc interop","power_level_content_override":{"events":{"org.matrix.msc3401.call.member":0,"m.call.member":0,"org.matrix.msc4143.rtc.member":0,"org.matrix.msc4075.rtc.notification":0,"m.rtc.notification":0}}}'
    if [ "${ENCRYPTED:-0}" = 1 ]; then
        CREATE_BODY=$(jq -c '.initial_state = [{"type":"m.room.encryption","state_key":"","content":{"algorithm":"m.megolm.v1.aes-sha2"}}]' <<<"$CREATE_BODY")
    fi
    ROOM=$(curl -sf -X POST "$HS/_matrix/client/v3/createRoom" \
        -H "Authorization: Bearer $OUR_TOKEN" -H 'Content-Type: application/json' \
        -d "$CREATE_BODY" | jq -r .room_id)
    [ -n "$ROOM" ] && [ "$ROOM" != null ] || die "creating the room"
    log "room: $ROOM; joining the call with our client"
    start_our_client "$ROOM"
    for i in $(seq 1 30); do
        [ "$(member_events "$ROOM" | jq '[.[] | select(.content != {})] | length')" = 1 ] && break
        [ "$i" = 30 ] && die "our membership did not appear"
        sleep 1
    done
    member_events "$ROOM" >"$LOG_DIR/state-ours-only.json"
    log "starting Element Call driver (EC joins via link)"
    start_driver join "$ROOM"
    wait_driver_event "EVENT joined" 180 >/dev/null || die "EC did not join our room"
fi

# --- mutual membership -----------------------------------------------------
log "waiting for both memberships in room state"
BOTH=""
for i in $(seq 1 60); do
    N=$(member_events "$ROOM" | jq '[.[] | select(.content != {})] | length')
    [ "$N" -ge 2 ] 2>/dev/null && { BOTH=1; break; }
    sleep 1
done
member_events "$ROOM" >"$LOG_DIR/state-both.json"
admin_state "$ROOM" >"$LOG_DIR/state-full-both.json"
if [ -n "$BOTH" ]; then
    result PASS state-both-memberships "2 active member events"
else
    result FAIL state-both-memberships "$(member_events "$ROOM" | jq -c '[.[]|{type,state_key}]')"
fi

# does OUR client parse EC's membership? (they appear in our membership list)
EC_USER=$(jq -r '.[] | select(.sender != "'"$OUR_ID"'") | .sender' "$LOG_DIR/state-both.json" | head -1)
if [ -n "$EC_USER" ] && timeout 30 bash -c \
    "until grep 'memberships changed' '$LOG_DIR/ours.log' | tail -1 | grep -q '$EC_USER'; do sleep 1; done"; then
    result PASS we-see-ec "our membership list contains $EC_USER"
else
    result FAIL we-see-ec "EC user ${EC_USER:-<none>} not in our membership list"
fi

# does EC render our tile?
if wait_tiles 2 60; then
    result PASS ec-renders-us "EC shows 2 video tiles"
else
    result FAIL ec-renders-us "EC tiles: $(grep 'EVENT tiles' "$LOG_DIR/driver.log" | tail -1)"
fi

# media/E2EE evidence from our side
sleep 5
grep -E "subscribed to track|e2ee state|set key|received .* frames" "$LOG_DIR/ours.log" \
    >"$LOG_DIR/ours-media-evidence.log"
if grep -q "subscribed to track" "$LOG_DIR/ours.log"; then
    result PASS we-subscribe-ec-track "$(grep 'subscribed to track' "$LOG_DIR/ours.log" | head -1)"
else
    result FAIL we-subscribe-ec-track "no TrackSubscribed from EC"
fi

# --- delayed leave: kill our client, EC must see us disappear ---------------
log "kill -9 our client; EC should drop our tile via the delayed leave"
kill -9 "$CLIENT_PID" 2>/dev/null
CLIENT_PID=""
T0=$(date +%s)
if wait_tiles 1 40; then
    result PASS ec-sees-delayed-leave "tile dropped after $(($(date +%s) - T0))s"
else
    result FAIL ec-sees-delayed-leave "EC still shows our tile 40s after kill -9"
fi
member_events "$ROOM" >"$LOG_DIR/state-after-kill.json"

# --- rejoin ------------------------------------------------------------------
log "rejoining with our client"
start_our_client "$ROOM"
if wait_tiles 2 60; then
    result PASS ec-sees-rejoin "tile back"
else
    result FAIL ec-sees-rejoin "EC did not show our tile again"
fi

# --- graceful leave ------------------------------------------------------------
kill -INT "$CLIENT_PID" 2>/dev/null
wait "$CLIENT_PID" 2>/dev/null
CLIENT_PID=""
if wait_tiles 1 20; then
    result PASS ec-sees-graceful-leave "tile dropped"
else
    result FAIL ec-sees-graceful-leave "EC did not drop our tile after graceful leave"
fi

# --- collect the wire evidence -------------------------------------------------
touch "$LOG_DIR/stop"
sleep 3
{
    echo "=== EC requests (MatrixRTC-relevant) ==="
    grep "EVENT request " "$LOG_DIR/driver.log" || true
    echo
    echo "=== EC request bodies ==="
    grep "EVENT request-body" "$LOG_DIR/driver.log" | awk '{print $3}' |
        while read -r b64; do echo "$b64" | base64 -d 2>/dev/null; echo; done
} >"$LOG_DIR/ec-wire.log" 2>/dev/null

log "===== SUMMARY ($SCENARIO, $MODE, EC $EC_VERSION) ====="
FAILED=0
for r in "${RESULTS[@]}"; do
    log "  $r"
    [[ $r == FAIL* ]] && FAILED=1
done
log "evidence bundle: $LOG_DIR"
exit $FAILED
