#!/usr/bin/env bash
# Spike E: raw protocol probe of a running Nextcloud pair (nextcloud-up.sh a b).
#
# Every request is printed with its status, response headers and (truncated)
# body so the results document can quote what the server said, not what the
# documentation says. Nothing here asserts; tests/ asserts. A section that
# fails prints why and the script goes on.
#
#   PROBE_OUT=measurements/probe scripts/nextcloud-probe.sh [section...]
#
# Sections: server etag etagsame races checksum truncated versions props lock
#           sharing activity time ocm ocmrecv office nolockapp
set -uo pipefail

PW="${DCP_NC_USER_PASSWORD:-dcp-user-Passw0rd-spike}"
OUT="${PROBE_OUT:-measurements/probe}"
A="http://nc-a"
B="http://nc-b"
mkdir -p "$OUT"
H=/tmp/probe.h
BODY=/tmp/probe.b

# req LABEL USER BASE METHOD PATH [curl args...]
req() {
  local label="$1" user="$2" base="$3" method="$4" path="$5"; shift 5
  echo
  echo "=== [$label] $user $method $base$path $*"
  local st
  st=$(curl -sS -D "$H" -o "$BODY" -w '%{http_code} total=%{time_total}s' -X "$method" \
        -u "$user:$PW" -H 'OCS-APIRequest: true' --max-time 120 "$@" "$base$path" 2>&1) || true
  echo "status: $st"
  sed 's/\r$//' "$H" 2>/dev/null | grep -iv '^\(set-cookie\|content-security\|referrer\|x-robots\|x-permitted\|x-download\|x-content-type\|x-frame\|x-xss\|permissions-policy\|strict-transport\|cache-control\|expires\|pragma\|keep-alive\|vary\|accept-ranges\)'
  echo '--- body'
  head -c "${PROBE_BODY_MAX:-3000}" "$BODY" 2>/dev/null; echo
  cp "$BODY" "$OUT/$label.body" 2>/dev/null || true
  cp "$H" "$OUT/$label.headers" 2>/dev/null || true
}

# hdr NAME: a header of the last response
hdr() { sed 's/\r$//' "$H" | grep -i "^$1:" | tail -1 | sed 's/^[^:]*: *//'; }
# jget EXPR: a field of the last JSON body
jget() { python3 -c "import json,sys; d=json.load(open('$BODY')); print($1)" 2>/dev/null; }
occ() { local inst="$1"; shift; docker exec -u www-data "nc-$inst" php occ "$@" 2>&1; }
# A change made with occ is not seen by the running web server until it restarts:
# the enabled-apps list is cached in the server's own memory (APCu).
restart_nc() {
  docker restart "nc-$1" >/dev/null
  for i in $(seq 1 60); do
    curl -fsS "http://nc-$1/status.php" 2>/dev/null | grep -q '"installed":true' && return 0
    sleep 2
  done
  echo "nc-$1 did not come back" >&2
}

PF_ALL='<?xml version="1.0"?><d:propfind xmlns:d="DAV:" xmlns:oc="http://owncloud.org/ns" xmlns:nc="http://nextcloud.org/ns"><d:allprop/></d:propfind>'
PF_NAMED='<?xml version="1.0"?><d:propfind xmlns:d="DAV:" xmlns:oc="http://owncloud.org/ns" xmlns:nc="http://nextcloud.org/ns"><d:prop><d:getetag/><d:getlastmodified/><d:getcontentlength/><oc:fileid/><oc:id/><oc:permissions/><oc:owner-id/><oc:checksums/><nc:share-permissions/><nc:share-attributes/><oc:share-types/><nc:is-mount-root/><nc:mount-type/><nc:creation_time/><nc:upload_time/><nc:lock/><nc:lock-owner-type/><nc:lock-owner/><nc:lock-owner-displayname/><nc:lock-owner-editor/><nc:lock-time/><nc:lock-timeout/><nc:lock-token/><d:lockdiscovery/><d:supportedlock/><nc:version-label/><nc:version-author/></d:prop></d:propfind>'
LOCKINFO='<?xml version="1.0" encoding="utf-8"?><d:lockinfo xmlns:d="DAV:"><d:lockscope><d:exclusive/></d:lockscope><d:locktype><d:write/></d:locktype><d:owner>dcp-probe</d:owner></d:lockinfo>'

section_server() {
  echo; echo "##### server"
  for inst in a b; do
    curl -sS "http://nc-$inst/status.php"; echo
  done
  req capabilities-a alice "$A" GET '/ocs/v2.php/cloud/capabilities?format=json'
  python3 - "$OUT/capabilities-a.body" <<'PY' 2>&1 | head -80
import json,sys
d=json.load(open(sys.argv[1]))['ocs']['data']
print('version', d['version'])
c=d['capabilities']
print('capability apps:', sorted(c))
for k in ('dav','files','files_sharing','versions'):
    if k in c: print(k, json.dumps(c[k], sort_keys=True))
PY
  occ a app:list --output=json > "$OUT/apps-a.json"
  python3 -c "
import json
d=json.load(open('$OUT/apps-a.json'))
print('enabled apps (a):', ' '.join(sorted(d['enabled'])))
print('disabled apps (a):', ' '.join(sorted(d['disabled'])))"
  req options-files alice "$A" OPTIONS /remote.php/dav/files/alice/
  req options-versions alice "$A" OPTIONS /remote.php/dav/versions/alice/
  req dav-root alice "$A" PROPFIND /remote.php/dav/ -H 'Depth: 1'
}

