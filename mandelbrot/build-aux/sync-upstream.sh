#!/usr/bin/env bash
# Upstream synchronization script for Mandelbrot (downstream fork of GNOME Fractal).
#
# Usage: build-aux/sync-upstream.sh [upstream_branch]
#
# Fetches https://gitlab.gnome.org/World/fractal.git, creates a sync branch
# from the current HEAD, and merges an upstream branch into it.
#
# Without an argument the newest upstream stable branch (`fractal-N`) is
# merged. Stable branches only carry Fractal releases, which build against
# released matrix-sdk/ruma crates. Upstream `main` is Fractal's development
# branch and regularly pins matrix-sdk and ruma to unreleased git revisions,
# which this fork does not adopt; take fixes from it with
# `git cherry-pick -x <sha>` instead (the script lists the candidates).
# Passing `main` explicitly still merges it, with a warning about any git
# dependencies it would bring in. See docs/UPSTREAM.md.

set -euo pipefail

UPSTREAM_URL="${UPSTREAM_URL:-https://gitlab.gnome.org/World/fractal.git}"
BASELINE_COMMIT="a262c7d656cc4c3a87656370e10cd279cf6e081a"

# Configure upstream remote
if git remote get-url upstream >/dev/null 2>&1; then
    git remote set-url upstream "$UPSTREAM_URL"
else
    git remote add upstream "$UPSTREAM_URL"
fi

if [ $# -ge 1 ]; then
    UPSTREAM_BRANCH="$1"
else
    UPSTREAM_BRANCH=$(git ls-remote --heads upstream 'refs/heads/fractal-*' |
        sed 's|.*refs/heads/||' | sort -V | tail -n 1)
    if [ -z "$UPSTREAM_BRANCH" ]; then
        echo "Could not find an upstream stable branch (fractal-N)." >&2
        exit 1
    fi
fi

echo "=== Mandelbrot Upstream Sync ==="
echo "Upstream URL:    $UPSTREAM_URL"
echo "Upstream Branch: $UPSTREAM_BRANCH"
echo "Baseline Commit: $BASELINE_COMMIT"
echo "================================"

echo "Fetching upstream refs..."
git fetch upstream --tags
git fetch upstream main "$UPSTREAM_BRANCH"

LATEST_UPSTREAM_COMMIT=$(git rev-parse "upstream/$UPSTREAM_BRANCH")
MERGE_BASE=$(git merge-base HEAD "upstream/$UPSTREAM_BRANCH" || echo "$BASELINE_COMMIT")
NEW_COMMITS_COUNT=$(git rev-list --count "$MERGE_BASE..upstream/$UPSTREAM_BRANCH")

echo "Merge Base:              $MERGE_BASE"
echo "Latest Upstream Commit:  $LATEST_UPSTREAM_COMMIT"
echo "New upstream commits:    $NEW_COMMITS_COUNT"

# Upstream comments out its git dependency lines on stable branches, so only
# count uncommented ones.
GIT_DEPS=$(git show "upstream/$UPSTREAM_BRANCH:Cargo.toml" | grep -nE '^[[:space:]]*git[[:space:]]*=' || true)
if [ -n "$GIT_DEPS" ]; then
    echo ""
    echo "WARNING: upstream/$UPSTREAM_BRANCH depends on unreleased git revisions:"
    echo "$GIT_DEPS"
    echo "Mandelbrot only uses released crates: keep our Cargo.toml/Cargo.lock when"
    echo "resolving, and drop any source change that needs those revisions."
fi

if [ "$NEW_COMMITS_COUNT" -eq 0 ]; then
    echo "Up to date with upstream/$UPSTREAM_BRANCH. No new commits to merge."
else
    echo ""
    echo "New upstream commits since merge base:"
    git log -n 25 --format="* %h - %s (%an, %as)" "$MERGE_BASE..upstream/$UPSTREAM_BRANCH"
    echo ""

    SYNC_BRANCH="sync/upstream-$(date +%Y%m%d)"
    echo "Creating sync branch: $SYNC_BRANCH"
    git checkout -B "$SYNC_BRANCH"

    echo "Attempting automated merge from upstream/$UPSTREAM_BRANCH..."
    if git merge --no-commit --no-ff "upstream/$UPSTREAM_BRANCH"; then
        echo "Automated merge succeeded without conflicts."
    else
        echo "Merge conflicts detected. Resolve them following docs/UPSTREAM.md"
        echo "(git status / git diff list them)."
    fi
    echo "Review staged changes, run the checks, and commit with:"
    echo "  git commit -s -m 'chore(upstream): merge Fractal <version> ($UPSTREAM_BRANCH branch)'"
fi

if [ "$UPSTREAM_BRANCH" != "main" ]; then
    echo ""
    echo "Cherry-pick candidates: upstream/main commits whose change is not in HEAD yet."
    echo "Pick fixes and translations with 'git cherry-pick -x <sha>'; skip dependency"
    echo "bumps and anything needing unreleased matrix-sdk/ruma:"
    git log --cherry-pick --right-only --no-merges --format="* %h - %s (%as)" \
        "HEAD...upstream/main"
fi
