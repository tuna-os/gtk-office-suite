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
# xdg-dbus-proxy (a flatpak dependency), gnome-keyring, libsecret-tools,
# xdg-desktop-portal, xdg-desktop-portal-gnome, xdg-desktop-portal-gtk,
# and, for the call test, at-spi2-core.
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
# The path is made absolute because the scripts cd before calling this.
in_dbus_session() { # "$0" "$@"
    if [ -z "${MANDELBROT_TEST_IN_DBUS:-}" ]; then
        export MANDELBROT_TEST_IN_DBUS=1
        local script
        script=$(realpath "$1")
        shift
        exec dbus-run-session -- bash "$script" "$@"
    fi
}

# The app keeps its session secrets in the keyring through the Secret portal
# (src/secret/linux.rs); without one it shows "Secret Portal Error" at
# startup and cannot log in. Start an unlocked gnome-keyring, point
# xdg-desktop-portal's Secret interface at it, and check both work before the
# app ever asks. The portals are D-Bus activated, so the bus's activation
# environment must carry the display and desktop first: xdg-desktop-portal-gtk
# (the Settings portal) exits immediately without a display.
# Must run inside the D-Bus session, after start_compositor.
start_secret_portal() {
    export XDG_CURRENT_DESKTOP=GNOME
    mkdir -p "$XDG_CONFIG_HOME/xdg-desktop-portal"
    printf '[preferred]\ndefault=gtk\norg.freedesktop.impl.portal.Secret=gnome-keyring\n' \
        >"$XDG_CONFIG_HOME/xdg-desktop-portal/portals.conf"
    # --unlock creates the "login" keyring with the password read from stdin,
    # unlocked; --start then adds the secrets component on the session bus.
    # Both print the daemon's environment (GNOME_KEYRING_CONTROL=…).
    local keyring_env
    keyring_env=$(printf 'mandelbrot-test' | gnome-keyring-daemon --unlock --components=secrets) ||
        gui_die "gnome-keyring-daemon --unlock failed"
    eval "$keyring_env"
    keyring_env=$(gnome-keyring-daemon --start --components=secrets) ||
        gui_die "gnome-keyring-daemon --start failed"
    eval "$keyring_env"
    export GNOME_KEYRING_CONTROL
    dbus-update-activation-environment WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XDG_RUNTIME_DIR \
        XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME HOME GNOME_KEYRING_CONTROL ||
        gui_die "dbus-update-activation-environment failed"

    printf 'probe' | secret-tool store --label=probe mandelbrot-test probe ||
        gui_die "the keyring does not accept secrets"
    [ "$(secret-tool lookup mandelbrot-test probe)" = probe ] ||
        gui_die "the keyring does not return secrets"
    # The first introspect D-Bus-activates xdg-desktop-portal, which can
    # answer before it has exported the Secret interface; give it 15 s.
    local _
    for _ in $(seq 1 30); do
        if gdbus introspect --session --dest org.freedesktop.portal.Desktop \
            --object-path /org/freedesktop/portal/desktop 2>/dev/null |
            grep -q "interface org.freedesktop.portal.Secret"; then
            gui_log "keyring unlocked; Secret portal available"
            return 0
        fi
        sleep 0.5
    done
    gui_die "xdg-desktop-portal does not export the Secret portal"
}

# A PulseAudio server with no hardware: it loads a null sink, whose monitor
# is the default source, so the app's microphone pipeline (autoaudiosrc)
# captures silence and publishes a real audio track.
start_audio() {
    pulseaudio --daemonize=yes --exit-idle-time=-1 --log-target=stderr \
        2>"$1" || gui_die "pulseaudio did not start"
}

stop_audio() {
    pulseaudio --kill 2>/dev/null || true
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
# UI looked like: a Rust panic, a GLib/GTK critical (a programming error, not
# an environment hiccup), or an ERROR the app itself logged (`fractal::…`
# targets; e.g. the keyring could not be read through the Secret portal).
# The session is complete enough — compositor, keyring, portals — that none
# of these are expected. The level and target are separated by ANSI escapes
# in the log, hence the loose match.
fatal_log_lines() { # log_file
    grep -nE "panicked|CRITICAL|ERROR.*fractal::" "$1" || true
}
