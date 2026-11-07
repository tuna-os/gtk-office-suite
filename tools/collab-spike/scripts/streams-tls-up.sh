#!/usr/bin/env bash
# Start a Durable Streams server behind TLS for the RFC-0004 TLS spike.
#
#   scripts/streams-tls-up.sh node|go
#
#   node  the Node reference server (plain HTTP/1.1, on STREAMS_ORIGIN_PORT) behind a Caddy
#         reverse proxy that terminates TLS (the Node server cannot: it is `node:http`)
#   go    the Go server (Caddy plugin, release v0.3.0): the same Caddy binary terminates TLS itself
#
# Both front ends are the Caddy v2.10.2 of the pinned release (streams-caddy-fetch.sh). Each also
# listens in plain HTTP (STREAMS_PLAIN_PORT) with the SAME handler chain and no TLS, so
# TLS and plain can be compared through the same proxy; for `node` the origin itself is a third,
# direct, plain URL.
#
# Env (defaults):
#   STREAMS_TLS_PORT 4440      STREAMS_PLAIN_PORT 4441      STREAMS_ORIGIN_PORT 4437 (node)
#   STREAMS_TLS_DIR            the PKI directory (scripts/streams-tls-certs.sh; made if missing)
#   STREAMS_DIR                work directory (data, Caddyfile, logs)
#   STREAMS_NAME               pid file label (kind-TLSPORT)
#   STREAMS_SERVER_PROTOCOLS   what the TLS front end offers: "h1 h2" (default) or "h1" ("h2" alone makes
#                              Caddy 2.10.2 close every TLS handshake, see the results document)
#   STREAMS_LONG_POLL_S 30     the server's long-poll timeout (and Node's SSE keep-alive interval)
#   STREAMS_SSE_RECONNECT_S 60 how long the Go server keeps one SSE response open
#   STREAMS_UPSTREAM_KEEPALIVE 3s   idle time of the proxy -> Node connection (Node closes at 5 s)
#   STREAMS_IN_MEMORY=1        no data directory (restart tests: everything is lost)
#   STREAMS_KEEP_DATA=1        do not wipe STREAMS_DIR/data first (restart tests)
#   STREAMS_NO_ORIGIN=1        node: start the front end only (every request is a proxy error)
#   STREAMS_ACCESS_LOG=0       no Caddy `log` directive. With it, the Go server's SSE answers 500
#                              "streaming not supported" (the logging writer is not an http.Flusher)
#   STREAMS_CERT=server|expired  which certificate of the PKI the TLS listener presents
#                              (expired: signed by the CA, for localhost, expired yesterday)
#   STREAMS_EXPORT_PREFIX=P    name the exports P_URL, P_PLAIN_URL, P_DIRECT_URL, P_LOG, P_ACCESS_LOG,
#                              P_PIDFILE instead of DCP_STREAMS_* (to run several servers in one shell)
#
# Prints `export` lines (so `eval "$(scripts/streams-tls-up.sh node)"` works) and leaves the
# processes running; stop them with streams-tls-down.sh.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
kind="${1:-}"
case "$kind" in node|go) ;; *) echo "usage: $0 node|go" >&2; exit 2 ;; esac
tls_port="${STREAMS_TLS_PORT:-4440}"
plain_port="${STREAMS_PLAIN_PORT:-4441}"
origin_port="${STREAMS_ORIGIN_PORT:-4437}"
protocols="${STREAMS_SERVER_PROTOCOLS:-h1 h2}"
long_poll_s="${STREAMS_LONG_POLL_S:-30}"
sse_s="${STREAMS_SSE_RECONNECT_S:-60}"
upstream_ka="${STREAMS_UPSTREAM_KEEPALIVE:-3s}"
name="${STREAMS_NAME:-tls-$kind-$tls_port}"
dir="${STREAMS_DIR:-${TMPDIR:-/tmp}/dcp-streams-tls-$kind-$tls_port}"
pki="${STREAMS_TLS_DIR:-${TMPDIR:-/tmp}/dcp-streams-pki}"
pidfile="${TMPDIR:-/tmp}/dcp-streams-$name.pid"
log="$dir/server.log"
access="$dir/access.log"
cert="${STREAMS_CERT:-server}"
pfx="${STREAMS_EXPORT_PREFIX:-DCP_STREAMS}"

if [ "${STREAMS_KEEP_DATA:-}" = 1 ]; then mkdir -p "$dir"; rm -f "$log" "$access"; else rm -rf "$dir"; mkdir -p "$dir"; fi
[ -f "$pki/server.pem" ] || "$here/streams-tls-certs.sh" "$pki" >&2
bin="$("$here/streams-caddy-fetch.sh")"
: >"$pidfile"