section_etag() {
  echo; echo "##### etag / conditional PUT"
  local f=/remote.php/dav/files/alice/etag.txt
  req etag-create alice "$A" PUT $f --data-binary 'one' -H 'If-None-Match: *'
  local e1; e1=$(hdr etag); local fid; fid=$(hdr oc-fileid); echo "ETAG1=$e1 FILEID=$fid"
  echo "$fid" > /tmp/probe.etag.fileid
  req etag-create-again alice "$A" PUT $f --data-binary 'one-bis' -H 'If-None-Match: *'
  req etag-ifmatch-wrong alice "$A" PUT $f --data-binary 'two' -H 'If-Match: "deadbeef"'
  req etag-ifmatch-right alice "$A" PUT $f --data-binary 'two' -H "If-Match: $e1"
  local e2; e2=$(hdr etag); echo "ETAG2=$e2"
  req etag-ifmatch-stale alice "$A" PUT $f --data-binary 'three' -H "If-Match: $e1"
  req etag-propfind alice "$A" PROPFIND $f -H 'Depth: 0' --data "$PF_NAMED"
  req etag-delete-ifmatch-stale alice "$A" DELETE $f -H "If-Match: $e1"
  req etag-mkcol alice "$A" MKCOL /remote.php/dav/files/alice/etag-dir
  req etag-mkcol-again alice "$A" MKCOL /remote.php/dav/files/alice/etag-dir
  req etag-get-ifnonematch alice "$A" GET $f -H "If-None-Match: $e2"
}

section_races() {
  echo; echo "##### races on conditional PUT (20 rounds each)"
  local both=0 none=0 one=0 n=20
  # If-Match race: two writers hold the same ETag and write at once.
  for i in $(seq 1 $n); do
    local f=/remote.php/dav/files/alice/race-m-$i.txt
    curl -sS -o /dev/null -u "alice:$PW" -X PUT "$A$f" --data-binary base
    local e; e=$(curl -sS -I -u "alice:$PW" "$A$f" | grep -i '^etag:' | sed 's/^[^:]*: *//; s/\r$//')
    curl -sS -o /dev/null -w '%{http_code}\n' -u "alice:$PW" -X PUT "$A$f" --data-binary w1 -H "If-Match: $e" > /tmp/r1 &
    curl -sS -o /dev/null -w '%{http_code}\n' -u "alice:$PW" -X PUT "$A$f" --data-binary w2 -H "If-Match: $e" > /tmp/r2 &
    wait
    local s1 s2; s1=$(cat /tmp/r1); s2=$(cat /tmp/r2)
    echo "if-match round $i: $s1 $s2"
    if [[ "$s1" =~ ^20 && "$s2" =~ ^20 ]]; then both=$((both+1)); elif [[ "$s1" =~ ^20 || "$s2" =~ ^20 ]]; then one=$((one+1)); else none=$((none+1)); fi
  done
  echo "RESULT if-match race: both-succeeded=$both exactly-one=$one none=$none of $n"
  both=0; one=0; none=0
  # If-None-Match: * race: two writers create the same name at once.
  for i in $(seq 1 $n); do
    local f=/remote.php/dav/files/alice/race-c-$i.txt
    curl -sS -o /dev/null -w '%{http_code}\n' -u "alice:$PW" -X PUT "$A$f" --data-binary w1 -H 'If-None-Match: *' > /tmp/r1 &
    curl -sS -o /dev/null -w '%{http_code}\n' -u "alice:$PW" -X PUT "$A$f" --data-binary w2 -H 'If-None-Match: *' > /tmp/r2 &
    wait
    local s1 s2; s1=$(cat /tmp/r1); s2=$(cat /tmp/r2)
    echo "if-none-match round $i: $s1 $s2"
    if [[ "$s1" =~ ^20 && "$s2" =~ ^20 ]]; then both=$((both+1)); elif [[ "$s1" =~ ^20 || "$s2" =~ ^20 ]]; then one=$((one+1)); else none=$((none+1)); fi
  done
  echo "RESULT if-none-match race: both-succeeded=$both exactly-one=$one none=$none of $n"
}

section_checksum() {
  echo; echo "##### OC-Checksum on upload"
  local f=/remote.php/dav/files/alice/cks.bin
  local good; good=$(printf 'checksum me' | sha256sum | cut -d' ' -f1)
  req cks-bad alice "$A" PUT $f --data-binary 'checksum me' -H 'OC-Checksum: SHA256:0000000000000000000000000000000000000000000000000000000000000000'
  req cks-bad-exists alice "$A" PROPFIND $f -H 'Depth: 0'
  req cks-good alice "$A" PUT $f --data-binary 'checksum me' -H "OC-Checksum: SHA256:$good"
  req cks-propfind alice "$A" PROPFIND $f -H 'Depth: 0' --data "$PF_NAMED"
  echo "expected sha256 $good"
}

