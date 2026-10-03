#!/usr/bin/env bash
# Tier C: open every fixture in the *shipped* Flatpak inside a real GNOME
# Wayland session and capture what reached the screen.
#
#   tools/render-lab/vm/run.sh <workdir> <bundles-dir> <fixtures-dir> <out-dir> [<prior-bundles-dir>]
#
# With a prior-bundles directory (the last release's bundles), it ends by
# upgrading from that release to these bundles (vm/upgrade.sh, #1209).
#
# Per fixture it stores:
#   C-<n>.png       the app's own render dump, produced inside the Flatpak
#                   (runtime fonts, GL GSK renderer, Wayland backend)
#   C-screen.png    a QMP screendump of the whole virtual monitor: proof
#                   the window was mapped and composited by Mutter
set -euo pipefail
# Every path is resolved before the cd into the work directory: a relative
# bundles directory (CI passes `bundles`) was looked up inside it, and the
# first Tier C run to boot the guest failed on `scp: stat local
# "bundles/*.flatpak"` (#1199).
WORK="$1"; BUNDLES="$(realpath "$2")"; FIX="$(realpath "$3")"; OUT="$(realpath "$4")"
PRIOR="${5:+$(realpath "$5")}"
HERE="$(dirname "$(realpath "$0")")"
cd "$WORK"
# -n: ssh must not read stdin. The fixture loop below is a `while read`
# fed by a pipe, and the first ssh inside it swallowed the rest of the
# list, so the run captured one fixture and "succeeded" (#1199).
SSH=(ssh -n -i id_lab -p 2222 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR lab@127.0.0.1)
SCP=(scp -i id_lab -P 2222 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)

qemu-system-x86_64 -enable-kvm -cpu host -smp 4 -m 6G \
    -drive file=guest.qcow2,if=virtio,snapshot=on \
    -device virtio-vga,xres=1600,yres=1400 -display none \
    -nic user,model=virtio,hostfwd=tcp::2222-:22 \
    -qmp unix:qmp.sock,server,nowait -serial file:run-serial.log &
QEMU=$!
trap 'kill $QEMU 2>/dev/null || true' EXIT

for _ in $(seq 120); do "${SSH[@]}" true 2>/dev/null && break; sleep 5; done
# Wait for the autologin session's Wayland socket.
"${SSH[@]}" 'for i in $(seq 60); do ls /run/user/$(id -u)/wayland-0 >/dev/null 2>&1 && exit 0; sleep 2; done; exit 1'

# QMP: `qmp <file>` screenshots the virtual monitor into <file>;
# `qmp_key <key>` presses a key on the guest's keyboard.
qmp() { python3 - "$1" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect("qmp.sock"); f = s.makefile("rw")
f.readline(); f.write(json.dumps({"execute": "qmp_capabilities"}) + "\n"); f.flush(); f.readline()
f.write(json.dumps({"execute": "screendump", "arguments": {"filename": sys.argv[1]}}) + "\n"); f.flush(); print(f.readline().strip())
PY
}
qmp_key() { python3 - "$1" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect("qmp.sock"); f = s.makefile("rw")
f.readline(); f.write(json.dumps({"execute": "qmp_capabilities"}) + "\n"); f.flush(); f.readline()
f.write(json.dumps({"execute": "send-key", "arguments": {"keys": [{"type": "qcode", "data": sys.argv[1]}]}}) + "\n"); f.flush(); print(f.readline().strip())
PY
}

# GNOME's welcome tour ("Welcome to Fedora Linux") opens over the first
# session of each GNOME version and stayed in front of every app in every
# screenshot. Marking it shown stops it opening; Return takes the Skip
# button, focused by default, if it already has. Both before any app
# runs, so the key can't reach one.
"${SSH[@]}" 'export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus;
    gsettings set org.gnome.shell welcome-dialog-last-shown-version "'"'"'999'"'"'" || true'
sleep 10
qmp_key ret >/dev/null
sleep 2

