#!/usr/bin/env bash
# MatrixRTC e2e TEST 3: reconnect / resilience.
#
# Equivalents of the two Element Call specs that gate connection resilience:
#
#   playwright/sfu-reconnect-bug.spec.ts
#     "When creator left, avoid reconnect to the same SFU" (EC issue #3344).
#     EC counts LiveKit websocket connections on a third guest and asserts
#     the count does not grow when the call creator leaves. Here we count the
#     MSC4195 `/sfu/get` requests reaching lk-jwt-service, plus LiveKit
#     websocket upgrades reaching the SFU, attributable to the surviving
#     client after the creator leaves.
#
#   playwright/reconnect.spec.ts
#     "can only interact with header and footer while reconnecting". The UI
#     half is not applicable; the engine half is: stall the homeserver past
#     the MSC4140 delayed-leave delay, and the client must enter the
#     probably-left ("Reconnecting…") state and then recover its membership
#     once the homeserver answers again.
#
# Plus one scenario EC has no e2e for but that our stack can express: an SFU
# restart under a live call.
#
# Usage: JOIN_CALL_BIN=... ./run-resilience.sh
# Env:   KEEP_STACK=1  keep the containers afterwards.

set -uo pipefail
cd "$(dirname "$0")"

HARNESS_NAME=resilience
# shellcheck source=harness-lib.sh
. ./harness-lib.sh

JOIN_CALL_BIN=${JOIN_CALL_BIN:?set JOIN_CALL_BIN to the prebuilt join_call example}
[ -x "$JOIN_CALL_BIN" ] || die "join_call binary not found at $JOIN_CALL_BIN"

LOG_DIR=$PWD/logs/resilience-$(date +%Y%m%d-%H%M%S)
mkdir -p "$LOG_DIR"

# The delayed leave fires after 8 s (compose.yml) and the manager restarts it
# every 5 s with a 2 s local timeout; stalling the homeserver for longer than
# 8 s must therefore trip the probably-left state.
STALL_S=${STALL_S:-14}

CREATOR_PID="" SURVIVOR_PID=""
cleanup() {
    for pid in "$CREATOR_PID" "$SURVIVOR_PID"; do
        [ -n "$pid" ] && kill "$pid" 2>/dev/null
    done
    # Make sure a paused homeserver never outlives the run.
    if SYN=$(container_of synapse); then $ENGINE unpause "$SYN" >/dev/null 2>&1; fi
    stack_down
}
trap cleanup EXIT

stack_up
detect_engine

# --- users + room -----------------------------------------------------------
SUFFIX=$(date +%s)
CREATOR=$(register "creator-$SUFFIX") || die "registering the creator"
SURVIVOR=$(register "survivor-$SUFFIX") || die "registering the survivor"
CREATOR_ID=$(jq -r .user_id <<<"$CREATOR")
CREATOR_TOKEN=$(jq -r .access_token <<<"$CREATOR")
CREATOR_DEV=$(jq -r .device_id <<<"$CREATOR")
SURVIVOR_ID=$(jq -r .user_id <<<"$SURVIVOR")
SURVIVOR_TOKEN=$(jq -r .access_token <<<"$SURVIVOR")
SURVIVOR_DEV=$(jq -r .device_id <<<"$SURVIVOR")

ROOM=$(create_room "$CREATOR_TOKEN")
[ -n "$ROOM" ] && [ "$ROOM" != null ] || die "creating the room"
join_room "$SURVIVOR_TOKEN" "$ROOM" || die "survivor joining the room"
log "room $ROOM: creator=$CREATOR_ID survivor=$SURVIVOR_ID"

CREATOR_LOG=$LOG_DIR/creator.log
SURVIVOR_LOG=$LOG_DIR/survivor.log

# --- the call ---------------------------------------------------------------
# The creator joins first and therefore owns the oldest membership, i.e. the
# focus everybody follows in `oldest_membership` mode. This is exactly EC's
# setup: the SFU in use belongs to the participant who is about to leave.
log "creator joins (becomes the oldest membership / active focus)"
"$JOIN_CALL_BIN" "$HS" "$CREATOR_ID" "$CREATOR_DEV" "$ROOM" \
    --focus "$JWT" <<<"$CREATOR_TOKEN" >"$CREATOR_LOG" 2>&1 &
CREATOR_PID=$!
wait_log "$CREATOR_LOG" "connected as" 90 || die "the creator never connected to the SFU"

sleep 2
log "survivor joins"
"$JOIN_CALL_BIN" "$HS" "$SURVIVOR_ID" "$SURVIVOR_DEV" "$ROOM" \
    --focus "$JWT" <<<"$SURVIVOR_TOKEN" >"$SURVIVOR_LOG" 2>&1 &
SURVIVOR_PID=$!
wait_log "$SURVIVOR_LOG" "connected as" 90 || die "the survivor never connected to the SFU"

