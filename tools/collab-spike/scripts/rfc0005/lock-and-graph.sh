#!/usr/bin/env bash
# Ran on branch plan/collab-in-apps-exp-a (a throwaway: the four dcp crates copied to tools/collab-promoted/ and made members of the product workspace, plus a seam crate collab-session and a tables `collab` feature). Kept here so the numbers in docs/rfc/0005 can be reproduced; not part of any build.
# RFC-0005 experiment: lockfile, graph, licence, vendoring measurements for option A.
# Run at the root of the option-A checkout; $BASE is a checkout of origin/main.
set -uo pipefail
BASE=${BASE:-../base}
OUT=${OUT:-$PWD/results}
mkdir -p "$OUT"
S="$OUT/summary.txt"
: > "$S"
say() { echo "$*" | tee -a "$S"; }
A=$PWD

say "### lockfile (baseline = origin/main, A = this branch)"
for w in "$BASE" "$A"; do
  l="$w/Cargo.lock"
  say "$(basename "$w"): packages=$(grep -c '^name = ' "$l") lines=$(wc -l < "$l") bytes=$(wc -c < "$l") distinct_names=$(grep '^name = ' "$l" | sort -u | wc -l) registry=$(grep -c '^source = "registry' "$l") git=$(grep -c '^source = "git' "$l")"
done
python3 - "$BASE/Cargo.lock" "$A/Cargo.lock" <<'PY' | tee -a "$S"
import re,sys
def load(p):
    d={}
    for n,v in re.findall(r'name = "([^"]+)"\nversion = "([^"]+)"',open(p).read()): d.setdefault(n,set()).add(v)
    return d
b,a=load(sys.argv[1]),load(sys.argv[2])
bp={(n,v) for n in b for v in b[n]}; ap={(n,v) for n in a for v in a[n]}
print("package@version pairs: base",len(bp),"A",len(ap),"added",len(ap-bp),"removed",len(bp-ap))
chg=[(n,sorted(b[n]),sorted(a[n])) for n in b if n in a and b[n]!=a[n]]
print("existing crates whose set of locked versions changed:",len(chg))
bump=[c for c in chg if not set(c[1])<=set(c[2])]
print("  of which an existing locked version was REPLACED (not only a second version added):",len(bump))
for n,x,y in bump: print("   ",n,x,"->",y)
print("names new to the lockfile:",len(set(a)-set(b)))
PY

say; say "### default build graph (unit graph, nothing compiled)"
ug() { # dir label args...
  local d=$1 label=$2; shift 2
  ( cd "$d" && RUSTC_BOOTSTRAP=1 cargo build --locked --release --unit-graph -Z unstable-options "$@" 2>"$OUT/ug-$label.err" ) > "$OUT/ug-$label.json" || say "unit-graph $label FAILED: $(tail -3 "$OUT/ug-$label.err")"
  jq -r '.units[].pkg_id' "$OUT/ug-$label.json" 2>/dev/null | sort -u > "$OUT/units-$label.txt"
  say "$label: units=$(jq '.units|length' "$OUT/ug-$label.json" 2>/dev/null) packages=$(wc -l < "$OUT/units-$label.txt")"
}
ug "$BASE" base-ship -p tables -p letters -p decks
ug "$A" A-ship -p tables -p letters -p decks
say "ship (-p tables -p letters -p decks, default features): package set identical? $(cmp -s "$OUT/units-base-ship.txt" "$OUT/units-A-ship.txt" && echo YES || echo NO)"
diff "$OUT/units-base-ship.txt" "$OUT/units-A-ship.txt" | tee "$OUT/units-ship.diff" | head -80 | tee -a "$S"
say "  package differences: $(grep -c '^[<>]' "$OUT/units-ship.diff")"
python3 - "$OUT/units-ship.diff" <<'PY' | tee -a "$S"
import re,sys
old={};new={}
for l in open(sys.argv[1]):
    m=re.match(r'([<>]) (\S+) (\S+)',l)
    if not m: continue
    (old if m[1]=='<' else new)[m[2]]=m[3]