section_truncated() {
  echo; echo "##### a PUT whose body is shorter than its Content-Length, then the connection closes"
  python3 - <<'PY'
import base64, socket, time
auth = base64.b64encode(b"alice:" + b"dcp-user-Passw0rd-spike").decode()
def attempt(name, extra=""):
    body = b"x" * 500
    s = socket.create_connection(("nc-a", 80))
    req = (f"PUT /remote.php/dav/files/alice/{name} HTTP/1.1\r\nHost: nc-a\r\nAuthorization: Basic {auth}\r\n"
           f"Content-Length: 1000\r\n{extra}\r\n").encode() + body
    s.sendall(req)
    s.shutdown(socket.SHUT_WR)
    s.settimeout(8)
    try:
        data = s.recv(4096)
        print(name, "response:", data.split(b"\r\n")[0].decode(errors="replace"))
    except Exception as e:
        print(name, "no response within 8s:", type(e).__name__)
    s.close()
    time.sleep(1)
    s2 = socket.create_connection(("nc-a", 80))
    s2.sendall(f"PROPFIND /remote.php/dav/files/alice/{name} HTTP/1.1\r\nHost: nc-a\r\nAuthorization: Basic {auth}\r\nDepth: 0\r\nConnection: close\r\n\r\n".encode())
    out = b""
    while True:
        c = s2.recv(4096)
        if not c: break
        out += c
    txt = out.decode(errors="replace")
    import re
    m = re.search(r"getcontentlength>(\d+)<", txt)
    print(name, "afterwards:", txt.split("\r\n")[0], "getcontentlength=", m.group(1) if m else None)
attempt("trunc-plain.bin")
attempt("trunc-expected-length.bin", "X-Expected-Entity-Length: 1000\r\n")
PY
}

section_versions() {
  echo; echo "##### versions endpoint"
  local f=/remote.php/dav/files/alice/ver.txt
  req ver-create alice "$A" PUT $f --data-binary 'version 1'
  local fid; fid=$(hdr oc-fileid); echo "FILEID=$fid"; echo "$fid" > /tmp/probe.ver.fileid
  for i in 2 3 4 5; do
    curl -sS -o /dev/null -w "overwrite $i: %{http_code}\n" -u "alice:$PW" -X PUT "$A$f" --data-binary "version $i"
  done
  req ver-list-immediately alice "$A" PROPFIND "/remote.php/dav/versions/alice/versions/$fid" -H 'Depth: 1' --data "$PF_ALL"
  sleep 2
  curl -sS -o /dev/null -w "overwrite 6 after 2s: %{http_code}\n" -u "alice:$PW" -X PUT "$A$f" --data-binary "version 6"
  req ver-list alice "$A" PROPFIND "/remote.php/dav/versions/alice/versions/$fid" -H 'Depth: 1' --data "$PF_NAMED"
  local first; first=$(grep -o '/versions/[0-9]*/[0-9]*</d:href>' "$BODY" | head -1 | sed 's#</d:href>##; s#.*/versions/[0-9]*/##')
  echo "FIRSTVERSION=$first"
  if [ -n "$first" ]; then
    req ver-get alice "$A" GET "/remote.php/dav/versions/alice/versions/$fid/$first"
    req ver-get-bob-forbidden bob "$A" GET "/remote.php/dav/versions/bob/versions/$fid/$first"
  fi
  req ver-put-into-versions alice "$A" PUT "/remote.php/dav/versions/alice/versions/$fid/12345" --data-binary forged
  req ver-restore-dest-propfind alice "$A" PROPFIND /remote.php/dav/versions/alice/restore -H 'Depth: 0'
  # Is the versions listing available for a file that is a share of someone else's?
}

section_props() {
  echo; echo "##### custom (dead) properties via PROPPATCH: where could 'parents' live?"
  local f=/remote.php/dav/files/alice/props.txt
  req pp-create alice "$A" PUT $f --data-binary 'props 1'
  local fid; fid=$(hdr oc-fileid)
  req pp-set alice "$A" PROPPATCH $f --data '<?xml version="1.0"?><d:propertyupdate xmlns:d="DAV:" xmlns:x="urn:dcp"><d:set><d:prop><x:parents>v1,v2</x:parents><x:version>{"dcp":1}</x:version></d:prop></d:set></d:propertyupdate>'
  req pp-get alice "$A" PROPFIND $f -H 'Depth: 0' --data '<?xml version="1.0"?><d:propfind xmlns:d="DAV:" xmlns:x="urn:dcp"><d:prop><x:parents/><x:version/></d:prop></d:propfind>'
  req pp-allprop alice "$A" PROPFIND $f -H 'Depth: 0' --data "$PF_ALL"
  req pp-overwrite alice "$A" PUT $f --data-binary 'props 2'
  req pp-get-after-overwrite alice "$A" PROPFIND $f -H 'Depth: 0' --data '<?xml version="1.0"?><d:propfind xmlns:d="DAV:" xmlns:x="urn:dcp"><d:prop><x:parents/><x:version/></d:prop></d:propfind>'
  sleep 1
  req pp-versions alice "$A" PROPFIND "/remote.php/dav/versions/alice/versions/$fid" -H 'Depth: 1' --data '<?xml version="1.0"?><d:propfind xmlns:d="DAV:" xmlns:x="urn:dcp"><d:prop><x:parents/><d:getcontentlength/></d:prop></d:propfind>'
  req pp-move alice "$A" MOVE $f -H "Destination: $A/remote.php/dav/files/alice/props-moved.txt"
  req pp-get-after-move alice "$A" PROPFIND /remote.php/dav/files/alice/props-moved.txt -H 'Depth: 0' --data '<?xml version="1.0"?><d:propfind xmlns:d="DAV:" xmlns:x="urn:dcp"><d:prop><x:parents/></d:prop></d:propfind>'
  # The same through a share: can a recipient with update rights read and write it?
  req pp-share alice "$A" POST /ocs/v2.php/apps/files_sharing/api/v1/shares -d path=/props-moved.txt -d shareType=0 -d shareWith=bob -d permissions=3 -d format=json
  req pp-bob-get bob "$A" PROPFIND /remote.php/dav/files/bob/props-moved.txt -H 'Depth: 0' --data '<?xml version="1.0"?><d:propfind xmlns:d="DAV:" xmlns:x="urn:dcp"><d:prop><x:parents/></d:prop></d:propfind>'
  req pp-bob-set bob "$A" PROPPATCH /remote.php/dav/files/bob/props-moved.txt --data '<?xml version="1.0"?><d:propertyupdate xmlns:d="DAV:" xmlns:x="urn:dcp"><d:set><d:prop><x:parents>by-bob</x:parents></d:prop></d:set></d:propertyupdate>'
  req pp-alice-get alice "$A" PROPFIND /remote.php/dav/files/alice/props-moved.txt -H 'Depth: 0' --data '<?xml version="1.0"?><d:propfind xmlns:d="DAV:" xmlns:x="urn:dcp"><d:prop><x:parents/></d:prop></d:propfind>'
}

