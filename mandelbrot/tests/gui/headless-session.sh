#!/usr/bin/env bash
# Shared helpers for driving the real Mandelbrot Flatpak bundle headlessly.
#
# Sourced by tests/gui/run-flatpak-smoke.sh and tests/e2e/run-gui-call.sh.
# The approach is upstream Fractal's (commit 209da3c2, "Launch a wayland
# display server for UI tests"): a private D-Bus session plus
# `mutter --headless --wayland --no-x11 --virtual-monitor`, so GTK gets a real
# Wayland display rather than Broadway.
#
# Host requirements: flatpak, mutter, dbus-daemon (dbus-run-session),
# xdg-dbus-proxy (a flatpak dependency) and, for the call test, at-spi2-core.
#
# Everything the app writes goes under $WORK: HOME, the XDG base dirs and
# XDG_RUNTIME_DIR are all private, so a run never sees or touches the user's
# own Mandelbrot data, keyring or Flatpak installation.

APP_ID=${APP_ID:-org.tunaos.mandelbrot}
# The runtime the bundle needs. Read from the manifest the bundle was built
# from, so a runtime bump in the manifest is picked up without editing this.
MANIFEST=${MANIFEST:-$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)/org.tunaos.mandelbrot.json}
FLATHUB_REPO=${FLATHUB_REPO:-https://dl.flathub.org/repo/flathub.flatpakrepo}

gui_log() { printf '[%s] %s\n' "${HARNESS_NAME:-gui}" "$*"; }
gui_die() {
    gui_log "FATAL: $*"
    exit 2
}

# Point HOME, the XDG dirs and the runtime dir at a fresh tree under $1.
isolate_env() { # work_dir
    WORK=$1
    mkdir -p "$WORK"/{home,config,data,cache,state,run}
    chmod 700 "$WORK/run"
    export HOME=$WORK/home
    export XDG_CONFIG_HOME=$WORK/config
    export XDG_DATA_HOME=$WORK/data
    export XDG_CACHE_HOME=$WORK/cache
    export XDG_STATE_HOME=$WORK/state
    export XDG_RUNTIME_DIR=$WORK/run
    unset DISPLAY WAYLAND_DISPLAY DBUS_SESSION_BUS_ADDRESS
}

# Install the bundle and its runtime into the (isolated) per-user
# installation.
install_bundle() { # bundle.flatpak
    local runtime
    runtime="$(jq -r .runtime "$MANIFEST")//$(jq -r '."runtime-version"' "$MANIFEST")"
    flatpak --user remote-add --if-not-exists flathub "$FLATHUB_REPO"
    gui_log "installing $runtime"
    flatpak --user install -y --noninteractive flathub "$runtime"
    gui_log "installing $1"
    flatpak --user install -y --noninteractive --bundle "$1"
    flatpak --user info "$APP_ID"
}

# Start a headless mutter on a private Wayland socket. Must run inside the
# D-Bus session the app will use (see in_dbus_session).
COMPOSITOR_PID=""
start_compositor() { # log_file
    export WAYLAND_DISPLAY=wayland-mandelbrot-test
    mutter --headless --wayland --no-x11 --virtual-monitor 1280x800 \
        --wayland-display="$WAYLAND_DISPLAY" >"$1" 2>&1 &
    COMPOSITOR_PID=$!
    local _
    for _ in $(seq 1 100); do
        [ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ] && return 0
        kill -0 "$COMPOSITOR_PID" 2>/dev/null || break
        sleep 0.2
    done
    cat "$1" >&2
    gui_die "mutter did not create $XDG_RUNTIME_DIR/$WAYLAND_DISPLAY"
}

stop_compositor() {
    [ -n "$COMPOSITOR_PID" ] && kill "$COMPOSITOR_PID" 2>/dev/null
    COMPOSITOR_PID=""
}

# Re-execute the calling script inside a private D-Bus session bus, once.
in_dbus_session() { # "$0" "$@"
    if [ -z "${MANDELBROT_TEST_IN_DBUS:-}" ]; then
        export MANDELBROT_TEST_IN_DBUS=1
        exec dbus-run-session -- bash "$@"
    fi
}

# Run the installed app in the background with debug logging for its own
# crate; its stdout+stderr go to $1. Extra `flatpak run` options may follow.
APP_PID=""
start_app() { # log_file [flatpak-run options…]
    local log_file=$1
    shift
    flatpak --user run \
        --env=RUST_LOG="${APP_RUST_LOG:-fractal=debug,mandelbrot_matrixrtc=debug,warn}" \
        --env=RUST_BACKTRACE=1 \
        "$@" "$APP_ID" >"$log_file" 2>&1 &
    APP_PID=$!
}

stop_app() {
    [ -n "$APP_PID" ] || return 0
    kill -TERM "$APP_PID" 2>/dev/null
    local _
    for _ in $(seq 1 25); do
        kill -0 "$APP_PID" 2>/dev/null || break
        sleep 0.2
    done
    kill -KILL "$APP_PID" 2>/dev/null
    wait "$APP_PID" 2>/dev/null
    APP_PID=""
}

# Lines in the app log that mean the run is broken regardless of what the
# UI looked like: a Rust panic, or a GLib/GTK critical (which is a
# programming error, not an environment hiccup).
fatal_log_lines() { # log_file
    grep -nE "panicked|CRITICAL" "$1" || true
}
