#!/usr/bin/env bash
# Add a release to the app's signed Flatpak repository, the one `flatpak
# update` follows, and make the release's bundles from it.
#
#   fork/flatpak-repo.sh <site> <build-repo> <version> <bundle-dir>
#
#   <site>        the published site (the gh-pages checkout): repo/ is the
#                 OSTree repository, created on the first release
#   <build-repo>  a repository holding app/<APP_ID>/<arch>/stable for every
#                 arch in ARCHES, as flatpak-builder --repo and
#                 flatpak build-import-bundle leave them (unsigned)
#   <version>     the release, e.g. 1.47.1-tuna6
#   <bundle-dir>  where the bundles are written
#
# Environment: GPG_HOMEDIR, a keyring holding the signing key's secret part;
# BASE_URL, where <site> is served (default https://tuna-os.github.io/lifafa);
# ARCHES (default "x86_64 aarch64"); PRUNE_DEPTH, the earlier commits kept
# per arch for `flatpak update --commit=` to go back to (default 3).
#
# Each arch's build is committed on top of the published one, signed, so its
# history carries on; build-update-repo then signs the summary, regenerates
# the appstream branches, makes static deltas from each new commit's parent
# (and from nothing), deletes the deltas no longer wanted, and prunes the
# commits past PRUNE_DEPTH with the objects only they used. An install on any
# older commit still updates: it fetches the newest commit's objects instead
# of a delta. The site also gets the .flatpakrepo and .flatpakref, with the
# public key embedded, and the key itself (fork/flatpak-repo.asc, whose
# fingerprint the keyring's secret key must match).
#
# The bundles are built from the signed commits and name the repository and
# key, so a bundle install updates from the repository too.
set -euo pipefail

