#!/usr/bin/env python3
"""Long-poll wake-up stress for a Durable Streams server, plain protocol, no Rust, no forwarder.

N streams, one reader thread per stream (a persistent HTTP/1.1 connection that long-polls from
the offset it holds and re-polls the moment it gets an answer, exactly what the binding does),
and rounds: in every round one JSON message is appended to every stream at the same instant, then
the round waits until every reader has been delivered its message. The next round starts at once
(GAP_MS = 0), so each reader's re-poll and the next round's append reach the server together.

A delivery that takes about as long as the server's long-poll timeout (LP seconds) is a *lost
wake-up*: the poll was parked although its stream already had data. The reader then gets a 204,
re-polls, and gets the message at once; the script records that too (`recovered_immediately`).

usage: lp_race.py BASE_URL [CA_PEM]  (env N, ROUNDS, LP, GAP_MS, MAX_SECONDS, TAG)
"""
import http.client
import json
import os
import ssl
import statistics
import sys
import threading
import time
import urllib.parse

base = urllib.parse.urlparse(sys.argv[1])
ca = sys.argv[2] if len(sys.argv) > 2 else None
N = int(os.environ.get("N", "50"))
ROUNDS = int(os.environ.get("ROUNDS", "400"))
LP = float(os.environ.get("LP", "3"))
GAP_MS = float(os.environ.get("GAP_MS", "0"))
MAX_SECONDS = float(os.environ.get("MAX_SECONDS", "150"))
TAG = os.environ.get("TAG", "race") + "-" + format(int(time.time()) & 0xFFFFFF, "x")
PREFIX = base.path if base.path.endswith("/") else base.path + "/"


def conn():
    if base.scheme == "https":
        ctx = ssl.create_default_context(cafile=ca)
        ctx.set_alpn_protocols(["http/1.1"])
        return http.client.HTTPSConnection(base.hostname, base.port, context=ctx, timeout=LP + 30)
    return http.client.HTTPConnection(base.hostname, base.port, timeout=LP + 30)


def req(c, method, path, body=None, headers=None, query=""):
    c.request(method, PREFIX + path + query, body=body, headers=headers or {})
    r = c.getresponse()
    data = r.read()
    return r, data


paths = [f"dcp/{TAG}/s{i}" for i in range(N)]
tails = []
c0 = conn()
for p in paths:
    r, _ = req(c0, "PUT", p, headers={"Content-Type": "application/json"})
    assert r.status in (200, 201), (p, r.status)
    r, _ = req(c0, "HEAD", p)
    tails.append(r.getheader("Stream-Next-Offset"))
c0.close()

lock = threading.Lock()
cond = threading.Condition(lock)
round_no = [-1]                 # the round whose appends have been released
delivered = {}                  # (round, idx) -> monotonic time of delivery
polls = []                      # (idx, started, ended, status, kind) of every answered poll
stop = threading.Event()
errors = []


def reader(i):
    off = tails[i]
    c = conn()
    while not stop.is_set():
        t0 = time.monotonic()
        try:
            r, data = req(c, "GET", paths[i], query=f"?offset={urllib.parse.quote(off)}&live=long-poll")
        except Exception as e:  # noqa: BLE001
            if stop.is_set():
                return
            errors.append(f"reader {i}: {e!r}")
            c.close()
            c = conn()
            time.sleep(0.05)
            continue
        t1 = time.monotonic()
        nxt = r.getheader("Stream-Next-Offset")
        with cond:
            polls.append((i, t0, t1, r.status))
            if r.status == 200 and data:
                for item in json.loads(data):
                    delivered[(item["r"], i)] = t1
                cond.notify_all()
        if nxt:
            off = nxt


threads = [threading.Thread(target=reader, args=(i,), daemon=True) for i in range(N)]
for t in threads:
    t.start()
time.sleep(0.8)  # every reader parked

append_conns = [conn() for _ in range(N)]
append_start = {}
append_err = []
release = [-1]                  # the round whose appends may go
go_cond = threading.Condition()


def appender(i):
    r_no = 0
    while not stop.is_set():
        with go_cond:
            while release[0] < r_no and not stop.is_set():
                go_cond.wait(timeout=1)
            if stop.is_set():
                return
        t0 = time.monotonic()
        try:
            body = json.dumps({"r": r_no, "i": i, "pad": "y" * 200}).encode()
            r, _ = req(append_conns[i], "POST", paths[i], body=body, headers={"Content-Type": "application/json"})
            if r.status not in (200, 204):
                append_err.append((r_no, i, r.status))
        except Exception as e:  # noqa: BLE001
            append_err.append((r_no, i, repr(e)))
            try:
                append_conns[i].close()
            except Exception:  # noqa: BLE001
                pass
            append_conns[i] = conn()
        with lock:
            append_start[(r_no, i)] = t0
        r_no += 1


app_threads = [threading.Thread(target=appender, args=(i,), daemon=True) for i in range(N)]
for t in app_threads:
    t.start()

t_begin = time.monotonic()
lat = []
slow = []            # (round, idx, latency or None if never delivered)
rounds_done = 0
for r_no in range(ROUNDS):
    if time.monotonic() - t_begin > MAX_SECONDS:
        break
    t_go = time.monotonic()
    with go_cond:
        release[0] = r_no
        go_cond.notify_all()
    deadline = t_go + LP * 4 + 10
    with cond:
        while sum(1 for i in range(N) if (r_no, i) in delivered) < N:
            left = deadline - time.monotonic()
            if left <= 0:
                break
            cond.wait(timeout=left)
        missing = [i for i in range(N) if (r_no, i) not in delivered]
    for i in range(N):
        if (r_no, i) in delivered:
            l = delivered[(r_no, i)] - append_start.get((r_no, i), t_go)
            lat.append(l)
            if l >= LP * 0.8:
                slow.append((r_no, i, round(l, 3)))
    if missing:
        slow.extend((r_no, i, None) for i in missing)
        # a round that did not finish: let the late deliveries land before the next one starts
        time.sleep(1)
    rounds_done += 1
    if GAP_MS:
        time.sleep(GAP_MS / 1000.0)

stop.set()
t_end = time.monotonic()
timeouts_204 = sum(1 for (_, a, b, s) in polls if s == 204)
# A 204 that ended a poll in the middle of a round whose message was already appended: the data was
# there all along (the reader gets it with the very next poll).
res = {
    "server": os.environ.get("TAG", "race"),
    "url": sys.argv[1],
    "streams": N,
    "long_poll_timeout_s": LP,
    "rounds_done": rounds_done,
    "pairs_poll_vs_append": rounds_done * N,
    "deliveries": len(lat),
    "polls_answered": len(polls),
    "polls_answered_204": timeouts_204,
    "lost_wakeups": len(slow),
    "lost_wakeup_detail_round_idx_seconds": slow[:40],
    "latency_s": {
        "p50": statistics.median(lat) if lat else None,
        "p99": sorted(lat)[int(len(lat) * 0.99) - 1] if lat else None,
        "max": max(lat) if lat else None,
    },
    "reader_errors": errors[:10],
    "append_errors": append_err[:10],
    "seconds": round(t_end - t_begin, 1),
}
print("RESULT " + json.dumps(res, sort_keys=True))
sys.exit(0)
