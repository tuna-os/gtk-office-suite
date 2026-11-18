#!/usr/bin/env bash
# shellcheck disable=SC2001 # indenting a multi-line list: sed is clearer
# Fail when the fork modifies an upstream file that fork/touchpoints.txt does
# not list. Every listed file is a place an upstream merge can conflict, so
# the list is kept short on purpose and grows only by a reviewed edit.
#
# Usage: lifafa/fork/check-touchpoints.sh [upstream-commit]
#        (default: the commit in lifafa/fork/UPSTREAM_BASE)
#
# Lifafa was imported into gtk-office-suite as a squashed tree (#1957), so
# upstream's history is not this repository's and there is no merge base to
# diff against. Instead this compares the tree of the upstream commit lifafa/
# is built on (fork/UPSTREAM_BASE, which fork/sync-upstream.sh advances) with
# lifafa/ as committed at HEAD. Files the fork adds (fork-only code) are not
# touchpoints and never fail.
set -euo pipefail

UPSTREAM_URL=${UPSTREAM_URL:-https://github.com/hyprlab/hylki.git}

root=$(git rev-parse --show-toplevel)
cd "$root"
list=lifafa/fork/touchpoints.txt
base=${1:-$(sed -n '/^[0-9a-f]\{40\}$/p' lifafa/fork/UPSTREAM_BASE | head -1)}
if [ -z "$base" ]; then
    echo "check-touchpoints: lifafa/fork/UPSTREAM_BASE holds no commit" >&2
    exit 2
fi
if ! git cat-file -e "$base^{tree}" 2>/dev/null; then
    git fetch --quiet --no-tags --depth=1 "$UPSTREAM_URL" "$base"
fi

modified=$(git diff --no-renames --name-status "$base" HEAD:lifafa \
    | awk '$1 != "A" && $2 !~ /^po\// { print $NF }' | sort -u)

unlisted=$(comm -23 <(printf '%s\n' "$modified" | sed '/^$/d') <(sed '/^$/d; /^#/d' "$list" | sort -u))
stale=$(comm -13 <(printf '%s\n' "$modified" | sed '/^$/d') <(sed '/^$/d; /^#/d' "$list" | sort -u))

if [ -n "$stale" ]; then
    echo "Listed in fork/touchpoints.txt but no longer modified (remove them):"
    sed 's/^/  /' <<<"$stale"
fi
if [ -n "$unlisted" ]; then
    echo "The fork modifies upstream files not listed in fork/touchpoints.txt:"
    sed 's/^/  /' <<<"$unlisted"
    echo "Either keep the change upstream-shaped and send it upstream, or add the"
    echo "file to fork/touchpoints.txt with a reason (see fork/MAINTAINING.md)."
    exit 1
fi
echo "touchpoints ok: $(printf '%s\n' "$modified" | sed '/^$/d' | wc -l) upstream files modified, all listed"
