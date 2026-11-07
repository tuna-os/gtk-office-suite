#!/usr/bin/env bash
# Start one or two throw-away Nextcloud instances (PostgreSQL) on one docker
# network for the RFC-0004 Spike E tests.
#
#   scripts/nextcloud-up.sh a [b]
#
# Each instance X is a container `nc-X` (apache, port 80) with a database
# container `dcp-nc-db-X`. Nothing is published: the runner reaches an instance
# at http://nc-X/ because this script appends `<container ip> nc-X` to
# /etc/hosts, and the containers reach each other at the same names through the
# network's DNS, so a URL means the same thing from every side.
#
# Env: NEXTCLOUD_IMAGE (required, pin it)  POSTGRES_IMAGE (required)
#      DCP_NC_NETWORK (dcp-nc)  DCP_NC_ADMIN_PASSWORD  DCP_NC_USER_PASSWORD
#      DCP_NC_FEDERATION (1: allow federated shares between local http servers)
#
# Users created on every instance: alice, bob, carol, dave (all with
# DCP_NC_USER_PASSWORD). Prints the pinned versions on stderr.
set -euo pipefail

: "${NEXTCLOUD_IMAGE:?set NEXTCLOUD_IMAGE (a pinned image reference)}"
: "${POSTGRES_IMAGE:?set POSTGRES_IMAGE (a pinned image reference)}"
net="${DCP_NC_NETWORK:-dcp-nc}"
admin_pw="${DCP_NC_ADMIN_PASSWORD:-dcp-admin-Passw0rd-spike}"
user_pw="${DCP_NC_USER_PASSWORD:-dcp-user-Passw0rd-spike}"
federation="${DCP_NC_FEDERATION:-1}"
[ "$#" -ge 1 ] || { echo "usage: $0 a [b]" >&2; exit 2; }

docker network inspect "$net" >/dev/null 2>&1 || docker network create "$net" >/dev/null

occ() { local inst="$1"; shift; docker exec -u www-data "nc-$inst" php occ "$@"; }

for inst in "$@"; do
  docker rm -f "nc-$inst" "dcp-nc-db-$inst" >/dev/null 2>&1 || true
  docker run -d --name "dcp-nc-db-$inst" --network "$net" \
    -e POSTGRES_USER=nextcloud -e POSTGRES_PASSWORD=nextcloud -e POSTGRES_DB=nextcloud \
    "$POSTGRES_IMAGE" >/dev/null
  # `dcp-host` is the runner, for the OCM receiver helper that runs there.
  docker run -d --name "nc-$inst" --network "$net" --add-host dcp-host:host-gateway \
    -e POSTGRES_HOST="dcp-nc-db-$inst" -e POSTGRES_DB=nextcloud \
    -e POSTGRES_USER=nextcloud -e POSTGRES_PASSWORD=nextcloud \
    -e NEXTCLOUD_ADMIN_USER=admin -e NEXTCLOUD_ADMIN_PASSWORD="$admin_pw" \
    -e NEXTCLOUD_TRUSTED_DOMAINS="nc-$inst" \
    "$NEXTCLOUD_IMAGE" >/dev/null
done

for inst in "$@"; do
  ip="$(docker inspect -f "{{(index .NetworkSettings.Networks \"$net\").IPAddress}}" "nc-$inst")"
  grep -q " nc-$inst\$" /etc/hosts || echo "$ip nc-$inst" | sudo tee -a /etc/hosts >/dev/null
  echo "# nc-$inst at $ip" >&2
done

for inst in "$@"; do
  ready=0
  for i in $(seq 1 180); do
    if curl -fsS "http://nc-$inst/status.php" 2>/dev/null | grep -q '"installed":true'; then ready=1; break; fi
    sleep 2
  done
  if [ "$ready" != 1 ]; then
    echo "nextcloud nc-$inst did not become ready" >&2
    docker logs "nc-$inst" 2>&1 | tail -50 >&2
    exit 1
  fi
  echo "# nc-$inst ready after about $((i * 2))s: $(curl -fsS "http://nc-$inst/status.php")" >&2

  occ "$inst" config:system:set overwrite.cli.url --value="http://nc-$inst" >&2
  occ "$inst" config:system:set auth.bruteforce.protection.enabled --value=false --type=boolean >&2
  occ "$inst" config:system:set ratelimit.protection.enabled --value=false --type=boolean >&2
  occ "$inst" config:system:set loglevel --value=1 --type=integer >&2
  if [ "$federation" = 1 ]; then
    # Servers on a private network speaking http: Nextcloud refuses both by
    # default (SSRF protection; https only).
    occ "$inst" config:system:set allow_local_remote_servers --value=true --type=boolean >&2
    occ "$inst" config:system:set sharing.federation.allowHttpFallback --value=true --type=boolean >&2
  fi
  for u in alice bob carol dave; do
    OC_PASS="$user_pw" docker exec -u www-data -e OC_PASS "nc-$inst" php occ user:add --password-from-env --display-name "$u" "$u" >&2
  done
  echo "# nc-$inst: $(occ "$inst" status 2>&1 | tr '\n' ' ')" >&2
done
