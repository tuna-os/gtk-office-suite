#!/usr/bin/env bash
# Reproduces the "no incoming-call GUI in Mandelbrot" report as an automated
# test, and pinpoints WHERE the ring is lost.
#
# Mandelbrot rings when the SDK's notification handler fires, and that only
# happens if the homeserver decides an m.rtc.notification (MSC4075) event
# should notify the callee — i.e. a push rule matches with a `notify` action.
# This test sends a real m.rtc.notification and asks the server, via the
# client API, whether it flagged it for the callee. No GUI, no desktop
# notification daemon — those are downstream of the question this answers.
#
# RESULT lines: PASS / FAIL. Exit 1 if any RESULT is FAIL.
#
# Usage: ./run-incoming-call-notification.sh
set -euo pipefail
cd "$(dirname "$0")"

HS=http://127.0.0.1:8008

log() { printf '[incoming-call] %s\n' "$*"; }
fail() { printf '[incoming-call] ERROR: %s\n' "$*" >&2; exit 2; }
result() { printf 'RESULT: %s %s\n' "$1" "$2"; [ "$1" = FAIL ] && FAILED=1 || true; }
FAILED=0

# --- compose provider (same detection as the other harness scripts) ---------
# A preset COMPOSE (or the older COMPOSE_OVERRIDE) forces the provider and
# must win over detection; see harness-lib.sh for why detection alone is not
# trustworthy on a host that has podman installed but not running.
if [ -n "${COMPOSE_OVERRIDE:-}" ]; then COMPOSE=$COMPOSE_OVERRIDE
elif [ -n "${COMPOSE:-}" ]; then :
elif command -v podman-compose >/dev/null; then COMPOSE="podman-compose"
elif podman compose version >/dev/null 2>&1; then COMPOSE="podman compose"
elif docker compose version >/dev/null 2>&1; then COMPOSE="docker compose"
else fail "no compose provider found"; fi
log "using compose provider: $COMPOSE"

cleanup() { [ "${KEEP_STACK:-0}" = 1 ] || $COMPOSE -f compose.yml down -v >/dev/null 2>&1 || true; }
trap cleanup EXIT

$COMPOSE -f compose.yml up -d
for _ in $(seq 1 60); do curl -sf "$HS/_matrix/client/versions" >/dev/null && break; sleep 1; done

# --- users + room -----------------------------------------------------------
SUFFIX=$(date +%s)
register() {
    curl -sf -X POST "$HS/_matrix/client/v3/register" \
        -H 'Content-Type: application/json' \
        -d "{\"username\":\"$1\",\"password\":\"testpassword\",\"auth\":{\"type\":\"m.login.dummy\"}}"
}
ALICE=$(register "alice-$SUFFIX") || fail "registering alice"
BOB=$(register "bob-$SUFFIX") || fail "registering bob"
ALICE_ID=$(jq -r .user_id <<<"$ALICE"); ALICE_TOKEN=$(jq -r .access_token <<<"$ALICE")
BOB_ID=$(jq -r .user_id <<<"$BOB"); BOB_TOKEN=$(jq -r .access_token <<<"$BOB")
BOB_DEV=$(jq -r .device_id <<<"$BOB")
log "caller $ALICE_ID, callee $BOB_ID"

ROOM=$(curl -sf -X POST "$HS/_matrix/client/v3/createRoom" \
    -H "Authorization: Bearer $ALICE_TOKEN" -H 'Content-Type: application/json' \
    -d "{\"invite\":[\"$BOB_ID\"],\"is_direct\":true}" | jq -r .room_id)
curl -sf -X POST "$HS/_matrix/client/v3/join/$ROOM" \
    -H "Authorization: Bearer $BOB_TOKEN" -H 'Content-Type: application/json' -d '{}' >/dev/null
# Prime the callee's sync so any since-token logic has a baseline.
curl -sf "$HS/_matrix/client/v3/sync?timeout=0" -H "Authorization: Bearer $BOB_TOKEN" >/dev/null

# --- the ring: caller sends an m.rtc.notification (MSC4075) -----------------
# Element uses this to ring the callee for a 1:1 MatrixRTC call. m.mentions
# targets the callee so a mention push rule could also catch it.
TXN="ring-$SUFFIX"
EVENT=$(curl -sf -X PUT \
    "$HS/_matrix/client/v3/rooms/$ROOM/send/m.rtc.notification/$TXN" \
    -H "Authorization: Bearer $ALICE_TOKEN" -H 'Content-Type: application/json' \
    -d "{\"notification_type\":\"ring\",\"m.mentions\":{\"user_ids\":[\"$BOB_ID\"]},\"lifetime\":30000,\"m.relates_to\":{\"rel_type\":\"m.reference\",\"event_id\":\"\$dummy\"}}" \
    | jq -r .event_id) || fail "sending m.rtc.notification (server rejected the event type)"
log "sent m.rtc.notification $EVENT"
sleep 3

# --- Q1: did the server flag it for the callee? -----------------------------
# GET /notifications returns exactly the events the server's push rules said to
# notify about. If our ring is here, the notify path works and any GUI failure
# is downstream (client). If it is absent, the ring never becomes a
# notification and no client code can make it ring.
NOTIFS=$(curl -sf "$HS/_matrix/client/v3/notifications" -H "Authorization: Bearer $BOB_TOKEN")
if jq -e --arg e "$EVENT" '.notifications[]?.event.event_id == $e' <<<"$NOTIFS" | grep -q true; then
    result PASS "homeserver flagged the m.rtc.notification for the callee (notify path works; investigate the client)"
else
    result FAIL "homeserver did NOT flag the m.rtc.notification for the callee (no matching push rule -> ring is lost server-side)"
fi

# --- Q2: is there any push rule that even mentions the call event type? ------
RULES=$(curl -sf "$HS/_matrix/client/v3/pushrules/" -H "Authorization: Bearer $BOB_TOKEN")
if jq -e '.. | objects | select((.rule_id? // "" | test("call|rtc")) or ((.conditions? // []) | tostring | test("m.rtc.notification|m.call")))' <<<"$RULES" >/dev/null 2>&1; then
    result PASS "the callee's account has a push rule referencing calls/rtc"
else
    result FAIL "the callee's account has NO push rule for calls/rtc — Mandelbrot must install one on login (as Element does) for incoming calls to ring"
fi

echo "----"
[ "$FAILED" = 0 ] && { log "incoming-call notification path is healthy"; exit 0; }
log "incoming-call notification is lost — see RESULT lines above"; exit 1
