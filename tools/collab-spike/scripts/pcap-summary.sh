#!/usr/bin/env bash
# Summarise what one host sent: pcap-summary.sh <file.pcap> <source-ip>
# One line per (protocol, destination, port): packet count, bytes, DNS names asked, TLS SNI.
set -euo pipefail
pcap=$1 src=$2
tshark -r "$pcap" -Y "ip.src==$src && (tcp || udp)" -T fields -E separator=, \
  -e ip.proto -e ip.dst -e udp.dstport -e tcp.dstport -e frame.len -e dns.qry.name -e tls.handshake.extensions_server_name 2>/dev/null |
awk -F, '
  # ICMP errors embed the header of the packet they answer: a field with two values. Skip them.
  $1 ~ /,/ { next }
  { proto = ($1==6) ? "tcp" : ($1==17) ? "udp" : ($1==1) ? "icmp" : $1
    port = ($1==6) ? $4 : ($1==17) ? $3 : "-"
    key = proto " " $2 ":" port
    n[key]++; bytes[key] += $5
    if ($6 != "" && !((key, $6) in seen)) { seen[key, $6] = 1; names[key] = names[key] " " $6 }
    if ($7 != "" && !((key, $7) in seen)) { seen[key, $7] = 1; sni[key] = sni[key] " " $7 } }
  END { for (k in n) printf "%-28s packets=%-5d bytes=%-8d dns=[%s ] sni=[%s ]\n", k, n[k], bytes[k], names[k], sni[k] }' | sort