lock_probe() {  # lock_probe TAG FILE
  local tag="$1" fname="$2"
  local f="/remote.php/dav/files/alice/$fname"
  echo; echo "##### LOCK ($tag) on $fname"
  req "$tag-put" alice "$A" PUT "$f" --data-binary 'lock me'
  req "$tag-share" alice "$A" POST /ocs/v2.php/apps/files_sharing/api/v1/shares -d "path=/$fname" -d shareType=0 -d shareWith=bob -d permissions=3 -d format=json
  req "$tag-supportedlock" alice "$A" PROPFIND "$f" -H 'Depth: 0' --data "$PF_NAMED"
  req "$tag-lock" alice "$A" LOCK "$f" -H 'Content-Type: application/xml' -H 'Timeout: Second-60' --data "$LOCKINFO"
  local tok; tok=$(hdr lock-token); echo "LOCKTOKEN=$tok"
  req "$tag-propfind-locked" alice "$A" PROPFIND "$f" -H 'Depth: 0' --data "$PF_NAMED"
  req "$tag-bob-put-locked" bob "$A" PUT "/remote.php/dav/files/bob/$fname" --data-binary 'bob while locked'
  req "$tag-bob-lock" bob "$A" LOCK "/remote.php/dav/files/bob/$fname" -H 'Content-Type: application/xml' -H 'Timeout: Second-60' --data "$LOCKINFO"
  req "$tag-alice-put-no-token" alice "$A" PUT "$f" --data-binary 'alice without token'
  if [ -n "$tok" ]; then
    req "$tag-alice-put-token" alice "$A" PUT "$f" --data-binary 'alice with token' -H "If: (${tok})"
    req "$tag-refresh" alice "$A" LOCK "$f" -H 'Timeout: Second-120' -H "If: (${tok})"
    req "$tag-bob-unlock" bob "$A" UNLOCK "/remote.php/dav/files/bob/$fname" -H "Lock-Token: ${tok}"
    req "$tag-unlock" alice "$A" UNLOCK "$f" -H "Lock-Token: ${tok}"
  fi
  req "$tag-bob-put-unlocked" bob "$A" PUT "/remote.php/dav/files/bob/$fname" --data-binary 'bob after unlock'
  # Expiry: a 2 second lock.
  req "$tag-lock-short" alice "$A" LOCK "$f" -H 'Content-Type: application/xml' -H 'Timeout: Second-2' --data "$LOCKINFO"
  sleep 4
  req "$tag-bob-put-after-expiry" bob "$A" PUT "/remote.php/dav/files/bob/$fname" --data-binary 'bob after expiry'
  # Lock on a file inside a shared folder as the recipient, and a lock-null (no file) resource.
  req "$tag-lock-nonexistent" alice "$A" LOCK "/remote.php/dav/files/alice/$fname.nonexistent" -H 'Content-Type: application/xml' -H 'Timeout: Second-30' --data "$LOCKINFO"
}

section_lock() {
  lock_probe lock-core lock-core.txt
  echo; echo "##### LOCK with X-User-Lock (the files_lock / desktop client convention)"
  local f=/remote.php/dav/files/alice/lock-user.txt
  req ul-put alice "$A" PUT $f --data-binary 'x'
  req ul-lock alice "$A" LOCK $f -H 'X-User-Lock: 1' -H 'Content-Type: application/xml' --data "$LOCKINFO"
  req ul-propfind alice "$A" PROPFIND $f -H 'Depth: 0' --data "$PF_NAMED"
  req ul-unlock alice "$A" UNLOCK $f -H 'X-User-Lock: 1'
}

section_office() {
  echo; echo "##### Nextcloud Office (richdocuments) and Collabora: what the image ships, and whether it can be added"
  occ a app:list --output=json | python3 -c "
import json,sys
d=json.load(sys.stdin)
for k in ('enabled','disabled'):
    print(k, [a for a in sorted(d[k]) if any(w in a for w in ('office','richdocuments','collabora','text','onlyoffice','files_lock','viewer'))])"
  for app in office richdocuments; do
    echo "--- apps/$app/appinfo/info.xml"
    docker exec nc-a sh -c "grep -m6 '<name>\\|<summary>\\|<description\\|<version>\\|<dependencies>\\|<nextcloud ' /var/www/html/apps/$app/appinfo/info.xml 2>/dev/null || echo '(not present in the image)'"
  done
  occ a app:install richdocuments
  occ a app:list --output=json | python3 -c "
import json,sys
d=json.load(sys.stdin)
print('richdocuments enabled:', 'richdocuments' in d['enabled'], d['enabled'].get('richdocuments'))"
  occ a config:app:get richdocuments wopi_url
  req office-capabilities alice "$A" GET '/ocs/v2.php/cloud/capabilities?format=json'
  python3 -c "
import json
c=json.load(open('$BODY'))['ocs']['data']['capabilities']
print('richdocuments capability:', json.dumps(c.get('richdocuments'), sort_keys=True)[:600])
print('files.directEditing:', json.dumps(c.get('files',{}).get('directEditing'), sort_keys=True)[:300])"
  req office-directediting alice "$A" GET '/ocs/v2.php/apps/files/api/v1/directEditing'
  occ a app:disable richdocuments
}

