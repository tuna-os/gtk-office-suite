#!/usr/bin/env bash
# shellcheck disable=SC2001 # indenting a multi-line list: sed is clearer
# Fail when the fork modifies an upstream file that fork/touchpoints.txt does
# not list. Every listed file is a place an upstream merge can conflict, so
# the list is kept short on purpose and grows only by a reviewed edit.
#
# Usage: fork/check-touchpoints.sh [upstream-ref]   (default: upstream/main)
# Files the fork adds (fork-only code) are not touchpoints and never fail.
set -euo pipefail

ref=${1:-upstream/main}
root=$(git rev-parse --show-toplevel)
list="$root/fork/touchpoints.txt"

modified=$(git -C "$root" diff --name-status "$ref"...HEAD \
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
