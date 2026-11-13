#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Bring upstream Hylki's changes into lifafa/ (docs/CUTOVER-LIFAFA-MANDELBROT.md,
# step 5).
#
#   lifafa/fork/sync-upstream.sh [upstream-ref]      (default: main)
#
# Lifafa was imported into gtk-office-suite as a squashed tree (#1957), so
# upstream's history is not this repository's and a `git merge` of
# hyprlab/hylki has no common base. Instead fork/UPSTREAM_BASE records the
# upstream commit lifafa/ is built on, and this applies upstream's own diff
# from that commit to the new one under lifafa/, three-way: where the fork
# changed the same lines, the file is left with conflict markers to resolve,
# never silently overwritten. On success UPSTREAM_BASE names the new commit.
#
# Exit status: 0 when it applied cleanly (or there was nothing new), 1 when
# files were left with conflicts (listed on stderr), 2 on any other failure.
# It changes the working tree only; committing is the caller's job.
set -euo pipefail

UPSTREAM_URL=${UPSTREAM_URL:-https://github.com/hyprlab/hylki.git}
target_ref=${1:-main}

root=$(git rev-parse --show-toplevel)
cd "$root"
base_file=lifafa/fork/UPSTREAM_BASE
base=$(sed -n '/^[0-9a-f]\{40\}$/p' "$base_file" | head -1)
if [ -z "$base" ]; then
    echo "sync-upstream: $base_file holds no commit" >&2
    exit 2
fi
if ! git diff --quiet HEAD -- lifafa/ || ! git diff --cached --quiet -- lifafa/; then
    echo "sync-upstream: lifafa/ has uncommitted changes; commit or stash them first" >&2
    exit 2
fi

# Both ends, with their trees: the patch's index lines name the base's blobs,
# which the three-way apply reads.
git fetch --quiet --no-tags --depth=1 "$UPSTREAM_URL" "$base"
git fetch --quiet --no-tags --depth=1 "$UPSTREAM_URL" "$target_ref"
target=$(git rev-parse FETCH_HEAD)

if [ "$target" = "$base" ]; then
    echo "sync-upstream: lifafa/ is already on $UPSTREAM_URL $target_ref (${base:0:12})"
    exit 0
fi

patch=$(mktemp)
trap 'rm -f "$patch"' EXIT
git diff --binary --full-index "$base" "$target" > "$patch"
if [ ! -s "$patch" ]; then
    echo "${target}" > "$base_file"
    echo "sync-upstream: no changes between ${base:0:12} and ${target:0:12}"
    exit 0
fi

status=0
if ! git apply --3way --whitespace=nowarn --directory=lifafa "$patch"; then
    status=1
fi
conflicts=$(git diff --name-only --diff-filter=U -- lifafa/)
if [ -n "$conflicts" ]; then
    status=1
fi

printf '%s\n' "$target" > "$base_file"
git add -- "$base_file"

if [ "$status" -ne 0 ]; then
    echo "sync-upstream: applied ${base:0:12}..${target:0:12} with conflicts in:" >&2
    printf '  %s\n' $conflicts >&2
    exit 1
fi
echo "sync-upstream: applied ${base:0:12}..${target:0:12} cleanly"