chg=[n for n in old if n in new]
print("  shipped-build crates compiled at a DIFFERENT version than on main:",len(chg))
print("  shipped-build crates only on A:",len([n for n in new if n not in old]),"only on main:",len([n for n in old if n not in new]))
PY
ug "$A" A-workspace --workspace
ug "$BASE" base-workspace --workspace
say "workspace (--workspace, what ci.yml clippy/nextest use): base packages=$(wc -l < "$OUT/units-base-workspace.txt") A packages=$(wc -l < "$OUT/units-A-workspace.txt")"
ug "$A" A-collab -p tables --features collab-matrix,collab-streams
say "A with tables/collab-matrix,collab-streams: packages=$(wc -l < "$OUT/units-A-collab.txt") (shipping default $(wc -l < "$OUT/units-A-ship.txt"))"

say; say "### cargo tree"
for w in "$BASE" "$A"; do
  say "$(basename "$w"): normal-edge packages (all targets, workspace, all features) = $(cd "$w" && cargo tree --locked --workspace --all-features --target all -e normal --prefix none 2>/dev/null | sort -u | wc -l); duplicated crate names (cargo tree -d) = $(cd "$w" && cargo tree --locked --workspace --all-features --target all -d --prefix none 2>/dev/null | grep -c '^[a-z]')"
done

say; say "### sqlite links"
say "sqlx in A lock: $(grep -c '^name = "sqlx' "$A/Cargo.lock"); rusqlite: $(grep -c '^name = "rusqlite"' "$A/Cargo.lock"); libsqlite3-sys: $(grep -A1 '^name = "libsqlite3-sys"' "$A/Cargo.lock" | tr '\n' ' ')"
say "base lock sqlx: $(grep -c '^name = "sqlx' "$BASE/Cargo.lock"); rusqlite: $(grep -c '^name = "rusqlite"' "$BASE/Cargo.lock"); libsqlite3-sys: $(grep -c '^name = "libsqlite3-sys"' "$BASE/Cargo.lock")"
( cd "$A" && cargo tree --locked -p collab-session --features matrix-sqlite,streams -i libsqlite3-sys 2>&1 | head -12 ) | tee -a "$S"
( cd "$A" && cargo metadata --locked --format-version 1 | jq -r '[.packages[]|select(.links!=null)|"\(.name) links=\(.links)"]|sort|.[]' | grep -i sqlite ) | tee -a "$S"

say; say "### licences (cargo metadata, all targets, workspace members excluded)"
for w in "$BASE" "$A"; do ( cd "$w" && cargo metadata --locked --format-version 1 --all-features > "$OUT/meta-$(basename "$w").json" 2>/dev/null ); done
python3 - "$OUT/meta-$(basename "$BASE").json" "$OUT/meta-$(basename "$A").json" <<'PY' | tee -a "$S"
import json,sys,collections
def lic(p):
    m=json.load(open(p)); ws=set(m['workspace_members'])
    d={}
    for x in m['packages']:
        if x['id'] in ws: continue
        d[(x['name'],x['version'])]=x.get('license') or ('LICENSE-FILE:'+str(x.get('license_file')))
    return d
