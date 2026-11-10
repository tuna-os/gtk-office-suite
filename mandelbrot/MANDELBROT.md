# Mandelbrot — Fractal fork roadmap

## Status (Current)

* **Published**: repo github.com/tuna-os/mandelbrot · flatpak
  `org.tunaos.mandelbrot` on the tuna-os remote (install verified end-to-end)
  · landing page live at tunaos.org/mandelbrot · docs at
  tunaos.org/docs/mandelbrot.

* **Done**: fork rename/branding; CI (checks, matrixrtc conformance, clippy,
  flatpak build+tests) + OCI publish pipeline; `matrixrtc/` engine crate
  (membership + MSC4140, E2EE key distribution, RtcCallSession, livekit
  connection behind feature flag — 114 ported conformance tests); native call
  UI shell (CallView/prescreen/tiles/call bar, demo-driven).

* **Done**: sliding sync (with classic fallback) · QR login (both
  directions) · full native calls (engine + app integration + media via
  livekit, mounted UI, portal-v2 call notifications) · threads · polls ·
  voice-message recording · interop e2e harness (phase 1 PASSED: live
  two-client encrypted call with delayed-leave cleanup on a local
  synapse+livekit stack). Feature docs: [docs/FEATURES.md](docs/FEATURES.md).

* **Interop verified**: phase 2 ran against Element Call v0.22.0 —
  same membership format both ways, calls connect in both directions,
  delayed-leave cleanup crosses implementations.
  See [tests/e2e/CONFORMANCE.md](tests/e2e/CONFORMANCE.md)
  and [tests/e2e/README.md](tests/e2e/README.md).
  Open: app-level media E2EE with EC (harness client has no Olm stack, the app
  does), and MSC4354 sticky events (EC's matrix_2_0 mode).

* **Done also**: spaces hierarchy browsing.

* **Done**: metainfo release notes for the first
  Mandelbrot tag; MatrixRTC e2e scenario harness committed — 20 engine-level
  Element-Call-scenario tests (`matrixrtc/tests/element_call_scenarios.rs`,
  on the PR gate) plus live-stack harness scripts (federation/huddle/
  resilience/restricted-sfu/incoming-call) and a nightly `matrixrtc-e2e.yml`.

* **Done**: camera capture — the camera toggle
  opens the camera through the XDG camera portal (aperture's PipeWire device
  provider, as used by the QR scanner), captures it with `GStreamer` into
  I420, and publishes it to the SFU as a LiveKit camera track. The pipeline
  tees into a `gtk4paintablesink` whose paintable drives both the pre-join
  preview and the in-call self-view (new `self-paintable` property on
  `CallState`). Turning the camera off (or dismissing the pre-join screen,
  or leaving the call) stops the pipeline, which closes the device, and
  unpublishes the track.

* **Roadmap items in progress**: live testing against matrix.org (calls, QR, threads receipts),
  threaded read receipts (MSC3771), location pin-drop.

Goal: take Fractal (the most GNOME-native Matrix client, matrix-sdk 0.18 +
matrix-sdk-ui) and adopt the modern Matrix Rust SDK features it doesn't use.
State of research: July 2026. Reference checkouts live in `~/dev/mandelbrot-refs/`
(matrix-rust-sdk, element-x-android, element-x-ios, aurora, gomuks, robrix, cinny).

## Where Fractal already is (no work needed)

Fractal is on the _current_ SDK release and already uses: matrix-sdk-ui
**Timeline** (`src/session/room/timeline/`), **event cache**, **send queue**
(offline resend), **OAuth 2.0/MSC3861 login** (plus SSO + password fallbacks),
full **E2EE** (SAS + QR verification, key backup, recovery, cross-signing),
SDK **NotificationSettings**, knocking, moderation, ignoring, location sharing,
reactions/edits/replies/read receipts/typing, media viewer.

## Gap analysis (the Mandelbrot deltas)

| # | Feature | Fractal today | SDK API | Server needs |
|---|---------|---------------|---------|--------------|
| 1 | **Video/voice calls** | Display-only ("use another client to answer", `src/session_view/room_history/call_row.rs`); upstream closed calls as out-of-scope (#1658) | `matrix_sdk::widget::{WidgetDriver, WidgetSettings::new_virtual_element_call_widget}` behind Cargo feature `experimental-widgets` | LiveKit SFU + jwt service via `.well-known` rtc_foci, MSC4140 delayed events (matrix.org: yes) |
| 2 | **Simplified sliding sync (MSC4186)** | Classic `sync_stream` loop (`src/session/mod.rs:455-490`) + hand-rolled `src/session/room_list/` | `matrix_sdk_ui::sync_service::SyncService` + `room_list_service::RoomListService` | Synapse ≥ 1.114 (default-on), conduwuit/continuwuity; keep /sync v2 fallback for old servers |
| 3 | **QR code login (MSC4108)** | Absent (QR only for verification) | `oauth().login_with_qr_code(...)` (scan-to-login) + grantor flow (0.16+), feature `experimental-oidc` | OAuth server (MAS / Synapse MSC3861) + `msc4108_enabled` rendezvous |
| 4 | **Threads** | Flat timeline | Timeline threading + event-cache thread support (0.17+) | none |
| 5 | **Polls** | Not rendered | Timeline poll items (`m.poll`) | none |
| 6 | **Spaces hierarchy** | Category/sidebar grouping only | new `matrix_sdk_ui::spaces` module | none |
| 7 | **Rich/push notifications** | Local gio::Notification from sync loop | `matrix_sdk_ui::notification_client::NotificationClient` (mobile-push oriented; desktop value is background/rich resolution) | push gateway only if we go full push |
| 8 | **Voice messages (send)** | Playback/rendering PRESENT (`src/utils/matrix/media_message.rs` handles `voice` flag); **recording ABSENT** (no recorder/mic code) | Send `m.audio` + `org.matrix.msc3245.voice`; record via GStreamer (autoaudiosrc → opusenc → oggmux) | none |
| 9 | **Video rooms (MSC3417)** | Detection only — `is_call_room` property + camera icon (`src/session/room/mod.rs:233`) | Rides on #1: a video room is a room whose primary view is the call, chat secondary | same as calls |
| 10 | **Location** | Mostly PRESENT: send current location (geoclue, preview+confirm in `message_toolbar`), render + map viewer (libshumate). Gaps: pin-drop (arbitrary point) and live location (MSC3489) | Pin-drop is pure UI on the existing shumate map; live location beacons are event-layer | none |

## How others do it

* **Element X (Android/iOS)** — the production template for everything: sole
  users of SyncService/RoomListService; QR login; calls via **embedded Element
  Call webview + rust-sdk WidgetDriver** (not native RTC). Their FFI layer shows
  exactly which sdk-ui surface matters.

* **Aurora** (element-hq, experimental Element X Web/Desktop) — rust-sdk WASM +
  TypeScript + Tauri. Cleanest "sdk-ui drives the whole client" example;
  validates the architecture Mandelbrot targets.

* **gomuks web** — best non-Element Call embedding prior art: plain
  matrix-widget-api, **bundles Element Call locally**, keeps the widget alive
  across room switches (pop-out, Feb 2026). Mine for widget lifecycle.

* **Robrix** — native-Rust Makepad client; closest analog for bridging
  SyncService/Timeline async streams into a retained-mode toolkit (what our
  GTK/glib MainContext glue must do). No calls yet.

* **Cinny** — js-sdk, but mainline now ships a full **custom-UI call
  implementation** (~2000 lines, `src/app/plugins/call/` + `src/app/features/call/`)
  and it is **our template for a native calling experience**: Element Call runs
  in a persistent iframe (`CallEmbed`) purely as the RTC engine + video grid,
  while Cinny renders its own prescreen, controls, member cards, and live-status
  chips natively, driving the widget via `ElementWidgetActions` (e.g.
  `DeviceMute`) and consuming `ElementMediaStateDetail` events back. The iframe
  outlives room switches. Translated to GNOME: WebKitGTK webview = video grid
  only; all chrome (call controls, roster, PiP, prescreen) is GTK/libadwaita.

* **NeoChat** — libQuotient, still no VoIP. **No GTK/Qt desktop client has
  shipped Element Call — Mandelbrot would be first.**

* **Commet** — Flutter + matrix-dart-sdk with SDK-native LiveKit calls; only
  relevant if we ever want non-widget native MatrixRTC.

## Roadmap

**Decision (2026-07-23): calls first.** The WebRTC prototype gate, then the
full call stack, precede everything else. Sliding sync and the rest follow.

### Phase 0 — Fork plumbing

Rename/app-id (`org.gnome.Fractal` → new id), branding, keep rebase-ability on
upstream in mind (mechanical renames in a single commit).

### Phase 1 — Calls (was Phase 3; see below)

Order: (a) WebKitGTK WebRTC prototype gate → (b) WidgetDriver bridge +
CallSession GObject → (c) native GTK chrome on the Cinny headless-EC template →
(d) video rooms. GNOME-native design inputs: GNOME Calls / libcall-ui /
Phosh call notifications / Dino (research in progress).

### Phase 2 — Sliding sync (foundation)

Replace the classic sync loop with `SyncService` + `RoomListService` +
encryption sync; rewire `src/session/room_list/` onto RoomListService diffs
(same VectorDiff pattern the timeline diff-minimizer already handles).
Keep a /sync v2 fallback path for servers without MSC4186 (SDK still provides
it). Upstream issue #1565 confirms nobody has done this yet. This phase touches
`Session` deeply — do it before anything else builds on sync behavior.

### Phase 2 — QR login

Small, self-contained: `login_with_qr_code` on the existing OAuth stack;
Fractal already has camera scanning (`src/components/camera/`) and QR rendering
(`src/contrib/qr_code.rs`) from verification. Add both directions: scan-to-login
on new device, and "link a new device" grantor flow in account settings.

### Phase 3 — Element Call ⚠ prototype first

Architecture: WebKitGTK `WebKitWebView` loading a **bundled** Element Call
build, script-message bridge ↔ `WidgetDriver` (`experimental-widgets`), URL from
`WidgetSettings::new_virtual_element_call_widget`. E2EE/to-device/OpenID/MSC4140
all flow through the host session — no second login.
**UI template: Cinny's headless-EC pattern** (see landscape section): webview
shows only the video grid; prescreen, call controls, roster, live chips, and
PiP are native GTK, driving EC via widget actions. Port Cinny's
`CallEmbed`/`CallControl` responsibilities to a `CallSession` GObject.
**Phase 3b — video rooms (MSC3417)**: detection already exists; make the call
the primary view with chat as a side pane.
**Risk gate: WebKitGTK WebRTC** (GstWebRTC backend still maturing; camera via
portal only since 2025; Element Call E2EE needs insertable streams /
`RTCRtpScriptTransform`). Build a throwaway WebKitGTK page that joins an
Element Call room _before_ investing in the bridge. Fallback: external browser
window with a widget URL.

### Phase 4 — Timeline features: threads, polls, voice messages

Threads: SDK timeline/event-cache thread support (0.17+), threaded view UI.
Polls: render + create `m.poll` timeline items. Voice messages: recording UI in
the message toolbar (GStreamer opus capture; playback already works). Location
pin-drop on the existing libshumate map. All isolated to
`room_history`/timeline/toolbar layers.

### Phase 5 — Spaces + notifications polish

Space hierarchy browsing on `matrix_sdk_ui::spaces`; evaluate
NotificationClient for richer notifications.

## Phase 1 findings (2026-07-23)

### WebRTC probe result — risk gate TRIPPED

`prototypes/webrtc-probe.py` (WebKitGTK 2.52.5, GNOME 50 runtime, Xvfb):
`getUserMedia` (mock), getDisplayMedia, WebCodecs, AudioWorklet, wasm, Workers
all work — but **`RTCPeerConnection` and `RTCRtpScriptTransform` are absent**.
webrtcbin/nice load fine in the same sandbox; only 3 string hits for the RTC
classes in `libwebkitgtk-6.0.so` and no `PeerConnection` runtime feature →
the GNOME runtime's WebKitGTK is **built with `ENABLE_WEB_RTC=OFF`** (upstream
default; host has no WebKitGTK at all). Element Call cannot run in a stock
WebKitGTK webview. Options:

* **A. Bundle a custom WebKitGTK** (`-DENABLE_WEB_RTC=ON`) in the flatpak.
  Keeps the Cinny/Element-X widget architecture; costs a browser-engine build
  in CI + we own its security updates + GstWebRTC vs LiveKit (simulcast,
  E2EE frame transforms) still unproven until tested.

* **B. Native MatrixRTC**: livekit rust SDK (bundles libwebrtc, has E2EE frame
  cryptors) + gtk4paintablesink for video + matrix-sdk for `m.call.member`
  signaling/key distribution. No browser engine, fully native UI — the most
  "GNOME native" outcome; largest engineering effort; we re-implement what
  Element Call does (prior art: Commet/matrix-dart-sdk does SDK-native LiveKit).

### GNOME-native calling design inputs (research brief)

* **libcall-ui** (GTK4/libadwaita, LGPL): `CuiCallDisplay` implements the whole
  voice-call UI over a `CuiCall` interface we can implement for Matrix calls;
  telephony-shaped (no video). Reuse for voice + design language.

* **GNOME Calls**: daemon + GNotification pattern; in-call layout from
  Design/app-mockups `calls/` (incoming/ongoing/audio-source mockup images).

* **Notifications**: xdg-desktop-portal ≥ 1.19.1 notification v2 has
  first-class call support — `category=call.incoming/ongoing/unanswered`,
  button `purpose=call.accept/call.decline/call.hang-up`, urgent priority,
  ring sound via fd. Authored by a Fractal maintainer (jsparber) + Phosh's agx;
  Phosh honors it today (lockscreen call UI, feedbackd ringtone), GNOME Shell
  is catching up (48+). Use it; degrade to plain action buttons elsewhere.

* **Video chrome template: Dino** (`main/src/ui/call_window/`, cloned):
  GtkOverlay — participant grid base, corner self-view via `add_overlay` +
  `get-child-position`, floating fading headerbar + bottom bar
  (EventControllerMotion + 3 s timeout), device-picker popovers.

* **PiP**: no system PiP on GNOME; xdg-pip rejected by GNOME (KDE ships it).
  Pattern: in-window overlay self-view + compact "ongoing call" bar when
  navigating away + optional small utility window (user pins manually).

### Bake-off: Track B (native livekit-rust) — PASS (2026-07-23)

`~/dev/mandelbrot-refs/spikes/livekit-spike/` built and ran on dilli:
publisher + subscriber rooms against `livekit-server --dev`, synthetic I420
frames published, `RESULT: received 10 video frames end-to-end`. Notes:
use rustls features (`rustls-tls-webpki-roots` + `signal-client-tokio`;
`native-tls` needs openssl/pkg-config and `signal-client-dispatcher` drags in
isahc/curl/openssl); libwebrtc is statically linked (~200 MB debug binary);
livekit 0.7 API drift vs docs (`NativeVideoSource::new` 2nd bool arg,
`VideoFrame.frame_metadata`). ### Bake-off: Track A (WebKitGTK + Element Call widget) — FAIL (2026-07-23)
Tested webkitgtk.org nightly MiniBrowser bundles (the only prebuilt
WebRTC-enabled WebKitGTK; 245 MB/day, ~1-month retention — no stable
distributable exists; GNOME runtime builds have WebRTC off). Transport plane
works (DTLS-SRTP loopback with decoded video after NSS surgery), but three
showstoppers: (1) **RTCRtpScriptTransform silently bypasses frames** (all
webrtc-encoded-transform tests upstream-skipped, WebKit bug 235885) → LiveKit
E2EE can't work and would fail UNSAFE (media sent unencrypted while looking
fine); (2) **simulcast not negotiated in SDP**; (3) rice-proto ICE panic
crashes the WebProcess in default config. Real call.element.io joins reach the
in-call UI but never start ICE. Verdict: reject.

