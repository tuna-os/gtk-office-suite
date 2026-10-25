#!/usr/bin/env python3
"""Tables for docs/rfc/0004-spike-relay-results.md from the `results.jsonl` files the wan driver
(tools/collab-spike/dcp-p2p/tests/wan.rs) writes: wan-results.py <dir with wan-<cond>-<group>/results.jsonl>."""
import json
import sys
from pathlib import Path

ORDER = ["unshaped", "rtt50", "rtt150", "loss1", "loss5", "rate1m", "worst"]


def load(root):
    rows = []
    for f in sorted(Path(root).rglob("results.jsonl")):
        for line in f.read_text().splitlines():
            line = line.strip()
            if line:
                try:
                    rows.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
    return rows


def conds(rows):
    seen = {r["condition"] for r in rows}
    return [c for c in ORDER if c in seen] + sorted(seen - set(ORDER))


def table(header, body):
    out = ["| " + " | ".join(header) + " |", "|" + "|".join("---" for _ in header) + "|"]
    out += ["| " + " | ".join(str(x) for x in r) + " |" for r in body]
    return "\n".join(out)


def main():
    rows = load(sys.argv[1] if len(sys.argv) > 1 else ".")
    cs = conds(rows)
    print(f"{len(rows)} result lines, conditions: {', '.join(cs)}\n")

    def key(r):
        return cs.index(r["condition"])

    def by_kind(kind):
        return [r for r in rows if r.get("kind") == kind]

    # journeys
    for mode in ("relay-only", "direct"):
        names = []
        cell = {}
        for r in rows:
            if r.get("kind") == "journey" and r["mode"] == mode:
                if r["journey"] not in names:
                    names.append(r["journey"])
                o = "pass" if r["outcome"] == "pass" else r["outcome"].upper()
                cell[(r["journey"], r["condition"])] = f'{o} {r["ms"] / 1000:.1f}s'
        if names:
            print(f"### Journeys, mode {mode}\n")
            print(table(["journey"] + cs, [[n] + [cell.get((n, c), "-") for c in cs] for n in names]))
            print()
            for r in rows:
                if r.get("kind") == "journey" and r["mode"] == mode and r["outcome"] == "FAIL":
                    print(f'- FAIL {r["condition"]} {r["journey"]}: {r["why"][:300]}')
            print()

    up = by_kind("upgrade_summary")
    if up:
        print("### Relay to direct upgrade (UDP allowed, addresses withheld)\n")
        print(table(["condition", "runs", "not upgraded", "median ms after handshake", "median ms after dial start", "each (ms after handshake)"],
                    [[r["condition"], r["reps"], r["not_upgraded"], r["median_after_handshake_ms"], r["median_after_dial_ms"], r["ip_selected_after_handshake_ms"]] for r in sorted(up, key=key)]))
        print()

    for kind, title in [("relay_killed", "Relay killed"), ("direct_blocked", "Direct path blocked mid-session"), ("path_proof", "Path proofs"), ("upgrade", "Upgrade runs"), ("ports", "Ports")]:
        rs = by_kind(kind)
        if rs:
            print(f"### {title}\n")
            for r in sorted(rs, key=key):
                d = {k: v for k, v in r.items() if k not in ("kind", "test", "a", "b", "firewall_forward_counters", "per_node", "egress_counters")}
                print(f"- `{json.dumps(d)}`")
            print()

    def simple(kind, fields, title):
        rs = by_kind(kind)
        if not rs:
            return
        print(f"### {title}\n")
        body = []
        for r in sorted(rs, key=lambda r: (r.get("mode", ""), key(r))):
            body.append([r["condition"], r.get("mode", "")] + [r.get(f, "") for f in fields])
        print(table(["condition", "mode"] + fields, body))
        print()

    simple("catch_up", ["versions", "ms"], "Offline catch-up")
    simple("concurrent_heads", ["two_heads_everywhere_ms", "merge_visible_everywhere_ms"], "Concurrent heads and merge")
    simple("lease_latency", ["writes", "min_ms", "median_ms", "p95_ms", "max_ms", "over_500ms"], "Lease write to visible at the other node")
    simple("connect", ["runs", "cold_median_ms", "warm_median_ms", "cold_all_ms"], "Connect and first session, then an open connection")
    simple("file", ["mib", "writer_publish_ms", "reader_lists_ms_after_publish", "fetch_ms", "fetch_mib_per_s"], "Files")
    simple(
        "state_growth",
        ["n", "complete", "total_ms", "reader_complete_ms_after_last_publish", "reader_cpu_ms_total", "reader_store_thread_cpu_ms",
         "reader_cpu_ms_after_last_publish", "reader_store_thread_cpu_ms_after_last_publish", "reader_veth_rx_bytes", "reader_state_bytes",
         "reader_ops_received", "reader_ops_applied", "reader_sync_sessions", "writer_sync_sessions"],
        "State growth",
    )

    pr = by_kind("paths")
    if pr:
        print("### Paths used while the journeys and scenarios ran (iroh's own counters, summed over the nodes of the test)\n")
        print(table(["condition", "test", "nodes", "connections", "with an IP path", "selected IP", "IP bytes", "relay bytes"],
                    [[r["condition"], r["test"], r["nodes"], r["conns"], r["conns_with_an_ip_path"], r["conns_that_selected_ip"], r["ip_bytes"], r["relay_bytes"]] for r in sorted(pr, key=lambda r: (r["test"], key(r)))]))
        print()


if __name__ == "__main__":
    main()
