#!/usr/bin/env bash
# Regenerate po/gtk-office-suite.pot from i18n() call sites.
# Prefers xtr (cargo install xtr) which understands Rust; falls back to
# xgettext's C parser, which handles our plain string literals fine.
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=po/gtk-office-suite.pot
SRCS=$(git ls-files '*.rs' | grep -v '^target/')
if command -v xtr >/dev/null 2>&1; then
    # xtr wants crate roots; run per app/lib main and merge.
    xtr --keywords i18n -o "$OUT" suite-common/src/lib.rs
else
    # shellcheck disable=SC2086
    xgettext --language=C --keyword=i18n --from-code=UTF-8 \
        --package-name=gtk-office-suite --add-comments=TRANSLATORS \
        -o "$OUT" $SRCS
fi
echo "wrote $OUT ($(grep -c ^msgid "$OUT") strings)"
