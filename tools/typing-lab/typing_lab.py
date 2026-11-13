"""Which letters does xdotool lose on the way to a GTK entry?

The release soak lost a "p" in about one journey per attempt ("apple" ->
"aple", "kept" -> "ket"), at 40ms and at 120ms per key. This types a
sentence full of p's into a plain GtkEntry many times in several modes and
prints, per mode, how often each letter went missing or doubled.

Modes vary xdotool's way of addressing the window (--window sends events to
a window; without it they go through the X server's focus), the key delay,
and CPU load (the soak's build runs beside it)."""
import collections
import os
import subprocess
import sys
import time

SENTENCE = "apple kept alpha keep pop"
TRIALS = int(os.environ.get("TRIALS", "200"))
OUT = "/tmp/typing-lab.out"
WID = "/tmp/typing-lab.wid"


def run(*argv, **kw):
    return subprocess.run(argv, capture_output=True, text=True, **kw)


def read_out():
    try:
        return open(OUT).read()
    except FileNotFoundError:
        return ""


def trial(window, delay, mode):
    run("xdotool", "key", "--window", window, "ctrl+a", "BackSpace")
    deadline = time.time() + 2
    while read_out() and time.time() < deadline:
        time.sleep(0.02)
    if mode == "window":
        run("xdotool", "type", "--delay", str(delay), "--window", window, "--", SENTENCE)
    elif mode == "focus":
        run("xdotool", "type", "--delay", str(delay), "--", SENTENCE)
    elif mode == "keys":
        for ch in SENTENCE:
            key = "space" if ch == " " else ch
            run("xdotool", "key", "--delay", str(delay), "--window", window, key)
    time.sleep(0.3)
    return read_out()


def diff(got):
    """Per-letter counts lost and gained against SENTENCE."""
    want = collections.Counter(SENTENCE)
    have = collections.Counter(got)
    lost = {c: want[c] - have[c] for c in want if want[c] > have[c]}
    gained = {c: have[c] - want[c] for c in have if have[c] > want[c]}
    return lost, gained


def main():
    env = dict(os.environ, TYPING_LAB_OUT=OUT, TYPING_LAB_WID=WID)
    for path in (OUT, WID):
        if os.path.exists(path):
            os.remove(path)
    app = subprocess.Popen([sys.executable, os.path.join(os.path.dirname(__file__), "entry_app.py")], env=env)
    for _ in range(100):
        if os.path.exists(WID):
            break
        time.sleep(0.1)
    window = open(WID).read().strip()
    print("window", window, flush=True)
    time.sleep(0.5)
    run("xdotool", "windowfocus", window)
    for load in (0, 2):
        burners = [subprocess.Popen(["sh", "-c", "while :; do :; done"]) for _ in range(load)]
        for mode, delay in (("window", 40), ("window", 120), ("window", 12), ("focus", 40), ("keys", 40)):
            lost_total = collections.Counter()
            gained_total = collections.Counter()
            bad = 0
            for _ in range(TRIALS):
                got = trial(window, delay, mode)
                lost, gained = diff(got)
                if got != SENTENCE:
                    bad += 1
                lost_total.update(lost)
                gained_total.update(gained)
            print(f"load={load} mode={mode} delay={delay}ms: {bad}/{TRIALS} sentences wrong; "
                  f"lost {dict(lost_total)} gained {dict(gained_total)}", flush=True)
        for b in burners:
            b.kill()
    app.kill()


if __name__ == "__main__":
    main()
