#!/usr/bin/env bash
# Start Collabora Online (collabora/code) for the Spike D real-editor test.
#
# Usage: collabora-up.sh            start (or restart) the container and wait
#        collabora-up.sh down       stop and remove it
#
# The container uses the host network so that it reaches the WopiHost the test
# starts on 127.0.0.1:$DCP_WOPI_HOST_PORT, and the test reaches it on
# 127.0.0.1:9980. TLS is off: this is a loopback test, never a deployment.
#
# Environment:
#   CONTAINER_ENGINE   docker or podman (default: whichever exists)
#   COLLABORA_IMAGE    default collabora/code:latest
#   COLLABORA_NAME     container name, default dcp-collabora
#   DCP_WOPI_HOST_PORT port of the WOPI host the test starts (default 18080)
#   COLLABORA_LOCK_REFRESH_SECS storage.wopi.locking.refresh (default 10 here;
#                      Collabora's own default is 900), so a refresh is observable
#   COLLABORA_READY_TIMEOUT seconds to wait for discovery (default 180)
#
# Exits non-zero, with the container log, when Collabora does not come up. It
# never reports success without having fetched /hosting/discovery.
set -euo pipefail

ENGINE="${CONTAINER_ENGINE:-}"
if [ -z "$ENGINE" ]; then
  if command -v docker >/dev/null 2>&1; then ENGINE=docker
  elif command -v podman >/dev/null 2>&1; then ENGINE=podman
  else
    echo "collabora-up.sh: neither docker nor podman is installed" >&2
    exit 2
  fi
fi
IMAGE="${COLLABORA_IMAGE:-collabora/code:latest}"
NAME="${COLLABORA_NAME:-dcp-collabora}"
PORT="${DCP_WOPI_HOST_PORT:-18080}"
TIMEOUT="${COLLABORA_READY_TIMEOUT:-180}"

"$ENGINE" rm -f "$NAME" >/dev/null 2>&1 || true
if [ "${1:-up}" = "down" ]; then
  exit 0
fi

# aliasgroup1 is the docker image's way to fill storage.wopi.alias_groups: the
# WOPI host origins Collabora may call back. The entrypoint escapes the dots.
"$ENGINE" run -d --name "$NAME" \
  --network host \
  --cap-add MKNOD \
  -e "aliasgroup1=http://127.0.0.1:${PORT}" \
  -e "extra_params=--o:ssl.enable=false --o:ssl.termination=false --o:storage.wopi.locking.refresh=${COLLABORA_LOCK_REFRESH_SECS:-10}" \
  -e "username=admin" -e "password=admin" \
  "$IMAGE"

echo "collabora-up.sh: waiting up to ${TIMEOUT}s for http://127.0.0.1:9980/hosting/discovery"
for _ in $(seq 1 "$TIMEOUT"); do
  if curl -fsS -o /dev/null "http://127.0.0.1:9980/hosting/discovery" 2>/dev/null; then
    echo "collabora-up.sh: Collabora is up ($IMAGE)"
    "$ENGINE" logs "$NAME" 2>&1 | grep -m1 -i "coolwsd-[0-9]" || true
    exit 0
  fi
  if [ "$("$ENGINE" inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null)" != "true" ]; then
    break
  fi
  sleep 1
done
echo "collabora-up.sh: Collabora did not become ready; container log follows" >&2
"$ENGINE" logs "$NAME" 2>&1 | tail -80 >&2
exit 1
