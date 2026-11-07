#!/usr/bin/env bash
# Download the Durable Streams Go server (a Caddy build with the durable_streams
# module), verify its pinned checksum, print the path of the binary.
#
# Release v0.3.0 of github.com/durable-streams/durable-streams (Caddy v2.10.2).
# The sha256 is the one in that release's checksums.txt.
set -euo pipefail
version="0.3.0"
sha256="120b03d91ad67adfb265bac72e2d06cae07c333621598b32ee3b0ae21d11acb9"
dir="${STREAMS_CADDY_DIR:-${TMPDIR:-/tmp}/dcp-streams-caddy-$version}"
bin="$dir/durable-streams-server"
if [ ! -x "$bin" ]; then
  mkdir -p "$dir"
  curl -fsSL -o "$dir/ds.tgz" "https://github.com/durable-streams/durable-streams/releases/download/v$version/durable-streams-server_${version}_linux_amd64.tar.gz"
  echo "$sha256  $dir/ds.tgz" | sha256sum -c - >&2
  tar -xzf "$dir/ds.tgz" -C "$dir"
fi
echo "$bin"
