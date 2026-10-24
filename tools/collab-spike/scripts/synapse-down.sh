#!/usr/bin/env bash
# Stop and remove the Synapse started by synapse-up.sh.
#   scripts/synapse-down.sh [default|relaxed]
set -uo pipefail
profile="${1:-relaxed}"
name="${SYNAPSE_NAME:-dcp-synapse-$profile}"
dir="${SYNAPSE_DIR:-${TMPDIR:-/tmp}/dcp-synapse-$profile}"
image="${SYNAPSE_IMAGE:-docker.io/matrixdotorg/synapse:latest}"
engine="${DCP_CONTAINER_ENGINE:-}"
if [ -z "$engine" ]; then
  if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then engine=docker
  elif command -v podman >/dev/null 2>&1; then engine=podman
  else echo "no container engine" >&2; exit 1; fi
fi
"$engine" rm -f "$name" >/dev/null 2>&1
# Files the container wrote are owned by root under docker; let it delete them.
rm -rf "$dir" 2>/dev/null || "$engine" run --rm -v "$dir:/d:Z" --entrypoint sh "$image" -c 'rm -rf /d/* /d/.[!.]*' >/dev/null 2>&1
rm -rf "$dir" 2>/dev/null
exit 0
