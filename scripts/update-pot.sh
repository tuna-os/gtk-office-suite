#!/usr/bin/env bash
# Regenerate po/gtk-office-suite.pot from i18n() call sites.
# Source file list is authoritative in po/POTFILES (one path per line,
# relative to repo root).
# Prefers xtr (cargo install xtr) which understands Rust; falls back to
# xgettext's C parser, which handles our plain string literals fine.
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=po/gtk-office-suite.pot

if [[ ! -f po/POTFILES ]]; then
    echo "error: po/POTFILES not found" >&2
    exit 1
fi

# Read file list, skipping comments and blank lines
mapfile -t SRCS < <(grep -v '^#' po/POTFILES | grep -v '^$' || true)
if [[ ${#SRCS[@]} -eq 0 ]]; then
    echo "error: po/POTFILES is empty" >&2
    exit 1
fi

if command -v xtr >/dev/null 2>&1; then
    # xtr wants crate roots; run per app/lib main and merge.
    # Use a temp file to accumulate since xtr writes per-root.
    TMP=$(mktemp)
    for src in "${SRCS[@]}"; do
        if [[ -f "$src" ]]; then
            xtr --keywords i18n -o "$TMP" "$src" 2>/dev/null || true
        fi
    done
    if [[ -s "$TMP" ]]; then
        # Deduplicate and produce a clean pot
        msguniq "$TMP" -o "$OUT" 2>/dev/null || cp "$TMP" "$OUT"
    else
        echo "warning: xtr produced no output; trying xgettext" >&2
        xgettext --language=C --keyword=i18n --from-code=UTF-8 \
            --package-name=gtk-office-suite --add-comments=TRANSLATORS \
            -o "$OUT" "${SRCS[@]}"
    fi
    rm -f "$TMP"
else
    # shellcheck disable=SC2086
    xgettext --language=C --keyword=i18n --from-code=UTF-8 \
        --package-name=gtk-office-suite --add-comments=TRANSLATORS \
        -o "$OUT" "${SRCS[@]}"
fi
echo "wrote $OUT ($(grep -c ^msgid "$OUT") strings)"