section_nolockapp() {
  echo; echo "##### files_lock DISABLED (occ app:disable files_lock): core locking only"
  occ a app:disable files_lock
  restart_nc a
  occ a app:list --output=json | python3 -c "import json,sys; d=json.load(sys.stdin); print('files_lock enabled:', 'files_lock' in d['enabled'])"
  req nolock-options alice "$A" OPTIONS /remote.php/dav/files/alice/
  req nolock-capabilities alice "$A" GET '/ocs/v2.php/cloud/capabilities?format=json'
  python3 -c "
import json
d=json.load(open('$BODY'))['ocs']['data']['capabilities']
print('files capability:', json.dumps({k:v for k,v in d.get('files',{}).items() if 'lock' in k}, sort_keys=True))"
  lock_probe nolock nolock.txt
  local f=/remote.php/dav/files/alice/nolock-user.txt
  req nolock-ul-put alice "$A" PUT $f --data-binary 'x'
  req nolock-ul-lock alice "$A" LOCK $f -H 'X-User-Lock: 1' -H 'Content-Type: application/xml' --data "$LOCKINFO"
  occ a app:enable files_lock
  restart_nc a
}

section_etagsame() {
  echo; echo "##### does the ETag change when a file is replaced by same-size content within the same second?"
  local f=/remote.php/dav/files/alice/same.txt
  req same-create alice "$A" PUT $f --data-binary 'aaaa'
  echo "ETAG0=$(hdr etag)"
  for i in 1 2 3; do
    req same-over-$i alice "$A" PUT $f --data-binary "b$i$i$i"
    echo "ETAG after same-size overwrite $i: $(hdr etag)"
  done
  sleep 1.2
  req same-over-later alice "$A" PUT $f --data-binary 'cccc'
  echo "ETAG after same-size overwrite one second later: $(hdr etag)"
  req same-over-bigger alice "$A" PUT $f --data-binary 'cccc-longer'
  echo "ETAG after a size change: $(hdr etag)"
  echo "--- the etag across LOCK / UNLOCK"
  req same-etag-before alice "$A" HEAD $f; echo "before LOCK: $(hdr etag)"
  req same-lock alice "$A" LOCK $f -H 'Content-Type: application/xml' -H 'Timeout: Second-30' --data "$LOCKINFO"
  local tok; tok=$(hdr lock-token)
  req same-etag-locked alice "$A" HEAD $f; echo "while locked: $(hdr etag)"
  req same-unlock alice "$A" UNLOCK $f -H "Lock-Token: $tok"
  req same-etag-after alice "$A" HEAD $f; echo "after UNLOCK: $(hdr etag)"
}

section_ocmrecv() {
  echo; echo "##### OCM: nc-a shares with a receiver that is a plain HTTP script on the runner (what crosses the wire)"
  cat > /tmp/ocm_recv.py <<'PY'
import json, sys, http.server
LOG = open(sys.argv[1], "a", buffering=1)
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def _body(self):
        n = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(n).decode("utf-8", "replace") if n else ""
    def _reply(self, code, obj):
        b = json.dumps(obj).encode()
        self.send_response(code); self.send_header("Content-Type", "application/json"); self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
    def _rec(self, body):
        LOG.write(json.dumps({"method": self.command, "path": self.path, "headers": dict(self.headers), "body": body}) + "\n")
    def do_GET(self):
        self._rec("")
        if self.path.startswith("/.well-known/ocm") or self.path.startswith("/ocm-provider"):
            return self._reply(200, {"enabled": True, "apiVersion": "1.1.0", "endPoint": "http://dcp-host:9099/ocm", "provider": "dcp-probe-receiver",
                "resourceTypes": [{"name": "file", "shareTypes": ["user"], "protocols": {"webdav": "/webdav/"}}], "capabilities": ["notifications"]})
        self._reply(404, {})
    def do_POST(self):
        b = self._body(); self._rec(b)
        if self.path.rstrip("/").endswith("/shares"):
            return self._reply(201, {"recipientDisplayName": "dave"})
        if self.path.rstrip("/").endswith("/notifications"):
            return self._reply(201, {})
        self._reply(404, {})
http.server.ThreadingHTTPServer(("0.0.0.0", 9099), H).serve_forever()
PY
  : > /tmp/ocm_recv.jsonl
  python3 /tmp/ocm_recv.py /tmp/ocm_recv.jsonl &
  local pid=$!
  sleep 1
  curl -sS http://dcp-host:9099/.well-known/ocm 2>&1 | head -c 200; echo
  docker exec nc-a curl -sS http://dcp-host:9099/.well-known/ocm 2>&1 | head -c 300; echo
  req ocr-mkcol alice "$A" MKCOL /remote.php/dav/files/alice/DCP-recv
  req ocr-put alice "$A" PUT /remote.php/dav/files/alice/DCP-recv/a.txt --data-binary 'hello'
  req ocr-share alice "$A" POST /ocs/v2.php/apps/files_sharing/api/v1/shares -d path=/DCP-recv -d shareType=6 -d shareWith=dave@http://dcp-host:9099 -d permissions=15 -d format=json
  sleep 3
  echo "--- what the receiver was sent:"
  python3 - <<'PY'
import json
for l in open('/tmp/ocm_recv.jsonl'):
    d = json.loads(l)
    print(d['method'], d['path'])
    print('  headers:', json.dumps({k: v for k, v in d['headers'].items() if k.lower() in ('content-type','signature','signature-input','digest','content-digest','date','user-agent','host','authorization')}))
    if d['body']:
        try: print('  body:', json.dumps(json.loads(d['body']), indent=1))
        except Exception: print('  body:', d['body'][:500])
PY
  cp /tmp/ocm_recv.jsonl "$OUT/ocm_recv.jsonl"
  local sid; req ocr-list alice "$A" GET '/ocs/v2.php/apps/files_sharing/api/v1/shares?path=/DCP-recv&format=json'
  sid=$(jget "d['ocs']['data'][0]['id']")
  req ocr-update alice "$A" PUT "/ocs/v2.php/apps/files_sharing/api/v1/shares/$sid" -d permissions=1 -d format=json
  sleep 2
  req ocr-delete alice "$A" DELETE "/ocs/v2.php/apps/files_sharing/api/v1/shares/$sid?format=json"
  sleep 2
  echo "--- after update and unshare:"
  python3 - <<'PY'
import json
for l in list(open('/tmp/ocm_recv.jsonl'))[-4:]:
    d = json.loads(l)
    print(d['method'], d['path'], d['body'][:600])
PY
  cp /tmp/ocm_recv.jsonl "$OUT/ocm_recv.jsonl"
  kill $pid 2>/dev/null
}

