#!/usr/bin/env bash
# Stop a server started by streams-up.sh:  scripts/streams-down.sh [node|caddy|caddy-auth]
set -euo pipefail
kind="${1:-node}"
port="${STREAMS_PORT:-4437}"
name="${STREAMS_NAME:-$kind-$port}"
pidfile="${TMPDIR:-/tmp}/dcp-streams-$name.pid"
if [ -f "$pidfile" ]; then
  kill "$(cat "$pidfile")" 2>/dev/null || true
  sleep 1
  kill -9 "$(cat "$pidfile")" 2>/dev/null || true
  rm -f "$pidfile"
fi
