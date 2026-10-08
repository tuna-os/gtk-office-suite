#!/usr/bin/env bash
# The installed Flatpak, used the way a desktop uses it (#1209): run inside
# the Tier C guest's GNOME session, after run.sh installed the bundles.
#
#   installed.sh <app> <ext> <mime> <document> <out-dir>
#   installed.sh --close <app>
#
# Copies <document> into ~/Downloads, opens it with `gio open` (the file
# manager's path: MIME default, desktop entry, Flatpak's file forwarding
# through the document portal), saves it with the app's own Save, and
# prints one JSON line of what happened. The sandbox can see only
# ~/Documents (--filesystem=xdg-documents), so a document in ~/Downloads
# reaches the app, and its save lands, only through the portal.
#   installed.sh --action <app> <action>
#   installed.sh --portal <app> <ext>
#
# Then the file chooser (run.sh drives it): `--action <app> save-as` and
# `--action <app> open` put up the portal's Save and Open dialogs, which
# run.sh answers on the guest's keyboard with a path in ~/Downloads, and
# `--portal` prints one JSON line of what happened: the document saved
# where it was asked to, valid, and the one opened in the app's recent
# files.
set -u
export XDG_RUNTIME_DIR="/run/user/$(id -u)" WAYLAND_DISPLAY=wayland-0
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
if [ "${1:-}" = --close ]; then
    flatpak kill "org.tunaos.$2" 2>/dev/null || true
    flatpak override --user --reset "org.tunaos.$2"
    exit 0
fi
if [ "${1:-}" = --action ]; then
    # What the file chooser needs, for when no dialog appears: the
    # portal's GNOME backend, the portal services on the session bus, and
    # what the portal said.
    diag="$HOME/lab/installed/$2-portal.log"
    {
        echo "== gapplication action org.tunaos.$2 $3"
        gapplication action "org.tunaos.$2" "$3" 2>&1; echo "exit $?"
        sleep 4
        echo "== packages"; rpm -q xdg-desktop-portal xdg-desktop-portal-gnome xdg-desktop-portal-gtk 2>&1
        echo "== session bus"; busctl --user list 2>/dev/null | grep -i -E "portal|tunaos"
        echo "== journal"; journalctl --user -n 80 --no-pager 2>&1 | grep -i -E "portal|filechooser|tunaos" | tail -40
    } >>"$diag" 2>&1
    exit 0
fi
if [ "${1:-}" = --portal ]; then
    app="$2"; ext="$3"; id="org.tunaos.$app"
    saved_doc="$HOME/Downloads/portal-saved-$app.$ext"
    saved=false
    for _ in $(seq 20); do [ -s "$saved_doc" ] && { saved=true; break; }; sleep 1; done
    valid="$(python3 -c 'import os, sys, zipfile
p = sys.argv[1]
print(str(os.path.exists(p) and zipfile.is_zipfile(p) and zipfile.ZipFile(p).testzip() is None).lower())' "$saved_doc")"
    recent="$(flatpak run --command=gsettings "$id" get "$id" recent-files 2>/dev/null || true)"
    opened=false
    case "$recent" in *"portal-open-$app.$ext"*) opened=true ;; esac
    echo "== recent-files after Open: $recent" >>"$HOME/lab/installed/$app-portal.log"
    python3 -c 'import json, sys
app, saved, valid, opened = sys.argv[1:]
print(json.dumps({"app": app, "portal_saved": saved == "true", "portal_valid": valid == "true", "portal_opened": opened == "true"}))' \
        "$app" "$saved" "$valid" "$opened"
    exit 0
fi
app="$1"; ext="$2"; mime="$3"; src="$4"; out="$5"
id="org.tunaos.$app"
# A non-login ssh shell lacks Flatpak's exports, where the desktop entries
# and their MIME associations are.
export XDG_DATA_DIRS="$HOME/.local/share/flatpak/exports/share:/var/lib/flatpak/exports/share:/usr/local/share:/usr/share"

mkdir -p ~/Downloads "$out/$app"
doc="$HOME/Downloads/installed-$app.$ext"
cp "$src" "$doc"
dump="$out/$app"
rm -f "$dump"/*

handlers="$(gio mime "$mime" 2>&1)"
# What the desktop sees, for when it doesn't see the app: the exported
# entry, the MIME index beside it, and the tools that keep that index.
{
    echo "== gio mime $mime"; printf '%s\n' "$handlers"
    echo "== XDG_DATA_DIRS=$XDG_DATA_DIRS"
    apps="$HOME/.local/share/flatpak/exports/share/applications"
    echo "== $apps"; ls -la "$apps" 2>&1
    echo "== $id.desktop"; grep -E '^(Exec|TryExec|MimeType)=' "$apps/$id.desktop" 2>&1
    echo "== mimeinfo.cache"; grep -F "$mime" "$apps/mimeinfo.cache" 2>&1
    echo "== packages"; rpm -q glib2 flatpak desktop-file-utils shared-mime-info 2>&1
} >"$out/$app-mime.log" 2>&1
default="$(printf '%s\n' "$handlers" | sed -n '1s/^Default application for .*: //p')"
registered=false
printf '%s\n' "$handlers" | grep -q "$id.desktop" && registered=true

# Only the render dump directory is writable: the document stays behind
# the portal, as it is for a user.
flatpak override --user --env=GTK_OFFICE_TEST_MODE=1 --env=GTK_OFFICE_RENDER_DUMP="$dump" \
    --env=GTK_OFFICE_RENDER_HOLD=1 --filesystem="$out" "$id"

# Out of the overview, so the screenshot shows the app (and any message
# it puts up) rather than the shell.
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.freedesktop.DBus.Properties.Set org.gnome.Shell OverviewActive '<false>' >/dev/null 2>&1 || true

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
leftovers="$(ls -A ~/Downloads | grep -c '^\.office-save-' || true)"
recent="$(flatpak run --command=gsettings "$id" get "$id" recent-files 2>/dev/null || true)"
in_recent=false
case "$recent" in *"installed-$app.$ext"*) in_recent=true ;; esac
# Where the app was given the document (the portal path, if forwarded).
echo "recent-files: $recent" >>"$out/$app-open.log"
running=false
flatpak ps --columns=application 2>/dev/null | grep -qx "$id" && running=true

# The app stays open: run.sh screenshots what it shows (a failed save's
# message is a dialog, not a log line), then closes it with `installed.sh
# --close <app>`.

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
