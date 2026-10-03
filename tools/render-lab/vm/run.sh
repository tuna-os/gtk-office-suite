#!/usr/bin/env bash
# Tier C: open every fixture in the *shipped* Flatpak inside a real GNOME
# Wayland session and capture what reached the screen.
#
#   tools/render-lab/vm/run.sh <workdir> <bundles-dir> <fixtures-dir> <out-dir>
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

"${SSH[@]}" 'mkdir -p ~/lab/bundles ~/lab/fixtures ~/lab/out'
"${SCP[@]}" "$BUNDLES"/*.flatpak lab@127.0.0.1:lab/bundles/
"${SCP[@]}" -r "$FIX"/. lab@127.0.0.1:lab/fixtures/
"${SSH[@]}" 'for b in ~/lab/bundles/*.flatpak; do flatpak install --user -y --noninteractive "$b"; done'

qmp() { python3 - "$1" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect("qmp.sock"); f = s.makefile("rw")
f.readline(); f.write(json.dumps({"execute": "qmp_capabilities"}) + "\n"); f.flush(); f.readline()
f.write(json.dumps({"execute": "screendump", "arguments": {"filename": sys.argv[1]}}) + "\n"); f.flush(); print(f.readline().strip())
PY
}

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
"${SCP[@]}" "$(dirname "$(realpath "$0")")/installed.sh" lab@127.0.0.1:lab/installed.sh
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
    "${SCP[@]}" "lab@127.0.0.1:lab/installed/$app-open.log" "$OUT/installed-$app.log" 2>/dev/null || true
done
cat "$OUT/installed.json"