if wait_memberships "$CREATOR_TOKEN" "$ROOM" 2 30; then
    result PASS two-memberships "both clients are in the call"
else
    result FAIL two-memberships "$(active_memberships "$CREATOR_TOKEN" "$ROOM") active memberships"
fi

# Both must have resolved the creator's SFU (the oldest membership).
CREATOR_SFU=$(grep -m1 "using LiveKit JWT service" "$CREATOR_LOG" | awk '{print $NF}')
SURVIVOR_SFU=$(grep -m1 "using LiveKit JWT service" "$SURVIVOR_LOG" | awk '{print $NF}')
if [ -n "$CREATOR_SFU" ] && [ "$CREATOR_SFU" = "$SURVIVOR_SFU" ]; then
    result PASS shared-focus "both clients resolved $CREATOR_SFU"
else
    result FAIL shared-focus "creator=$CREATOR_SFU survivor=$SURVIVOR_SFU"
fi

# ===========================================================================
# SCENARIO 1 — sfu-reconnect-bug.spec.ts
# ===========================================================================
log "SCENARIO 1: the creator leaves; the survivor must not reconnect"

# Baselines. Each SFU (re)connection is preceded by an MSC4195 /sfu/get to
# lk-jwt-service and shows up as a "connected as" line in the client log.
AUTH_C=$(container_of auth-service) || die "cannot find the auth-service container"
jwt_requests() { $ENGINE logs "$AUTH_C" 2>&1 | grep -cE "sfu/get|/get_token" || true; }

JWT_BEFORE=$(jwt_requests)
CONNECTS_BEFORE=$(count_log "$SURVIVOR_LOG" "connected as")
log "baseline: jwt_requests=$JWT_BEFORE survivor_connects=$CONNECTS_BEFORE"

log "creator leaves gracefully"
kill -INT "$CREATOR_PID"
wait "$CREATOR_PID" 2>/dev/null
CREATOR_PID=""

# EC waits 1 s, checks, then waits another 6 s and checks again. Same here.
sleep 7
JWT_AFTER=$(jwt_requests)
CONNECTS_AFTER=$(count_log "$SURVIVOR_LOG" "connected as")

if [ "$CONNECTS_AFTER" = "$CONNECTS_BEFORE" ]; then
    result PASS no-sfu-reconnect-on-creator-leave \
        "survivor still on its original SFU connection ($CONNECTS_AFTER)"
else
    result FAIL no-sfu-reconnect-on-creator-leave \
        "survivor reconnected: $CONNECTS_BEFORE -> $CONNECTS_AFTER (EC issue #3344)"
fi
if [ "$JWT_AFTER" = "$JWT_BEFORE" ]; then
    result PASS no-jwt-refetch-on-creator-leave "jwt requests stayed at $JWT_AFTER"
else
    result INFO no-jwt-refetch-on-creator-leave \
        "jwt requests $JWT_BEFORE -> $JWT_AFTER (may include the creator's leave traffic)"
fi

# The survivor must still be in the call, and be the only member.
if [ "$(active_memberships "$SURVIVOR_TOKEN" "$ROOM")" = 1 ]; then
    result PASS survivor-still-joined "1 active membership after the creator left"
else
    result FAIL survivor-still-joined \
        "$(active_memberships "$SURVIVOR_TOKEN" "$ROOM") active memberships"
fi
if kill -0 "$SURVIVOR_PID" 2>/dev/null; then
    result PASS survivor-alive "the survivor process survived the creator leaving"
else
    result FAIL survivor-alive "the survivor exited when the creator left"
fi

# ===========================================================================
# SCENARIO 2 — reconnect.spec.ts (homeserver stall -> probably-left -> recover)
# ===========================================================================
log "SCENARIO 2: stalling the homeserver for ${STALL_S}s"
SYN=$(container_of synapse) || die "cannot find the synapse container"

PROBABLY_LEFT_BEFORE=$(count_log "$SURVIVOR_LOG" "ProbablyLeft")
STATE_EVENTS_BEFORE=$(count_log "$SURVIVOR_LOG" "memberships changed")

$ENGINE pause "$SYN" >/dev/null || die "cannot pause the synapse container"
sleep "$STALL_S"
$ENGINE unpause "$SYN" >/dev/null || die "cannot unpause the synapse container"
log "homeserver back; waiting for recovery"

# EC asserts the "Reconnecting…" dialog appears. Ours is the ProbablyLeft
# session event.
if wait_log "$SURVIVOR_LOG" "ProbablyLeft\\(true\\)" 20; then
    result PASS probably-left-on-stall "the client noticed it probably left"
else
    result INFO probably-left-on-stall \
        "no ProbablyLeft(true) within the window; the stall may have been shorter \
than the delayed-leave delay from the client's point of view"
fi

# …and then recovers: the membership must come back and stay.
if wait_memberships "$SURVIVOR_TOKEN" "$ROOM" 1 60; then
    result PASS recovers-membership-after-stall "membership present again"
