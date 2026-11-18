# Architecture

A map of the Mandelbrot code base for contributors. Mandelbrot is a downstream fork of
[GNOME Fractal](https://gitlab.gnome.org/World/fractal): most of the tree is still Fractal's Rust,
GTK 4, and libadwaita client, and [UPSTREAM.md](UPSTREAM.md) lists exactly what Mandelbrot adds.
Treat code that is not listed there as inherited. For building and testing, see
[DEVELOPMENT.md](DEVELOPMENT.md).

## Overview

```text
Crates in this repository
├── fractal (Cargo.toml, the app)        GTK 4 / libadwaita UI on matrix-sdk + matrix-sdk-ui
│   └── depends on mandelbrot-matrixrtc  path dependency, `livekit` feature via `calls-media`
├── matrixrtc/  (mandelbrot-matrixrtc)   MatrixRTC session, membership and E2EE key logic
└── hooks/checks                          the pre-commit / CI check runner (separate crate)
```

The app crate, the Meson project, and the installed binary are all still named `fractal`; the
application ID, the user-visible name, and the Flatpak are `org.tunaos.mandelbrot` / Mandelbrot.

Key dependencies (see `Cargo.toml`): `matrix-sdk`, `matrix-sdk-ui` and `matrix-sdk-base` 0.19,
`ruma` 0.17 (with the MatrixRTC `unstable-msc3401` and `unstable-msc4140` features), `gtk4`,
`libadwaita`, GStreamer for media, `ashpd` for XDG portals, and `oo7` for secret storage.

## Runtime model

`src/main.rs` sets up `tracing`, gettext, and the GResources, then runs the GTK application.
Two executors run side by side:

* the GLib main loop, which owns every GTK object; `spawn!` (in `src/utils/macros.rs`) spawns a
  future on it;
* a shared multi-threaded tokio runtime (`RUNTIME` in `src/main.rs`) for the Matrix SDK and other
  network or blocking work; `spawn_tokio!` spawns on it and returns a handle the main loop can
  await.

The usual pattern is a `spawn!` future on the main loop that awaits a `spawn_tokio!` handle and
then updates GObject properties, which the Blueprint UI binds to.

## Source layout

| Path | Responsibility |
| --- | --- |
| `src/main.rs`, `src/application.rs`, `src/window.rs` | Entry point, `AdwApplication` subclass (actions, profiles), main window. |
| `src/session_list/` | The logged-in sessions: restoring them at startup, new and failed sessions. |
| `src/secret/` | Storing session secrets in the Secret Service or Secret portal (`oo7`), and the per-session data and cache directories. |
| `src/login/` | Login flow: homeserver discovery, password/SSO/OAuth (`in_browser_page`, `local_server`), QR code login (`qr_code_page`). |
| `src/session/` | The model of one account: the `Session` object, sync (`sync.rs`), rooms, calls, notifications, verification, and the account's other sessions. |
| `src/session/room/` | The `Room` object, its members, permissions, and the timeline wrapper over `matrix-sdk-ui`'s `Timeline` (`timeline/`). |
| `src/session/room_list/`, `src/session/sidebar_data/` | Room list and sidebar models. |
| `src/session/space_hierarchy/` | Space hierarchy browsing. |
| `src/session/call/` | `CallManager`: native calls for a session (see [MatrixRTC calling](#matrixrtc-calling)). |
| `src/session/notifications/` | Desktop notifications, including incoming-call notifications. |
| `src/session_view/` | The UI of a logged-in session: sidebar, room history, room details, explore, space overview, dialogs, media viewer. |
| `src/session_view/room_history/` | The timeline view: `message_row/`, the composer in `message_toolbar/`, `thread_panel`, `call_row`. |
| `src/session_view/call/` | The call UI: pre-join screen, call view, participant tiles, call bar, driven by `CallState`. |
| `src/account_settings/` | Account settings pages (general, encryption, notifications, safety, sessions). |
| `src/identity_verification_view/` | Device and user verification flows. |
| `src/components/` | Reusable widgets: avatars, camera and QR scanner, crypto setup, dialogs, media, pills, rows. |
| `src/utils/` | Helpers: Matrix utilities, media (including `audio_recorder.rs`), location, strings, list models. |
| `src/system_settings/` | Desktop settings read through the Settings portal. |
| `src/intent.rs` | Intents a session handles once, such as showing a call from a notification. |
| `data/` | Desktop file, AppStream metainfo, GSettings schema, icons, stylesheet, other resources. |
| `po/` | Translations. |
| `build-aux/` | Development Flatpak manifest, Blueprint compile helper, upstream sync script. |
| `tests/e2e/` | MatrixRTC live-stack interop harness. |

## Where Mandelbrot features live

| Feature | Code |
| --- | --- |
| Native calls | `matrixrtc/`, `src/session/call/`, `src/session_view/call/`, `src/session_view/room_history/call_row.rs`, `src/session/notifications/` |
| Simplified sliding sync (MSC4186) | `src/session/sync.rs`, `is_sliding_sync_unsupported_error` in `src/session/mod.rs` |
| QR code login (MSC4108) | `src/login/qr_code_page.rs`, `src/components/camera/`, `src/account_settings/user_session/link_device_subpage.rs` |
| Threads (MSC3440) | `src/session_view/room_history/thread_panel.rs` |
| Polls (MSC3381) | `src/session_view/room_history/message_toolbar/create_poll_dialog.rs`, `src/session_view/room_history/message_row/poll.rs` |
| Voice message recording (MSC3245) | `src/utils/media/audio_recorder.rs` |
| Spaces navigation and hierarchy | `src/session/sidebar_data/space_{graph,list,item}.rs` (space graph, switcher, per-space filters), `src/session_view/sidebar/space_row.rs`, `src/session/space_hierarchy/`, `src/session_view/space_overview/`; design in [design/spaces-navigation.md](design/spaces-navigation.md) |
| Packaging | `org.tunaos.mandelbrot.json`, `build-aux/org.tunaos.mandelbrot.Devel.json`, `.github/workflows/publish-flatpak.yml` |

## UI layer

Widgets are GObject subclasses written in Rust with their templates in Blueprint (`.blp`) files
alongside. Meson compiles every file listed in `src/ui-blueprint-resources.in` to `.ui` with
`build-aux/compile-blueprints.sh` and bundles them into `ui-resources.gresource`; everything else
(icons, the SCSS stylesheet compiled with `grass` or `sass`, assets) goes into
`resources.gresource` from `data/resources/resources.gresource.xml`. Both are installed into the
package data directory and loaded at startup, which is why a host build has to be installed to
run. Models expose GObject properties and signals, and templates bind to them.

## Sync

`Session::sync` in `src/session/sync.rs` picks the sync method once per session start:

1. `detect_sync_mode` asks the homeserver for its supported versions. If it advertises MSC4186,
   the session uses `matrix-sdk-ui`'s `SyncService` and `RoomListService` (simplified sliding
   sync); otherwise it uses classic `/sync` with `SyncSettings`.
2. If the sync service later fails with an error that `is_sliding_sync_unsupported_error`
   recognises (for example `M_UNRECOGNIZED` from a proxy), the session stops sliding sync and
   switches to classic sync for the rest of its lifetime.

Both paths feed the same room list and timelines, so the rest of the app does not depend on which
one is active.

## MatrixRTC calling

### `mandelbrot-matrixrtc` crate

`matrixrtc/` is a Rust port of matrix-js-sdk's `matrixrtc` module. Its core does no I/O: the app
implements the `RtcClientApi` trait (`matrixrtc/src/client.rs`) on top of its Matrix client, and
feeds room state and to-device events into the engine.

| Module | Role |
| --- | --- |
| `membership_data.rs`, `call_membership.rs` | Wire format and validation of `m.call.member` (`org.matrix.msc3401.call.member`) session memberships. |
| `session.rs` | Builds the call's membership list from room state. |
| `membership_manager.rs` | Join/leave state machine for our own membership: MSC4140 delayed leave events with keep-alive, refreshing before expiry, rejoining if removed, and fallback to plain state events when the server has no delayed events. |
| `encryption_manager.rs`, `key_transport.rs`, `outdated_key_filter.rs` | Per-participant media keys: distribution over `io.element.call.encryption_keys` to-device messages, rotation on leave, and dropping out-of-order keys. |
| `call_session.rs` | `RtcCallSession`, which ties the above together (mirrors `MatrixRTCSession.joinRTCSession`). |
| `livekit_connection.rs` (feature `livekit`) | Fetches an SFU token from an MSC4195 `lk-jwt-service` and connects to LiveKit with end-to-end encryption, wiring the key ring into LiveKit's frame cryptor. |

Its tests in `matrixrtc/tests/` are ported from matrix-js-sdk, plus Element Call scenarios in
`element_call_scenarios.rs`; `matrixrtc/examples/join_call.rs` is the headless client the
`tests/e2e/` harness drives.

### App integration

* `src/session/call/client_api.rs` implements `RtcClientApi` over `matrix-sdk`. Key to-device
  messages are Olm-encrypted per device (`encrypt_and_send_raw_to_device`, using the SDK's
  `experimental-send-custom-to-device` feature).
* `src/session/call/mod.rs` holds the `CallManager`: one `RtcCallSession` per room with an
  ongoing call. It watches `m.call.member` state from sync, feeds decrypted key events into the
  engines, fetches the preferred LiveKit foci from the homeserver's `.well-known`
  (`org.matrix.msc4143.rtc_foci`), and exposes per-room call activity and a `CallState` to the UI.
* `src/session/call/media.rs` (feature `calls-media`) runs the media connection on tokio: it gets
  an OpenID token, fetches the SFU configuration, connects to LiveKit, and only then publishes our
  membership. It publishes the microphone (GStreamer `autoaudiosrc`), forwards remote video frames
  to the UI, and reconnects with exponential backoff while we remain in the call.
* `src/session/call/camera.rs` (feature `calls-media`) opens the camera through the camera portal
  (via `aperture`'s PipeWire device provider) and tees a GStreamer pipeline into the self-view and
  the published track.
* `src/session_view/call/` renders `CallState` (`state.rs`): participants, connection state,
  mute and camera toggles.

```text
Join Call button ─► CallManager::join_call ─► RtcCallSession (matrixrtc)
                                  │                 │ membership + MSC4140 delayed leave
                                  │                 ▼
                                  │            homeserver (m.call.member state,
                                  │            Olm to-device key messages)
                                  ▼
                       media::start (calls-media)
                         OpenID token ─► lk-jwt-service ─► LiveKit SFU (E2EE media)
                                  │
                                  ▼
                       CallState ─► src/session_view/call/ widgets
```

## Tests

| Layer | Location | CI |
| --- | --- | --- |
| App unit tests | `#[cfg(test)]` modules in `src/` | not run by `ci.yml` today |
| Metadata validation | `test()` targets in `data/meson.build` | `Flatpak build + tests` |
| MatrixRTC conformance | `matrixrtc/tests/` | `MatrixRTC conformance tests` |
| Live interop | `tests/e2e/` ([README](../tests/e2e/README.md), [CONFORMANCE](../tests/e2e/CONFORMANCE.md)) | `matrixrtc-e2e.yml` (nightly, manual) |

## Build system

Meson is the entry point: it checks the system dependencies, generates `src/config.rs` (app ID,
profile, version, paths) from `src/config.rs.in`, compiles the Blueprint and GResource bundles,
then calls `cargo build --locked` (`--release` outside the development profiles) with the target
directory inside the Meson build directory, and installs the binary and data. Cargo features are
driven by Meson options: `-Dcalls-media=true` adds `--features calls-media`. See
[DEVELOPMENT.md](DEVELOPMENT.md#build-options-and-profiles) for the options.

Flatpak builds use `org.tunaos.mandelbrot.json` (GNOME 51 runtime, used by CI and releases) or
`build-aux/org.tunaos.mandelbrot.Devel.json` (GNOME nightly). Releases are covered in
[RELEASING.md](../RELEASING.md).

## Dependencies and review

* Renovate (the repository root's `renovate.json`) opens dependency update pull requests.
* `cargo-deny` (`deny.toml`) checks advisories, bans, licenses, and sources, and cargo-machete
  flags unused dependencies; both run in the `hooks/checks` crate.
* The app binary is built with `--locked`, so `Cargo.lock` is what ships.
* `matrixrtc/` has its own `Cargo.lock`.
* Review is routed through `.github/CODEOWNERS`.

## Localization

Mandelbrot uses gettext with the `fractal` domain. Translatable source files are listed in
`po/POTFILES.in` (checked by `hooks/checks`); the `po/*.po` files are inherited from Fractal's
translations on GNOME's [Damned Lies](https://l10n.gnome.org/module/fractal/), and are not
edited by hand here. New Mandelbrot strings go through `gettext` like any other.
