#!/usr/bin/env bash
# Render lab driver. On the host it re-executes itself inside the
# render-lab container; inside, it runs the whole pipeline:
#   fixtures -> LibreOffice reference -> build -> capture (A, B) -> compare
#
#   tools/render-lab/run.sh                      # everything
#   tools/render-lab/run.sh --app decks --tier A # narrower
#   RENDER_LAB_OUT=/tmp/x tools/render-lab/run.sh
#   RENDER_LAB_CORPUS=real tools/render-lab/run.sh --app letters
#   RENDER_LAB_CORPUS=real tools/render-lab/run.sh --app letters --shard 2/6
#
# RENDER_LAB_CORPUS=real runs the published real documents of
# tools/render-lab/real_corpus (#1200) instead of the single-feature
# fixtures: fetched by fetch_real_corpus.py into RENDER_LAB_CORPUS_CACHE
# (default .cache/render-lab-corpus), no edit journeys, and ratcheted
# against tools/render-lab/baseline-real.json.
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
        -e RENDER_LAB_CORPUS="${RENDER_LAB_CORPUS:-}" \
        -e RENDER_LAB_CORPUS_CACHE="${RENDER_LAB_CORPUS_CACHE:+/workspace/$(realpath --relative-to="$REPO" "${RENDER_LAB_CORPUS_CACHE}")}" \
        -w /workspace "$IMAGE" tools/render-lab/run.sh "$@"
fi

APP_ARGS=()
TIER_ARGS=()
UPDATE_ARGS=()
SHARD_ARGS=()
while [ $# -gt 0 ]; do
    case "$1" in
        --app) APP_ARGS=(--app "$2"); shift 2 ;;
        --tier) TIER_ARGS+=(--tier "$2"); shift 2 ;;
        --update-baseline) UPDATE_ARGS=(--update-baseline); shift ;;
        # K/N: this run's share of the real corpus (CI splits each app).
        --shard) SHARD_ARGS=(--shard "$2"); shift 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

REAL="${RENDER_LAB_CORPUS:-}"
case "$REAL" in
    "") BASELINE="$LAB/baseline.json" ;;
    real) BASELINE="$LAB/baseline-real.json" ;;
    *) echo "unknown RENDER_LAB_CORPUS: $REAL (only 'real')" >&2; exit 2 ;;
esac

mkdir -p "$OUT"
echo "== fixtures"
if [ -n "$REAL" ]; then
    python3 "$LAB/fetch_real_corpus.py" "$OUT/fixtures" "${APP_ARGS[@]}" "${SHARD_ARGS[@]}" \
        --cache "${RENDER_LAB_CORPUS_CACHE:-$REPO/.cache/render-lab-corpus}"
else
    python3 "$LAB/fixtures.py" "$OUT/fixtures"
fi
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
if [ -z "$REAL" ]; then
    echo "== edit journeys"
    # Edit, save and reopen through the GUI; the saved files join the
    # manifest as <app>/edited-journey (#1201), so the LibreOffice reference
    # and the capture below cover them like any fixture.
    python3 "$LAB/edit_journeys.py" "$OUT/fixtures" "${APP_ARGS[@]}" || echo "(an edit journey failed; see above)"
fi
echo "== LibreOffice reference"
python3 "$LAB/lo_render.py" "$OUT/fixtures" "$OUT" "${APP_ARGS[@]}" || echo "(some references failed; see above)"
echo "== capture"
python3 "$LAB/capture.py" "$OUT/fixtures" "$OUT" "${APP_ARGS[@]}" "${TIER_ARGS[@]}"
echo "== compare"
python3 "$LAB/compare.py" "$OUT/fixtures" "$OUT" "${APP_ARGS[@]}" --baseline "$BASELINE" "${UPDATE_ARGS[@]}"
echo "report: $OUT/report.html"