### ARCHITECTURE DECISION (2026-07-23): native MatrixRTC

Calls are implemented natively: `matrixrtc/` crate (MatrixRTC session,
membership + MSC4140 delayed leave, E2EE key distribution — ported from
matrix-js-sdk semantics with its test suite) + **livekit-rust SDK** (statically
linked libwebrtc, E2EE frame cryptors) + gtk4paintablesink for video. UX
follows the Cinny model (native chrome) and GNOME Calls/libcall-ui/Dino design
language; notifications via portal v2 call categories. No browser engine.

## Flatpak distribution + CI (tuna-os pattern, researched 2026-07-23)

Org convention (from Tavern / gtk-office-suite / flatpak-index clones): flatpaks
ship as **OCI images on GHCR** + static JSON index in `tuna-os/docs` served at
tunaos.org (`oci+https` remote) — no OSTree hosting, no GPG, no R2 (R2 is for
ISO repos only), all on GitHub-hosted runners. Setup for Mandelbrot:

* Manifest `org.tunaos.mandelbrot.json` at repo root, derived from
  `build-aux/org.gnome.Fractal.Devel.json`: runtime pinned `"50"` (not master),
  rust-stable sdk-extension + `--share=network` (org tolerates network builds;
  no cargo vendoring needed), keep grass/protobuf-c/libshumate modules.