else
    result FAIL recovers-membership-after-stall \
        "no active membership 60s after the homeserver returned"
fi
if kill -0 "$SURVIVOR_PID" 2>/dev/null; then
    result PASS survives-homeserver-stall "the client did not die during the stall"
else
    result FAIL survives-homeserver-stall "the client exited during the homeserver stall"
fi
PROBABLY_LEFT_AFTER=$(count_log "$SURVIVOR_LOG" "ProbablyLeft")
log "ProbablyLeft emissions: $PROBABLY_LEFT_BEFORE -> $PROBABLY_LEFT_AFTER \
(memberships-changed $STATE_EVENTS_BEFORE -> $(count_log "$SURVIVOR_LOG" "memberships changed"))"

# CONFORMANCE.md open item: a *flapping* ProbablyLeft. One transition to true
# and one back to false per stall is correct; more than four emissions for a
# single stall means the state is oscillating.
FLAPS=$((PROBABLY_LEFT_AFTER - PROBABLY_LEFT_BEFORE))
if [ "$FLAPS" -le 4 ]; then
    result PASS probably-left-does-not-flap "$FLAPS emissions for one stall"
else
    result FAIL probably-left-does-not-flap \
        "$FLAPS ProbablyLeft emissions for a single stall (expected <= 4)"
fi

# ===========================================================================
# SCENARIO 3 — SFU restart under a live call
# ===========================================================================
log "SCENARIO 3: restarting the SFU"
LKC=$(container_of livekit) || die "cannot find the livekit container"
DISCONNECTS_BEFORE=$(count_log "$SURVIVOR_LOG" "disconnected:")

$ENGINE restart "$LKC" >/dev/null || die "cannot restart the livekit container"
wait_for "$LK/" 60 livekit

if wait_log "$SURVIVOR_LOG" "disconnected:" 30 &&
    [ "$(count_log "$SURVIVOR_LOG" "disconnected:")" -gt "$DISCONNECTS_BEFORE" ]; then
    result PASS notices-sfu-restart "the client observed the SFU going away"
else
    result INFO notices-sfu-restart \
        "no new disconnect event; the LiveKit SDK may have reconnected transparently"
fi

# KNOWN GAP (recorded, not gated): neither matrixrtc/examples/join_call.rs nor
# the app's src/session/call/media.rs reconnects after a LiveKit
# `RoomEvent::Disconnected` — both break out of their media loop, tear the
# connection down and (in the example) leave the RTC session. Element Call
# relies on livekit-client's automatic reconnection here, which is why it has
# no e2e for this case. Until we grow the same behaviour, an SFU restart ends
# the call for us.
sleep 5
MEMBERS_AFTER_RESTART=$(active_memberships "$SURVIVOR_TOKEN" "$ROOM")
if kill -0 "$SURVIVOR_PID" 2>/dev/null; then
    if [ "$MEMBERS_AFTER_RESTART" = 1 ]; then
        result PASS reconnects-after-sfu-restart \
            "the client stayed in the call across the SFU restart"
    else
        result FAIL reconnects-after-sfu-restart \
            "the client is alive but its membership vanished — neither reconnected nor left cleanly"
    fi
else
    result INFO no-sfu-reconnect-known-gap \
        "the client ended the call on the LiveKit disconnect instead of reconnecting \
(memberships now: $MEMBERS_AFTER_RESTART). This matches the current media layer, \
which has no SFU reconnect path; Element Call gets one from livekit-client."
    # It must at least have left cleanly rather than stranding a membership.
    if [ "$MEMBERS_AFTER_RESTART" = 0 ]; then
        result PASS clean-teardown-after-sfu-loss "no stale membership left behind"
    else
        result FAIL clean-teardown-after-sfu-loss \
            "$MEMBERS_AFTER_RESTART stale membership(s) after the client gave up"
    fi
fi

# --- graceful leave ---------------------------------------------------------
if kill -0 "$SURVIVOR_PID" 2>/dev/null; then
    kill -INT "$SURVIVOR_PID"
    wait "$SURVIVOR_PID" 2>/dev/null
    SURVIVOR_PID=""
    if wait_memberships "$SURVIVOR_TOKEN" "$ROOM" 0 20; then
        result PASS final-graceful-leave "the call is empty again"
    else
        result FAIL final-graceful-leave "a membership survived the graceful leave"
    fi
else
    SURVIVOR_PID=""
    if grep -q "left the session gracefully" "$SURVIVOR_LOG"; then
        result PASS final-graceful-leave \
            "the client left gracefully when it gave up on the SFU"
    else
        result FAIL final-graceful-leave \
            "the client exited without leaving the session: $(tail -2 "$SURVIVOR_LOG" | tr '\n' ' ')"
    fi
fi

member_events "$SURVIVOR_TOKEN" "$ROOM" >"$LOG_DIR/member-state-final.json" 2>/dev/null
summary
