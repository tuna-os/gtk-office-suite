# Mandelbrot features

Mandelbrot keeps every Fractal feature and adds the ones below. Features marked
**experimental** are new in Mandelbrot and still being validated against live
homeservers — please file issues.

## Native voice & video calls (experimental)

Mandelbrot implements MatrixRTC calling natively — no embedded browser. The
implementation lives in the `mandelbrot-matrixrtc` crate and follows the same
semantics as Element Call (its conformance suite is ported from matrix-js-sdk:
114+ tests run in CI, plus a live interop harness under `tests/e2e/`).

**Interop with Element Call is verified.** Against Element Call v0.22.0 on a
live stack, both clients write the same membership format, read each other,
and connect calls in either direction, with crash cleanup working across
implementations — see [tests/e2e/CONFORMANCE.md](../tests/e2e/CONFORMANCE.md)
for the full report, including the two known gaps (media encryption with
Element Call is expected to work but is not yet proven end to end, and Element
Call's experimental MSC4354 "matrix 2.0" mode is not supported yet). The user guide, with
requirements, limitations, and troubleshooting, is [CALLING.md](CALLING.md).

* **Join a call**: rooms with an active call, MSC3417 video rooms, and rooms where you are allowed
  to start a call show a camera button in the room header. It opens a pre-join screen (mic/camera
  toggles) and then the call view.

* **Call view**: participant grid with speaking highlights, your own preview in
  the corner, auto-hiding controls (mute, camera, audio output, hang up).
  Closing the view keeps the call running; a compact call bar above the room
  timeline returns you to it.

* **Incoming calls**: urgent system notifications with Accept/Decline, using
  the freedesktop notification portal v2 `call.incoming` category (fully
  supported on Phosh; GNOME Shell support is landing across releases —
  elsewhere they degrade to normal notifications with action buttons).

* **Encryption**: calls are end-to-end encrypted. Per-participant media keys
  are distributed over Olm-encrypted to-device messages and rotated when
  someone leaves (grace periods prevent rotation storms on joins).

* **Reliability**: if the app crashes or loses connection mid-call, an
  MSC4140 delayed event automatically removes your call membership after a
  short window, so you never appear stuck in a call.

* **Camera**: the camera toggle captures your camera through the XDG camera
  portal (so it works in the Flatpak sandbox, and the camera is only opened
  while the toggle is on), and publishes it to the call. The same stream feeds
  the preview on the pre-join screen and the self-view during the call.

* **Requirements**: the homeserver must advertise a LiveKit SFU
  (`org.matrix.msc4143.rtc_foci` in `.well-known`, e.g. matrix.org) and
  support MSC4140 delayed events (Synapse ≥ 1.114 with the feature enabled).

* Spec surface: MSC4143 (MatrixRTC), MSC3401 (`m.call.member`), MSC4195
  (LiveKit focus), MSC4140 (delayed events), MSC4075 (call notifications).

## Simplified sliding sync (MSC4186)

On homeservers that support it (Synapse ≥ 1.114 by default), Mandelbrot uses
the SDK's `SyncService` — much faster initial sync and a room list that loads
instantly. On servers without it, Mandelbrot automatically falls back to
classic `/sync` (and even falls back at runtime if a proxy starts rejecting
sliding sync mid-session). The active mode is logged at startup.

## QR code login (MSC4108)

Both directions, on OAuth 2.0 homeservers (matrix.org and anything running
matrix-authentication-service):

* **Sign in with QR code** (login screen): scan the code shown by your
  existing device, compare the two-digit check code, and the new session is
  set up with encryption transferred — no password typing.

* **Link another device** (Account Settings → Sessions → "Link New Device…"):
  shows the QR code for another device to scan, with check-code confirmation.

## Threads (MSC3440)

* Messages that start a thread show a "N replies in thread" button; it opens
  the thread in a side panel (an overlay on narrow windows).
* The thread panel has its own composer — replies automatically carry the
  `m.thread` relation — plus thread-scoped reply, edit, reactions, and
  per-thread drafts.
* "Reply in Thread" in any message's context menu starts a new thread.
* Thread replies are hidden from the main timeline (Element-style).
* Not yet: threaded read receipts (MSC3771); thread activity does not mark a
  room unread.

## Polls (MSC3381)

* Create polls from the composer's attach menu: question, 2–20 answers, and a
  choice of showing results while the poll is ongoing (disclosed) or only at
  the end (undisclosed).

* Vote by clicking an answer; disclosed polls show live counts and proportional
  bars after you vote; ended polls highlight the winning answer(s).

* Poll creators (and moderators with redact power) can end a poll from the
  message context menu.

## Voice messages (MSC3245)

* With an empty composer, press the microphone button to record (Ogg Opus,
  up to 30 minutes) with live elapsed time and level meter; send or cancel.

* Sent messages carry the voice-message flag, duration, and waveform, so
  Element and other clients render them as voice messages. Playback of
  received voice messages was already supported.

## Spaces

* **Space switcher**: once you have joined a space, a rail on the left of the
  room list shows "Home" and your top-level spaces, in the order other clients
  set (`m.space_order`, MSC3230). On narrow windows the rail becomes an overlay:
  open it with the "Show Spaces" button in the header bar or by swiping from
  the left edge.

* **Per-space room list**: selecting a space shows only the rooms of that
  space and of all its subspaces, still grouped into Invited, Favorites,
  Rooms, and so on. "Home" shows every room. The room you have open stays
  open when you switch spaces, and search always covers every room.

* **Subspaces**: inside a space, its subspaces are listed in the "Spaces"
  section. Activating one opens it in the room list, with a back button to
  the parent space.

* **Unread badges**: each space in the rail adds up the unread state of its
  rooms, subspaces included: the notification count (in the accent colour
  when you were mentioned), or a dot for unread messages. Low-priority rooms
  are not counted.

* **Space overview**: the space name above the room list opens the space's
  overview. It lists the space's rooms and subspaces, including the ones you
  have not joined, with suggested rooms marked, and lets you join or knock.

* **Keyboard**: `Ctrl+Alt+PgDn` / `Ctrl+Alt+PgUp` for the next / previous
  space, `Ctrl+Alt+Home` for Home.

* The selected space is remembered per account. Design notes and a comparison
  with other clients:
  [design/spaces-navigation.md](design/spaces-navigation.md).

## Under the hood

* `matrixrtc/` — the MatrixRTC engine crate (membership state machine, E2EE
  key distribution, call session, LiveKit connection behind the `livekit`
  feature). Its test suite is ported from matrix-js-sdk's matrixrtc suite and
  runs as the "MatrixRTC conformance tests" CI job.

* `tests/e2e/` — a podman-compose interop harness (synapse + LiveKit +
  lk-jwt-service) that runs real calls between clients; see
  [tests/e2e/README.md](../tests/e2e/README.md) and
  [tests/e2e/CONFORMANCE.md](../tests/e2e/CONFORMANCE.md).

* Media (`calls-media` cargo feature / `-Dcalls-media` meson option) links
  LiveKit's Rust SDK with a statically-built libwebrtc; the default build
  stays lean without it. Flatpak builds enable it.
