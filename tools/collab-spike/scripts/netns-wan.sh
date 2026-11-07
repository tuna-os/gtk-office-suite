#!/usr/bin/env bash
# Network namespaces for the RFC-0004 relay spike (docs/rfc/0004-spike-relay-results.md).
# Needs root and iproute2 + iptables + openssl; CI runs it with sudo.
#
#   dcp-a, dcp-b, dcp-c   one peer each: veth vn-X, address 10.77.I.2/24 (I = 1, 2, 3)
#   dcp-r                 the "internet": veth vr-X, 10.77.I.1/24, forwarding on, and the
#                         relay at 10.77.0.1 (on dcp-r's loopback; HTTPS 443, QAD UDP 7842)
#
# A peer reaches the relay in one hop and another peer in two hops, both through dcp-r.
# Between peers, dcp-r forwards (mode `direct`) or drops everything (mode `relay-only`).
# Relay traffic is INPUT/OUTPUT on dcp-r, so the FORWARD rules never touch it.
#
#   netns-wan.sh up | down
#   netns-wan.sh mode relay-only|direct       (can be switched while peers run)
#   netns-wan.sh shape <rtt_ms> <loss_pct> <rate>   (rate: e.g. 1mbit, or - for none; 0 0 - clears)
#   netns-wan.sh relay-up | relay-down | relay-metrics
#   netns-wan.sh counters                     (FORWARD rule hit counters on dcp-r)
#   netns-wan.sh egress open|tcp443-only|none (what the peers may send out; OUTPUT rules in dcp-a/b/c)
#   netns-wan.sh egress-counters              (hit counters of those rules: what was tried and refused)
#   netns-wan.sh check                        (ping and TCP reachability matrix)
#
# Shaping: netem on both ends of every veth. With R the requested peer-to-peer RTT, every
# veth end gets delay R/4, so a peer-to-peer path (two links, four crossings per round trip)
# has RTT R, a peer-to-relay path R/2, and a relayed peer-to-peer path (peer -> relay -> peer)
# R as well. Loss is L/2 per veth end, so one direction of a two-link path loses about L;
# the single link to the relay loses L/2 per direction. Rate applies to every veth end.
set -euo pipefail

WORK=${DCP_WAN_DIR:-/tmp/dcp-wan}
RELAY_IP=10.77.0.1
NODES="a b c"
idx() { case $1 in a) echo 1;; b) echo 2;; c) echo 3;; esac; }

r() { ip netns exec dcp-r "$@"; }

up() {
  mkdir -p "$WORK"
  for ns in dcp-r dcp-a dcp-b dcp-c; do ip netns add "$ns"; done
  for ns in dcp-r dcp-a dcp-b dcp-c; do
    ip netns exec "$ns" ip link set lo up
    ip netns exec "$ns" sysctl -qw net.ipv6.conf.all.disable_ipv6=1
  done
  r ip addr add "$RELAY_IP/32" dev lo
  r sysctl -qw net.ipv4.ip_forward=1
  for n in $NODES; do
    i=$(idx "$n")
    ip link add "vr-$n" type veth peer name "vn-$n" netns "dcp-$n"
    ip link set "vr-$n" netns dcp-r
    r ip addr add "10.77.$i.1/24" dev "vr-$n"
    r ip link set "vr-$n" up
    ip netns exec "dcp-$n" ip addr add "10.77.$i.2/24" dev "vn-$n"
    ip netns exec "dcp-$n" ip link set "vn-$n" up
    ip netns exec "dcp-$n" ip route add default via "10.77.$i.1"
  done
  mode direct
  echo "namespaces up"
}

down() {
  relay-down || true
  for ns in dcp-a dcp-b dcp-c dcp-r; do ip netns del "$ns" 2>/dev/null || true; done
  echo "namespaces down"
}

mode() {
  r iptables -F FORWARD
  case "$1" in
    relay-only) r iptables -A FORWARD -j DROP ;;
    direct) r iptables -A FORWARD -j ACCEPT ;;
    *) echo "mode: relay-only|direct" >&2; exit 2 ;;
  esac
  echo "mode $1"
}

counters() { r iptables -L FORWARD -v -n -x | tail -n +2; }

# Outbound rules in every peer namespace. tcp443-only: TCP to the relay's port 443 and nothing else
# (named DROP rules in front of the catch-all so the counters say what was tried).
egress() {
  for n in $NODES; do
    ns="dcp-$n"
    ip netns exec "$ns" iptables -F OUTPUT
    case "$1" in
      open) ;;
      tcp443-only)
        ip netns exec "$ns" iptables -A OUTPUT -o lo -j ACCEPT
        ip netns exec "$ns" iptables -A OUTPUT -p tcp -d "$RELAY_IP" --dport 443 -j ACCEPT
        ip netns exec "$ns" iptables -A OUTPUT -p tcp -d "$RELAY_IP" --dport 80 -j DROP
        ip netns exec "$ns" iptables -A OUTPUT -p udp -d "$RELAY_IP" --dport 7842 -j DROP
        ip netns exec "$ns" iptables -A OUTPUT -p udp -j DROP
        ip netns exec "$ns" iptables -A OUTPUT -j DROP
        ;;
      none)
        ip netns exec "$ns" iptables -A OUTPUT -o lo -j ACCEPT
        ip netns exec "$ns" iptables -A OUTPUT -j DROP
        ;;
      *) echo "egress: open|tcp443-only|none" >&2; exit 2 ;;
    esac
  done
  echo "egress $1"
}

