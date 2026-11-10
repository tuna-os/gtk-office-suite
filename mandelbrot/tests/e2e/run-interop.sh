#!/usr/bin/env bash
# MatrixRTC interop e2e test for the mandelbrot-matrixrtc crate.
#
# TEST 1 (native <-> native): two instances of our join_call example join
# the same room call and must
#   - both publish a well-formed org.matrix.msc3401.call.member state event,
#   - see each other's membership,
#   - exchange encryption keys (io.element.call.encryption_keys to-device),
#   - subscribe to and decrypt each other's audio track,
#   - clean up their membership on graceful leave (SIGINT) and, after an
#     ungraceful kill -9, via the MSC4140 delayed leave event fired by the
#     homeserver within the delay window.
#
# Usage: ./run-interop.sh
# Env:
#   JOIN_CALL_BIN  path to a prebuilt join_call example binary. If unset,
#                  it is built with `cargo build --features livekit
#                  --example join_call` in ../../matrixrtc.
#   KEEP_STACK=1   don't tear the container stack down at the end.
#   ASSERT_TIMEOUT seconds for the in-client assertions (default 90).

set -euo pipefail
cd "$(dirname "$0")"

HS=http://127.0.0.1:8008
JWT=http://127.0.0.1:6080
LK=http://127.0.0.1:7880
ASSERT_TIMEOUT=${ASSERT_TIMEOUT:-90}
# The delayed leave must fire within delay (8 s) + restart interval (5 s)
# + slack.
DELAYED_LEAVE_WINDOW=30

LOG_DIR=logs/$(date +%Y%m%d-%H%M%S)
mkdir -p "$LOG_DIR"

log() { printf '[run-interop] %s\n' "$*"; }
capture_stack_logs() {
    [ -n "${COMPOSE:-}" ] || return
    $COMPOSE -f compose.yml ps >"$LOG_DIR/compose-ps.log" 2>&1 || true
    $COMPOSE -f compose.yml logs --no-color >"$LOG_DIR/containers.log" 2>&1 || true
}
fail() {
    log "FAIL: $*"
    # The EXIT trap removes containers before the workflow-level evidence
    # collector runs. Capture compose state and logs while they still exist.
    capture_stack_logs
    for f in "$LOG_DIR"/alice.log "$LOG_DIR"/bob.log; do
        [ -f "$f" ] && { log "---- tail $f ----"; tail -n 40 "$f"; }
    done
    exit 1
}

# --- compose provider -------------------------------------------------------
# Set COMPOSE in the environment to force a provider; CI does, because
# ubuntu-latest ships a podman binary whose socket is never started while the
# rest of the workflow (image pull, teardown) speaks to docker.
#
# Auto-detection asks each candidate to talk to its backend rather than just
# checking that the binary is on PATH: `<provider> version` answers happily
# with nothing running behind it, whereas `ps` has to reach the engine, which
# separates "installed" from "usable".
compose_reaches_backend() { # provider words… -> 0 when the engine answers
    "$@" -f compose.yml ps -q >/dev/null 2>&1
}

