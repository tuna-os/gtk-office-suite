#!/usr/bin/env bash
# RFC-0003 spike 1: open and save through GVfs against a local WebDAV
# server, the protocol Nextcloud's files use through GNOME Online Accounts.
# Run inside the render-lab container (Ubuntu 24.04) from the repo root:
#   podman run --rm --device /dev/fuse --cap-add SYS_ADMIN \
#     -v "$PWD:/workspace" -w /workspace render-lab tools/cloud-spike/run.sh
# It installs gvfs and rclone into the throwaway container, serves a
# WebDAV directory, mounts it with `gio mount`, records what the backend
# reports, and runs suite-common's remote_locations test against it.
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null
apt-get install -y -qq gvfs gvfs-backends gvfs-daemons gvfs-fuse rclone dbus fuse3 python3-pip >/dev/null
# WsgiDAV honours If-Match, as Nextcloud (sabre/dav) does; rclone does not.
pip install -q --break-system-packages wsgidav cheroot >/dev/null

export XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$XDG_RUNTIME_DIR/gvfs" && chmod 700 "$XDG_RUNTIME_DIR"
exec dbus-run-session -- bash -euo pipefail -c '
mkdir -p /tmp/dav
rclone serve webdav /tmp/dav --addr 127.0.0.1:8080 --etag-hash MD5 -vv --dump headers >/tmp/rclone.log 2>&1 &
sleep 2
DAV=dav://127.0.0.1:8080/
ls /usr/share/gvfs/mounts/ | tr "\n" " "; echo
curl -sS -X PROPFIND -H "Depth: 0" http://127.0.0.1:8080/ | head -c 300; echo
gio mount "$DAV" </dev/null || GIO_DEBUG=1 G_MESSAGES_DEBUG=all gio mount "$DAV" </dev/null
echo "== mounted: $(gio mount -l | grep -i dav || true)"
/usr/libexec/gvfsd-fuse "$XDG_RUNTIME_DIR/gvfs" -f >/tmp/gvfsd-fuse.log 2>&1 &
sleep 2

echo "hello" > /tmp/h.txt
gio copy /tmp/h.txt "${DAV}probe.txt"
echo "== gio info of a file on the mount"
gio info -a "standard::*,etag::value,time::modified,access::*" "${DAV}probe.txt" | sed -n "1,40p"
echo "== a rename on the backend (what an atomic save needs)"
gio copy /tmp/h.txt "${DAV}probe.tmp"
if gio move "${DAV}probe.tmp" "${DAV}probe.txt" 2>&1; then echo "rename over an existing file: ok"; else echo "rename over an existing file: FAILED"; fi
echo "== FUSE path, if gvfsd-fuse is running"
ls "$XDG_RUNTIME_DIR/gvfs" 2>&1 || true
FUSE="$(gio info "${DAV}probe.txt" | sed -n "s/^local path: //p")"
echo "fuse path: $FUSE"
if [ -n "$FUSE" ] && cat "$FUSE" >/dev/null 2>&1; then
  D="$(dirname "$FUSE")"
  if printf "atomic" > "$D/.probe.txt.tmp" && mv -f "$D/.probe.txt.tmp" "$FUSE"; then
    echo "atomic save over FUSE (temp + rename): ok, now: $(cat "$FUSE")"
  else
    echo "atomic save over FUSE (temp + rename): FAILED"
  fi
  echo "etag after the FUSE save: $(gio info -a etag::value "${DAV}probe.txt" | grep etag)"
else
  echo "FUSE path not usable here: $(cat /tmp/gvfsd-fuse.log | tail -3)"
fi

echo "== does the server honour If-Match? (a PUT with a wrong etag)"
curl -s -o /dev/null -w "PUT with If-Match: \"wrong\" -> HTTP %{http_code}\n" -X PUT -H "If-Match: \"wrong\"" --data-binary x http://127.0.0.1:8080/probe.txt
echo "== what GVfs sends when GIO is given an etag"
printf a > /tmp/a.txt
ETAG="$(gio info -a etag::value "${DAV}probe.txt" | sed -n "s/.*etag::value: //p")"
gio save --etag="$ETAG" "${DAV}probe.txt" < /tmp/a.txt || true
grep -i "if-match\|if:" /tmp/rclone.log | tail -5 || true
echo "== the same against WsgiDAV, which checks If-Match"
mkdir -p /tmp/dav2
wsgidav --host 127.0.0.1 --port 8081 --root /tmp/dav2 --auth anonymous -v >/tmp/wsgidav.log 2>&1 &
sleep 3
DAV2=dav://127.0.0.1:8081/
gio mount "$DAV2" </dev/null
gio save "${DAV2}probe.txt" < /tmp/h.txt
echo "etag on WsgiDAV: $(gio info -a etag::value "${DAV2}probe.txt" | sed -n "s/.*etag::value: //p")"
curl -s -o /dev/null -w "PUT with a wrong If-Match to WsgiDAV -> HTTP %{http_code}\n" -X PUT -H "If-Match: \"wrong\"" --data-binary x http://127.0.0.1:8081/probe.txt
if gio save --etag="\"wrong\"" "${DAV2}probe.txt" < /tmp/a.txt; then echo "gio save with a wrong etag through GVfs: ACCEPTED (GVfs sent no If-Match)"; else echo "gio save with a wrong etag through GVfs: refused"; fi

echo "== suite-common remote_locations test"
set +e
status=0
for d in "$DAV" "$DAV2"; do echo "-- against $d"; TUNAOS_REMOTE_DIR="$d" cargo test -q -p suite-common --test remote_locations -- --nocapture || status=1; done
echo "== If-Match as WsgiDAV saw it"
grep -ci "if-match" /tmp/wsgidav.log || true
exit $status
'