egress-counters() {
  for n in $NODES; do
    echo "-- dcp-$n OUTPUT"
    ip netns exec "dcp-$n" iptables -L OUTPUT -v -n -x | tail -n +2
  done
}

shape() {
  local rtt=$1 loss=$2 rate=${3:--}
  for n in $NODES; do
    for end in "dcp-$n vn-$n" "dcp-r vr-$n"; do
      set -- $end
      ns=$1 dev=$2
      ip netns exec "$ns" tc qdisc del dev "$dev" root 2>/dev/null || true
      args=()
      if [ "$rtt" != 0 ]; then args+=(delay "$(awk "BEGIN{printf \"%.3f\", $rtt/4}")ms"); fi
      if [ "$loss" != 0 ]; then args+=(loss "$(awk "BEGIN{printf \"%.3f\", $loss/2}")%"); fi
      if [ "$rate" != "-" ]; then args+=(rate "$rate"); fi
      if [ ${#args[@]} -gt 0 ]; then
        ip netns exec "$ns" tc qdisc add dev "$dev" root netem limit 10000 "${args[@]}"
      fi
    done
  done
  echo "shape rtt=$rtt loss=$loss rate=$rate"
  ip netns exec dcp-a tc qdisc show dev vn-a
  r tc qdisc show dev vr-a
}

certs() {
  [ -f "$WORK/relay.crt" ] && return 0
  cd "$WORK"
  openssl ecparam -name prime256v1 -genkey -noout -out ca.key
  openssl req -x509 -new -key ca.key -sha256 -days 7 -subj "/CN=dcp spike test CA" -out ca.crt
  openssl ecparam -name prime256v1 -genkey -noout -out relay.key
  openssl req -new -key relay.key -subj "/CN=relay.dcp.test" -out relay.csr
  printf 'subjectAltName=IP:%s,DNS:relay.dcp.test\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n' "$RELAY_IP" > relay.ext
  openssl x509 -req -in relay.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 7 -sha256 -extfile relay.ext -out relay.crt
  openssl x509 -in ca.crt -outform der -out ca.der
  chmod a+r "$WORK"/*
}

relay-up() {
  mkdir -p "$WORK"
  if [ -f "$WORK/relay.pid" ] && kill -0 "$(cat "$WORK/relay.pid")" 2>/dev/null; then
    echo "relay already up (pid $(cat "$WORK/relay.pid"))"
    return 0
  fi
  certs
  cat > "$WORK/relay.toml" <<EOF
enable_relay = true
http_bind_addr = "0.0.0.0:80"
enable_quic_addr_discovery = true
enable_metrics = true
metrics_bind_addr = "127.0.0.1:9090"

[tls]
https_bind_addr = "0.0.0.0:443"
quic_bind_addr = "0.0.0.0:7842"
cert_mode = "Manual"
manual_cert_path = "$WORK/relay.crt"
manual_key_path = "$WORK/relay.key"
hostname = "relay.dcp.test"
EOF
  : "${IROH_RELAY_BIN:=iroh-relay}"
  RUST_LOG=${RELAY_LOG:-info} nohup ip netns exec dcp-r "$IROH_RELAY_BIN" --config-path "$WORK/relay.toml" \
    >> "$WORK/relay.log" 2>&1 &
  echo $! > "$WORK/relay.pid"
  # Ready when the HTTPS port answers inside dcp-r.
  for _ in $(seq 1 50); do
    if r curl -sS --cacert "$WORK/ca.crt" -o /dev/null "https://$RELAY_IP/generate_204" 2>/dev/null; then
      echo "relay up (pid $(cat "$WORK/relay.pid"))"
      return 0
    fi
    sleep 0.2
  done
  echo "relay did not become ready; log:" >&2
  tail -30 "$WORK/relay.log" >&2
  exit 1
}

relay-down() {
  [ -f "$WORK/relay.pid" ] || return 0
  pid=$(cat "$WORK/relay.pid")
  # nohup and `ip netns exec` both exec the next program, so this pid is the relay itself.
  kill -KILL "$pid" 2>/dev/null || true
  # Wait until the port is gone.
  for _ in $(seq 1 50); do
    if ! r curl -s -m 1 --cacert "$WORK/ca.crt" -o /dev/null "https://$RELAY_IP/generate_204" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  rm -f "$WORK/relay.pid"
  echo "relay down"
}

relay-metrics() { r curl -s http://127.0.0.1:9090/metrics; }

check() {
  echo "-- ICMP between peers (mode-dependent), peer to relay (always)"
  for a in $NODES; do for b in $NODES; do
    [ "$a" = "$b" ] && continue
    if ip netns exec "dcp-$a" ping -c1 -W1 "10.77.$(idx "$b").2" >/dev/null 2>&1; then echo "dcp-$a -> dcp-$b: reachable"; else echo "dcp-$a -> dcp-$b: unreachable"; fi
  done; done
  for a in $NODES; do
    if ip netns exec "dcp-$a" ping -c1 -W1 "$RELAY_IP" >/dev/null 2>&1; then echo "dcp-$a -> relay: reachable"; else echo "dcp-$a -> relay: unreachable"; fi
  done
}

cmd=${1:-}; shift || true
case "$cmd" in
  up|down|mode|shape|relay-up|relay-down|relay-metrics|counters|egress|egress-counters|check) "$cmd" "$@" ;;
  *) sed -n '2,28p' "$0"; exit 2 ;;
esac
