# RFC-0004 TLS spike: the Durable Streams binding over TLS and HTTP/2

Date: 2026-10-25 · Closes the "TLS and HTTP/2" item of [Spike B](0004-spike-b-results.md#not-verified) · Spec [section 8.4](../DOCUMENT-COLLABORATION-SPEC.md) · Code: [`tools/collab-spike/dcp-streams/`](../../tools/collab-spike/dcp-streams/) (`src/http.rs`, `tests/streams/{tls,tls_measure,idle,fwd,rawtls,live}.rs`), [`tools/collab-spike/scripts/streams-tls-*.sh`](../../tools/collab-spike/scripts/) · Workflow: [`collab-spike.yml`](../../.github/workflows/collab-spike.yml) (the spike ran it as its own `collab-spike-tls.yml`, merged into it at integration; now the jobs `streams-tls-deps`, `streams-tls` and `streams-tls-measure`)

**Everything here comes from GitHub-hosted runners (ubuntu-24.04, rustc 1.99.1), client and server on one machine over loopback, against two servers: the Node reference server `@durable-streams/server` 0.3.9 (Node v22) and the Go server release v0.3.0, each behind a Caddy v2.10.2 (the binary pinned by `scripts/streams-caddy-fetch.sh`) that terminates TLS with a generated test CA. Both TLS terminators are therefore the same Go `crypto/tls` + `x/net/http2` stack; no other TLS server was tried.** Loopback has no latency and no loss: where a network is needed (`fwd.rs`) it is emulated by a userspace TCP forwarder and says so. Anything not run is under [Not verified](#not-verified).

## Result

**GO for L0 to L2 over `https`, HTTP/1.1 and HTTP/2, on both servers, with the conditions of [Spec corrections](#spec-corrections).** Nothing of the Spike B suite behaved differently because of TLS or HTTP/2.

| Claim | Evidence | Result |
|---|---|---|
| The whole Spike B suite (11 journeys of spec 12 with nothing skipped, tampered record dropped, wrong key fails closed, revocation with a real second principal and its control, lease compare-and-set races, snapshot bootstrap, upload cut on the wire, **restart: SIGTERM, SIGKILL, in-memory**) passes over TLS for Node/Go x HTTP/1.1/HTTP/2 | [run 40179818637](https://github.com/tuna-os/gtk-office-suite/actions/runs/40179818637) (commit `8e4f228`): 32 passed, 0 failed, 20 `#[ignore]`d measurements, in Node h1, Node h2, Go h1; Go h2: the same test step passed (it failed afterwards in a probe step, fixed in `167aa64`, see below) | pass |
| The version under test is the one used, checked three ways | (1) the client requires it (ALPN offers only it; every response's version is checked, a mismatch is an error); (2) rustls reports ALPN and TLS 1.3 (`TLS-NEGOTIATED` line); (3) **the server's own access log**: `scripts/streams-tls-check-log.sh` over Caddy's JSON log: Node h2 7 987 TLS requests `HTTP/2.0`, 1 other; Go h1 9 778 `HTTP/1.1`, 2 other (the other-version control requests of `the_negotiated_version_is_the_one_under_test`); all TLS 1.3, cipher `TLS_AES_128_GCM_SHA256` | pass; the same check expecting the other protocol **fails** (`expected HTTP/1.1: 1 other: 7987`), so it can fail |
| The client verifies the chain against the test CA | wrong CA: `invalid peer certificate: UnknownIssuer`, no response accepted; no CA configured (there are no built-in roots): the same | pass, fails closed; control (right CA, same URL) works |
| Host-name verification | by IP `127.0.0.1`: `certificate not valid for name "127.0.0.1"; certificate is only valid for DnsName("localhost")`; by `streams.invalid` resolved to the server: `not valid for name`; control: `localhost` works | pass |
| Expired certificate | `certificate expired: ... not valid after ... (88533 seconds ago)` (certificate signed by the right CA for the right name; only the dates are wrong) | pass |
| No downgrade | server offering `http/1.1` only: client requiring h2 fails `received fatal alert: NoApplicationProtocol`, no response accepted; client requiring HTTP/1.1 works; **a client that does not say what it needs silently runs HTTP/1.1** | pass; the last is why a run must name the version |
| No plaintext, no redirect | `http://` URL with `require_tls`: refused before any request (0 requests); a plaintext request to the TLS port is answered `400`; a `307` from `https` to `http` is returned as an error and the plain listener sees **0** connections; control: a stock reqwest client with default redirects follows it (1 connection) | pass |
| openssl agrees, independent of our code | `openssl s_client` per job: `Verify return code: 0`, ALPN, TLSv1.3; other CA `20 (unable to get local issuer certificate)`; other name `62 (hostname mismatch)`; expired `10 (certificate has expired)`; h1-only listener refuses h2 | pass (run 40179818637) |

The dependency tree (`deps` job, same run): **228 crates** in the normal tree of `dcp-streams` against **205** without TLS; the 23 added include `rustls 0.23.46` (features `ring, std, tls12`), `rustls-webpki 0.103.16`, `tokio-rustls 0.26.6`, `hyper-rustls 0.27.10` (`http1,http2,ring,tls12`), `ring 0.17.14`, `h2 0.4.20`, `hyper 1.12.0`; **no** openssl, native-tls, aws-lc, webpki-roots, platform verifier or quinn (asserted; the grep is shown to match what must be there). `reqwest 0.12.28` with `default-features = false, features = ["rustls-tls-manual-roots", "http2"]`. Dev-dependencies add 8 crates (rustls, tokio-rustls, h2, http, bytes). `Cargo.lock` is committed and `cargo check --locked` passes.

## What was built

- **Client** (`src/http.rs`): `Http::with_transport(base, Transport)`. `Transport` = trusted CA PEM (**no built-in roots**: with none, every `https` server fails closed), required HTTP version (`Http1` offers only `http/1.1` in ALPN, `Http2` only `h2`, plus a check on every response), `require_tls`, pool idle time, host-to-address `resolve`, HTTP/2 keep-alive PING interval, adaptive window. Redirects are never followed; `https_only` is set for an `https` base. Transport errors now carry the whole error chain (the certificate verdict is two sources down in reqwest). Counters: responses by HTTP version, transport errors.
- **Servers behind TLS** (`scripts/streams-tls-up.sh node|go`): Node: plain Node origin behind a Caddy `reverse_proxy` that terminates TLS (`flush_interval -1`, upstream keep-alive 3 s because Node closes idle connections at 5 s). Go: the same Caddy binary with the `durable_streams` handler and TLS. Each front end also serves plain HTTP with the same handler chain, so TLS can be compared through the same proxy. `scripts/streams-tls-certs.sh`: openssl PKI (ECDSA P-256 CA, leaf with SAN `DNS:localhost` only, a second CA, a leaf that expired yesterday).
- **Controls with their own servers**: an expired-certificate front end, an `http/1.1`-only front end.
- **Tests**: `tls.rs` (above); the Spike B tests unchanged except that they build their `Http` from the environment; the restart tests start and kill the whole TLS stack (Go's restart is verified here for the first time: it kept every acknowledged record across SIGTERM and SIGKILL, `Stream-Seq` included; 8 requests and 10 bytes to resume); `fwd.rs`, a TCP forwarder that counts connections and bytes (TLS included), delays, stalls a connection like a retransmission, and ends idle or old connections by FIN, RST or silence.
- **Missing server**: every TLS test fails without a TLS server and skips only with `DCP_SKIP_STREAMS_TLS=1`, saying so; checked in each job (no URL: fails; `http://` URL: fails; `https` without CA or without `DCP_STREAMS_HTTP`: fails; explicit skip: prints `SKIPPED ... (DCP_SKIP_STREAMS_TLS=1)`).

## Measurements

Release build, [run 40178415302](https://github.com/tuna-os/gtk-office-suite/actions/runs/40178415302) (commit `63e0bf9`, jobs `tls-measure`, all four green; artifacts `tls-measurements-*`). 4 vCPU runner. Single run each: **p50/p95 are one sample of 200 or 50, not a distribution of runs; differences under about 1 ms, and every max, are noise** (maxes of 90 to 500 ms appear in single runs of plain HTTP too). Command: `cargo test --release --locked -p dcp-streams --test streams -- --ignored --test-threads=1 --nocapture <tests>` with the `streams-tls-up.sh` servers (access log off).

### Latency (Spike B's tests, unchanged), ms p50 / p95

Append to a second client's verified read (`measure::latency_append_to_a_second_client`), 100-byte update, 200 samples:

| | plain, origin direct | plain through Caddy | TLS HTTP/1.1 | TLS HTTP/2 |
|---|---|---|---|---|
| Node: A appends -> B's long poll returns | 2.99 / 5.15 (h1) | 3.78 / 6.41 | 3.74 / 5.38 | 2.29 / 6.12 |
| Node: A appends -> B has decrypted and verified | 5.20 / 7.71 | 6.72 / 9.89 | 6.72 / 8.80 | 4.45 / 10.49 |
| Go: long poll returns | | 1.80 / 2.81 | 1.83 / 2.87 | 1.70 / 2.27 |
| Go: verified in B | | 2.60 / 3.72 | 2.65 / 3.86 | 2.75 / 3.92 |
| Bare 620-byte append, Node / Go | 1.33 / n.a. | 1.48 / 0.74 | 1.62 / 0.73 | 1.21 / 0.86 |

(The Node HTTP/1.1 and HTTP/2 jobs each ran their own direct-origin and plain-through-Caddy baselines; the direct-origin long-poll p50 was 2.99 ms in the h1 job and 1.67 ms in the h2 job: that is the run-to-run spread.) **TLS added nothing measurable to a warm connection on loopback; on Node most of the difference to the direct origin is the proxy hop (about 0.8 ms).** Lease acquire/renew (settle 0): Node direct 7.5 / 7.5, TLS h1 12.4 / 11.4, h2 8.1 / 7.8; Go plain 3.3 / 3.2, TLS h1 3.4 / 3.2, h2 4.2 / 4.0.

### Cold start, bytes per request

`tls_measure::handshake_and_cold_start_cost` (50 handshakes each): TCP connect 0.06 to 0.10 ms p50; **full TLS 1.3 handshake 0.33 to 0.61 ms p50** (p95 0.38 to 0.72), **resumed 0.24 to 0.41 ms**, 50 of 50 resumed (`handshake_kind` read from rustls). Loopback and an ECDSA P-256 certificate: a real network adds one round trip (TLS 1.3) per new connection, a full chain adds its size. Bytes on the wire for one handshake (counted by the forwarder): **309 up / 1 032 down** full, 457 / 439 resumed (HTTP/1.1: 315 / 978). A fresh `Http` (builds the client and loads the CA: 0.03 to 0.04 ms), first request 0.9 to 1.3 ms against 0.2 to 0.4 ms plain, then 0.2 to 0.4 ms warm.

Bytes per record, one connection, 200 sequential appends of a 620-byte message after 20 warm-up (`tls_measure::request_overhead_per_record`, forwarder counts include TLS record framing):

| | client to server per append (beyond the body) | server to client per response | per `HEAD` up / down |
|---|---|---|---|
| plain HTTP/1.1 | 762 (+140) | 772 to 803 | 87 / 912 to 943 |
| TLS HTTP/1.1 | 784 (+162) | 765 to 794 | 109 / 934 |
| TLS HTTP/2 | 720 (+98) | **about 81** | 61 / (HPACK) |

HTTP/2 (HPACK) cuts the repeated response headers from about 790 to about 81 bytes and the request overhead from 140 to 98; TLS record framing costs 22 bytes per request. Relative to a 747-byte record (Spike B) the HTTP framing of HTTP/1.1 is as large as the record; with HTTP/2 it is about a eighth.

### HTTP/2 limits and 50 streams on one client

`tls_measure::the_servers_http2_settings_and_ping_time`: **both servers (same Caddy) allow 250 concurrent streams** per connection (`SETTINGS_MAX_CONCURRENT_STREAMS`); a PING to the terminator takes 0.06 / 0.12 ms p50 (Node / Go). 50 live readers plus appends fit with a margin of 200.

`tls_measure::fifty_*` (one `Http` client, 50 streams, each with a live reader: long poll as the binding does, or SSE which the binding does not use; paced = one message to one stream at a time, burst = a message to all 50 at once, then the same while an 8 MiB file is downloaded on the same client):

- **Connections used for 50 parked readers: HTTP/2 1, HTTP/1.1 50** (102 at the end of the run with appends), both servers, long poll and SSE.
- Paced wake-up p50 / p95: Node h2 3.8 / 13.3 (long poll), 3.9 / 11.1 (SSE); Node h1 4.5 / 5.4, 4.8 / 6.0; Go h2 3.4 / 4.0, 3.4 / 3.8; Go h1 3.5 / 4.0, 3.5 / 4.8. Burst of 50 (includes 50 concurrent appends): Node h2 29 / 70, h1 57 / 100; Go h2 31 / 35, h1 34 / 50.
- During an 8 MiB download on the same client, a small wake-up on another stream: Node h2 24 ms, h1 40 ms; Go h2 3.4 / 5.4, h1 6.7 (few samples: the download takes 20 to 40 ms). **No head-of-line blocking was seen at the HTTP/2 level on loopback**, and none was expected: it is TCP's.
- With emulated retransmission stalls (about 1 chunk in 200 held 250 ms, `fifty_*_when_the_connection_stalls`): paced max 265 ms for long poll in all four combinations; for SSE on Node h2 the paced max was 34 ms and on h1 266 ms. Burst p95 Node h2 292 (long poll) against h1 110; Go h2 46 against h1 71. **No consistent HTTP/2 versus HTTP/1.1 difference, in either direction, came out of this emulation**: the stall model (a pause of one direction of a connection) does not discriminate with 50 mostly idle streams. Real loss was not tested.
- **Large transfers under latency, an HTTP/2 finding** (`a_large_download_over_a_long_path`, 8 MiB, forwarder delay; the forwarder does not model TCP, so the HTTP/1.1 numbers are optimistic and only the HTTP/2 rows are limited by their own flow control): HTTP/2 with reqwest's fixed 64 KiB window: 180 / 226 MiB/s at 0 ms RTT (Node / Go), **28 / 30 at 40 ms, 12.5 / 12.6 at 100 ms**. With `http2_adaptive_window`: 84 / 87, 14 / 14, 5.9 / 5.9: **worse** in this setup. HTTP/1.1 (optimistic): 105 / 316, 57 / 81, 30 / 36. At the measured 12.6 MiB/s a 32 MiB file at 100 ms RTT over HTTP/2 with the default window would take about 2.5 s [INFERENCE from the 8 MiB rate, not run]. Not measured against a real long-RTT path.

### Idle connections

`tls_measure::reconnecting_after_the_server_closes_an_idle_connection`, front end with `timeouts { idle 5s }` (Node's own keep-alive timeout is 5 s): a request after an idle gap of 1 s or 3 s reuses the connection (2.8 to 4.6 ms); after 7 s or 12 s the server has closed it, the client opens a new one **with no error and no retry**, and the request takes 6.0 to 7.2 ms: **a reconnect costs about 3 ms, the TLS handshake and a TCP connect**, in all four combinations (the Node origin direct, plain: 3.0 against 3.1 for the reused one: no handshake). The idle server close is noticed by hyper before the next request on both HTTP versions.

`idle::a_proxy_that_closes_idle_connections_after_60_s` ([`fwd.rs`](../../tools/collab-spike/dcp-streams/tests/streams/fwd.rs) in front of the TLS port; "idle": nothing in either direction for 60 s; "age": the connection is 60 s old whatever it carries; ended by FIN, RST, or silence; two server configurations; pool idle time raised to 600 s so the idle connection is what is reused). Between requests (idle 65 s, then one `HEAD`), all four combinations alike:

| proxy | HTTP/1.1 | HTTP/2 |
|---|---|---|
| no rule | reused, 0 errors | reused |
| FIN or RST after 60 s idle | one new connection, **no error**, 10 to 20 ms | same |
| silent after 60 s idle | the request waits for its 30 s timeout, the retry opens a new connection: **succeeds after 30.06 s** | **fails after 90.15 s**: 3 attempts, each 30 s, all on the same dead connection |
| HTTP/2 PING every 15 s | n/a | the idle rule never fires (bytes flow); reused, 0 new connections |

In flight (a reader parked on a quiet stream; a record appended at 75 s, straight to the origin):

- **Server heartbeat longer than the proxy timeout** (long poll 90 s; Node SSE keep-alive 90 s; Go SSE 120 s): the proxy ends the poll at 60 s (`transport_errors` 1, `Http` retries), **the record is delivered 3 to 9 ms after the append** for long poll and SSE (offset resume loses nothing), FIN and RST alike, both servers, both HTTP versions. **A long poll that is always cut at 60 s never completes**: the binding's `send` gives up after 3 attempts (a transport error to the caller after about 3 minutes in this configuration), so a reader must treat a transport error as "resume from my offset" (the test reader does; the one in `measure.rs` did not).
- **Server heartbeat shorter** (long poll 30 s; Node SSE keep-alive 30 s; Go SSE lifetime 55 s): the proxy never fires (0 connections ended), 0 errors, delivery 4 to 14 ms.
- **Silent proxy, in flight, no pings**: long poll: delivered after **45.06 s** (the client's own 120 s timeout, then a retry), HTTP/1.1 and HTTP/2 alike (the retry on HTTP/2 found a usable connection in these runs); **SSE: never** within the 80 s watched (no response timeout exists on a stream).
- **Silent proxy, in flight, HTTP/2 PING every 15 s** (10 s to answer): the dead connection is replaced and the record delivered after **10.03 to 10.08 s** (long poll and SSE, both servers).

## Findings that are not numbers

1. **The Node server's `Location` header names the origin, not the proxy** (`LOCATION-HEADER ... location="http://127.0.0.1:4437/v1/stream/..."` on a `PUT` through the proxy; the Go server answers its own `https` URL). The binding never reads it. Anything that followed it would go around the TLS terminator to a plaintext origin.
2. **Go server 0.3.0 behind Caddy's `log` directive answers SSE with `500 streaming not supported`** (its handler asserts `http.Flusher` on the response writer, which the logging wrapper is not); without the directive SSE works (probe step of the `tls` job; asserted from `167aa64` on). Long poll is unaffected. A deployment that logs through Caddy cannot use SSE with this server.
3. **Caddy 2.10.2 with `protocols h2` alone closes every TLS handshake** (openssl: `unexpected eof`, no ALPN; probe step, recorded, not asserted), so an HTTP/2-only listener could not be tested; an HTTP/1.1-only listener (`protocols h1`) works and refuses h2 with `no application protocol`.
4. The reference Node server closes idle keep-alive connections at 5 s; a reverse proxy that keeps upstream connections longer than that races it (the script sets 3 s). Not measured with a longer setting.
5. `Http::send` retries a request up to 3 times on a transport error, appends included: a lost reply can store a record twice (readers drop the second copy, Spike B). With an HTTP/2 connection that died silently, all 3 attempts used the dead connection (idle test above).

## Spec corrections

Proposed additions to **8.4** (and one to 4.3); the spec file itself was not edited.

> **Transport.** A client MUST reach a Durable Streams server over `https` and MUST verify the server certificate chain against a trust anchor configured for that deployment (a CA the deployment names, or the platform's roots) and MUST verify that the certificate covers the host name of the URL; a client MUST NOT disable verification, MUST NOT fall back to `http` or to a lower TLS version (TLS 1.2 is the minimum, 1.3 is expected), and MUST fail closed, reporting a transport error and never an empty document, when verification fails. `http` MAY be used for a server on the loopback interface in development.
>
> **No redirects, no `Location`.** A client MUST NOT follow a redirect that leaves the origin or that goes from `https` to `http`, and SHOULD NOT follow any; it MUST NOT take the address of a stream from a response header (`Location`): a server behind a proxy may name its internal plaintext origin there (observed).
>
> **HTTP version.** HTTP/1.1 and HTTP/2 are both valid. A client that requires one MUST offer only that one in ALPN and MUST fail if the response is of another; a client that requires none takes what ALPN gives (HTTP/2 when offered) and cannot tell it was downgraded. With HTTP/2 a client SHOULD use one connection per origin (50 live streams used one), MUST NOT assume more than 100 concurrent streams unless the server's `SETTINGS` say so (both servers measured allow 250), and SHOULD enable adaptive flow-control windows for large file transfers.
>
> **Live reads and idle connections.** A server's long-poll timeout and its SSE keep-alive interval MUST be shorter than the shortest idle timeout of any proxy on the path; 60 s is common, so a server SHOULD use 30 s or less (both reference servers default to 30 s for long poll; the Go server's SSE sends nothing while idle and ends the response after 60 s by default, which equals a 60 s proxy). A client MUST treat any transport error on a live read as "resume from the last offset" (never as a fatal error and never as an empty document) and MUST bound consecutive failures with a back-off. A client SHOULD apply a response timeout to every live read and, on HTTP/2, SHOULD send keep-alive PING frames at no more than a quarter of the shortest idle timeout it must survive (15 s for 60 s): pings keep an idle rule from firing and replace a silently dead connection within about 10 s (measured), whereas without them a silent loss costs 30 s on HTTP/1.1, 90 s for a request and 120 s for a long poll on HTTP/2, and an SSE stream never recovers. An idle connection closed by the server or by a proxy with FIN or RST is not an error: the next request opens a new one (about 3 ms on loopback plus a round trip).
>
> **Measured** (Node 0.3.9 and Go 0.3.0 behind Caddy 2.10.2, loopback, one run): the Spike B suite passes over TLS 1.3 on HTTP/1.1 and HTTP/2; append to a second client's verified read 4.5 to 6.7 ms p50 (Node), 2.6 to 2.8 ms (Go), no measurable difference from plain HTTP; a full TLS 1.3 handshake 0.3 to 0.6 ms and 1.3 kB on the wire; per request HTTP framing 98 bytes on HTTP/2 against 162 on HTTP/1.1.

**4.3 lease**: no change needed; the `409` compare-and-set behaved identically over TLS (20 x 8 races, Spike B test, all combinations).

## Not verified

- **A TLS terminator other than Caddy 2.10.2** (nginx, Envoy, a cloud load balancer, a CDN) and a server with native TLS; **mutual TLS**, **certificate revocation (OCSP/CRL)**, **pinning**, a chain with an intermediate CA, **TLS 1.2-only** servers, other cipher suites, **0-RTT**, **HTTP/3** (neither server was run with it).
- **A real network**: latency, loss, reordering, MTU, a real long-RTT path. The forwarder does not lose packets, does not model TCP congestion or flow control (so its HTTP/1.1 bulk numbers are optimistic), and its stalls do not discriminate HTTP/2 from HTTP/1.1; **TCP-level head-of-line blocking was therefore not measured**, nor `tc netem`.
- A real middlebox: the 60 s idle, age and silent rules are emulated; real load balancers differ (ALB closes idle at 60 s by default and answers RST, NAT gateways go silent; not tried).
- More than 50 streams on one connection, the 250-stream limit being reached, servers that allow fewer, `REFUSED_STREAM`.
- **Go server with HTTP/2 only**, and the Go server's SSE behind a logging front end (it fails, above).
- The reverse proxy to Node with an upstream keep-alive above the origin's 5 s (the 502 race).
- **Large files over a high-RTT HTTP/2 path** beyond the emulated 8 MiB; the 32 MiB file; the adaptive window result is one setup.
- Windows and macOS clients, the platform trust store (`rustls-native-certs` was kept out of the tree on purpose; a product needs a decision), certificate rotation while connected, a CA that changes under a running client.
- Anything of Spike B's own "not verified" list that is not about TLS (retention, forks, power loss, a malicious server, more than 50 members).
- The final commit's correctness run: see [Runs](#runs).

## Runs

| What | Run | Commit | Conclusion |
|---|---|---|---|
| Correctness matrix, first attempt | [40177622859](https://github.com/tuna-os/gtk-office-suite/actions/runs/40177622859) | `02af576` | failed in the openssl step (a negative probe exits non-zero under `bash -e`), fixed |
| Measurements (4 jobs) | [40178415302](https://github.com/tuna-os/gtk-office-suite/actions/runs/40178415302) | `63e0bf9` | `tls-measure` x4 **success**; its `tls` jobs failed on the same openssl step |
| Correctness matrix, deps | [40179818637](https://github.com/tuna-os/gtk-office-suite/actions/runs/40179818637) | `8e4f228` | `deps` success; Node h1, Node h2, Go h1 **success**; Go h2 failed only in the final SSE probe step (the probe started a second Caddy with another CA than the run's; the journeys, openssl, log check passed) |
| Correctness matrix, after the probe fix | [40182677787](https://github.com/tuna-os/gtk-office-suite/actions/runs/40182677787) | `167aa64` | `deps`, Node h1 and **Go h2 success** (the probe step now passes: Go h2 is green end to end); Node h2 and Go h1 were still queued when this was written (both passed in run 40179818637 at `8e4f228`) |

## Reproducing

```
cd tools/collab-spike
(cd scripts/streams-server && npm ci)
scripts/streams-tls-certs.sh /tmp/pki; export STREAMS_TLS_DIR=/tmp/pki
eval "$(scripts/streams-tls-up.sh node)"        # or go; prints DCP_STREAMS_URL (https), _CA, _PLAIN_URL, ...
export DCP_STREAMS_HTTP=2                       # or 1.1; required for an https URL
cargo test -p dcp-streams --test streams -- --nocapture --test-threads=2
scripts/streams-tls-check-log.sh "$DCP_STREAMS_ACCESS_LOG" HTTP/2.0
scripts/streams-tls-down.sh "$DCP_STREAMS_PIDFILE"
# measurements, release, access log off (Go SSE): STREAMS_ACCESS_LOG=0 on the up script
cargo test --release -p dcp-streams --test streams -- --ignored --test-threads=1 --nocapture tls_measure::
# the existing plain-HTTP jobs need DCP_SKIP_STREAMS_TLS=1 (the TLS tests fail without a TLS server)
```
