#!/usr/bin/env bash
# Run a command against a fresh Xvfb display, waiting until the server
# actually accepts connections before starting it.
#
# `xvfb-run -a` starts Xvfb and immediately execs the command without
# waiting for it to serve, so anything that initialises GTK straight away
# can lose the race and fail on a DISPLAY that exists but is not listening
# yet. That is not hypothetical: it surfaced twice in one evening as
# `suite_common::gtk_test`'s deliberate "GTK could not be initialised"
# assertion, which reads like a product bug in the job summary — a widget
# test named in the failure, a panic in gtk_test.rs — when the display was
# the only thing wrong. Both cleared on a re-run with no code change, and
# in both cases the sibling job on the identical commit passed.
#
# Xvfb's -displayfd writes the chosen display number only once it is
# serving, so waiting for that write is a readiness handshake rather than a
# sleep. tests/gui/run_gui_tests.sh already does this for the AT-SPI
# journeys; this is the same mechanism for the plain `cargo test` lanes,
# which had none.
set -euo pipefail

if [ "$#" -eq 0 ]; then
    echo "usage: $0 <command> [args...]" >&2
    exit 2
fi

SCREEN="${WITH_DISPLAY_SCREEN:-1920x1080x24}"
# 0.1s per tick. Generous: a loaded CI runner can take seconds, and waiting
# out a slow start costs far less than a spurious red lane.
TICKS="${WITH_DISPLAY_TICKS:-600}"

DISPLAY_NUM_FILE="$(mktemp -t with-display-XXXXXXXX)"
Xvfb -displayfd 3 -screen 0 "$SCREEN" 3>"$DISPLAY_NUM_FILE" &
XVFB_PID=$!
# `wait` after the kill so the script reaps its own child rather than
# leaving a zombie for init. Without it `pgrep Xvfb` still counts the
# defunct entry, which looks exactly like a leaked server to anyone
# checking — it fooled me once while writing this.
trap 'kill "$XVFB_PID" 2>/dev/null || true; wait "$XVFB_PID" 2>/dev/null || true; rm -f "$DISPLAY_NUM_FILE"' EXIT

DISPLAY_NUM=""
for _ in $(seq 1 "$TICKS"); do
    DISPLAY_NUM="$(tr -d '[:space:]' < "$DISPLAY_NUM_FILE")"
    [ -n "$DISPLAY_NUM" ] && break
    # Our own server dying is a distinct failure from a slow one, and
    # reporting it as such beats waiting out the whole timeout.
    if ! kill -0 "$XVFB_PID" 2>/dev/null; then
        echo "error: Xvfb exited before it began serving" >&2
        exit 1
    fi
    sleep 0.1
done

if [ -z "$DISPLAY_NUM" ]; then
    echo "error: Xvfb did not report a display within $((TICKS / 10))s" >&2
    exit 1
fi

export DISPLAY=":$DISPLAY_NUM"
# Not `exec`: that would replace this shell and skip the EXIT trap, leaving
# the server running for the rest of the job.
status=0
"$@" || status=$?
exit "$status"
