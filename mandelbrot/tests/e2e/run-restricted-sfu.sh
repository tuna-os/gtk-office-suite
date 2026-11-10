#!/usr/bin/env bash
# MatrixRTC e2e TEST 4: restricted SFU / JWT behaviour.
#
# Equivalent of Element Call's playwright/restricted-sfu.spec.ts and the JWT
# half of playwright/errors.spec.ts.
#
# EC gates two properties:
#
#   1. "Should request JWT token before starting the call" — the MSC4195
#      `/sfu/get` request must complete BEFORE the `m.call.member` state
#      event is sent. The point is that on a deployment where call creation
#      is restricted to authorised users, an unauthorised client must never
#      advertise a membership it cannot back with media.
#
#   2. "Error when pre-warming the focus are caught by the ErrorBoundary" /
#      "Should show error screen if fails to get JWT token" — a non-retryable
#      JWT failure must surface as an error instead of a half-joined call,
#      and a retryable one (429) must be retried.
#
# Property 1 is the interesting one for Mandelbrot, because our join path is
# ordered the other way round (see the report in CONFORMANCE.md): the
# membership manager publishes the state event, and only then does the media
# task resolve the focus and fetch the JWT. This script measures it rather
# than asserting a behaviour we do not have, so the recorded RESULT lines are
# the evidence.
#
# Usage: JOIN_CALL_BIN=... ./run-restricted-sfu.sh
# Env:   KEEP_STACK=1  keep the containers afterwards.

set -uo pipefail
cd "$(dirname "$0")"

HARNESS_NAME=restricted-sfu
# shellcheck source=harness-lib.sh
. ./harness-lib.sh

JOIN_CALL_BIN=${JOIN_CALL_BIN:?set JOIN_CALL_BIN to the prebuilt join_call example}
[ -x "$JOIN_CALL_BIN" ] || die "join_call binary not found at $JOIN_CALL_BIN"

LOG_DIR=$PWD/logs/restricted-sfu-$(date +%Y%m%d-%H%M%S)
mkdir -p "$LOG_DIR"

CLIENT_PID=""
cleanup() {
    [ -n "$CLIENT_PID" ] && kill "$CLIENT_PID" 2>/dev/null
    if AUTH=$(container_of auth-service); then $ENGINE start "$AUTH" >/dev/null 2>&1; fi
    stack_down
}
trap cleanup EXIT

stack_up
detect_engine
AUTH_C=$(container_of auth-service) || die "cannot find the auth-service container"

SUFFIX=$(date +%s)
USER_JSON=$(register "restricted-$SUFFIX") || die "registering the user"
USER_ID=$(jq -r .user_id <<<"$USER_JSON")
USER_TOKEN=$(jq -r .access_token <<<"$USER_JSON")
USER_DEV=$(jq -r .device_id <<<"$USER_JSON")

# ===========================================================================
# SCENARIO 1 — ordering: JWT fetch vs. membership state event
# ===========================================================================
# We cannot instrument the client's internals from here, so we time both
# observable events:
#   - the SFU token arriving, which the client logs as "got SFU config"
#     immediately after the MSC4195 `/sfu/get` response,
#   - the `m.call.member` state event appearing in room state.
# Both are polled at 500 ms, which is far finer than the seconds-scale gap
# the ordering question is about. The polling must stay cheap: a
# `docker logs`/`podman logs` call per iteration reads the whole container
# log and will melt the host, so the client's own log file is used instead.
log "SCENARIO 1: is the JWT fetched before the membership is advertised?"
ROOM=$(create_room "$USER_TOKEN")
[ -n "$ROOM" ] && [ "$ROOM" != null ] || die "creating the room"

CLIENT_LOG=$LOG_DIR/ordering.log
: >"$CLIENT_LOG"
"$JOIN_CALL_BIN" "$HS" "$USER_ID" "$USER_DEV" "$ROOM" \
    --focus "$JWT" <<<"$USER_TOKEN" >"$CLIENT_LOG" 2>&1 &
CLIENT_PID=$!

MEMBER_AT="" JWT_AT=""
for _ in $(seq 1 240); do
    NOW=$(date +%s.%N)
    if [ -z "$MEMBER_AT" ] &&
        [ "$(active_memberships "$USER_TOKEN" "$ROOM")" = 1 ]; then
        MEMBER_AT=$NOW
    fi
    if [ -z "$JWT_AT" ] && grep -q "got SFU config" "$CLIENT_LOG"; then
        JWT_AT=$NOW
    fi
    [ -n "$MEMBER_AT" ] && [ -n "$JWT_AT" ] && break
    kill -0 "$CLIENT_PID" 2>/dev/null || break
    sleep 0.5
done

if [ -z "$MEMBER_AT" ] || [ -z "$JWT_AT" ]; then
    result FAIL ordering-observable "member_at=${MEMBER_AT:-never} jwt_at=${JWT_AT:-never}"