section_sharing() {
  echo; echo "##### sharing permissions on a document folder"
  req sh-mkcol alice "$A" MKCOL /remote.php/dav/files/alice/DCP-probe
  req sh-mkcol-sub alice "$A" MKCOL /remote.php/dav/files/alice/DCP-probe/versions
  req sh-put alice "$A" PUT /remote.php/dav/files/alice/DCP-probe/versions/v1.json --data-binary '{"a":1}'
  req sh-share-viewer alice "$A" POST /ocs/v2.php/apps/files_sharing/api/v1/shares -d path=/DCP-probe -d shareType=0 -d shareWith=bob -d permissions=1 -d format=json
  local sid; sid=$(jget "d['ocs']['data']['id']"); echo "SHAREID=$sid"; echo "$sid" > /tmp/probe.shareid
  req sh-bob-root bob "$A" PROPFIND /remote.php/dav/files/bob/ -H 'Depth: 1' --data "$PF_NAMED"
  req sh-bob-folder bob "$A" PROPFIND /remote.php/dav/files/bob/DCP-probe -H 'Depth: 0' --data "$PF_NAMED"
  req sh-bob-put-viewer bob "$A" PUT /remote.php/dav/files/bob/DCP-probe/versions/v2.json --data-binary '{"a":2}'
  req sh-bob-overwrite-viewer bob "$A" PUT /remote.php/dav/files/bob/DCP-probe/versions/v1.json --data-binary '{"a":"bob"}'
  req sh-bob-mkcol-viewer bob "$A" MKCOL /remote.php/dav/files/bob/DCP-probe/newdir
  req sh-bob-reshare bob "$A" POST /ocs/v2.php/apps/files_sharing/api/v1/shares -d path=/DCP-probe -d shareType=0 -d shareWith=carol -d permissions=1 -d format=json
  # Create-only: can add files, cannot overwrite or delete.
  req sh-update-5 alice "$A" PUT "/ocs/v2.php/apps/files_sharing/api/v1/shares/$sid" -d permissions=5 -d format=json
  req sh-bob-put-create-only bob "$A" PUT /remote.php/dav/files/bob/DCP-probe/versions/v3.json --data-binary '{"a":3}' -H 'If-None-Match: *'
  req sh-bob-create-again-create-only bob "$A" PUT /remote.php/dav/files/bob/DCP-probe/versions/v3.json --data-binary '{"a":"again"}' -H 'If-None-Match: *'
  req sh-bob-overwrite-create-only bob "$A" PUT /remote.php/dav/files/bob/DCP-probe/versions/v1.json --data-binary '{"a":"bob"}'
  req sh-bob-delete-create-only bob "$A" DELETE /remote.php/dav/files/bob/DCP-probe/versions/v3.json
  req sh-bob-move-create-only bob "$A" MOVE /remote.php/dav/files/bob/DCP-probe/versions/v1.json -H "Destination: $A/remote.php/dav/files/bob/DCP-probe/versions/v1-moved.json"
  # Editor: 15 = read + update + create + delete.
  req sh-update-15 alice "$A" PUT "/ocs/v2.php/apps/files_sharing/api/v1/shares/$sid" -d permissions=15 -d format=json
  req sh-bob-bob-overwrite-editor bob "$A" PUT /remote.php/dav/files/bob/DCP-probe/versions/v1.json --data-binary '{"a":"overwritten by editor"}'
  req sh-bob-folder-editor bob "$A" PROPFIND /remote.php/dav/files/bob/DCP-probe -H 'Depth: 0' --data "$PF_NAMED"
  req sh-bob-reshare-editor bob "$A" POST /ocs/v2.php/apps/files_sharing/api/v1/shares -d path=/DCP-probe -d shareType=0 -d shareWith=carol -d permissions=1 -d format=json
  req sh-bob-list-shares bob "$A" GET '/ocs/v2.php/apps/files_sharing/api/v1/shares?path=/DCP-probe&format=json'
  req sh-bob-shared-with-me bob "$A" GET '/ocs/v2.php/apps/files_sharing/api/v1/shares?shared_with_me=true&format=json'
  # Owner-level: 31 = everything incl. reshare.
  req sh-update-31 alice "$A" PUT "/ocs/v2.php/apps/files_sharing/api/v1/shares/$sid" -d permissions=31 -d format=json
  req sh-bob-folder-31 bob "$A" PROPFIND /remote.php/dav/files/bob/DCP-probe -H 'Depth: 0' --data "$PF_NAMED"
  req sh-bob-reshare-31 bob "$A" POST /ocs/v2.php/apps/files_sharing/api/v1/shares -d path=/DCP-probe -d shareType=0 -d shareWith=carol -d permissions=1 -d format=json
  req sh-alice-list alice "$A" GET '/ocs/v2.php/apps/files_sharing/api/v1/shares?path=/DCP-probe&reshares=true&format=json'
  # Revocation by unsharing.
  req sh-delete alice "$A" DELETE "/ocs/v2.php/apps/files_sharing/api/v1/shares/$sid?format=json"
  req sh-bob-after-unshare bob "$A" PROPFIND /remote.php/dav/files/bob/DCP-probe -H 'Depth: 0'
  req sh-bob-get-after-unshare bob "$A" GET /remote.php/dav/files/bob/DCP-probe/versions/v1.json
  # A share of the same folder to a group.
  occ a group:add dcp-editors
  occ a group:adduser dcp-editors carol
  req sh-group alice "$A" POST /ocs/v2.php/apps/files_sharing/api/v1/shares -d path=/DCP-probe -d shareType=1 -d shareWith=dcp-editors -d permissions=15 -d format=json
  req sh-carol-folder carol "$A" PROPFIND /remote.php/dav/files/carol/DCP-probe -H 'Depth: 0' --data "$PF_NAMED"
  # Public link: a token-only reader.
  req sh-link alice "$A" POST /ocs/v2.php/apps/files_sharing/api/v1/shares -d path=/DCP-probe -d shareType=3 -d permissions=1 -d format=json
}

