#!/usr/bin/env bash
# RFC-0003 spike 1, the sandbox gap: can the Flatpak reach a GVfs
# location, and which permission does it need?
#
# Runs where the app's Flatpak is installed and GVfs and WsgiDAV are on the
# host (CI: .github/workflows/cloud-sandbox.yml; locally, see
# docs/rfc/0003-spike-results.md). It serves a WebDAV directory, mounts it
# with the host's GVfs, then reads a file from inside the sandbox with GIO
# under each candidate permission set, and prints one line per set.
#
#   tools/cloud-spike/sandbox.sh org.tunaos.tables
set -uo pipefail
APP="${1:-org.tunaos.tables}"

if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  exec dbus-run-session -- "$0" "$@"
fi
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp/xdg-$(id -u)}"
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"

ROOT="$(mktemp -d)"
echo "hello from the server" > "$ROOT/probe.txt"
wsgidav --host 127.0.0.1 --port 8082 --root "$ROOT" --auth anonymous >/tmp/wsgidav-sandbox.log 2>&1 &
SERVER=$!
trap 'kill $SERVER 2>/dev/null' EXIT
for _ in $(seq 30); do curl -s -o /dev/null http://127.0.0.1:8082/ && break; sleep 0.5; done

DAV=dav://127.0.0.1:8082/
gio mount "$DAV" </dev/null || { echo "RESULT host: gio mount failed"; exit 1; }
echo "RESULT host: $(gio cat "${DAV}probe.txt")"
FUSE="$(gio info "${DAV}probe.txt" | sed -n 's/^local path: //p')"
echo "host FUSE path: ${FUSE:-none}"

# What the sandbox has to work with: GIO's own client, and `gio` if the
# runtime ships it.
if flatpak run --command=sh "$APP" -c 'command -v gio' >/dev/null 2>&1; then
  READ=(gio cat "${DAV}probe.txt")
else
  READ=(python3 -c "import gi; gi.require_version('Gio','2.0'); from gi.repository import Gio; print(Gio.File.new_for_uri('${DAV}probe.txt').load_contents(None)[1].decode())")
fi
echo "sandbox reader: ${READ[*]}"

try() {
  local label="$1"; shift
  local out
  out="$(flatpak run "$@" --command="${READ[0]}" "$APP" "${READ[@]:1}" 2>&1)"
  if grep -q "hello from the server" <<<"$out"; then
    echo "RESULT $label: ok"
  else
    echo "RESULT $label: FAILED: $(tr '\n' ' ' <<<"$out" | cut -c1-200)"
  fi
}

try "as shipped (no GVfs permission)"
try "--talk-name=org.gtk.vfs.*" --talk-name='org.gtk.vfs.*'
try "--talk-name=org.gtk.vfs.* --filesystem=xdg-run/gvfsd" --talk-name='org.gtk.vfs.*' --filesystem=xdg-run/gvfsd
if [ -n "$FUSE" ]; then
  out="$(flatpak run --filesystem=xdg-run/gvfs --command=cat "$APP" "$FUSE" 2>&1)"
  grep -q "hello from the server" <<<"$out" && echo "RESULT FUSE path with --filesystem=xdg-run/gvfs: ok" \
    || echo "RESULT FUSE path with --filesystem=xdg-run/gvfs: FAILED: $(tr '\n' ' ' <<<"$out" | cut -c1-200)"
fi
