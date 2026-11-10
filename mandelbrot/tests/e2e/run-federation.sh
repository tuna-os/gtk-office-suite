#!/usr/bin/env bash
# MatrixRTC e2e TEST 6: federated call across two homeservers.
#
# Equivalent of Element Call's
#   playwright/widget/federation-oldest-membership-bug.spec.ts
#   playwright/widget/federated-call.test.ts
#
# Topology (compose.yml + compose-federated.yml):
#   site 1  synapse.m.localhost            SFU :7880   JWT :6080
#   site 2  synapse.othersite.m.localhost  SFU :17880  JWT :16080
#
# The property under test is the one EC's bug report is about: the client
# that joins second must publish on the SFU of the OLDEST membership — which
# lives on the *other* homeserver — and not on its own preferred SFU. If it
# gets that wrong, both sides connect to different SFUs and nobody sees
# anybody ("Waiting for media…" in EC's UI; no subscribed tracks here).
#
# The engine-level version of the same assertion runs on every PR as
# `federation_oldest_membership_*` in matrixrtc/tests/element_call_scenarios.rs.
# This script proves it over the wire, including the part the unit tests
# cannot cover: that site 2's client can actually obtain an MSC4195 token
# from site 1's auth service using an OpenID token issued by site 2, which
# lk-jwt-service validates over federation.
#
# Usage: JOIN_CALL_BIN=... ./run-federation.sh [order]
#   order: remote-first (default) — site 2 creates the call, site 1 joins
#          local-first             — site 1 creates the call, site 2 joins
# Env:   KEEP_STACK=1  keep the containers afterwards.

set -uo pipefail
cd "$(dirname "$0")"

HARNESS_NAME=federation
# shellcheck source=harness-lib.sh
. ./harness-lib.sh

JOIN_CALL_BIN=${JOIN_CALL_BIN:?set JOIN_CALL_BIN to the prebuilt join_call example}
[ -x "$JOIN_CALL_BIN" ] || die "join_call binary not found at $JOIN_CALL_BIN"

ORDER=${1:-remote-first}
HS1=http://127.0.0.1:8008
HS2=http://127.0.0.1:18008
JWT1=http://127.0.0.1:6080
JWT2=http://127.0.0.1:16080

LOG_DIR=$PWD/logs/federation-$ORDER-$(date +%Y%m%d-%H%M%S)
mkdir -p "$LOG_DIR"

PIDS=()
cleanup() {
    for pid in "${PIDS[@]:-}"; do
        [ -n "$pid" ] && kill -9 "$pid" 2>/dev/null
    done
    save_container_logs -f compose.yml -f compose-federated.yml
    if [ "${KEEP_STACK:-0}" != 1 ]; then
        $COMPOSE -f compose.yml -f compose-federated.yml down -v >/dev/null 2>&1
    else
        log "KEEP_STACK=1: leaving the stack running"
    fi
}
trap cleanup EXIT

# --- the two-site stack ------------------------------------------------------
detect_compose
detect_engine
mkdir -p synapse_tmp synapse_tmp_othersite
if curl -sf "$HS2/_matrix/client/versions" >/dev/null; then
    log "federated stack already running"
else
    log "starting the federated stack (two homeservers, two SFUs)"
    $COMPOSE -f compose.yml -f compose-federated.yml up -d >/dev/null 2>&1 ||
        die "compose up failed"
fi
HS=$HS1 wait_for "$HS1/_matrix/client/versions" 180 "synapse (site 1)"
wait_for "$HS2/_matrix/client/versions" 180 "synapse (site 2)"
wait_for "http://127.0.0.1:7880/" 60 "livekit (site 1)"
wait_for "http://127.0.0.1:17880/" 60 "livekit (site 2)"
wait_for "$JWT1/healthz" 60 "lk-jwt-service (site 1)"
wait_for "$JWT2/healthz" 60 "lk-jwt-service (site 2)"

