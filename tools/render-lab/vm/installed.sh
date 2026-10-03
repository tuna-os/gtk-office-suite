#!/usr/bin/env bash
# The installed Flatpak, used the way a desktop uses it (#1209): run inside
# the Tier C guest's GNOME session, after run.sh installed the bundles.
#
#   installed.sh <app> <ext> <mime> <document> <out-dir>
#
# Copies <document> into ~/Documents, opens it with `gio open` (the file
# manager's path: MIME default, desktop entry, Flatpak's file forwarding
# through the document portal), saves it with the app's own Save, and
# prints one JSON line of what happened. The app's sandbox can read the
# host but not write it (--filesystem=host:ro), so the save only lands
# through the portal.
set -u
app="$1"; ext="$2"; mime="$3"; src="$4"; out="$5"
id="org.tunaos.$app"
export XDG_RUNTIME_DIR="/run/user/$(id -u)" WAYLAND_DISPLAY=wayland-0
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
# A non-login ssh shell lacks Flatpak's exports, where the desktop entries
# and their MIME associations are.
export XDG_DATA_DIRS="$HOME/.local/share/flatpak/exports/share:/var/lib/flatpak/exports/share:/usr/local/share:/usr/share"

mkdir -p ~/Documents "$out/$app"
doc="$HOME/Documents/installed-$app.$ext"
cp "$src" "$doc"
dump="$out/$app"
rm -f "$dump"/*

handlers="$(gio mime "$mime" 2>&1)"
default="$(printf '%s\n' "$handlers" | sed -n '1s/^Default application for .*: //p')"
registered=false
printf '%s\n' "$handlers" | grep -q "$id.desktop" && registered=true

# Only the render dump directory is writable: the document stays behind
# the portal, as it is for a user.
flatpak override --user --env=GTK_OFFICE_TEST_MODE=1 --env=GTK_OFFICE_RENDER_DUMP="$dump" \
    --env=GTK_OFFICE_RENDER_HOLD=1 --filesystem="$out" "$id"

before="$(stat -c '%i %Y %s' "$doc")"
setsid gio open "$doc" </dev/null >"$out/$app-open.log" 2>&1 &
opened=false
for _ in $(seq 60); do [ -f "$dump/geom.json" ] && { opened=true; break; }; sleep 1; done
sleep 2

saved=false
if $opened; then
    gapplication action "$id" save >>"$out/$app-open.log" 2>&1
    for _ in $(seq 30); do
        [ "$(stat -c '%i %Y %s' "$doc")" != "$before" ] && { saved=true; break; }
        sleep 1
    done
    sleep 1
fi
valid="$(python3 -c 'import sys, zipfile
p = sys.argv[1]
print(str(zipfile.is_zipfile(p) and zipfile.ZipFile(p).testzip() is None).lower())' "$doc")"
leftovers="$(ls -A ~/Documents | grep -c '^\.office-save-' || true)"
recent="$(flatpak run --command=gsettings "$id" get "$id" recent-files 2>/dev/null || true)"
in_recent=false
case "$recent" in *"installed-$app.$ext"*) in_recent=true ;; esac
running=false
flatpak ps --columns=application 2>/dev/null | grep -qx "$id" && running=true

flatpak kill "$id" 2>/dev/null || true
flatpak override --user --reset "$id"

python3 - "$app" "$default" "$registered" "$opened" "$saved" "$valid" "$leftovers" "$in_recent" "$running" <<'PY'
import json, sys
app, default, *flags = sys.argv[1:]
registered, opened, saved, valid, leftovers, in_recent, running = flags
b = lambda v: v == "true"
print(json.dumps({
    "app": app,
    "default_handler": default,
    "registered": b(registered),
    "opened": b(opened),
    "saved": b(saved),
    "valid_after_save": b(valid),
    "save_leftovers": int(leftovers),
    "in_recent_files": b(in_recent),
    "running_after_save": b(running),
}))
PY
