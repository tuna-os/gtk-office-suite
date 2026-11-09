#!/usr/bin/env bash
# MatrixRTC e2e TEST 5: group call ("huddle").
#
# Equivalent of Element Call's playwright/widget/huddle-call.test.ts, which
# puts five participants in one room call and asserts that every client sees
# five tiles, nobody is stuck on "Waiting for media…", and a mute on one
# participant is reflected everywhere.
#
# We have no tiles, so the equivalents are:
#   - five well-formed `m.call.member` state events,
#   - every client's membership list contains all five,
#   - every client received an `io.element.call.encryption_keys` key from
#     each of the other four (this is the key-distribution fan-out that
#     actually breaks in large calls),
#   - every client subscribed to at least four remote tracks,
#   - one participant leaving is noticed by all the others,
#   - one participant being killed is cleaned up by the MSC4140 delayed leave
#     and noticed by all the others.
#
# Usage: JOIN_CALL_BIN=... ./run-huddle.sh [participants]
# Env:   KEEP_STACK=1   keep the containers afterwards
#        SETTLE_S       seconds to let the call stabilise (default 45)

set -uo pipefail
cd "$(dirname "$0")"

HARNESS_NAME=huddle
# shellcheck source=harness-lib.sh
. ./harness-lib.sh

JOIN_CALL_BIN=${JOIN_CALL_BIN:?set JOIN_CALL_BIN to the prebuilt join_call example}
[ -x "$JOIN_CALL_BIN" ] || die "join_call binary not found at $JOIN_CALL_BIN"

N=${1:-5}
SETTLE_S=${SETTLE_S:-45}
LOG_DIR=$PWD/logs/huddle-$(date +%Y%m%d-%H%M%S)
mkdir -p "$LOG_DIR"

PIDS=()
cleanup() {
    for pid in "${PIDS[@]:-}"; do
        [ -n "$pid" ] && kill -9 "$pid" 2>/dev/null
    done
    stack_down
}
trap cleanup EXIT

stack_up

# --- users + room -----------------------------------------------------------
SUFFIX=$(date +%s)
# EC uses Valere, Timo, Robin, Halfshot, Florian; keep the names so that the
# logs line up with theirs.
NAMES=(valere timo robin halfshot florian alice bob carol dave erin)
IDS=() TOKENS=() DEVS=()
for i in $(seq 0 $((N - 1))); do
    JSON=$(register "${NAMES[$i]}-$SUFFIX") || die "registering ${NAMES[$i]}"
    IDS+=("$(jq -r .user_id <<<"$JSON")")
    TOKENS+=("$(jq -r .access_token <<<"$JSON")")
    DEVS+=("$(jq -r .device_id <<<"$JSON")")
done

ROOM=$(create_room "${TOKENS[0]}")
[ -n "$ROOM" ] && [ "$ROOM" != null ] || die "creating the room"
for i in $(seq 1 $((N - 1))); do
    join_room "${TOKENS[$i]}" "$ROOM" || die "${IDS[$i]} joining the room"
done
log "room $ROOM with $N participants"

# --- everyone joins the call ------------------------------------------------
for i in $(seq 0 $((N - 1))); do
    "$JOIN_CALL_BIN" "$HS" "${IDS[$i]}" "${DEVS[$i]}" "$ROOM" \
        --focus "$JWT" <<<"${TOKENS[$i]}" >"$LOG_DIR/${NAMES[$i]}.log" 2>&1 &
    PIDS+=($!)
    # Stagger the joins slightly: the first joiner must win the oldest
    # membership deterministically, as in EC's test where Valere starts the
    # call and the rest join afterwards.
    sleep 2
done

log "waiting up to ${SETTLE_S}s for the call to settle"
if wait_memberships "${TOKENS[0]}" "$ROOM" "$N" "$SETTLE_S"; then
    result PASS all-memberships "$N active m.call.member events"
else
    result FAIL all-memberships \
        "$(active_memberships "${TOKENS[0]}" "$ROOM") of $N active memberships"
fi

STATE=$(member_events "${TOKENS[0]}" "$ROOM")
echo "$STATE" >"$LOG_DIR/member-state.json"

WELLFORMED=$(jq '[.[] | select(.content != {}) | .content |
    select(
        .application == "m.call" and
        .call_id == "" and
        (.device_id | type == "string" and length > 0) and
        .focus_active.type == "livekit" and
        (.foci_preferred | type == "array" and length >= 1)
    )] | length' <<<"$STATE")
if [ "$WELLFORMED" = "$N" ]; then
    result PASS wellformed-memberships "all $N contents are well-formed"
else
    result FAIL wellformed-memberships "$WELLFORMED of $N are well-formed"
fi