* `.github/workflows/ci.yml`: checks job reusing Fractal's own
  `hooks/checks` tool (fmt/typos/cargo-deny/machete/POTFILES); clippy+tests in
  the flatpak sandbox (upstream `rust-tests` pattern, `"run-tests": true`);
  flatpak build job on `ghcr.io/flathub-infra/flatpak-github-actions:gnome-50`
  (Tavern `flatpak.yml`); optional metainfo lint + xvfb/at-spi GUI smoke
  (gtk-office-suite `gui-tests.yml`).

* `publish-flatpak.yml` copied from Tavern (build OCI → skopeo to
  `ghcr.io/tuna-os/mandelbrot:latest` → `update-index.py` pushes to
  `tuna-os/docs`); optional `promote-to-prod.yml` for the main→prod model.

* One secret: `FLATPAK_INDEX_TOKEN` (PAT that pushes to tuna-os/docs); GHCR
  uses `GITHUB_TOKEN`. First publish auto-registers in the index.

* Expect 30–60 min cold flatpak builds (matrix-sdk); mold + flatpak-builder
  cache mitigate. x86_64 only initially.

## MatrixRTC conformance testing (strategy, researched 2026-07-23)

No official conformance suite exists for MSC4143/MSC4195/MSC4140; the de-facto
spec is **matrix-js-sdk's `spec/unit/matrixrtc/` suite** (~4200 lines, pure
logic, zero DOM/WebRTC deps — mocks 5 client methods; highly portable to Rust
with `tokio::time::pause()`):

