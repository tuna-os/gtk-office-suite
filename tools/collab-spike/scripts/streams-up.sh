#!/usr/bin/env bash
# Start a throw-away Durable Streams server for the RFC-0004 Spike B tests.
#
#   scripts/streams-up.sh [node|caddy|caddy-auth]      (default: node)
#
#   node        the Node reference server, @durable-streams/server pinned in
#               scripts/streams-server/package-lock.json (run `npm ci` there once)
#   caddy       the Go server (Caddy plugin), release v0.3.0, sha256 pinned in
#               scripts/streams-caddy-fetch.sh, file-backed, no access control
#   caddy-auth  the same behind Caddy's own basic_auth: user `reader` may only
#               GET/HEAD, user `writer` may do anything (the strongest gate a
#               Caddyfile gives without a custom module; per-stream rules need
#               a forward_auth service, see the results document)
#
# Env: STREAMS_PORT (4437)  STREAMS_DIR (scratch dir)  STREAMS_NAME (pid file label)
#
# Prints `export` lines (so `eval "$(scripts/streams-up.sh node)"` works) and
# leaves the server running in the background; stop it with streams-down.sh.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
kind="${1:-node}"
port="${STREAMS_PORT:-4437}"
dir="${STREAMS_DIR:-${TMPDIR:-/tmp}/dcp-streams-$kind-$port}"
name="${STREAMS_NAME:-$kind-$port}"
pidfile="${TMPDIR:-/tmp}/dcp-streams-$name.pid"
log="$dir/server.log"

rm -rf "$dir"
mkdir -p "$dir/data"

case "$kind" in
  node)
    [ -d "$here/streams-server/node_modules/@durable-streams/server" ] || { echo "run 'npm ci' in $here/streams-server first" >&2; exit 1; }
    cd "$here/streams-server"
    nohup node server.mjs --port "$port" --data-dir "$dir/data" >"$log" 2>&1 &
    echo $! >"$pidfile"
    ready='READY http'
    ;;
  caddy|caddy-auth)
    bin="$("$here/streams-caddy-fetch.sh")"
    if [ "$kind" = caddy-auth ]; then
      rh="$("$bin" hash-password --plaintext reader-secret)"
      wh="$("$bin" hash-password --plaintext writer-secret)"
      cat >"$dir/Caddyfile" <<CADDY
{
	admin off
	auto_https off
}

:$port {
	@read method GET HEAD OPTIONS
	@write not method GET HEAD OPTIONS
	basic_auth @read {
		reader $rh
		writer $wh
	}
	basic_auth @write {
		writer $wh
	}
	route /v1/stream/* {
		durable_streams {
			data_dir $dir/data
		}
	}
}
CADDY
    else
      cat >"$dir/Caddyfile" <<CADDY
{
	admin off
	auto_https off
}

:$port {
	route /v1/stream/* {
		durable_streams {
			data_dir $dir/data
		}
	}
}
CADDY
    fi
    nohup "$bin" run --config "$dir/Caddyfile" >"$log" 2>&1 &
    echo $! >"$pidfile"
    ready='serving initial configuration'
    ;;
  *) echo "kind must be node, caddy or caddy-auth" >&2; exit 2 ;;
esac
disown || true

for i in $(seq 1 60); do
  if grep -q "$ready" "$log" 2>/dev/null && (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then ok=1; break; fi
  if ! kill -0 "$(cat "$pidfile")" 2>/dev/null; then echo "server exited:" >&2; cat "$log" >&2; exit 1; fi
  sleep 0.5
done
[ "${ok:-}" = 1 ] || { echo "server did not become ready:" >&2; cat "$log" >&2; exit 1; }

echo "# $kind server on port $port, pid $(cat "$pidfile"), data $dir/data, log $log" >&2
echo "export DCP_STREAMS_URL=http://127.0.0.1:$port/v1/stream/"
echo "export DCP_STREAMS_LOG=$log"
echo "export DCP_STREAMS_PIDFILE=$pidfile"