# Every membership must have a distinct state key: a collision would make
# participants overwrite each other.
DISTINCT=$(jq '[.[] | select(.content != {}) | .state_key] | unique | length' <<<"$STATE")
if [ "$DISTINCT" = "$N" ]; then
    result PASS distinct-state-keys "$N distinct state keys"
else
    result FAIL distinct-state-keys "$DISTINCT distinct state keys for $N members"
fi

# All participants must have converged on one SFU (the oldest membership's).
mapfile -t SFUS < <(for i in $(seq 0 $((N - 1))); do
    grep -m1 "using LiveKit JWT service" "$LOG_DIR/${NAMES[$i]}.log" | awk '{print $NF}'
done)
UNIQUE_SFUS=$(printf '%s\n' "${SFUS[@]}" | sort -u | grep -c .)
if [ "$UNIQUE_SFUS" = 1 ]; then
    result PASS single-focus "all $N participants resolved the same SFU"
else
    result FAIL single-focus "$UNIQUE_SFUS distinct SFUs: ${SFUS[*]}"
fi

# --- per-client fan-out ------------------------------------------------------
PEERS=$((N - 1))
for i in $(seq 0 $((N - 1))); do
    NAME=${NAMES[$i]}
    LOGF=$LOG_DIR/$NAME.log

    SEEN=$(grep "memberships changed" "$LOGF" | tail -1 |
        grep -o ':[A-Z]*"' | wc -l)
    if [ "$SEEN" -ge "$N" ]; then
        result PASS "$NAME-sees-everyone" "$SEEN memberships in the last update"
    else
        result FAIL "$NAME-sees-everyone" "$SEEN of $N memberships"
    fi

    KEYS=$(grep -o "set key [0-9]* for [^ ]*" "$LOGF" |
        awk '{print $NF}' | sort -u | wc -l)
    if [ "$KEYS" -ge "$PEERS" ]; then
        result PASS "$NAME-received-keys" "keys from $KEYS peers"
    else
        result FAIL "$NAME-received-keys" "keys from $KEYS of $PEERS peers"
    fi

    TRACKS=$(grep -o "subscribed to track [^ ]* of [^ ]*" "$LOGF" |
        awk '{print $NF}' | sort -u | wc -l)
    if [ "$TRACKS" -ge "$PEERS" ]; then
        result PASS "$NAME-subscribed-tracks" "$TRACKS remote publishers"
    else
        result FAIL "$NAME-subscribed-tracks" "$TRACKS of $PEERS remote publishers"
    fi
done

# --- one participant leaves gracefully --------------------------------------
LEAVER=$((N - 1))
log "SCENARIO: ${NAMES[$LEAVER]} leaves gracefully"
for i in $(seq 0 $((N - 1))); do
    cp "$LOG_DIR/${NAMES[$i]}.log" "$LOG_DIR/${NAMES[$i]}.before-leave.log"
done
kill -INT "${PIDS[$LEAVER]}"
wait "${PIDS[$LEAVER]}" 2>/dev/null
PIDS[$LEAVER]=""

if wait_memberships "${TOKENS[0]}" "$ROOM" "$((N - 1))" 30; then
    result PASS graceful-leave-clears-membership "$((N - 1)) memberships left"
else
    result FAIL graceful-leave-clears-membership \
        "$(active_memberships "${TOKENS[0]}" "$ROOM") memberships after a graceful leave"
fi

NOTICED=0
for i in $(seq 0 $((N - 2))); do
    LAST=$(grep "memberships changed" "$LOG_DIR/${NAMES[$i]}.log" | tail -1)
    grep -q "${IDS[$LEAVER]}" <<<"$LAST" || NOTICED=$((NOTICED + 1))
done
if [ "$NOTICED" = "$((N - 1))" ]; then
    result PASS everyone-notices-the-leave "all $((N - 1)) remaining clients dropped the leaver"
else
    result FAIL everyone-notices-the-leave \
        "$NOTICED of $((N - 1)) remaining clients dropped the leaver"
fi

# --- one participant is killed ungracefully ---------------------------------
VICTIM=$((N - 2))
log "SCENARIO: kill -9 ${NAMES[$VICTIM]}; the delayed leave must clean up"
kill -9 "${PIDS[$VICTIM]}"
PIDS[$VICTIM]=""
T0=$(date +%s)
if wait_memberships "${TOKENS[0]}" "$ROOM" "$((N - 2))" 30; then
    result PASS delayed-leave-clears-membership "cleared after $(($(date +%s) - T0))s"
else
    result FAIL delayed-leave-clears-membership \
        "$(active_memberships "${TOKENS[0]}" "$ROOM") memberships 30s after kill -9"
fi

member_events "${TOKENS[0]}" "$ROOM" >"$LOG_DIR/member-state-final.json"
summary
