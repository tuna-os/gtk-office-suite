#!/usr/bin/env bash
set -e

# Start private dbus-daemon unconditionally unless NO_PRIVATE_DBUS is set
if [ -z "$NO_PRIVATE_DBUS" ]; then
    echo "Starting private dbus-daemon..."
    DBUS_OUTPUT=$(dbus-daemon --session --fork --print-address --print-pid)
    export DBUS_SESSION_BUS_ADDRESS=$(echo "$DBUS_OUTPUT" | head -n 1)
    CUSTOM_DBUS_PID=$(echo "$DBUS_OUTPUT" | tail -n 1)
    echo "Private DBus started at: $DBUS_SESSION_BUS_ADDRESS (PID: $CUSTOM_DBUS_PID)"
fi

# Keep track of running background pids
PIDS=()
cleanup() {
    echo "Cleaning up background processes..."
    for pid in "${PIDS[@]}"; do
        if kill -0 "$pid" 2>/dev/null; then
            echo "Killing background PID $pid"
            kill "$pid" || true
        fi
    done
    if [ -n "$CUSTOM_DBUS_PID" ]; then
        echo "Killing custom DBus PID $CUSTOM_DBUS_PID"
        kill "$CUSTOM_DBUS_PID" || true
    fi
}
# Trap exit, error, signals to clean up background processes
trap cleanup EXIT INT TERM

# Ensure binaries are built unless NO_BUILD is set
if [ -z "$NO_BUILD" ]; then
    cargo build
fi

unset GTK_A11Y
export NO_AT_BRIDGE=0
/usr/libexec/at-spi-bus-launcher --launch-immediately &
sleep 2

echo "=== Starting Tables ==="
target/debug/tables &
TABLES_PID=$!
PIDS+=($TABLES_PID)
sleep 2

echo "Running Tables GUI tests..."
/usr/bin/python3 tests/gui/test_tables.py

echo "Stopping Tables..."
kill $TABLES_PID 2>/dev/null || true

echo "=== Starting Decks ==="
target/debug/decks &
DECKS_PID=$!
PIDS+=($DECKS_PID)
sleep 2

echo "Running Decks GUI tests..."
/usr/bin/python3 tests/gui/test_decks.py

echo "Stopping Decks..."
kill $DECKS_PID 2>/dev/null || true

echo "=== Starting Letters ==="
target/debug/letters &
LETTERS_PID=$!
PIDS+=($LETTERS_PID)
sleep 2

echo "Running Letters GUI tests..."
/usr/bin/python3 tests/gui/test_letters.py

echo "Stopping Letters..."
kill $LETTERS_PID 2>/dev/null || true


echo "All GUI tests passed successfully!"