"${SSH[@]}" 'mkdir -p ~/lab/bundles ~/lab/fixtures ~/lab/out'
"${SCP[@]}" "$BUNDLES"/*.flatpak lab@127.0.0.1:lab/bundles/
"${SCP[@]}" -r "$FIX"/. lab@127.0.0.1:lab/fixtures/
"${SSH[@]}" 'for b in ~/lab/bundles/*.flatpak; do flatpak install --user -y --noninteractive "$b"; done'


python3 -c 'import json,sys; [print(f["app"], f["feature"], f["file"]) for f in json.load(open(sys.argv[1]))]' "$FIX/manifest.json" |
while read -r app feature file; do
    dest="$OUT/$app/$feature"; mkdir -p "$dest"; rm -f "$dest"/C-*.png
    remote_out="lab/out/$app-$feature"
    "${SSH[@]}" "rm -rf ~/$remote_out; mkdir -p ~/$remote_out; \
        export XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-0 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus; \
        setsid flatpak run --filesystem=home \
          --env=GTK_OFFICE_TEST_MODE=1 --env=GTK_OFFICE_RENDER_DUMP=\$HOME/$remote_out --env=GTK_OFFICE_RENDER_HOLD=1 \
          org.tunaos.$app \$HOME/lab/fixtures/$file >\$HOME/$remote_out/app.log 2>&1 < /dev/null & \
        for i in \$(seq 60); do [ -f ~/$remote_out/geom.json ] && break; sleep 1; done; sleep 2"
    qmp "$PWD/screen.ppm" >/dev/null
    python3 -c 'import sys; from PIL import Image; Image.open(sys.argv[1]).save(sys.argv[2])' screen.ppm "$dest/C-screen.png"
    "${SSH[@]}" "flatpak kill org.tunaos.$app 2>/dev/null || true"
    for f in $("${SSH[@]}" "cd ~/$remote_out && ls A-*.png 2>/dev/null" || true); do
        "${SCP[@]}" "lab@127.0.0.1:$remote_out/$f" "$dest/C-${f#A-}"
    done
    "${SCP[@]}" "lab@127.0.0.1:$remote_out/app.log" "$dest/C.log" 2>/dev/null || true
    echo "vm  $app/$feature: $(ls "$dest"/C-[0-9]*.png 2>/dev/null | wc -l) page(s)"
done

# The installed Flatpak used the way a desktop uses it (#1209): open a
# document from "the file manager" (gio open) and save it through the
# document portal. installed.json is what installed_check.py judges.
"${SCP[@]}" "$HERE/installed.sh" lab@127.0.0.1:lab/installed.sh
: > "$OUT/installed.json"
for spec in "letters docx application/vnd.openxmlformats-officedocument.wordprocessingml.document" \
            "tables xlsx application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" \
            "decks pptx application/vnd.openxmlformats-officedocument.presentationml.presentation"; do
    read -r app ext mime <<<"$spec"
    file="$(python3 -c 'import json, sys
print(next((f["file"] for f in json.load(open(sys.argv[1])) if f["app"] == sys.argv[2] and f["file"].endswith("." + sys.argv[3])), ""))' "$FIX/manifest.json" "$app" "$ext")"
    [ -n "$file" ] || { echo "installed: no .$ext fixture for $app" >&2; continue; }
    "${SSH[@]}" "bash ~/lab/installed.sh $app $ext $mime \$HOME/lab/fixtures/$file \$HOME/lab/installed" >>"$OUT/installed.json" \
        || echo "installed: $app check did not finish" >&2
    qmp "$PWD/screen.ppm" >/dev/null
    python3 -c 'import sys; from PIL import Image; Image.open(sys.argv[1]).save(sys.argv[2])' screen.ppm "$OUT/installed-$app.png"
    "${SSH[@]}" "bash ~/lab/installed.sh --close $app" || true
    "${SCP[@]}" "lab@127.0.0.1:lab/installed/$app-open.log" "$OUT/installed-$app.log" 2>/dev/null || true
done
cat "$OUT/installed.json"

# Upgrading from the prior release (#1209), last: it starts each app from
# a clean slate and leaves it in dark style, which no check above expects.
# upgrade.json is what upgrade_check.py judges.
if [ -n "$PRIOR" ] && ls "$PRIOR"/*.flatpak >/dev/null 2>&1; then
    "${SSH[@]}" 'mkdir -p ~/lab/prior'
    "${SCP[@]}" "$PRIOR"/*.flatpak lab@127.0.0.1:lab/prior/
    prior_tag="$(cat "$PRIOR/prior-tag.txt" 2>/dev/null || echo prior)"
    candidate="${GITHUB_SHA:0:8}"
    "${SCP[@]}" "$HERE/upgrade.sh" lab@127.0.0.1:lab/upgrade.sh
    : > "$OUT/upgrade.json"
    # Each app's edit: an action that changes the document without a dialog,
    # present in the prior release.
    for spec in "letters docx bullet-list" "tables xlsx hide-selected-rows" "decks pptx add-text-box"; do
        read -r app ext action <<<"$spec"
        file="$(python3 -c 'import json, sys
print(next((f["file"] for f in json.load(open(sys.argv[1])) if f["app"] == sys.argv[2] and f["file"].endswith("." + sys.argv[3])), ""))' "$FIX/manifest.json" "$app" "$ext")"
        [ -n "$file" ] || { echo "upgrade: no .$ext fixture for $app" >&2; continue; }
        "${SSH[@]}" "bash ~/lab/upgrade.sh $app \$HOME/lab/prior/$app.flatpak \$HOME/lab/bundles/$app.flatpak \$HOME/lab/fixtures/$file $action $prior_tag ${candidate:-candidate}" >>"$OUT/upgrade.json" \
            || echo "upgrade: $app check did not finish" >&2
        qmp "$PWD/screen.ppm" >/dev/null
        python3 -c 'import sys; from PIL import Image; Image.open(sys.argv[1]).save(sys.argv[2])' screen.ppm "$OUT/upgrade-$app.png"
        "${SSH[@]}" "bash ~/lab/upgrade.sh --close $app" || true
        for phase in prior candidate; do
            "${SCP[@]}" "lab@127.0.0.1:/tmp/upgrade-$app-$phase.log" "$OUT/upgrade-$app-$phase.log" 2>/dev/null || true
        done
    done
    cat "$OUT/upgrade.json"
fi
