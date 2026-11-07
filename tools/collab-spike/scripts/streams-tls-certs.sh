#!/usr/bin/env bash
# Generate the throw-away PKI the TLS spike (RFC-0004, docs/rfc/0004-spike-tls-results.md) uses:
#
#   scripts/streams-tls-certs.sh DIR
#
#   DIR/ca.pem, ca.key           the test CA the client is told to trust (ECDSA P-256, CA:TRUE, pathlen 0)
#   DIR/server.pem, server.key   the server certificate, signed by ca: SAN DNS:localhost only
#                                (NOT 127.0.0.1, NOT any other name: that is the hostname-mismatch control)
#   DIR/other-ca.pem, other-ca.key  a second, unrelated CA: the wrong-CA control
#   DIR/expired.pem, expired.key    a certificate signed by ca that expired yesterday: SAN DNS:localhost
#
# Needs only openssl (3.x or LibreSSL-free 1.1.1: no -addext on `x509`, extensions go in a file).
set -euo pipefail
dir="${1:?usage: streams-tls-certs.sh DIR}"
mkdir -p "$dir"
cd "$dir"

ec() { openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$1" 2>/dev/null; }

mkca() { # name cn
  ec "$1.key"
  cat >"$1.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = v3
prompt = no
[dn]
CN = $2
[v3]
basicConstraints = critical, CA:TRUE, pathlen:0
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
EOF
  openssl req -x509 -new -key "$1.key" -sha256 -days 30 -config "$1.cnf" -out "$1.pem"
}

mkleaf() { # name days
  ec "$1.key"
  cat >"$1.cnf" <<EOF
[req]
distinguished_name = dn
prompt = no
[dn]
CN = localhost
[v3]
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature
extendedKeyUsage = serverAuth
subjectAltName = DNS:localhost
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid
EOF
  openssl req -new -key "$1.key" -config "$1.cnf" -out "$1.csr"
  openssl x509 -req -in "$1.csr" -CA ca.pem -CAkey ca.key -CAcreateserial -sha256 -days "$2" \
    -extfile "$1.cnf" -extensions v3 -out "$1.pem" 2>/dev/null
}

mkca ca "dcp-streams spike test CA"
mkca other-ca "dcp-streams spike OTHER CA"
mkleaf server 30

# An expired leaf: valid for one day starting 3 days ago. `openssl ca` is the portable way to
# set both dates.
ec expired.key
cat >expired.cnf <<EOF
[req]
distinguished_name = dn
prompt = no
[dn]
CN = localhost
EOF
openssl req -new -key expired.key -config expired.cnf -out expired.csr
cat >expired.ext <<EOF
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature
extendedKeyUsage = serverAuth
subjectAltName = DNS:localhost
EOF
mkdir -p expired-ca
: >expired-ca/index.txt
echo 01 >expired-ca/serial
cat >expired-ca.cnf <<EOF
[ca]
default_ca = c
[c]
dir = expired-ca
database = expired-ca/index.txt
new_certs_dir = expired-ca
serial = expired-ca/serial
default_md = sha256
policy = p
unique_subject = no
copy_extensions = none
[p]
commonName = supplied
EOF
start="$(date -u -d '3 days ago' +%Y%m%d%H%M%SZ)"
end="$(date -u -d '1 day ago' +%Y%m%d%H%M%SZ)"
openssl ca -batch -config expired-ca.cnf -cert ca.pem -keyfile ca.key -in expired.csr \
  -startdate "$start" -enddate "$end" -extfile expired.ext -notext -out expired.pem 2>/dev/null

# Prove what we made: server.pem verifies against ca, not against other-ca; expired.pem is expired.
openssl verify -CAfile ca.pem -verify_hostname localhost server.pem >/dev/null
if openssl verify -CAfile other-ca.pem server.pem >/dev/null 2>&1; then echo "server.pem verifies against the wrong CA" >&2; exit 1; fi
if openssl verify -CAfile ca.pem expired.pem >/dev/null 2>&1; then echo "expired.pem verifies" >&2; exit 1; fi
echo "# test PKI in $dir: $(openssl x509 -in server.pem -noout -subject -ext subjectAltName 2>/dev/null | tr '\n' ' ')" >&2
rm -f ./*.csr ./*.srl