detect_compose() {
    if [ -n "${COMPOSE:-}" ]; then
        log "using compose provider (forced): $COMPOSE"
        return
    fi
    local installed=() candidate
    command -v podman-compose >/dev/null && installed+=("podman-compose")
    podman compose version >/dev/null 2>&1 && installed+=("podman compose")
    docker compose version >/dev/null 2>&1 && installed+=("docker compose")
    [ ${#installed[@]} -gt 0 ] ||
        fail "no compose provider found (podman-compose, podman compose or docker compose)"
    for candidate in "${installed[@]}"; do
        # shellcheck disable=SC2086 # provider may be two words ("docker compose")
        if compose_reaches_backend $candidate; then
            COMPOSE=$candidate
            log "using compose provider: $COMPOSE"
            return
        fi
    done
    # Every probe failed. Fall back to the old first-installed-wins pick so
    # the caller still gets the engine's own error instead of ours.
    COMPOSE=${installed[0]}
    log "using compose provider: $COMPOSE (warning: no provider reached a running backend)"
}
detect_compose

# --- client binary -----------------------------------------------------------
if [ -z "${JOIN_CALL_BIN:-}" ]; then
    log "building join_call example (this may take a while)"
    (cd ../../matrixrtc && cargo build --features livekit --example join_call)
    JOIN_CALL_BIN=../../target/debug/examples/join_call
fi
[ -x "$JOIN_CALL_BIN" ] || fail "join_call binary not found at $JOIN_CALL_BIN"

# --- stack up ----------------------------------------------------------------
ALICE_PID="" BOB_PID=""
teardown() {
    [ -n "$ALICE_PID" ] && kill "$ALICE_PID" 2>/dev/null || true
    [ -n "$BOB_PID" ] && kill "$BOB_PID" 2>/dev/null || true
    if [ "${KEEP_STACK:-0}" != 1 ]; then
        log "tearing down the stack"
        $COMPOSE -f compose.yml down -v >/dev/null 2>&1 || true
    else
        log "KEEP_STACK=1: leaving the stack running"
    fi
}
trap teardown EXIT

mkdir -p synapse_tmp
log "starting the stack"
$COMPOSE -f compose.yml up -d

log "waiting for synapse"
for i in $(seq 1 180); do
    curl -sf "$HS/_matrix/client/versions" >/dev/null && break
    [ "$i" = 180 ] && fail "synapse did not come up"
    sleep 1
done
log "waiting for livekit"
for i in $(seq 1 60); do
    curl -sf "$LK/" >/dev/null && break
    [ "$i" = 60 ] && fail "livekit did not come up"
    sleep 1
done
log "waiting for lk-jwt-service"
for i in $(seq 1 60); do
    curl -s -o /dev/null "$JWT/healthz" && break
    [ "$i" = 60 ] && fail "lk-jwt-service did not come up"
    sleep 1
done

# --- users + room ------------------------------------------------------------
SUFFIX=$(date +%s)
register() { # username -> JSON
    curl -sf -X POST "$HS/_matrix/client/v3/register" \
        -H 'Content-Type: application/json' \
        -d "{\"username\":\"$1\",\"password\":\"testpassword\",\"auth\":{\"type\":\"m.login.dummy\"}}"
}
ALICE=$(register "alice-$SUFFIX") || fail "registering alice"
BOB=$(register "bob-$SUFFIX") || fail "registering bob"
ALICE_ID=$(jq -r .user_id <<<"$ALICE")
ALICE_TOKEN=$(jq -r .access_token <<<"$ALICE")
ALICE_DEV=$(jq -r .device_id <<<"$ALICE")
BOB_ID=$(jq -r .user_id <<<"$BOB")
BOB_TOKEN=$(jq -r .access_token <<<"$BOB")
BOB_DEV=$(jq -r .device_id <<<"$BOB")
log "registered $ALICE_ID ($ALICE_DEV) and $BOB_ID ($BOB_DEV)"

ROOM=$(curl -sf -X POST "$HS/_matrix/client/v3/createRoom" \
    -H "Authorization: Bearer $ALICE_TOKEN" -H 'Content-Type: application/json' \
    -d '{"preset":"public_chat","name":"matrixrtc interop","power_level_content_override":{"events":{"org.matrix.msc3401.call.member":0,"org.matrix.msc4075.rtc.notification":0}}}' | jq -r .room_id)
[ -n "$ROOM" ] && [ "$ROOM" != null ] || fail "creating the room"
curl -sf -X POST "$HS/_matrix/client/v3/join/$ROOM" \
    -H "Authorization: Bearer $BOB_TOKEN" -H 'Content-Type: application/json' \
    -d '{}' >/dev/null || fail "bob joining the room"
log "created room $ROOM"

# --- TEST 1: two native clients ----------------------------------------------
log "starting two join_call instances"
# join_call reads the access token from stdin; it never goes into argv.
"$JOIN_CALL_BIN" "$HS" "$ALICE_ID" "$ALICE_DEV" "$ROOM" \
    --focus "$JWT" --assert-peer "$BOB_ID:$BOB_DEV" --assert-timeout "$ASSERT_TIMEOUT" \
    <<<"$ALICE_TOKEN" >"$LOG_DIR/alice.log" 2>&1 &
ALICE_PID=$!
sleep 2 # let alice become the oldest membership (focus selection)
"$JOIN_CALL_BIN" "$HS" "$BOB_ID" "$BOB_DEV" "$ROOM" \
    --focus "$JWT" --assert-peer "$ALICE_ID:$ALICE_DEV" --assert-timeout "$ASSERT_TIMEOUT" \
    <<<"$BOB_TOKEN" >"$LOG_DIR/bob.log" 2>&1 &
BOB_PID=$!

log "waiting for in-client assertions (membership, tracks, keys, frames)"
for i in $(seq 1 $((ASSERT_TIMEOUT + 20))); do
    grep -q "ASSERTIONS FAILED" "$LOG_DIR/alice.log" && fail "alice assertions failed"
    grep -q "ASSERTIONS FAILED" "$LOG_DIR/bob.log" && fail "bob assertions failed"
    kill -0 "$ALICE_PID" 2>/dev/null || fail "alice exited prematurely"
    kill -0 "$BOB_PID" 2>/dev/null || fail "bob exited prematurely"
    if grep -q "ASSERTIONS PASSED" "$LOG_DIR/alice.log" &&
        grep -q "ASSERTIONS PASSED" "$LOG_DIR/bob.log"; then
        break
    fi
    [ "$i" = $((ASSERT_TIMEOUT + 20)) ] && fail "timed out waiting for in-client assertions"
    sleep 1
done
log "both clients: ASSERTIONS PASSED"

# --- server-side state well-formedness ---------------------------------------
member_events() {
    curl -sf "$HS/_matrix/client/v3/rooms/$ROOM/state" \
        -H "Authorization: Bearer $ALICE_TOKEN" |
        jq '[.[] | select(.type == "org.matrix.msc3401.call.member")]'
}

STATE=$(member_events)
echo "$STATE" >"$LOG_DIR/member-state.json"
ACTIVE=$(jq '[.[] | select(.content != {})] | length' <<<"$STATE")
[ "$ACTIVE" = 2 ] || fail "expected 2 active m.call.member events, got $ACTIVE"

WELLFORMED=$(jq '[.[] | select(.content != {}) | .content |
    select(
        .application == "m.call" and
        .call_id == "" and
        (.device_id | type == "string" and length > 0) and
        .focus_active.type == "livekit" and
        .focus_active.focus_selection == "oldest_membership" and
        (.foci_preferred | type == "array" and length >= 1) and
        (.foci_preferred[0].type == "livekit") and
        (.foci_preferred[0].livekit_service_url | type == "string")
    )] | length' <<<"$STATE")
[ "$WELLFORMED" = 2 ] || {
    jq . <<<"$STATE"
    fail "m.call.member contents are not well-formed ($WELLFORMED/2 passed)"
}
log "both m.call.member state events are present and well-formed"

# --- delayed leave after ungraceful kill --------------------------------------
log "kill -9 bob; waiting for the MSC4140 delayed leave to fire"
kill -9 "$BOB_PID"
T0=$(date +%s)
BOB_GONE=""
while true; do
    NOW=$(date +%s)
    ELAPSED=$((NOW - T0))
    LEFT=$(member_events | jq --arg u "$BOB_ID" \
        '[.[] | select(.sender == $u and .content == {})] | length')
    if [ "$LEFT" = 1 ]; then
        BOB_GONE=$ELAPSED
        break
    fi
    [ "$ELAPSED" -ge "$DELAYED_LEAVE_WINDOW" ] &&
        fail "bob's membership did not clear within ${DELAYED_LEAVE_WINDOW}s of kill -9"
    sleep 1
done
log "delayed leave fired: bob's membership cleared ${BOB_GONE}s after kill -9"

# --- graceful leave ------------------------------------------------------------
log "SIGINT alice; waiting for graceful leave"
kill -INT "$ALICE_PID"
wait "$ALICE_PID" || true
ALICE_PID=""
grep -q "left the session gracefully" "$LOG_DIR/alice.log" ||
    fail "alice did not report a graceful leave"
for i in $(seq 1 10); do
    LEFT=$(member_events | jq --arg u "$ALICE_ID" \
        '[.[] | select(.sender == $u and .content == {})] | length')
    [ "$LEFT" = 1 ] && break
    [ "$i" = 10 ] && fail "alice's membership did not clear after graceful leave"
    sleep 1
done
log "graceful leave: alice's membership cleared"

BOB_PID=""
log "TEST 1 PASSED (logs in $LOG_DIR)"