if [ $# -ne 4 ]; then
    sed -n '4,13p' "$0" >&2
    exit 2
fi
site=$1
build_repo=$2
version=$3
bundle_dir=$4
: "${GPG_HOMEDIR:?the keyring holding the signing key}"
BASE_URL=${BASE_URL:-https://tuna-os.github.io/lifafa}
ARCHES=${ARCHES:-x86_64 aarch64}
PRUNE_DEPTH=${PRUNE_DEPTH:-3}

here=$(cd "$(dirname "$0")" && pwd)
identity=$("$here/rebrand.sh" --identity)
field() { sed -n "s/^$1=//p" <<<"$identity"; }
APP_ID=$(field APP_ID) APP_NAME=$(field APP_NAME) SUMMARY=$(field SUMMARY)
HOMEPAGE=$(field HOMEPAGE) SOURCE_URL=$(field SOURCE_URL) ICON=$here/../$(field ICON)
test -n "$APP_ID" && test -n "$APP_NAME" && test -s "$ICON"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# The key the repository is signed with is the committed one, and nothing else.
armored="$here/flatpak-repo.asc"
gpg --batch --quiet --homedir "$work" --import "$armored" 2>/dev/null
key=$(gpg --batch --homedir "$work" --with-colons --list-keys | awk -F: '$1 == "fpr" { print $10; exit }')
gpg --batch --homedir "$work" --export "$key" > "$work/key.gpg"
if ! gpg --batch --homedir "$GPG_HOMEDIR" --with-colons --list-secret-keys "$key" >/dev/null 2>&1; then
    echo "the keyring has no secret key for fork/flatpak-repo.asc ($key)" >&2
    exit 1
fi
sign=(--gpg-sign="$key" --gpg-homedir="$GPG_HOMEDIR")
gpg_key_b64=$(base64 -w0 "$work/key.gpg")

repo="$site/repo"
if [ ! -e "$repo/config" ]; then
    # What `ostree init --mode=archive-z2` writes, so that this needs no
    # ostree command beside flatpak.
    mkdir -p "$repo"
    printf '[core]\nrepo_version=1\nmode=archive-z2\n' > "$repo/config"
fi
# Git keeps no empty directories, and OSTree needs these to open the repository.
mkdir -p "$repo"/{tmp,extensions,state,refs/heads,refs/mirrors,refs/remotes,objects}

for arch in $ARCHES; do
    ref="app/$APP_ID/$arch/stable"
    flatpak build-commit-from "${sign[@]}" --no-update-summary \
        --src-repo="$build_repo" --subject="$APP_NAME $version" "$repo" "$ref"
done

flatpak build-update-repo "${sign[@]}" --gpg-import="$work/key.gpg" \
    --title="$APP_NAME" --comment="$SUMMARY" --homepage="$HOMEPAGE" \
    --icon="$BASE_URL/$APP_ID.svg" --default-branch=stable \
    --generate-static-deltas --prune --prune-depth="$PRUNE_DEPTH" "$repo"
rm -rf "${repo:?}"/tmp/* "$repo/.lock"

cat > "$site/$APP_ID.flatpakrepo" <<EOF
[Flatpak Repo]
Title=$APP_NAME
Url=$BASE_URL/repo/
Homepage=$HOMEPAGE
Comment=$SUMMARY
Icon=$BASE_URL/$APP_ID.svg
DefaultBranch=stable
GPGKey=$gpg_key_b64
EOF

cat > "$site/$APP_ID.flatpakref" <<EOF
[Flatpak Ref]
Name=$APP_ID
Branch=stable
Title=$APP_NAME
Url=$BASE_URL/repo/
SuggestRemoteName=$(tr '[:upper:]' '[:lower:]' <<<"$APP_NAME")
Homepage=$HOMEPAGE
Comment=$SUMMARY
Icon=$BASE_URL/$APP_ID.svg
RuntimeRepo=https://dl.flathub.org/repo/flathub.flatpakrepo
IsRuntime=false
GPGKey=$gpg_key_b64
EOF

cp "$armored" "$site/$APP_ID.asc"
cp "$ICON" "$site/$APP_ID.svg"
touch "$site/.nojekyll"
cat > "$site/.gitignore" <<'EOF'
/repo/tmp/
/repo/.lock
EOF
remote=$(tr '[:upper:]' '[:lower:]' <<<"$APP_NAME")
cat > "$site/index.html" <<EOF
<!doctype html>
<html lang="en">
<meta charset="utf-8">
<title>$APP_NAME Flatpak repository</title>
<h1>$APP_NAME</h1>
<p>$SUMMARY. Version $version. Source: <a href="$SOURCE_URL">$SOURCE_URL</a>.</p>
<p>Install, and get updates through <code>flatpak update</code>:</p>
<pre>flatpak install --user $BASE_URL/$APP_ID.flatpakref</pre>
<p>Or add the repository first:</p>
<pre>flatpak remote-add --user --if-not-exists $remote $BASE_URL/$APP_ID.flatpakrepo
flatpak install --user $remote $APP_ID</pre>
<p>The repository is signed with the key <a href="$APP_ID.asc">$key</a>.</p>
</html>
EOF

mkdir -p "$bundle_dir"
for arch in $ARCHES; do
    case "$arch" in
        x86_64) bundle="$APP_ID-$version.flatpak" ;;
        *) bundle="$APP_ID-$version-$arch.flatpak" ;;
    esac
    flatpak build-bundle --arch="$arch" --repo-url="$BASE_URL/repo/" \
        --gpg-keys="$work/key.gpg" \
        --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
        "$repo" "$bundle_dir/$bundle" "$APP_ID" stable
done

du -sh "$repo"
# GitHub refuses files over 100 MB, and Pages sites over 1 GB.
big=$(find "$site" -path "$site/.git" -prune -o -type f -size +95M -print)
if [ -n "$big" ]; then
    echo "files too large for GitHub: $big" >&2
    exit 1
fi
size=$(du -sm --exclude=.git "$site" | cut -f1)
if [ "$size" -gt 900 ]; then
    echo "the site is $size MB, near Pages' 1 GB limit; lower PRUNE_DEPTH" >&2
    exit 1
fi
