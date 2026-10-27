#!/usr/bin/env python3
"""Controlled long-poll vs append race against a Durable Streams server (plain HTTP).

One thread, raw sockets, microsecond spacing: a long poll from the stream's tail goes out on one
connection and an append to the same stream on another, DELTA_US apart (negative: the append
first).  The server is kept busy by decoy processes appending to other streams.  A poll that has
not answered after 1 s although its stream got a message is a lost wake-up (the server's
long-poll timeout LP is 2 s here; the poll then answers 204 and the next poll returns the message
at once, which the script checks).

usage: lp_race2.py BASE_URL   (env TRIALS per delta, DELTAS_US, DECOYS, LP)
"""
import json
import multiprocessing as mp
import os
import select
import socket
import sys
import threading
import time
import urllib.parse
import http.client

base = urllib.parse.urlparse(sys.argv[1])
PREFIX = base.path if base.path.endswith("/") else base.path + "/"
TRIALS = int(os.environ.get("TRIALS", "300"))
DELTAS = [int(x) for x in os.environ.get("DELTAS_US", "-400,-200,-100,-50,-20,0,20,50,100,200,400").split(",")]
DECOYS = int(os.environ.get("DECOYS", "3"))
LP = float(os.environ.get("LP", "2"))
STREAMS = int(os.environ.get("VICTIMS", "20"))
TAG = "race2-" + format(int(time.time()) & 0xFFFFFF, "x")
HOST = f"{base.hostname}:{base.port}"


def decoy(n, stop_at):
    c = http.client.HTTPConnection(base.hostname, base.port, timeout=30)
    p = f"{PREFIX}dcp/{TAG}/decoy{n}"
    c.request("PUT", p, headers={"Content-Type": "application/json"})
    c.getresponse().read()

    def loop():
        cc = http.client.HTTPConnection(base.hostname, base.port, timeout=30)
        while time.time() < stop_at:
            cc.request("POST", p, body=b'{"d":"' + b"x" * 200 + b'"}', headers={"Content-Type": "application/json"})
            cc.getresponse().read()

    ts = [threading.Thread(target=loop, daemon=True) for _ in range(4)]
    for t in ts:
        t.start()
    for t in ts:
        t.join()


def recv_response(sock, buf):
    """Read one HTTP response from sock; returns (status, headers dict, body, rest)."""
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(65536)
        if not chunk:
            raise EOFError
        buf += chunk
    head, _, rest = buf.partition(b"\r\n\r\n")
    lines = head.decode("latin1").split("\r\n")
    status = int(lines[0].split()[1])
    hd = {}
    for l in lines[1:]:
        k, _, v = l.partition(":")
        hd[k.strip().lower()] = v.strip()
    n = int(hd.get("content-length", "0"))
    while len(rest) < n:
        chunk = sock.recv(65536)
        if not chunk:
            raise EOFError
        rest += chunk
    return status, hd, rest[:n], rest[n:]


def dial():
    s = socket.create_connection((base.hostname, base.port))
    s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    return s


def poll_bytes(path, off):
    return (f"GET {PREFIX}{path}?offset={urllib.parse.quote(off)}&live=long-poll HTTP/1.1\r\nHost: {HOST}\r\n\r\n").encode()


def append_bytes(path, i):
    body = json.dumps({"i": i, "pad": "y" * 200}).encode()
    return (f"POST {PREFIX}{path} HTTP/1.1\r\nHost: {HOST}\r\nContent-Type: application/json\r\nContent-Length: {len(body)}\r\n\r\n").encode() + body


def spin_until(t_ns):
    while time.perf_counter_ns() < t_ns:
        pass


