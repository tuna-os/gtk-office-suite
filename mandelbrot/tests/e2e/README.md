# MatrixRTC interop e2e tests

End-to-end conformance harness for the native MatrixRTC implementation in
[`matrixrtc/`](../../matrixrtc) (see the "MatrixRTC conformance testing"
section of `MANDELBROT.md`). It spins up a real homeserver + SFU stack and
proves, over the wire, that our client produces spec-conformant MatrixRTC
sessions (MSC4143 memberships, MSC4140 delayed leave, MSC4195 SFU auth,
`io.element.call.encryption_keys` key transport, E2EE media).

## Contents

| Path                    | Purpose                                                              |
| ----------------------- | -------------------------------------------------------------------- |
| `compose.yml`           | Minimal single-homeserver stack (synapse, nginx, livekit, lk-jwt)    |
| `compose-federated.yml` | Overlay adding a **second** homeserver, SFU and auth service, federated with the first — the topology element-call uses for its federation specs |
| `synapse/homeserver.yaml` | Synapse config: MSC4140 (`max_event_delay_duration`), MSC4222, MSC4354, open registration |
| `synapse/homeserver-federated.yaml`, `synapse/homeserver-othersite.yaml` | The two sites of the federated stack (mutual `trusted_key_servers`, `federation_verify_certificates: false`) |
| `livekit/livekit.yaml`, `livekit/livekit-othersite.yaml` | LiveKit SFU configs (host networking, dev key `devkey:secret`); site 2 uses ports 17880/17881 and UDP 50300-50400 |
| `nginx/nginx.conf`      | TLS at `synapse.m.localhost` (needed by lk-jwt federation OpenID check) + plain-http client access on host port 8008 |
| `nginx/nginx-federated.conf` | Same, for both sites; site 2 is reachable on host port 18008 (http) and shares host port 8448 (https) routed by SNI |
| `tls/`                  | Self-signed `*.m.localhost` dev certificates, copied verbatim from element-call's `backend/` (TESTING ONLY) |
| `harness-lib.sh`        | Shared helpers (stack lifecycle, registration, PASS/FAIL recording) for the scripts below |
| `run-interop.sh`        | TEST 1: two native clients in the same call, full assertion suite    |
| `run-interop-ec.sh`     | TEST 2: our client ↔ a real Element Call in a browser (manual)       |
| `run-resilience.sh`     | TEST 3: EC's `sfu-reconnect-bug.spec.ts` + `reconnect.spec.ts` + SFU restart |
| `run-restricted-sfu.sh` | TEST 4: EC's `restricted-sfu.spec.ts` — JWT-vs-membership ordering, SFU auth unavailable |
| `run-huddle.sh`         | TEST 5: EC's `widget/huddle-call.test.ts` — a five-participant call  |
| `run-federation.sh`     | TEST 6: EC's `widget/federation-oldest-membership-bug.spec.ts` and `widget/federated-call.test.ts`, across two homeservers |

The configs are adapted from element-call's `docker-compose-dev.yml` +
`backend/` dev fixtures, minimized to a single site.

## Architecture notes

* **Clients talk plain HTTP/WS**: our test client uses webpki roots
  (rustls), which cannot trust the self-signed dev CA, so synapse is
  reached at `http://127.0.0.1:8008` (nginx port 80), the JWT service at
  `http://127.0.0.1:6080` and the SFU at `ws://127.0.0.1:7880`. TLS exists
  _only_ inside the compose network because `lk-jwt-service` validates
  MSC4195 OpenID tokens via the Matrix federation API, which
  gomatrixserverlib only speaks over https (hence nginx +
  `LIVEKIT_INSECURE_SKIP_VERIFY_TLS`).

* **LiveKit runs with host networking** so its ICE candidates are
  reachable by clients on the same host. With rootless podman, container
  IPs are not routable from the host and the SFU would advertise
  unreachable candidates.

* Rootless podman works; no ports below 1024 are published.

## Running

Prerequisites: `podman-compose` (or `docker compose`), `curl`, `jq`, and a
Rust toolchain able to build the `matrixrtc` crate with the `livekit`
feature (needs a C/C++ toolchain for libwebrtc; on a pkg-config-poor host
use a distrobox).

