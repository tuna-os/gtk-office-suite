#!/usr/bin/env bash
# What does an iroh endpoint contact by itself? Runs dcp-p2p-footprint variants in a namespace
# (dcp-fp) that reaches the internet through NAT on the host, captures its traffic, and lists the
# destinations. Needs root, tshark, iptables. Usage: netns-footprint.sh <footprint-binary> <outdir>
#
# minimal is the negative control of the footprint claim: it must put no packet on the wire, and
# the n0 variant must put some, or the capture proves nothing.
set -euo pipefail
BIN=$1 OUT=$2
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT"
WAN=$(ip route show default | awk '{print $5; exit}')
NS=$(awk '/^nameserver/{print $2; exit}' /run/systemd/resolve/resolv.conf 2>/dev/null || true)
NS=${NS:-1.1.1.1}
echo "uplink $WAN, nameserver $NS" | tee "$OUT/setup.txt"

ip netns add dcp-fp
ip link add vfp-h type veth peer name vfp netns dcp-fp
ip addr add 10.88.0.1/24 dev vfp-h
ip link set vfp-h up
ip netns exec dcp-fp ip link set lo up
ip netns exec dcp-fp ip addr add 10.88.0.2/24 dev vfp
ip netns exec dcp-fp ip link set vfp up
ip netns exec dcp-fp ip route add default via 10.88.0.1
ip netns exec dcp-fp sysctl -qw net.ipv6.conf.all.disable_ipv6=1
sysctl -qw net.ipv4.ip_forward=1
iptables -t nat -A POSTROUTING -s 10.88.0.0/24 -o "$WAN" -j MASQUERADE
iptables -I FORWARD -s 10.88.0.0/24 -j ACCEPT
iptables -I FORWARD -d 10.88.0.0/24 -j ACCEPT
mkdir -p /etc/netns/dcp-fp
echo "nameserver $NS" > /etc/netns/dcp-fp/resolv.conf
cleanup() { ip netns del dcp-fp 2>/dev/null || true; ip link del vfp-h 2>/dev/null || true; rm -rf /etc/netns/dcp-fp; }
trap cleanup EXIT

echo "-- the namespace reaches the internet?" | tee -a "$OUT/setup.txt"
ip netns exec dcp-fp ping -c2 -W3 1.1.1.1 2>&1 | tail -3 | tee -a "$OUT/setup.txt" || true
ip netns exec dcp-fp getent hosts dns.iroh.link 2>&1 | tee -a "$OUT/setup.txt" || echo "dns.iroh.link did not resolve" | tee -a "$OUT/setup.txt"

status=0
for v in minimal n0-relay n0; do
  echo "== variant $v"
  ip netns exec dcp-fp tcpdump -i vfp -nn -s 0 -U -w "$OUT/$v.pcap" >/dev/null 2>&1 &
  tp=$!
  sleep 1
  RUST_LOG="${FOOTPRINT_LOG:-info,iroh=debug,iroh_relay=debug,iroh_dns=debug}" ip netns exec dcp-fp "$BIN" --variant "$v" --secs 20 > "$OUT/$v.out" 2> "$OUT/$v.log" || echo "the probe exited non-zero" | tee -a "$OUT/$v.out"
  sleep 1
  kill "$tp" 2>/dev/null || true
  wait "$tp" 2>/dev/null || true
  cat "$OUT/$v.out"
  "$HERE/pcap-summary.sh" "$OUT/$v.pcap" 10.88.0.2 | tee "$OUT/$v.summary.txt"
  total=$(tshark -r "$OUT/$v.pcap" -Y 'ip.src==10.88.0.2' 2>/dev/null | wc -l)
  echo "packets sent by the endpoints on the wire: $total" | tee -a "$OUT/$v.summary.txt"
  # What the DNS answers say, to put names on the addresses.
  tshark -r "$OUT/$v.pcap" -Y 'dns.flags.response==1' -T fields -e dns.qry.name -e dns.a -e dns.aaaa 2>/dev/null | sort -u | tee "$OUT/$v.dns-answers.txt"
  if [ "$v" = minimal ] && [ "$total" -ne 0 ]; then echo "FAIL: the minimal endpoint put $total packets on the wire"; status=1; fi
  if [ "$v" = n0 ] && [ "$total" -eq 0 ]; then echo "FAIL: the n0 endpoint put no packet on the wire: the capture is blind"; status=1; fi
done
exit $status