section_activity() {
  echo; echo "##### who wrote it? Activity API and DAV properties after a recipient's write"
  req ac-share alice "$A" POST /ocs/v2.php/apps/files_sharing/api/v1/shares -d path=/DCP-probe -d shareType=0 -d shareWith=bob -d permissions=15 -d format=json
  req ac-bob-put bob "$A" PUT /remote.php/dav/files/bob/DCP-probe/versions/written-by-bob.json --data-binary '{"by":"bob"}'
  local fid; fid=$(hdr oc-fileid); echo "FILEID=$fid"
  req ac-propfind alice "$A" PROPFIND /remote.php/dav/files/alice/DCP-probe/versions/written-by-bob.json -H 'Depth: 0' --data "$PF_ALL"
  sleep 3
  req ac-activity-file alice "$A" GET "/ocs/v2.php/apps/activity/api/v2/activity/filter?format=json&object_type=files&object_id=$fid&limit=10"
  req ac-activity-all alice "$A" GET '/ocs/v2.php/apps/activity/api/v2/activity?format=json&limit=15'
  req ac-comments alice "$A" PROPFIND "/remote.php/dav/comments/files/$fid" -H 'Depth: 1'
}

section_time() {
  echo; echo "##### the server's clock: Date header resolution and how it advances"
  for i in 1 2 3 4 5 6; do
    curl -sS -D - -o /dev/null "$A/status.php" | grep -i '^date:' | tr -d '\r'
    sleep 0.3
  done
  python3 - <<'PY'
import socket, time, email.utils
def date():
    s = socket.create_connection(("nc-a", 80)); s.sendall(b"GET /status.php HTTP/1.1\r\nHost: nc-a\r\nConnection: close\r\n\r\n")
    out = b""
    while True:
        c = s.recv(4096)
        if not c: break
        out += c
    for line in out.split(b"\r\n"):
        if line.lower().startswith(b"date:"):
            return email.utils.parsedate_to_datetime(line.split(b": ",1)[1].decode()).timestamp()
last = date(); t0 = time.monotonic()
while True:
    d = date()
    if d != last:
        print("second boundary found after %.3fs of polling; server second %d" % (time.monotonic() - t0, d)); break
PY
}