```sh
# Optionally prebuild the client:
(cd ../../matrixrtc && cargo build --features livekit --example join_call)

JOIN_CALL_BIN=../../matrixrtc/target/debug/examples/join_call ./run-interop.sh
```

`KEEP_STACK=1` keeps the containers running afterwards for debugging.
Client and state logs land in `logs/<timestamp>/`.

### What TEST 1 asserts

1. Both clients publish an `org.matrix.msc3401.call.member` state event
   whose content is well-formed `SessionMembershipData`:
   `application: "m.call"`, `call_id: ""`, a `device_id`,
   `focus_active: {type: livekit, focus_selection: oldest_membership}` and
   at least one `foci_preferred` entry with a `livekit_service_url`.

2. Each client sees the other's membership (via room state).

3. Each client receives the other's media encryption key (to-device
   `io.element.call.encryption_keys`; **unencrypted** in this harness —
   the example does not carry an Olm stack).

4. Each client subscribes to the other's audio track and receives
   _decrypted_ audio frames (the LiveKit frame cryptor drops frames it
   cannot decrypt, so frame delivery proves the E2EE key exchange).

5. `kill -9` on one client: the homeserver fires the MSC4140 delayed
   leave event and the membership content becomes `{}` within the delay
   window (8 s delay + 5 s keep-alive interval + slack).

6. SIGINT on the other client: graceful leave clears the membership
   immediately.

## CI

`.github/workflows/matrixrtc-e2e.yml` runs this suite on **manual dispatch
and nightly**, never on the PR merge gate:

* it pulls ~1.5 GB of container images and builds libwebrtc (~10 min cold),
* it needs UDP 50100-50200 (and 50300-50400 for the federated stack) plus
  host networking for LiveKit,
* media timing makes it inherently flakier than the unit suite
  (`matrixrtc/tests/`), which stays the merge gate as the
  `matrixrtc-conformance` job in `ci.yml`.

A GitHub-hosted `ubuntu-latest` runner is sufficient: the job owns the VM's
network namespace, so host networking and arbitrary UDP ranges work, and
Docker is preinstalled so `docker compose` satisfies the compose-provider
probe. The evidence bundles in `logs/` are uploaded as an artifact.

The one part that stays manual is `run-interop-ec.sh` (Element Call in a
headless Chromium): a further ~1 GB image plus Playwright browsers, and its
value is a conformance report rather than a gate.

### Running one script

```sh
JOIN_CALL_BIN=../../matrixrtc/target/debug/examples/join_call ./run-resilience.sh
JOIN_CALL_BIN=... ./run-restricted-sfu.sh
JOIN_CALL_BIN=... ./run-huddle.sh 5
JOIN_CALL_BIN=... ./run-federation.sh remote-first   # needs compose-federated.yml
```

Each records `RESULT PASS|FAIL|INFO <name> <details>` lines and exits 1 if
any FAIL was recorded. `KEEP_STACK=1` keeps the containers up between runs,
which is much faster when running several scripts in a row.

Note the harness scripts poll; keep expensive commands out of poll loops
(`docker logs` on a busy container reads the whole log every call and will
overload a small host).

## Phase 2 (planned): Element Call interop

Join the _same_ call with a headless Element Call to prove interop with
the reference client, modeled on element-call's Playwright setup:

* add the `ghcr.io/element-hq/element-call` container (config pointing at
  `http://127.0.0.1:8008` + `livekit_service_url` `http://127.0.0.1:6080`;
  Chromium treats `localhost` as a secure context, so plain http/ws works),

* drive it with Playwright Chromium (`--use-fake-device-for-media-stream`),
  log in as bob, open the room call,

* assert mutual membership (state API), media both ways, and EC-side
  reception of _our_ keys. NOTE: Element Call sends its keys via
  **Olm-encrypted** to-device messages when the room is encrypted; the
  plain harness client can only interop in unencrypted rooms, and phase 2
  must verify which key-transport mode EC uses there (it may still refuse
  unencrypted transport — in that case the harness needs an Olm stack via
  matrix-sdk-crypto).
