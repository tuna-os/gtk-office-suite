#!/usr/bin/env bash
# The locked source archive a release's offline builds start from (#1209):
# the revision's tracked files and every crate Cargo.lock names, vendored,
# so a Flatpak builds with no network (as Flathub requires) from one file
# with a known SHA-256.
#
#   tools/release/source_archive.sh <revision> <out-dir>
#
# Writes gtk-office-suite-<revision>.tar.gz and its .sha256. Reproducible:
# the same revision gives the same bytes (sorted, owner 0, the commit's
# time as every mtime, gzip without a timestamp).
set -euo pipefail
rev="$1"; out="$(realpath -m "$2")"
repo="$(cd "$(dirname "$0")/../.." && pwd)"
commit="$(git -C "$repo" rev-parse "$rev^{commit}")"
epoch="$(git -C "$repo" log -1 --format=%ct "$commit")"
name="gtk-office-suite-${commit:0:12}"
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
mkdir -p "$work/$name" "$out"
git -C "$repo" archive "$commit" | tar -x -C "$work/$name"
# cargo vendor rewrites a git crate's workspace-inherited manifest into a
# standalone one, which a hand-made vendor directory could not.
mkdir -p "$work/$name/.cargo"
(cd "$work/$name" && cargo vendor --locked --versioned-dirs vendor > .cargo/config.toml)
tar --sort=name --mtime="@$epoch" --owner=0 --group=0 --numeric-owner \
    -C "$work" -cf - "$name" | gzip -n -9 > "$out/$name.tar.gz"
(cd "$out" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
cat "$out/$name.tar.gz.sha256"