def main():
    sys.setswitchinterval(1e-5)
    stop_at = time.time() + 3600
    decoys = [mp.Process(target=decoy, args=(n, stop_at), daemon=True) for n in range(DECOYS)]
    for d in decoys:
        d.start()
    time.sleep(1.0)
    setup = http.client.HTTPConnection(base.hostname, base.port, timeout=30)
    paths = []
    tails = []
    for i in range(STREAMS):
        p = f"dcp/{TAG}/v{i}"
        setup.request("PUT", PREFIX + p, headers={"Content-Type": "application/json"})
        r = setup.getresponse()
        r.read()
        setup.request("HEAD", PREFIX + p)
        r = setup.getresponse()
        r.read()
        paths.append(p)
        tails.append(r.getheader("Stream-Next-Offset"))
    sp, sa = dial(), dial()
    bp, ba = b"", b""
    results = {}
    seq = 0
    for delta in DELTAS:
        lost = 0
        lost_ok = 0
        empty200 = 0
        lats = []
        for trial in range(TRIALS):
            v = trial % STREAMS
            path, off = paths[v], tails[v]
            seq += 1
            pb, ab = poll_bytes(path, off), append_bytes(path, seq)
            t0 = time.perf_counter_ns()
            if delta >= 0:
                sp.sendall(pb)
                spin_until(t0 + delta * 1000)
                sa.sendall(ab)
            else:
                sa.sendall(ab)
                spin_until(t0 + (-delta) * 1000)
                sp.sendall(pb)
            ts = time.perf_counter()
            got_poll = got_app = None
            while got_poll is None or got_app is None:
                socks = [s for s, g in ((sp, got_poll), (sa, got_app)) if g is None]
                ready, _, _ = select.select(socks, [], [], 1.0 if got_poll is None else 5)
                if not ready:
                    break
                for s in ready:
                    if s is sp:
                        st, hd, body, bp = recv_response(sp, bp)
                        got_poll = (st, hd, body, time.perf_counter() - ts)
                    else:
                        st, hd, body, ba = recv_response(sa, ba)
                        got_app = (st, hd, body)
            if got_app is None:
                print(f"append did not answer (delta {delta})", flush=True)
                sa.close(); sa = dial(); ba = b""
                continue
            if got_poll is None:
                # lost wake-up: the poll is parked although the append was answered
                lost += 1
                st, hd, body, bp = recv_response(sp, bp)   # answers at the long-poll timeout
                waited = time.perf_counter() - ts
                nxt = hd.get("stream-next-offset")
                sp.sendall(poll_bytes(path, nxt))
                t1 = time.perf_counter()
                st2, hd2, body2, bp = recv_response(sp, bp)
                if st == 204 and st2 == 200 and time.perf_counter() - t1 < 0.5:
                    lost_ok += 1
                print(f"LOST delta_us={delta} stream={v} poll_answered={st} after={waited:.2f}s next_poll={st2} in {(time.perf_counter()-t1)*1000:.1f} ms", flush=True)
                tails[v] = hd2.get("stream-next-offset") or got_app[1].get("stream-next-offset")
                continue
            if got_poll[0] == 200 and not json.loads(got_poll[2] or b"[]"):
                empty200 += 1
                print(f"EMPTY200 delta_us={delta} stream={v}: a 200 with no message (the reader would skip it)", flush=True)
            lats.append(got_poll[3])
            tails[v] = got_poll[1].get("stream-next-offset") or got_app[1].get("stream-next-offset")
        lats.sort()
        results[delta] = {"trials": TRIALS, "lost": lost, "lost_confirmed_data_was_there": lost_ok, "empty_200": empty200,
                          "p50_ms": round(lats[len(lats) // 2] * 1000, 2) if lats else None,
                          "max_ms": round(lats[-1] * 1000, 2) if lats else None}
        print(f"DELTA {delta:>5} us: trials={TRIALS} lost={lost} {results[delta]}", flush=True)
    print("RESULT " + json.dumps({"url": sys.argv[1], "decoys": DECOYS, "lp_s": LP, "by_delta_us": results}, sort_keys=True))
    os._exit(0)


if __name__ == "__main__":
    main()
