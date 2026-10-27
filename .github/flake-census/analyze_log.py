#!/usr/bin/env python3
"""Find parked long polls in a Caddy JSON access log: a long poll that ended with 204 after about
the server's long-poll timeout although an append to the same stream was answered while it was
parked.  usage: analyze_log.py ACCESS_LOG LP_SECONDS [MIN_FRACTION=0.8]"""
import json
import sys
import urllib.parse

path, lp = sys.argv[1], float(sys.argv[2])
frac = float(sys.argv[3]) if len(sys.argv) > 3 else 0.8
polls, posts = [], {}
n = 0
for line in open(path, errors="replace"):
    try:
        e = json.loads(line)
    except ValueError:
        continue
    r = e.get("request") or {}
    uri = r.get("uri", "")
    u = urllib.parse.urlsplit(uri)
    q = urllib.parse.parse_qs(u.query)
    end = float(e.get("ts", 0))
    dur = float(e.get("duration", 0))
    n += 1
    if r.get("method") == "GET" and q.get("live") == ["long-poll"]:
        polls.append((u.path, end - dur, end, dur, e.get("status"), q.get("offset", [""])[0], e.get("resp_headers", {})))
    elif r.get("method") == "POST":
        posts.setdefault(u.path, []).append((end - dur, end, e.get("status")))
parked = [p for p in polls if p[4] == 204 and p[3] >= lp * frac]
lost = []
for p in parked:
    for (a0, a1, st) in posts.get(p[0], []):
        if p[1] <= a1 <= p[2]:
            lost.append((p, (a0, a1, st)))
            break
print(f"LOG lines={n} long_polls={len(polls)} polls_204_after>={frac*lp:.1f}s={len(parked)} of_which_an_append_was_answered_meanwhile={len(lost)}")
for p, a in lost[:20]:
    print(f"LOST stream={p[0]} poll_start={p[1]:.3f} poll_end={p[2]:.3f} poll_s={p[3]:.3f} offset={p[5]} next={p[6].get('Stream-Next-Offset')} append_end={a[1]:.3f} append_status={a[2]} append_to_poll_end_s={p[2]-a[1]:.3f}")