* `MembershipManager.spec.ts` — join/leave state machine, MSC4140 delayed-leave
  scheduling/rescheduling/keep-alive, 404→rejoin, rate-limit retry, fallback to
  plain state events, expiry extension.

* `RTCEncryptionManager.spec.ts` — key distribution: rotate on leave (delayed
  rollout), no rotation within grace period, re-distribute on `created_ts`
  change, out-of-order key handling.

* `MatrixRTCSession.spec.ts` — session from room state, membership filtering,
  oldest-membership foci selection, `m.rtc.notification` race rules.

* `CallMembership.spec.ts` / `ToDeviceKeyTransport.spec.ts` /
  `MembershipData.spec.ts` (identity-hash golden vector).
Plan: (a) port ~20 named tests (list in research notes) as the test suite of a
new `matrixrtc` module, mocking an `RtcClientApi` trait; (b) wire-level golden
tests against the event templates in `spec/unit/matrixrtc/mocks.ts`;
(c) interop e2e reusing **element-call's `docker-compose-dev.yml` +
`backend/` stack verbatim** (synapse ×2 federated + livekit ×2 + lk-jwt +
nginx TLS at `m.localhost`): headless Element Call joins the same room as
Mandelbrot; assert mutual membership, decrypted media both ways, and
delayed-leave cleanup after ungraceful kill (modeled on
`playwright/spa-call-sticky.spec.ts` and `widget/hotswap-legacy-compat.test.ts`
for the legacy/compat/2_0 matrix). Complement only covers MSC4140
homeserver-side (`tests/msc4140/delayed_event_test.go`) — read as reference,
not reusable for clients.
Format target: **`m.call.member` + `SessionMembershipData` first** (what
deployed Element Call interops with; ruma already types it as
`CallMemberEventContent` — reuse ruma types instead of hand-validating), with
the membership manager behind a trait so MSC4354 sticky `m.rtc.member` can slot
in later (mirrors js-sdk's `IMembershipManager`/`StickyEventMembershipManager`
split). matrix-rust-sdk has no MatrixRTC session logic to reuse (only widget
passthrough + `m.call.member` room-state detection).

## Landmines

* `matrix-sdk-ui` is officially "experimental" — expect API churn each SDK
  release (Fractal already lives with this for Timeline).

* QR login flaky with some MAS versions (MAS #5601, rendezvous 404s); watch MSC4388.

* Self-hosted servers often lack LiveKit foci / MSC4140 → calls must degrade
  gracefully (feature-detect via `.well-known`).

* Widget must survive room switches (gomuks solved this with pop-out/persistent
  widget; plan the GTK widget lifecycle accordingly).
