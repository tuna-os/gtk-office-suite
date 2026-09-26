#!/usr/bin/env bash
# Export-parity driver (docs/EXPORT-PARITY-SPEC.md). On the host it
# re-executes itself inside the render-lab container; inside, it runs:
#   fixtures -> LibreOffice reference -> build -> export (ours PDF -> PNG)
#   -> export_compare (ours vs lo, existing metrics, separate baseline)
#
#   tools/render-lab/run-export.sh --app letters
#   tools/render-lab/run-export.sh --app decks --update-baseline
#
# Output: render-lab-out/export-report.html and
# render-lab-out/scorecard-export.json. The committed export baseline
# (tools/render-lab/baseline-export.json, verdicts only, never
# baseline.json) is the ratchet; --update-baseline rewrites it after an
# improvement the reviewer has looked at.
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
        -e CARGO_PROFILE_DEV_DEBUG=line-tables-only -e CARGO_PROFILE_TEST_DEBUG=line-tables-only -e CARGO_INCREMENTAL=0 \
        -e RENDER_LAB_OUT="/workspace/$(realpath --relative-to="$REPO" "$OUT")" \
        -e RENDER_LAB_SKIP_BUILD="${RENDER_LAB_SKIP_BUILD:-}" \
        -w /workspace "$IMAGE" tools/render-lab/run-export.sh "$@"
fi

APP_ARGS=()
UPDATE_ARGS=()
while [ $# -gt 0 ]; do
    case "$1" in
        --app) APP_ARGS=(--app "$2"); shift 2 ;;
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
    if [ ${#APP_ARGS[@]} -gt 0 ]; then
        cargo build --bin "${APP_ARGS[1]}"
    else
        cargo build --bin letters --bin decks
    fi
fi
echo "== export (our PDF, rasterized like the reference)"
python3 "$LAB/export_render.py" "$OUT/fixtures" "$OUT" "${APP_ARGS[@]}"
echo "== export compare"
python3 "$LAB/export_compare.py" "$OUT/fixtures" "$OUT" "${APP_ARGS[@]}" --baseline "$LAB/baseline-export.json" "${UPDATE_ARGS[@]}"
echo "report: $OUT/export-report.html"
