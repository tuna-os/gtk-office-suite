#!/usr/bin/env bash
# run_gui_tests.sh — Headless GUI test runner for gtk-office-rust apps
# Requires: xvfb-run (or DISPLAY set), dbus-daemon, at-spi-bus-launcher, python3-dogtail
set -euo pipefail

# ── DBus session ─────────────────────────────────────────────────────────────
# If no session bus exists, spin up a private one.
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    echo "[dbus] Starting private session bus..."
    eval "$(dbus-launch --sh-syntax)"
    PRIVATE_DBUS_PID="$DBUS_SESSION_PID"
    export DBUS_SESSION_BUS_ADDRESS
fi

# ── Cleanup on exit ──────────────────────────────────────────────────────────
APP_PIDS=()
ATSPI_PID=""

cleanup() {
    echo "[cleanup] Stopping app processes..."
    for pid in "${APP_PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    if [ -n "$ATSPI_PID" ] && kill -0 "$ATSPI_PID" 2>/dev/null; then
        echo "[cleanup] Stopping at-spi-bus-launcher (PID $ATSPI_PID)..."
        kill "$ATSPI_PID" 2>/dev/null || true
    fi
    if [ -n "${PRIVATE_DBUS_PID:-}" ] && kill -0 "$PRIVATE_DBUS_PID" 2>/dev/null; then
        echo "[cleanup] Stopping private dbus-daemon (PID $PRIVATE_DBUS_PID)..."
        kill "$PRIVATE_DBUS_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

# ── AT-SPI accessibility bus ──────────────────────────────────────────────────
# Find at-spi-bus-launcher in common locations
ATSPI_LAUNCHER=""
for candidate in \
    /usr/libexec/at-spi-bus-launcher \
    /usr/lib/at-spi2-core/at-spi-bus-launcher \
    /usr/lib/at-spi2/at-spi-bus-launcher; do
    if [ -x "$candidate" ]; then
        ATSPI_LAUNCHER="$candidate"
        break
    fi
done

if [ -n "$ATSPI_LAUNCHER" ]; then
    echo "[atspi] Starting $ATSPI_LAUNCHER ..."
    # Suppress systemd-activation errors; they are non-fatal in non-systemd envs
    "$ATSPI_LAUNCHER" --launch-immediately 2>/dev/null &
    ATSPI_PID=$!
    sleep 1
    echo "[atspi] Launcher PID: $ATSPI_PID"
else
    echo "[atspi] WARNING: at-spi-bus-launcher not found; accessibility may be limited"
fi

# Enable GTK accessibility bridge
export GTK_A11Y=atspi
unset NO_AT_BRIDGE

# ── Build ─────────────────────────────────────────────────────────────────────
if [ -z "${NO_BUILD:-}" ]; then
    echo "[build] Running cargo build..."
    cargo build
fi

# ── Helper: run app + test ────────────────────────────────────────────────────
run_test() {
    local label="$1"
    local binary="$2"
    local testscript="$3"

    echo ""
    echo "=== $label ==="
    "$binary" &
    local app_pid=$!
    APP_PIDS+=("$app_pid")
    sleep 2

    echo "[test] Running $testscript ..."
    python3 "$testscript"

    echo "[test] $label: stopping app (PID $app_pid)..."
    kill "$app_pid" 2>/dev/null || true
    # Remove from PIDS list so cleanup doesn't double-kill
    APP_PIDS=("${APP_PIDS[@]/$app_pid/}")
    sleep 1
}

# ── Run tests ─────────────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

run_test "Tables"  "$REPO_ROOT/target/debug/tables"  "$SCRIPT_DIR/test_tables.py"
run_test "Decks"   "$REPO_ROOT/target/debug/decks"   "$SCRIPT_DIR/test_decks.py"
run_test "Letters" "$REPO_ROOT/target/debug/letters" "$SCRIPT_DIR/test_letters.py"

echo ""
echo "✓ All GUI tests passed!"
