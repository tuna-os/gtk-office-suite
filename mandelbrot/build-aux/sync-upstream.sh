#!/usr/bin/env bash
# Bring upstream GNOME Fractal's changes into mandelbrot/ (docs/UPSTREAM.md,
# and docs/CUTOVER-LIFAFA-MANDELBROT.md step 5 in the suite).
#
#   mandelbrot/build-aux/sync-upstream.sh [upstream-ref]
#
# Without an argument the newest upstream stable branch (`fractal-N`) is used.
# Stable branches only carry Fractal releases, which build against released
# matrix-sdk/ruma crates. Upstream `main` regularly pins matrix-sdk and ruma
# to unreleased git revisions, which Mandelbrot does not adopt; take fixes
# from it one by one instead (the script lists the candidates).
#
# Mandelbrot lives in gtk-office-suite as a squashed tree (#1961), so a
# `git merge` of Fractal has no common history and would put Fractal's files
# at the suite's root. Instead build-aux/UPSTREAM_BASE records the upstream
# commit mandelbrot/ is synced to, and this applies upstream's own diff from
# that commit to the new one under mandelbrot/, three-way: where Mandelbrot
# changed the same lines, the file is left with conflict markers to resolve
# by docs/UPSTREAM.md's conflict rules, never silently overwritten. On
# success UPSTREAM_BASE names the new commit.
#
# Exit status: 0 when it applied cleanly (or there was nothing new), 1 when
# files were left with conflicts (listed on stderr), 2 on any other failure.
# It changes the working tree only; committing is the caller's job.
set -euo pipefail

UPSTREAM_URL=${UPSTREAM_URL:-https://gitlab.gnome.org/World/fractal.git}

root=$(git rev-parse --show-toplevel)
cd "$root"
dir=mandelbrot
base_file=$dir/build-aux/UPSTREAM_BASE
base=$(sed -n '/^[0-9a-f]\{40\}$/p' "$base_file" | head -1)
if [ -z "$base" ]; then
    echo "sync-upstream: $base_file holds no commit" >&2
    exit 2
fi
if ! git diff --quiet HEAD -- "$dir/" || ! git diff --cached --quiet -- "$dir/"; then
    echo "sync-upstream: $dir/ has uncommitted changes; commit or stash them first" >&2
    exit 2
fi

if [ $# -ge 1 ]; then
    target_ref=$1
else
    target_ref=$(git ls-remote --heads "$UPSTREAM_URL" 'refs/heads/fractal-*' |
        sed 's|.*refs/heads/||' | sort -V | tail -n 1)
    if [ -z "$target_ref" ]; then
        echo "sync-upstream: no upstream stable branch (fractal-N) found" >&2
        exit 2
    fi
fi

# Both ends, with their trees: the patch's index lines name the base's blobs,
# which the three-way apply reads.
git fetch --quiet --no-tags "$UPSTREAM_URL" "$base"
git fetch --quiet --no-tags "$UPSTREAM_URL" "$target_ref"
target=$(git rev-parse FETCH_HEAD)

echo "sync-upstream: $UPSTREAM_URL $target_ref is ${target:0:12}; $dir/ is on ${base:0:12}"

# Upstream comments out its git dependency lines on stable branches, so only
# uncommented ones count.
git_deps=$(git show "$target:Cargo.toml" | grep -nE '^[[:space:]]*git[[:space:]]*=' || true)
if [ -n "$git_deps" ]; then
    echo "sync-upstream: WARNING: $target_ref depends on unreleased git revisions:" >&2
    echo "$git_deps" >&2
    echo "  Mandelbrot only uses released crates: keep its Cargo.toml/Cargo.lock when" >&2
    echo "  resolving, and drop any source change that needs those revisions." >&2
fi

if [ "$target" = "$base" ]; then
    echo "sync-upstream: $dir/ is already on $target_ref (${base:0:12})"
    exit 0
fi

echo "sync-upstream: upstream commits since ${base:0:12}:"
git log -n 25 --format="  %h %s (%an, %as)" "$base..$target" || true

# Upstream edits to files Mandelbrot deleted or renamed (the GitLab CI, the
# org.gnome.Fractal manifests and metainfo) have nothing here to apply to,
# and `git apply` is all or nothing: one such file used to abort the whole
# sync. They are left out and listed, to carry over by hand where they still
# matter (docs/UPSTREAM.md: upstream-only files stay deleted). Renames are
# off so an upstream rename is a deletion plus an addition.
excluded=()
while IFS= read -r path; do
    [ -n "$path" ] || continue
    if ! git cat-file -e "HEAD:$dir/$path" 2>/dev/null; then
        excluded+=("$path")
    fi
done < <(git diff --no-renames --name-only --diff-filter=MD "$base" "$target")

patch=$(mktemp)
trap 'rm -f "$patch"' EXIT
git diff --no-renames --binary --full-index "$base" "$target" -- . \
    "${excluded[@]/#/:(exclude)}" > "$patch"
if [ ${#excluded[@]} -gt 0 ]; then
    echo "sync-upstream: left out, not in $dir/ (deleted or renamed here):" >&2
    printf '  %s\n' "${excluded[@]}" >&2
fi
if [ ! -s "$patch" ]; then
    printf '%s\n' "$target" > "$base_file"
    echo "sync-upstream: nothing to apply between ${base:0:12} and ${target:0:12}"
    exit 0
fi

status=0
if ! git apply --3way --whitespace=nowarn --directory="$dir" "$patch"; then
    status=1
fi
conflicts=$(git diff --name-only --diff-filter=U -- "$dir/")
if [ -n "$conflicts" ]; then
    status=1
fi

printf '%s\n' "$target" > "$base_file"
git add -- "$base_file"

if [ "$status" -ne 0 ]; then
    if [ -z "$conflicts" ]; then
        echo "sync-upstream: git apply failed before applying anything; see its errors above" >&2
        git checkout HEAD -- "$base_file" 2>/dev/null || true
        exit 2
    fi
    echo "sync-upstream: applied ${base:0:12}..${target:0:12} with conflicts in:" >&2
    printf '  %s\n' $conflicts >&2
    exit 1
fi
echo "sync-upstream: applied ${base:0:12}..${target:0:12} cleanly"