data_node="$dir/data"
data_go_tls="$dir/data-tls"
data_go_plain="$dir/data-plain"
mkdir -p "$data_node" "$data_go_tls" "$data_go_plain"

log_block=""
if [ "${STREAMS_ACCESS_LOG:-1}" != 0 ]; then
  log_block="	log {
		output file $access
		format json
	}"
fi

idle_block=""
if [ -n "${STREAMS_IDLE_TIMEOUT_S:-}" ]; then
  idle_block="		timeouts {
			idle ${STREAMS_IDLE_TIMEOUT_S}s
		}"
fi

if [ "$kind" = node ]; then
  chain="reverse_proxy 127.0.0.1:$origin_port {
		flush_interval -1
		transport http {
			keepalive $upstream_ka
		}
	}"
  chain_tls="$chain"
  chain_plain="$chain"
else
  dsopts() { # data dir
    local d=""
    [ "${STREAMS_IN_MEMORY:-}" = 1 ] || d="data_dir $1"
    printf 'durable_streams {\n\t\t\t%s\n\t\t\tlong_poll_timeout %ss\n\t\t\tsse_reconnect_interval %ss\n\t\t}' "$d" "$long_poll_s" "$sse_s"
  }
  chain_tls="route /v1/stream/* {
		$(dsopts "$data_go_tls")
	}"
  chain_plain="route /v1/stream/* {
		$(dsopts "$data_go_plain")
	}"
fi

cat >"$dir/Caddyfile" <<CADDY
{
	admin off
	auto_https off
	servers :$tls_port {
		protocols $protocols
$idle_block
	}
	servers :$plain_port {
		protocols h1
	}
}

http://:$plain_port {
$log_block
	$chain_plain
}

https://:$tls_port {
	tls $pki/$cert.pem $pki/$cert.key
$log_block
	$chain_tls
}
CADDY

if [ "$kind" = node ] && [ "${STREAMS_NO_ORIGIN:-}" != 1 ]; then
  [ -d "$here/streams-server/node_modules/@durable-streams/server" ] || { echo "run 'npm ci' in $here/streams-server first" >&2; exit 1; }
  nargs=(--port "$origin_port" --long-poll-ms "$((long_poll_s * 1000))")
  [ "${STREAMS_IN_MEMORY:-}" = 1 ] || nargs+=(--data-dir "$data_node")
  cd "$here/streams-server"
  nohup node server.mjs "${nargs[@]}" >"$dir/origin.log" 2>&1 &
  echo $! >>"$pidfile"
  cd "$here"
  for _ in $(seq 1 60); do grep -q 'READY http' "$dir/origin.log" 2>/dev/null && break; sleep 0.25; done
  grep -q 'READY http' "$dir/origin.log" || { echo "the Node origin did not become ready:" >&2; cat "$dir/origin.log" >&2; exit 1; }
fi

nohup "$bin" run --config "$dir/Caddyfile" >"$log" 2>&1 &
echo $! >>"$pidfile"
disown || true

ready=0
for _ in $(seq 1 80); do
  if grep -q 'serving initial configuration' "$log" 2>/dev/null \
     && (exec 3<>"/dev/tcp/127.0.0.1/$tls_port") 2>/dev/null \
     && (exec 3<>"/dev/tcp/127.0.0.1/$plain_port") 2>/dev/null; then ready=1; break; fi
  if ! kill -0 "$(tail -n1 "$pidfile")" 2>/dev/null; then echo "caddy exited:" >&2; cat "$log" >&2; exit 1; fi
  sleep 0.25
done
[ "$ready" = 1 ] || { echo "caddy did not become ready:" >&2; cat "$log" >&2; exit 1; }

echo "# $kind behind TLS ($protocols) on port $tls_port, plain on $plain_port, pids $(tr '\n' ' ' <"$pidfile"), dir $dir" >&2
echo "export ${pfx}_URL=https://localhost:$tls_port/v1/stream/"
echo "export ${pfx}_PLAIN_URL=http://127.0.0.1:$plain_port/v1/stream/"
[ "$kind" = node ] && echo "export ${pfx}_DIRECT_URL=http://127.0.0.1:$origin_port/v1/stream/"
echo "export DCP_STREAMS_CA=$pki/ca.pem"
echo "export DCP_STREAMS_OTHER_CA=$pki/other-ca.pem"
echo "export DCP_STREAMS_TLS_DIR=$pki"
echo "export DCP_STREAMS_KIND=$kind"
echo "export ${pfx}_LOG=$log"
echo "export ${pfx}_ACCESS_LOG=$access"
echo "export ${pfx}_PIDFILE=$pidfile"