section_ocm() {
  echo; echo "##### OCM discovery"
  for p in /.well-known/ocm /.well-known/ocm.json /ocm-provider /ocm-provider/ /index.php/ocm-provider/ /ocm; do
    req "ocm-disc$(echo "$p" | tr '/.' '__')" alice "$A" GET "$p" -H 'Accept: application/json'
  done
  echo; echo "##### OCM: alice@nc-a shares DCP-ocm with dave@nc-b"
  occ a config:system:get sharing.federation.allowHttpFallback
  req oc-mkcol alice "$A" MKCOL /remote.php/dav/files/alice/DCP-ocm
  req oc-put alice "$A" PUT /remote.php/dav/files/alice/DCP-ocm/a.txt --data-binary 'hello from a'
  req oc-share alice "$A" POST /ocs/v2.php/apps/files_sharing/api/v1/shares -d path=/DCP-ocm -d shareType=6 -d shareWith=dave@http://nc-b -d permissions=15 -d format=json
  sleep 3
  req oc-pending dave "$B" GET '/ocs/v2.php/apps/files_sharing/api/v1/remote_shares/pending?format=json'
  local rid; rid=$(jget "d['ocs']['data'][0]['id']")
  echo "REMOTE_SHARE_ID=$rid"
  if [ -n "$rid" ]; then
    req oc-accept dave "$B" POST "/ocs/v2.php/apps/files_sharing/api/v1/remote_shares/pending/$rid?format=json"
  fi
  sleep 3
  req oc-accepted-list dave "$B" GET '/ocs/v2.php/apps/files_sharing/api/v1/remote_shares?format=json'
  req oc-dave-root dave "$B" PROPFIND /remote.php/dav/files/dave/ -H 'Depth: 1' --data "$PF_NAMED"
  req oc-dave-folder dave "$B" PROPFIND /remote.php/dav/files/dave/DCP-ocm -H 'Depth: 1' --data "$PF_NAMED"
  req oc-dave-get dave "$B" GET /remote.php/dav/files/dave/DCP-ocm/a.txt
  req oc-dave-put-create dave "$B" PUT /remote.php/dav/files/dave/DCP-ocm/b.txt --data-binary 'hello from b' -H 'If-None-Match: *'
  req oc-dave-put-create-again dave "$B" PUT /remote.php/dav/files/dave/DCP-ocm/b.txt --data-binary 'again' -H 'If-None-Match: *'
  local e; req oc-dave-etag dave "$B" PROPFIND /remote.php/dav/files/dave/DCP-ocm/a.txt -H 'Depth: 0' --data "$PF_NAMED"
  req oc-dave-head dave "$B" HEAD /remote.php/dav/files/dave/DCP-ocm/a.txt
  e=$(hdr etag); echo "ETAG_VIA_B=$e"
  req oc-dave-ifmatch-wrong dave "$B" PUT /remote.php/dav/files/dave/DCP-ocm/a.txt --data-binary 'wrong' -H 'If-Match: "nope"'
  req oc-dave-ifmatch-right dave "$B" PUT /remote.php/dav/files/dave/DCP-ocm/a.txt --data-binary 'edited by dave over ocm' -H "If-Match: $e"
  req oc-dave-ifmatch-stale dave "$B" PUT /remote.php/dav/files/dave/DCP-ocm/a.txt --data-binary 'stale' -H "If-Match: $e"
  req oc-alice-sees alice "$A" GET /remote.php/dav/files/alice/DCP-ocm/a.txt
  req oc-alice-propfind-b alice "$A" PROPFIND /remote.php/dav/files/alice/DCP-ocm/b.txt -H 'Depth: 0' --data "$PF_NAMED"
  req oc-dave-lock dave "$B" LOCK /remote.php/dav/files/dave/DCP-ocm/a.txt -H 'Content-Type: application/xml' -H 'Timeout: Second-60' --data "$LOCKINFO"
  local tok; tok=$(hdr lock-token)
  req oc-alice-put-while-b-locked alice "$A" PUT /remote.php/dav/files/alice/DCP-ocm/a.txt --data-binary 'alice while dave holds a lock'
  [ -n "$tok" ] && req oc-dave-unlock dave "$B" UNLOCK /remote.php/dav/files/dave/DCP-ocm/a.txt -H "Lock-Token: $tok"
  req oc-dave-versions dave "$B" PROPFIND "/remote.php/dav/versions/dave/versions/$(curl -sS -u "alice:$PW" -X PROPFIND "$A/remote.php/dav/files/alice/DCP-ocm/a.txt" --data "$PF_NAMED" -H 'Depth: 0' | grep -o '<oc:fileid>[0-9]*' | sed 's/<oc:fileid>//')" -H 'Depth: 1'
  req oc-share-list alice "$A" GET '/ocs/v2.php/apps/files_sharing/api/v1/shares?path=/DCP-ocm&format=json'
  local sid; sid=$(jget "d['ocs']['data'][0]['id']")
  req oc-share-update alice "$A" PUT "/ocs/v2.php/apps/files_sharing/api/v1/shares/$sid" -d permissions=1 -d format=json
  sleep 2
  req oc-dave-put-after-downgrade dave "$B" PUT /remote.php/dav/files/dave/DCP-ocm/c.txt --data-binary 'after downgrade'
  req oc-share-delete alice "$A" DELETE "/ocs/v2.php/apps/files_sharing/api/v1/shares/$sid?format=json"
  sleep 3
  req oc-dave-after-unshare dave "$B" PROPFIND /remote.php/dav/files/dave/DCP-ocm -H 'Depth: 0'
  req oc-dave-list-after dave "$B" GET '/ocs/v2.php/apps/files_sharing/api/v1/remote_shares?format=json'
  echo; echo "##### nextcloud.log (a) federation lines"
  docker exec nc-a sh -c 'grep -i "ocm\|federat\|cloudfederation" /var/www/html/data/nextcloud.log | tail -40' 2>&1 | cut -c1-900
  echo; echo "##### nextcloud.log (b) federation lines"
  docker exec nc-b sh -c 'grep -i "ocm\|federat\|cloudfederation" /var/www/html/data/nextcloud.log | tail -40' 2>&1 | cut -c1-900
  echo; echo "##### apache access log (b), OCM calls"
  docker logs nc-b 2>&1 | grep -i 'ocm\|notification\|public.php' | tail -30 | cut -c1-400
  echo; echo "##### apache access log (a), OCM calls"
  docker logs nc-a 2>&1 | grep -i 'ocm\|notification\|public.php' | tail -30 | cut -c1-400
}

sections=("$@")
[ "${#sections[@]}" -gt 0 ] || sections=(server etag etagsame races checksum truncated versions props lock sharing activity time ocm ocmrecv office nolockapp)
for s in "${sections[@]}"; do
  "section_$s" 2>&1 | tee -a "$OUT/probe-$s.log"
done