# --- users on both sites -----------------------------------------------------
SUFFIX=$(date +%s)
register_on() { # base_url username -> registration JSON
    curl -sf -X POST "$1/_matrix/client/v3/register" \
        -H 'Content-Type: application/json' \
        -d "{\"username\":\"$2\",\"password\":\"testpassword\",\"auth\":{\"type\":\"m.login.dummy\"}}"
}
# EC's fixtures name them florian (site 1) and timo (site 2); keep the names.
FLORIAN=$(register_on "$HS1" "florian-$SUFFIX") || die "registering florian on site 1"
TIMO=$(register_on "$HS2" "timo-$SUFFIX") || die "registering timo on site 2"
FLORIAN_ID=$(jq -r .user_id <<<"$FLORIAN")
FLORIAN_TOKEN=$(jq -r .access_token <<<"$FLORIAN")
FLORIAN_DEV=$(jq -r .device_id <<<"$FLORIAN")
TIMO_ID=$(jq -r .user_id <<<"$TIMO")
TIMO_TOKEN=$(jq -r .access_token <<<"$TIMO")
TIMO_DEV=$(jq -r .device_id <<<"$TIMO")
log "florian=$FLORIAN_ID (site 1)  timo=$TIMO_ID (site 2)"

# --- a federated room --------------------------------------------------------
# Florian creates the room on site 1 and invites Timo on site 2. Timo joining
# is itself the proof that federation works at all.
ROOM=$(curl -sf -X POST "$HS1/_matrix/client/v3/createRoom" \
    -H "Authorization: Bearer $FLORIAN_TOKEN" -H 'Content-Type: application/json' \
    -d "{\"preset\":\"public_chat\",\"name\":\"federated matrixrtc\",\"invite\":[\"$TIMO_ID\"],\"power_level_content_override\":{\"events\":{\"org.matrix.msc3401.call.member\":0,\"org.matrix.msc4075.rtc.notification\":0}}}" |
    jq -r .room_id)
[ -n "$ROOM" ] && [ "$ROOM" != null ] || die "creating the federated room"

JOINED=""
for _ in $(seq 1 60); do
    if curl -sf -X POST "$HS2/_matrix/client/v3/join/$ROOM" \
        -H "Authorization: Bearer $TIMO_TOKEN" -H 'Content-Type: application/json' \
        -d '{}' >/dev/null 2>&1; then
        JOINED=1
        break
    fi
    sleep 2
done
if [ -n "$JOINED" ]; then
    result PASS federation-room-join "timo (site 2) joined a room created on site 1"
else
    result FAIL federation-room-join "timo could not join the federated room"
    summary
    exit 1
fi
log "federated room $ROOM"

# --- who creates the call ----------------------------------------------------
if [ "$ORDER" = remote-first ]; then
    # EC's federation-oldest-membership-bug setup: Timo (site 2) creates the
    # call, so the active focus is site 2's SFU and Florian (site 1) must
    # follow it away from his own preferred SFU.
    FIRST=(timo "$HS2" "$TIMO_ID" "$TIMO_DEV" "$TIMO_TOKEN" "$JWT2")
    SECOND=(florian "$HS1" "$FLORIAN_ID" "$FLORIAN_DEV" "$FLORIAN_TOKEN" "$JWT1")
    EXPECTED_SFU_PORT=17880
else
    FIRST=(florian "$HS1" "$FLORIAN_ID" "$FLORIAN_DEV" "$FLORIAN_TOKEN" "$JWT1")
    SECOND=(timo "$HS2" "$TIMO_ID" "$TIMO_DEV" "$TIMO_TOKEN" "$JWT2")
    EXPECTED_SFU_PORT=7880
fi
log "order=$ORDER: ${FIRST[0]} creates the call, ${SECOND[0]} joins"

start() { # name hs user device token jwt
    "$JOIN_CALL_BIN" "$2" "$3" "$4" "$ROOM" --focus "$6" \
        <<<"$5" >"$LOG_DIR/$1.log" 2>&1 &
    PIDS+=($!)
}

start "${FIRST[@]}"
wait_log "$LOG_DIR/${FIRST[0]}.log" "connected as" 90 ||
    die "${FIRST[0]} never connected to its SFU"
sleep 3 # make the oldest membership unambiguous
start "${SECOND[@]}"

if wait_log "$LOG_DIR/${SECOND[0]}.log" "connected as" 120; then
    result PASS second-joiner-connects "${SECOND[0]} reached an SFU"
else
    result FAIL second-joiner-connects \
        "${SECOND[0]} never connected: $(tail -3 "$LOG_DIR/${SECOND[0]}.log" | tr '\n' ' ')"
fi

