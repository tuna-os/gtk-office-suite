#!/usr/bin/env bash
# Start a throw-away Synapse for the RFC-0004 Spike A tests.
#
#   scripts/synapse-up.sh [default|relaxed]      (profile; default: relaxed)
#
# Env: SYNAPSE_PORT (8008)  SYNAPSE_IMAGE (docker.io/matrixdotorg/synapse:latest)
#      SYNAPSE_SHARED_SECRET  SYNAPSE_DIR  DCP_CONTAINER_ENGINE (docker|podman;
#      auto: docker if it works, else podman)  SYNAPSE_NAME
#
# On success prints `export` lines for DCP_SYNAPSE_URL & co. and writes them to
# $SYNAPSE_DIR/env, so `eval "$(scripts/synapse-up.sh relaxed)"` works.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
profile="${1:-relaxed}"
case "$profile" in default|relaxed) ;; *) echo "profile must be default or relaxed" >&2; exit 2;; esac

port="${SYNAPSE_PORT:-8008}"
image="${SYNAPSE_IMAGE:-docker.io/matrixdotorg/synapse:latest}"
secret="${SYNAPSE_SHARED_SECRET:-dcp-spike-shared-secret}"
name="${SYNAPSE_NAME:-dcp-synapse-$profile}"
dir="${SYNAPSE_DIR:-${TMPDIR:-/tmp}/dcp-synapse-$profile}"
server_name="localhost"

engine="${DCP_CONTAINER_ENGINE:-}"
if [ -z "$engine" ]; then
  if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then engine=docker
  elif command -v podman >/dev/null 2>&1; then engine=podman
  else echo "neither a working docker nor podman found" >&2; exit 1; fi
fi
echo "# container engine: $engine ($($engine --version))" >&2

"$engine" rm -f "$name" >/dev/null 2>&1 || true
rm -rf "$dir" 2>/dev/null || "$engine" run --rm -v "$dir:/d:Z" --entrypoint sh "$image" -c 'rm -rf /d/* /d/.[!.]*' 2>/dev/null || true
mkdir -p "$dir"
chmod 777 "$dir"

rnd() { head -c 24 /dev/urandom | base64 | tr -d '/+=\n'; }
sed -e "s|__SERVER_NAME__|$server_name|g" \
    -e "s|__PORT__|$port|g" \
    -e "s|__MACAROON__|$(rnd)|" \
    -e "s|__FORM__|$(rnd)|" \
    -e "s|__SHARED_SECRET__|$secret|" \
    "$here/synapse.homeserver.yaml.tmpl" > "$dir/homeserver.yaml"
cat "$here/synapse.ratelimits.$profile.yaml" >> "$dir/homeserver.yaml"
cp "$here/synapse.log.config" "$dir/log.config"
chmod 644 "$dir/homeserver.yaml" "$dir/log.config"

"$engine" pull -q "$image" >/dev/null 2>&1 || true
# Signing key: written by Synapse itself.
"$engine" run --rm -v "$dir:/data:Z" --entrypoint python "$image" \
  -m synapse.app.homeserver --config-path /data/homeserver.yaml --generate-keys >&2

"$engine" run -d --name "$name" -p "127.0.0.1:$port:8008" -v "$dir:/data:Z" \
  -e SYNAPSE_CONFIG_PATH=/data/homeserver.yaml -e UID=0 -e GID=0 "$image" >/dev/null

url="http://127.0.0.1:$port"
for i in $(seq 1 90); do
  if curl -fsS "$url/health" >/dev/null 2>&1; then ready=1; break; fi
  sleep 1
done
if [ "${ready:-0}" != 1 ]; then
  echo "Synapse did not become healthy in 90 s; container log:" >&2
  "$engine" logs "$name" >&2 || true
  exit 1
fi

version="$(curl -fsSI "$url/_matrix/client/versions" | tr -d '\r' | awk -F': ' 'tolower($1)=="server"{print $2}')"
image_id="$("$engine" image inspect --format '{{.Id}}' "$image" 2>/dev/null || echo unknown)"
{
  echo "export DCP_SYNAPSE_URL=$url"
  echo "export DCP_SYNAPSE_SHARED_SECRET=$secret"
  echo "export DCP_SYNAPSE_PROFILE=$profile"
  echo "export DCP_SYNAPSE_CONTAINER=$name"
  echo "export DCP_CONTAINER_ENGINE=$engine"
  echo "export DCP_SYNAPSE_VERSION='$version'"
  echo "export DCP_SYNAPSE_IMAGE_ID='$image_id'"
} | tee "$dir/env"
echo "# Synapse up: $version, profile=$profile, image=$image ($image_id)" >&2