else
    DELTA=$(awk -v a="$JWT_AT" -v b="$MEMBER_AT" 'BEGIN { printf "%.2f", a - b }')
    if awk -v a="$JWT_AT" -v b="$MEMBER_AT" 'BEGIN { exit !(a < b) }'; then
        result PASS jwt-before-membership \
            "the SFU JWT was fetched ${DELTA}s before the membership event (matches Element Call)"
    else
        result FAIL jwt-before-membership \
            "the membership event was published ${DELTA}s BEFORE the SFU JWT was fetched \
(Element Call gates the opposite order in restricted-sfu.spec.ts)"
    fi
fi

kill -INT "$CLIENT_PID" 2>/dev/null
wait "$CLIENT_PID" 2>/dev/null
CLIENT_PID=""
wait_memberships "$USER_TOKEN" "$ROOM" 0 20 || log "warning: membership did not clear"

# ===========================================================================
# SCENARIO 2 — the SFU auth service is unavailable (restricted deployment)
# ===========================================================================
# EC's expectation: no call, an error screen, and — crucially — no
# advertised membership. This is the "call creation is restricted to
# authorised users" case, modelled here by taking lk-jwt-service away
# entirely (connection refused is the strongest form of "you may not have a
# token").
log "SCENARIO 2: joining with lk-jwt-service stopped"
ROOM2=$(create_room "$USER_TOKEN")
[ -n "$ROOM2" ] && [ "$ROOM2" != null ] || die "creating the second room"

$ENGINE stop "$AUTH_C" >/dev/null || die "cannot stop the auth-service container"
sleep 1

DENIED_LOG=$LOG_DIR/jwt-unavailable.log
"$JOIN_CALL_BIN" "$HS" "$USER_ID" "$USER_DEV" "$ROOM2" \
    --focus "$JWT" <<<"$USER_TOKEN" >"$DENIED_LOG" 2>&1 &
CLIENT_PID=$!

# Sample continuously rather than once at the end: if the client advertises a
# membership and then dies, the MSC4140 delayed leave clears it again within
# ~8 s, so a single late sample would wrongly report that nothing was ever
# advertised. What matters is whether a membership existed at any point.
MEMBERS=0
for _ in $(seq 1 30); do
    N=$(active_memberships "$USER_TOKEN" "$ROOM2")
    [ "$N" -gt "$MEMBERS" ] 2>/dev/null && MEMBERS=$N
    sleep 0.5
done

if [ "$MEMBERS" = 0 ]; then
    result PASS no-membership-without-sfu-access \
        "nothing was advertised while the SFU could not be reached"
else
    result FAIL no-membership-without-sfu-access \
        "at peak $MEMBERS membership(s) advertised although the SFU JWT service was unreachable \
— on a restricted deployment this shows a participant who can never send media"
fi

if kill -0 "$CLIENT_PID" 2>/dev/null; then
    result INFO client-behaviour-without-sfu "the client is still running (retrying or stuck)"
else
    result INFO client-behaviour-without-sfu "the client exited: $(tail -2 "$DENIED_LOG" | tr '\n' ' ')"
fi

kill -9 "$CLIENT_PID" 2>/dev/null
CLIENT_PID=""

# ===========================================================================
# SCENARIO 3 — recovery once the SFU auth service comes back
# ===========================================================================
log "SCENARIO 3: bringing lk-jwt-service back and rejoining"
$ENGINE start "$AUTH_C" >/dev/null || die "cannot start the auth-service container"
wait_for "$JWT/healthz" 60 lk-jwt-service
# The delayed leave from the aborted attempt has to land before we retry, so
# that we measure a clean join.
sleep 10

RECOVER_LOG=$LOG_DIR/recovered.log
"$JOIN_CALL_BIN" "$HS" "$USER_ID" "$USER_DEV" "$ROOM2" \
    --focus "$JWT" <<<"$USER_TOKEN" >"$RECOVER_LOG" 2>&1 &
CLIENT_PID=$!

if wait_log "$RECOVER_LOG" "connected as" 90; then
    result PASS joins-after-sfu-returns "the client connected once the JWT service was back"
else
    result FAIL joins-after-sfu-returns "the client never connected: $(tail -3 "$RECOVER_LOG" | tr '\n' ' ')"
fi
if wait_memberships "$USER_TOKEN" "$ROOM2" 1 30; then
    result PASS membership-after-sfu-returns "1 active membership"
else
    result FAIL membership-after-sfu-returns \
        "$(active_memberships "$USER_TOKEN" "$ROOM2") active memberships"
fi

kill -INT "$CLIENT_PID" 2>/dev/null
wait "$CLIENT_PID" 2>/dev/null
CLIENT_PID=""

summary
