# MatrixRTC E2E Conformance and Operational Runbook

Operational and troubleshooting procedures for the Mandelbrot MatrixRTC
conformance testing harness, Synapse homeserver services, LiveKit SFU
stack, and Flatpak release builds.

## Table of Contents

* [1. Architecture Overview](#1-architecture-overview)
* [2. Operational Readiness](#2-operational-readiness)
* [3. Troubleshooting MatrixRTC E2E Failures](#3-troubleshooting-matrixrtc-e2e-failures)
* [4. Service Diagnostics](#4-service-diagnostics)
* [5. Flatpak Release and CI Pipeline Operations](#5-flatpak-release-and-ci-pipeline-operations)

---

## 1. Architecture Overview

Mandelbrot implements native MatrixRTC real-time group communications
support (`matrixrtc/`), which interoperates with the Matrix ecosystem
(MSC3401, MSC4143, MSC4140, MSC4195, and LiveKit SFU).

### Core Stack Components

* **Homeserver (Synapse)**: Provides Matrix Client-Server and Federation
  APIs, delayed leave tracking (MSC4140), and user authentication.
* **LiveKit SFU**: Selective Forwarding Unit providing WebRTC media routing
  and E2EE frame distribution.
* **LiveKit JWT Service (`lk-jwt-service`)**: Validates Matrix OpenID tokens
  via federation endpoints to issue SFU join tokens.
* **Nginx Reverse Proxy**: Terminate internal TLS for federation validation
  while routing plain HTTP client access on host port 8008.

---

## 2. Operational Readiness

Before running test harnesses or deploying client updates, verify that each
service in the compose stack responds to health and readiness checks.

The harness does not hard-code a compose provider. `stack_up` in
`tests/e2e/harness-lib.sh` picks `podman compose` or `docker compose`,
whichever is installed, and honours `COMPOSE` when it is set. The commands
below use `$COMPOSE` so they match whatever the harness itself selected:

```sh
export COMPOSE="podman compose"   # or: docker compose
```

### Port Allocation and Health Endpoints

| Service | Port | Endpoint / Health Check | Expected Status |
| --- | --- | --- | --- |
| Synapse HTTP | `8008` | `GET http://127.0.0.1:8008/_matrix/client/versions` | HTTP 200 OK |
| LiveKit SFU | `7880` | `GET http://127.0.0.1:7880/` | HTTP 200 (LiveKit response) |
| LiveKit JWT | `6080` | `GET http://127.0.0.1:6080/healthz` | HTTP 200 OK |
| Synapse Federation | `8448` | `GET https://127.0.0.1:8448/_matrix/federation/v1/version` | HTTP 200 OK |

### Manual Health Verification Procedure

1. Verify Synapse Client-Server readiness. This is the URL `wait_for`
   polls, with the same 180-second budget the harness allows it:

   ```sh
   curl -sf http://127.0.0.1:8008/_matrix/client/versions >/dev/null \
     && echo "Synapse HTTP: OK" || echo "Synapse HTTP: FAIL"
   ```

2. Verify LiveKit SFU readiness (60-second budget in the harness):

   ```sh
   curl -sf http://127.0.0.1:7880/ \
     && echo "LiveKit SFU: OK" || echo "LiveKit SFU: FAIL"
   ```

3. Verify LiveKit JWT auth handler readiness (60-second budget). The
   compose service is named `auth-service`; `lk-jwt-service` is the image:

   ```sh
   curl -sf http://127.0.0.1:6080/healthz \
     && echo "LiveKit JWT: OK" || echo "LiveKit JWT: FAIL"
   ```

---

## 3. Troubleshooting MatrixRTC E2E Failures

The MatrixRTC conformance test suite (`tests/e2e/`) validates wire
conformance across multiple scenarios.

### Common Failure Modes and Mitigations

#### Failure Mode A: Synapse Container Fails to Start or Times Out

* **Symptom**: `wait_for` in `tests/e2e/harness-lib.sh` gives up and calls
  `die`, so the run ends with
  `synapse did not come up at http://127.0.0.1:8008/_matrix/client/versions`.
  The same message appears with `livekit` or `lk-jwt-service` in place of
  `synapse` when one of those is the service that never answered.
* **Root Causes**:
  * Port conflict on host port `8008` or `8448`.
  * Database lock or uncleaned state in temporary directory.
* **Mitigation Steps**:
  1. Inspect container logs:

     ```sh
     $COMPOSE -f tests/e2e/compose.yml logs synapse
     ```

  2. Identify and clear conflicting host processes:

     ```sh
     ss -tulpn | grep -E '8008|8448|7880|6080'
     ```

  3. Tear down orphaned containers and volumes:

     ```sh
     $COMPOSE -f tests/e2e/compose.yml down -v
     ```

#### Failure Mode B: Media Frame Decryption or Key Exchange Failure

* **Symptom**: A scenario reaches the call but no client reports a
  received key, so `run-huddle.sh` and `run-interop.sh` fail their
  `io.element.call.encryption_keys` assertions.
* **Root Causes**:
  * LiveKit frame cryptor did not receive matching encryption keys.
  * UDP media port range blocked by the host firewall. LiveKit is
    configured for `50100-50200` in `tests/e2e/livekit/livekit.yaml`
    (`rtc.port_range_start` / `rtc.port_range_end`).
* **Mitigation Steps**:
  1. Inspect UDP port accessibility on the test host:

     ```sh
     nc -z -v -u 127.0.0.1 50100
     ```

  2. Verify that host networking is enabled for the LiveKit container in
     `tests/e2e/compose.yml`.
  3. Review the evidence bundle for to-device key transport events
     (`io.element.call.encryption_keys`). Each runner writes its own
     directory under `tests/e2e/logs/`, named for the scenario and the
     start time — `huddle-<timestamp>/`, `interop-<timestamp>/`,
     `resilience-<timestamp>/`, `federation-<order>-<timestamp>/`. The
     path is printed at the end of the run as `evidence bundle: ...`.

---

## 4. Service Diagnostics

When investigating federation or token issuance failures:

### Inspecting LiveKit JWT Service Federation Handshake

1. Verify Nginx certificate and SNI routing for `synapse.m.localhost`:

   ```sh
   curl -kv --resolve synapse.m.localhost:8448:127.0.0.1 \
     https://synapse.m.localhost:8448/_matrix/federation/v1/version
   ```

2. Check OpenID token resolution logs in the JWT service. The compose
   service is `auth-service`, not `lk-jwt`:

   ```sh
   $COMPOSE -f tests/e2e/compose.yml logs auth-service
   ```

### Recovering from Orphaned Test Environments

To reset the test environment completely:

```sh
# Stop containers and remove volumes
$COMPOSE -f tests/e2e/compose.yml down -v --remove-orphans

# The federated topology is a separate stack, not an overlay on the first
$COMPOSE -f tests/e2e/compose-federated.yml down -v --remove-orphans

# Clean transient test logs older than 7 days
find tests/e2e/logs/ -mindepth 1 -maxdepth 1 -type d -mtime +7 -exec rm -rf {} +
```

---

## 5. Flatpak Release and CI Pipeline Operations

### Flatpak Build Verification

Mandelbrot is packaged as Flatpak application `org.tunaos.mandelbrot`.

* **Local Verification via Flatpak-Builder**:

  ```sh
  flatpak-builder --user --force-clean --install-deps-from=flathub \
    --repo=_repo build-dir org.tunaos.mandelbrot.json
  ```

* **Publish Trigger Policy**:
  * Releases are triggered by pushing a version tag (e.g. `v0.1.0`) or via
    workflow dispatch (`.github/workflows/publish-flatpak.yml`).
  * Workflow utilizes `tuna-os/.github/.github/workflows/publish-flatpak.yml@main`.

### Release Rollback Procedure

Mandelbrot is not on Flathub: users get whatever the publish workflow last pushed as `latest` to
the TunaOS Flatpak index. If a release fails post-publish validation or causes critical
regressions, re-publish the last known-good tag (**Actions → Build and Publish Flatpak OCI → Run
workflow** on that tag), mark the broken GitHub Release as a pre-release, and fix forward with a
new tag. [RELEASING.md](../RELEASING.md#recovery) has the full recovery procedure.
