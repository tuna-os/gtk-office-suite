#!/usr/bin/env bash
# What the SERVER says it served, from Caddy's JSON access log (independent of what the client
# claims): every request that arrived over TLS, by HTTP protocol, ALPN, TLS version and cipher.
#
#   scripts/streams-tls-check-log.sh ACCESS_LOG HTTP/2.0|HTTP/1.1 [MAX_OTHER=4] [MIN=200]
#
# Fails unless: at least MIN TLS requests used the expected protocol; at most MAX_OTHER used the
# other one (the tests send a few on purpose, as controls); every TLS request was TLS 1.3.
set -euo pipefail
log="${1:?access log}"
want="${2:?HTTP/2.0 or HTTP/1.1}"
max_other="${3:-4}"
min="${4:-200}"
command -v jq >/dev/null || { echo "jq is needed" >&2; exit 2; }

echo "TLS requests in $log by protocol / ALPN / TLS version / cipher suite / resumed:"
jq -r 'select(.request.tls != null) | [.request.proto, .request.tls.proto, .request.tls.version, .request.tls.cipher_suite, .request.tls.resumed] | @tsv' "$log" \
  | sort | uniq -c | sort -rn
echo "plain-HTTP requests in the same log (the front end's plain listener):" \
  "$(jq -c 'select(.request.tls == null)' "$log" | wc -l)"

n_want="$(jq -c --arg p "$want" 'select(.request.tls != null and .request.proto == $p)' "$log" | wc -l)"
n_other="$(jq -c --arg p "$want" 'select(.request.tls != null and .request.proto != $p)' "$log" | wc -l)"
n_not13="$(jq -c 'select(.request.tls != null and .request.tls.version != 772)' "$log" | wc -l)"
echo "expected protocol $want: $n_want   other: $n_other   not TLS 1.3: $n_not13"
[ "$n_want" -ge "$min" ] || { echo "::error::only $n_want TLS requests used $want (need $min)"; exit 1; }
[ "$n_other" -le "$max_other" ] || { echo "::error::$n_other TLS requests used another protocol (at most $max_other are deliberate controls)"; exit 1; }
[ "$n_not13" -eq 0 ] || { echo "::error::$n_not13 TLS requests were not TLS 1.3"; exit 1; }