# --- THE assertion: both on the oldest membership's SFU ----------------------
FIRST_SFU=$(grep -m1 "got SFU config" "$LOG_DIR/${FIRST[0]}.log" | sed 's/.*url=//')
SECOND_SFU=$(grep -m1 "got SFU config" "$LOG_DIR/${SECOND[0]}.log" | sed 's/.*url=//')
FIRST_JWT=$(grep -m1 "using LiveKit JWT service" "$LOG_DIR/${FIRST[0]}.log" | awk '{print $NF}')
SECOND_JWT=$(grep -m1 "using LiveKit JWT service" "$LOG_DIR/${SECOND[0]}.log" | awk '{print $NF}')
log "SFUs: ${FIRST[0]}=$FIRST_SFU  ${SECOND[0]}=$SECOND_SFU"
log "JWT services: ${FIRST[0]}=$FIRST_JWT  ${SECOND[0]}=$SECOND_JWT"

if [ -n "$FIRST_SFU" ] && [ "$FIRST_SFU" = "$SECOND_SFU" ]; then
    result PASS same-sfu "both federated participants are on $FIRST_SFU"
else
    result FAIL same-sfu \
        "${FIRST[0]}=$FIRST_SFU ${SECOND[0]}=$SECOND_SFU — the second joiner did not follow \
the oldest membership (element-hq/element-call federation-oldest-membership bug)"
fi
if grep -q ":$EXPECTED_SFU_PORT" <<<"$SECOND_SFU"; then
    result PASS follows-oldest-membership-sfu \
        "the second joiner uses the call creator's SFU (port $EXPECTED_SFU_PORT)"
else
    result FAIL follows-oldest-membership-sfu \
        "expected port $EXPECTED_SFU_PORT, got $SECOND_SFU"
fi

# The second joiner fetched its token from the *other site's* auth service,
# which had to validate an OpenID token issued by its own homeserver over
# federation. This is the cross-site MSC4195 path.
if [ -n "$SECOND_JWT" ] && [ "$SECOND_JWT" = "$FIRST_JWT" ]; then
    result PASS cross-site-msc4195 \
        "the second joiner obtained an SFU token from $SECOND_JWT (the other site)"
else
    result FAIL cross-site-msc4195 \
        "the second joiner used $SECOND_JWT rather than the creator's $FIRST_JWT"
fi

# --- both memberships visible on both homeservers ---------------------------
members_on() { # base_url token -> count
    curl -sf "$1/_matrix/client/v3/rooms/$ROOM/state" \
        -H "Authorization: Bearer $2" |
        jq '[.[] | select(.type == "org.matrix.msc3401.call.member" and .content != {})] | length'
}
BOTH1="" BOTH2=""
for _ in $(seq 1 60); do
    [ "$(members_on "$HS1" "$FLORIAN_TOKEN")" = 2 ] && BOTH1=1
    [ "$(members_on "$HS2" "$TIMO_TOKEN")" = 2 ] && BOTH2=1
    [ -n "$BOTH1" ] && [ -n "$BOTH2" ] && break
    sleep 1
done
if [ -n "$BOTH1" ] && [ -n "$BOTH2" ]; then
    result PASS memberships-replicate "both m.call.member events are visible on both sites"
else
    result FAIL memberships-replicate \
        "site1=$(members_on "$HS1" "$FLORIAN_TOKEN") site2=$(members_on "$HS2" "$TIMO_TOKEN")"
fi

# --- media both ways ---------------------------------------------------------
sleep 10
for name in "${FIRST[0]}" "${SECOND[0]}"; do
    LOGF=$LOG_DIR/$name.log
    if grep -q "subscribed to track" "$LOGF"; then
        result PASS "$name-subscribes" "$(grep -c 'subscribed to track' "$LOGF") track(s)"
    else
        result FAIL "$name-subscribes" "no remote track — the two sides are not on the same SFU"
    fi
    if grep -q "set key" "$LOGF"; then
        result PASS "$name-receives-keys" "$(grep -c 'set key' "$LOGF") key event(s)"
    else
        result FAIL "$name-receives-keys" "no encryption key received from the federated peer"
    fi
done

curl -sf "$HS1/_matrix/client/v3/rooms/$ROOM/state" \
    -H "Authorization: Bearer $FLORIAN_TOKEN" >"$LOG_DIR/state-site1.json"
curl -sf "$HS2/_matrix/client/v3/rooms/$ROOM/state" \
    -H "Authorization: Bearer $TIMO_TOKEN" >"$LOG_DIR/state-site2.json"

summary
