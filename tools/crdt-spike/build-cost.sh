#!/usr/bin/env bash
# Build cost per candidate: clean release build time of the candidate crate
# alone (its whole dependency tree, nothing of ours), its dependency tree
# size, and how much it adds to the spike binary. Run from tools/crdt-spike.
set -euo pipefail
cd "$(dirname "$0")"
TARGET=${CARGO_TARGET_DIR:-$PWD/target}
echo "rustc: $(rustc --version); cores: $(nproc)"
for c in automerge loro yrs; do
  dir=$(mktemp -d)
  start=$(date +%s.%N)
  CARGO_TARGET_DIR=$dir cargo build --release --locked -q -p "$c" 2>/dev/null || echo "$c: build FAILED"
  end=$(date +%s.%N)
  lines=$(cargo tree --locked -p "$c" -e normal | wc -l)
  uniq=$(cargo tree --locked -p "$c" -e normal,build --prefix none | sed 's/ (\*)//' | sort -u | wc -l)
  echo "$c: clean release build $(awk "BEGIN{printf \"%.1f\", $end - $start}") s; cargo tree lines $lines; unique crates (normal+build) $uniq"
  rm -rf "$dir"
done
# Feature-gated spike builds: each candidate alone must build; the binary
# growth over the no-candidate build is the candidate's code-size cost.
for f in "" automerge loro yrs; do
  if [ -z "$f" ]; then args=(--no-default-features); name=none; else args=(--no-default-features --features "$f"); name=$f; fi
  CARGO_TARGET_DIR=$TARGET cargo build --release --locked -q "${args[@]}"
  cp "$TARGET/release/crdt-spike" "/tmp/crdt-spike-$name"
  strip "/tmp/crdt-spike-$name"
  echo "spike binary with [$name]: $(stat -c %s "/tmp/crdt-spike-$name") bytes stripped; spike tree lines $(cargo tree --locked "${args[@]}" -e normal | wc -l)"
done
CARGO_TARGET_DIR=$TARGET cargo build --release --locked -q
