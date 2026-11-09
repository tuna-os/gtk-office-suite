#!/usr/bin/env bash
#
# Copy upstream Hylki's accounts, settings, mail cache and keyring entries into
# the fork's app (Lifafa), once, before its first start.
#
#   fork/import-from-hylki.sh
#
# The fork's app keeps its own state (its own app ID, data directories and
# keyring service, all set in the identity block of fork/rebrand.sh) so it can
# run beside Hylki, and so it does not carry upstream's data over by itself:
# the app's own carry-over (src/desktop/legacy.rs) is built for a predecessor
# that is being replaced, and renames its directories in place. This copies
# instead, and leaves Hylki untouched.
#
# The source is upstream's Flatpak (~/.var/app/co.hyprlab.Hylki) when it has
# data, else a native install (~/.config/hylki and its siblings). The target is
# the fork app's Flatpak. Quit both apps first. Nothing is overwritten: a
# target directory that already exists is left alone, as is a keyring entry.
set -euo pipefail

identity() {
    "$(dirname "$0")/rebrand.sh" --identity | sed -n "s/^$1=//p"
}
old_id=co.hyprlab.Hylki
new_id=$(identity APP_ID)
new_dir=$(identity DATA_DIR_NAME)
new_service=$(identity KEYRING_SERVICE)
name=$(identity APP_NAME)
target="$HOME/.var/app/$new_id"

if [ -d "$HOME/.var/app/$old_id/config/hylki" ]; then
    source_dir() { echo "$HOME/.var/app/$old_id/$1/hylki"; }
else
    source_dir() {
        case "$1" in
            config) echo "${XDG_CONFIG_HOME:-$HOME/.config}/hylki" ;;
            data) echo "${XDG_DATA_HOME:-$HOME/.local/share}/hylki" ;;
            cache) echo "${XDG_CACHE_HOME:-$HOME/.cache}/hylki" ;;
        esac
    }
fi
if [ ! -d "$(source_dir config)" ]; then
    echo "no Hylki settings found at $(source_dir config)" >&2
    exit 1
fi

for base in config data cache; do
    from=$(source_dir "$base")
    to="$target/$base/$new_dir"
    if [ ! -d "$from" ]; then
        continue
    fi
    if [ -e "$to" ]; then
        echo "kept $to (already there)"
        continue
    fi
    mkdir -p "$(dirname "$to")"
    cp -a --reflink=auto "$from" "$to"
    echo "copied $from to $to"
done

# Keyring entries: passwords, OAuth refresh tokens and Tuta or Proton
# sessions, keyed by the same names under the new service. The attributes are
# the ones the keyring crate writes and searches by.
if ! command -v secret-tool >/dev/null; then
    echo "secret-tool not found (libsecret-tools); $name will ask for passwords again" >&2
    exit 0
fi
copied=0
while IFS= read -r key; do
    [ -n "$key" ] || continue
    if secret-tool lookup service "$new_service" username "$key" >/dev/null 2>&1; then
        continue
    fi
    secret-tool lookup service "$old_id" username "$key" \
        | secret-tool store --label="$key@$new_service:default" \
            service "$new_service" username "$key" target default application rust-keyring
    copied=$((copied + 1))
done < <(secret-tool search --all --unlock service "$old_id" 2>&1 \
    | sed -n 's/^attribute\.username = //p' | sort -u)
echo "copied $copied keyring entr$([ "$copied" = 1 ] && echo y || echo ies) from $old_id to $new_service"
