#!/usr/bin/env bash
# Render lab driver. On the host it re-executes itself inside the
# render-lab container; inside, it runs the whole pipeline:
#   fixtures -> LibreOffice reference -> build -> capture (A, B) -> compare
#
#   tools/render-lab/run.sh                      # everything
#   tools/render-lab/run.sh --app decks --tier A # narrower
#   RENDER_LAB_OUT=/tmp/x tools/render-lab/run.sh
#
# Output: render-lab-out/report.html and render-lab-out/scorecard.json.
# The committed baseline (tools/render-lab/baseline.json, verdicts only) is
# the ratchet; --update-baseline rewrites it after an improvement.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LAB="$REPO/tools/render-lab"
OUT="${RENDER_LAB_OUT:-$REPO/render-lab-out}"

if [ -z "${RENDER_LAB:-}" ]; then
    ENGINE="${CONTAINER_ENGINE:-$(command -v podman || command -v docker)}"
    IMAGE="${RENDER_LAB_IMAGE:-render-lab}"
    mkdir -p "${HOME}/.cache/render-lab-cargo"
    exec "$ENGINE" run --rm -i \
        -v "$REPO:/workspace:Z" \
        -v "${HOME}/.cache/render-lab-cargo:/cargo-home:Z" \
        -e CARGO_HOME=/cargo-home -e PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin \
        -e RENDER_LAB_OUT="/workspace/$(realpath --relative-to="$REPO" "$OUT")" \
        -e RENDER_LAB_SKIP_BUILD="${RENDER_LAB_SKIP_BUILD:-}" \
        -w /workspace "$IMAGE" tools/render-lab/run.sh "$@"
fi

APP_ARGS=()
TIER_ARGS=()
UPDATE_ARGS=()
while [ $# -gt 0 ]; do
    case "$1" in
        --app) APP_ARGS=(--app "$2"); shift 2 ;;
        --tier) TIER_ARGS+=(--tier "$2"); shift 2 ;;
        --update-baseline) UPDATE_ARGS=(--update-baseline); shift ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

mkdir -p "$OUT"
echo "== fixtures"
python3 "$LAB/fixtures.py" "$OUT/fixtures"
echo "== LibreOffice reference"
python3 "$LAB/lo_render.py" "$OUT/fixtures" "$OUT" "${APP_ARGS[@]}" || echo "(some references failed; see above)"
if [ -z "${RENDER_LAB_SKIP_BUILD:-}" ]; then
    echo "== build"
    # Only the app being tested when --app is given: CI runs one job per
    # app, and building the other two would triple each job's build.
    if [ ${#APP_ARGS[@]} -gt 0 ]; then
        cargo build --bin "${APP_ARGS[1]}"
    else
        cargo build --bin letters --bin tables --bin decks
    fi
fi
echo "== capture"
python3 "$LAB/capture.py" "$OUT/fixtures" "$OUT" "${APP_ARGS[@]}" "${TIER_ARGS[@]}"
echo "== compare"
python3 "$LAB/compare.py" "$OUT/fixtures" "$OUT" "${APP_ARGS[@]}" --baseline "$LAB/baseline.json" "${UPDATE_ARGS[@]}"
echo "report: $OUT/report.html"
