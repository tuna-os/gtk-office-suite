#!/usr/bin/env bash
# Run a command inside the GUI test container, with this checkout mounted.
#
#   tests/gui/container/run.sh tests/gui/run_gui_tests.sh test_smoke.py
#   tests/gui/container/run.sh --build cargo build --bin letters
#
# The point is that a CI failure is reproducible: this is the same image
# the workflows use, so "works on my machine" and "works in CI" stop
# being different claims.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
IMAGE="${GUI_TEST_IMAGE:-ghcr.io/tuna-os/gtk-office-suite/gui-test:main}"

ENGINE="${CONTAINER_ENGINE:-}"
if [ -z "$ENGINE" ]; then
    if command -v podman >/dev/null 2>&1; then
        ENGINE=podman
    elif command -v docker >/dev/null 2>&1; then
        ENGINE=docker
    else
        echo "error: neither podman nor docker is installed" >&2
        exit 1
    fi
fi

BUILD_LOCAL=0
if [ "${1:-}" = "--build" ]; then
    BUILD_LOCAL=1
    IMAGE="gtk-office-gui-test:local"
    shift
fi

if [ "$#" -eq 0 ]; then
    set -- bash
fi

# -t on a pipe (CI, `just`, a script) makes the engine refuse to start.
TTY_FLAGS=(-i)
if [ -t 0 ] && [ -t 1 ]; then
    TTY_FLAGS=(-it)
fi

if [ "$BUILD_LOCAL" = "1" ]; then
    "$ENGINE" build -t "$IMAGE" "$REPO_ROOT/tests/gui/container"
elif ! "$ENGINE" image inspect "$IMAGE" >/dev/null 2>&1; then
    "$ENGINE" pull "$IMAGE"
fi

# Host and container builds must not share target/: the same path with a
# different toolchain and glibc means one side rebuilds the world every
# time, and stale artifacts are hard to tell apart from a real failure.
exec "$ENGINE" run --rm "${TTY_FLAGS[@]}" \
    -v "$REPO_ROOT:/workspace:z" \
    -w /workspace \
    --shm-size=1g \
    -e CARGO_TARGET_DIR=/workspace/target/container \
    -e GUI_TEST_VIDEO="${GUI_TEST_VIDEO:-}" \
    -e GUI_TEST_VIDEO_DIR="${GUI_TEST_VIDEO_DIR:-}" \
    "$IMAGE" "$@"
