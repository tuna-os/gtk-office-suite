#!/usr/bin/env bash
# Stop what streams-tls-up.sh started:  scripts/streams-tls-down.sh [-9] PIDFILE
# (the path it printed as DCP_STREAMS_PIDFILE). -9: SIGKILL at once, no goodbye.
set -euo pipefail
sig=TERM
if [ "${1:-}" = -9 ]; then sig=KILL; shift; fi
pidfile="${1:?usage: streams-tls-down.sh [-9] PIDFILE}"
[ -f "$pidfile" ] || exit 0
pids="$(cat "$pidfile")"
for p in $pids; do kill "-$sig" "$p" 2>/dev/null || true; done
if [ "$sig" = TERM ]; then
  for _ in $(seq 1 40); do
    alive=0
    for p in $pids; do kill -0 "$p" 2>/dev/null && alive=1; done
    [ "$alive" = 0 ] && break
    sleep 0.25
  done
  for p in $pids; do kill -9 "$p" 2>/dev/null || true; done
fi
rm -f "$pidfile"