b,a=lic(sys.argv[1]),lic(sys.argv[2])
new={k:v for k,v in a.items() if k not in b}
print("non-workspace packages: base",len(b),"A",len(a),"new",len(new))
c=collections.Counter(new.values())
print("licence expressions among the NEW packages:")
for k,v in c.most_common(): print("  %4d  %s"%(v,k))
unseen=set(c)-set(b.values())
print("expressions new to the dependency tree (not used by any base package):")
for k in sorted(unseen): print("  ",k,"<-",", ".join(sorted(f"{n}" for (n,v),l in new.items() if l==k)[:12]))
PY
cat > "$OUT/deny-lic.toml" <<'T'
[graph]
all-features = true
[licenses]
confidence-threshold = 0.8
allow = ["MIT","Apache-2.0","Apache-2.0 WITH LLVM-exception","BSD-2-Clause","BSD-3-Clause","ISC","Unicode-3.0","Zlib","MPL-2.0","CC0-1.0","0BSD","BSL-1.0","GPL-3.0-or-later","GPL-3.0-only","LGPL-2.1-or-later","LGPL-3.0-or-later","Unlicense","CDLA-Permissive-2.0","OpenSSL","MIT-0","bzip2-1.0.6","NCSA"]
T
say; say "### cargo deny (repo deny.toml, as ci.yml: check advisories) and a permissive-allowlist licence check"
if command -v cargo-deny >/dev/null; then
  for w in "$BASE" "$A"; do
    n=$(basename "$w")
    ( cd "$w" && cargo deny check advisories > "$OUT/deny-adv-$n.txt" 2>&1; echo "advisories $n (default features) exit=$?" ) | tee -a "$S"
    ( cd "$w" && cargo deny --all-features check advisories > "$OUT/deny-adv-allf-$n.txt" 2>&1; echo "advisories $n --all-features exit=$?" ) | tee -a "$S"
    grep -E '^(error|warning)\[' "$OUT/deny-adv-allf-$n.txt" | sort | uniq -c | sort -rn | head -40 > "$OUT/deny-adv-allf-$n.kinds"
    ( cd "$w" && cargo deny --all-features --config "$OUT/deny-lic.toml" check licenses > "$OUT/deny-lic-$n.txt" 2>&1; echo "licenses(allowlist) $n exit=$?" ) | tee -a "$S"
  done
  say "advisory findings on A not on base (--all-features):"
  diff <(grep -E '^(error|warning)\[' "$OUT/deny-adv-allf-$(basename "$BASE").txt" | sort -u) <(grep -E '^(error|warning)\[' "$OUT/deny-adv-allf-$(basename "$A").txt" | sort -u) | grep '^>' | tee -a "$S"
  say "licence check, A: $(grep -c '^error\[' "$OUT/deny-lic-$(basename "$A").txt") errors"
  grep -A6 '^error\[rejected\]' "$OUT/deny-lic-$(basename "$A").txt" | grep -E 'license|crate' | head -30 | tee -a "$S"
else say "cargo-deny not installed"; fi

say; say "### crate download size (cargo fetch --locked; ~/.cargo/registry/cache)"
rm -rf "$HOME/.cargo/registry"
( cd "$BASE" && cargo fetch --locked 2>/dev/null )
b_bytes=$(du -sb "$HOME/.cargo/registry/cache" | cut -f1); b_n=$(find "$HOME/.cargo/registry/cache" -name '*.crate' | wc -l)
( cd "$A" && cargo fetch --locked 2>/dev/null )
a_bytes=$(du -sb "$HOME/.cargo/registry/cache" | cut -f1); a_n=$(find "$HOME/.cargo/registry/cache" -name '*.crate' | wc -l)
say "base: $b_n crate files, $b_bytes bytes; after A (cumulative): $a_n files, $a_bytes bytes; A adds $((a_n-b_n)) files, $((a_bytes-b_bytes)) bytes"

say; say "### flatpak-cargo-generator cargo-sources.json (what flathub/README.md step 1 runs)"
curl -sSL https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py -o "$OUT/flatpak-cargo-generator.py"
pip install --quiet aiohttp tomlkit toml 2>&1 | tail -1
for w in "$BASE" "$A"; do
  n=$(basename "$w")
  python3 "$OUT/flatpak-cargo-generator.py" "$w/Cargo.lock" -o "$OUT/cargo-sources-$n.json" >"$OUT/gen-$n.log" 2>&1 || say "generator failed on $n: $(tail -3 "$OUT/gen-$n.log")"
  say "$n: cargo-sources.json bytes=$(wc -c < "$OUT/cargo-sources-$n.json" 2>/dev/null) entries=$(jq length "$OUT/cargo-sources-$n.json" 2>/dev/null) crate-archive-sources=$(jq '[.[]|select(.type=="archive")]|length' "$OUT/cargo-sources-$n.json" 2>/dev/null)"
done
