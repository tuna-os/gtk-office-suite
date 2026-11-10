# Voice and video calls

Mandelbrot has native MatrixRTC voice and video calling: calls run inside the app, with a GNOME
call UI, and no embedded browser. Calling is **experimental** — it works against live
homeservers and interoperates with Element Call, but it is new code and still has the gaps listed
under [Known limitations](#known-limitations). Please
[file issues](https://github.com/tuna-os/mandelbrot/issues) for anything that does not work.

For the implementation side, see [ARCHITECTURE.md](ARCHITECTURE.md#matrixrtc-calling).

## Requirements

Calls need support from your homeserver, not just from the app:

* **A LiveKit SFU advertised by your homeserver.** Mandelbrot reads the
  `org.matrix.msc4143.rtc_foci` list from `https://<your server name>/.well-known/matrix/client`
  (the server name is the part of your Matrix ID after the `:`). The entry must be a `livekit`
  focus whose `livekit_service_url` points at an MSC4195 `lk-jwt-service`. matrix.org advertises
  one. If your server advertises none, Mandelbrot can still join a call that someone else started,
  using the SFU recorded in their call membership — but it cannot start a call on its own.
* **MSC4140 delayed events** (for example, Synapse ≥ 1.114 with the feature enabled). Delayed
  events let the homeserver remove your call membership automatically if Mandelbrot crashes or
  loses its connection. Without them Mandelbrot falls back to plain membership state events, which
  only go away when you leave cleanly or when the membership expires (Mandelbrot sets a 4 hour
  expiry).
* **Permission to send `m.call.member` state events** in the room. Starting a call and joining one
  both publish your call membership as that state event, so your power level must allow it.
* **Network access to the SFU**: the WebRTC media goes to the LiveKit server, not to your
  homeserver. Restrictive firewalls that block the SFU's ports will stop media even when the Matrix
  side of the call works.
* **The Flatpak build** (or a build with the `calls-media` option, see
  [DEVELOPMENT.md](DEVELOPMENT.md#native-call-media)). Builds without it can show calls but have no
  media connection.

## Making and joining a call

Every Matrix call is a call _in a room_: a one-to-one call is a call in your direct-message room
with that person, and a group call is a call in a group room. There is no separate "dial a user"
flow.

1. Open the room. The **Join Call** button (a camera icon) appears in the room header when:
   * a call is already active in the room,
   * the room is a video room, or
   * you have joined the room and are allowed to start a call.
2. Click it. A pre-join screen opens with microphone and camera toggles and a preview of your
   camera if it is on.
3. Join. The call view shows a grid of participants with speaking highlights and your own preview
   in the corner. The controls (mute, camera, audio output, leave) hide automatically and come back
   when you move the pointer.
4. Closing the call view does **not** leave the call. A compact call bar above the room timeline
   brings you back to it. Use the **Leave Call** button to hang up.

The connection status is shown in the call view: _Connecting…_, _Reconnecting…_ (your call
membership on the homeserver is being re-established), or _Connection failed_. Mandelbrot only
publishes your call membership after it has connected to the SFU, so other participants never see
you "in" a call whose media you could not reach. If the SFU connection drops mid-call, Mandelbrot
retries it with a growing delay (up to 30 seconds) for as long as you are in the call.

### Incoming calls

When someone starts a call and their client sends a call notification (MSC4075
`m.rtc.notification`), Mandelbrot shows an urgent desktop notification — "Incoming call" or
"Incoming video call" — with **Accept** and **Decline** buttons. It uses the freedesktop
notification portal v2 `call.incoming` category: fully supported on Phosh, landing across GNOME
Shell releases, and shown as a normal notification with action buttons elsewhere.

Whether the notification arrives at all depends on your homeserver's push rules matching the
`m.rtc.notification` event. Mandelbrot does not ring through its own system; if the server does not
flag the event as notifying, there is nothing to show.

## Audio and video

* **Microphone**: Mandelbrot captures the system default audio input. There is no in-app input
  device picker yet; choose the input device in your system sound settings (GNOME Settings →
  Sound) before joining. If the microphone cannot be opened, you still join the call, without
  publishing audio.
* **Audio output**: the **Audio Output** menu in the call view currently only offers _System
  Default_. Choose the output device in your system sound settings.
* **Camera**: the camera toggle opens the camera through the XDG camera portal, so the first use
  asks for permission. The camera is only open while the toggle is on; turning it off, dismissing
  the pre-join screen, or leaving the call closes it. If the camera fails to start, the toggle
  switches itself back off.

The Flatpak has no direct device access to cameras: it relies on the camera portal and PipeWire.
If no camera shows up, check that your desktop's portal backend (for example
`xdg-desktop-portal-gnome`) and PipeWire are running, and that camera access for Mandelbrot is
allowed in GNOME Settings → Apps → Mandelbrot.

## Security and encryption

Call media is end-to-end encrypted:

* Each participant has its own media key. Mandelbrot sends its key to the other participants'
  devices as `io.element.call.encryption_keys` to-device messages, Olm-encrypted with the same
  device encryption used for messages.
* The existing key is shared with new joiners, and a new key is rotated in when someone leaves, so
  a participant who left cannot decrypt what follows.
* The LiveKit SFU forwards media it cannot decrypt.

The SFU and the `lk-jwt-service` still learn metadata: who is connected to which call, when, from
which IP address, and how much media flows. See [PRIVACY.md](PRIVACY.md#calls) for the wider
picture.

## Interoperability

Mandelbrot implements the same MatrixRTC semantics as Element Call; its session and membership
logic is a port of matrix-js-sdk's `matrixrtc` module with the conformance suite ported along with
it. Against **Element Call v0.22.0** on a live stack (see
[tests/e2e/CONFORMANCE.md](../tests/e2e/CONFORMANCE.md)):

* both clients write the same `m.call.member` membership format and read each other's,
* calls connect in either direction,
* crash cleanup through delayed events works across the two implementations.

Known interop gaps:

* End-to-end encrypted _media_ with Element Call is expected to work but has not yet been proven
  end to end.
* Element Call's experimental MSC4354 "matrix 2.0" mode (sticky timeline memberships) is not
  supported: Mandelbrot cannot see Element Call participants running in that mode.

Other MatrixRTC clients have not been tested.

## Known limitations

* **Experimental status**: calling is marked experimental in [FEATURES.md](FEATURES.md).
* **Mandelbrot does not ring**: starting a call does not send an `m.rtc.notification`, so the
  other side is not alerted. Tell them, or start the call from a client that rings.
* **No device pickers**: input and output follow the system defaults (see
  [Audio and video](#audio-and-video)).
* **No calls without a LiveKit focus**: homeservers that advertise no `rtc_foci` cannot host a
  call that Mandelbrot starts.
* **Live-stack test status**: the per-PR `MatrixRTC conformance tests` CI job covers the session
  logic; the nightly live-stack harness (`.github/workflows/matrixrtc-e2e.yml`) is tracked in
  [#66](https://github.com/tuna-os/mandelbrot/issues/66).

## Troubleshooting calls

Start Mandelbrot from a terminal to see its log (see
[TROUBLESHOOTING.md](TROUBLESHOOTING.md#logs-and-crash-reports)):

```sh
flatpak run --env=RUST_LOG=fractal=debug,mandelbrot_matrixrtc=debug,warn org.tunaos.mandelbrot
```

| Symptom | Likely cause and what to check |
| --- | --- |
| No **Join Call** button | No call is active and you lack permission to start one, or you have not joined the room. |
| _Connection failed_ right away, log says `no LiveKit focus available for this call` | Your homeserver's `.well-known/matrix/client` has no `org.matrix.msc4143.rtc_foci` LiveKit entry, and nobody else is in the call. Ask your server admin. |
| _Connection failed_, log says `failed to fetch OpenID token` or `failed to fetch the SFU configuration` | The homeserver refused to issue an OpenID token, or the `lk-jwt-service` rejected it. Both are server-side. |
| _Connection failed_, log says `failed to connect to the SFU` | The SFU itself is unreachable from your network (firewall, VPN, proxy). |
| Repeated _Reconnecting…_ | The calling engine thinks your call membership on the homeserver may have been lost and is re-establishing it, for example after a network interruption. Check the offline banner and your network. |
| _Connection failed_, log says `Call membership manager failed` | The homeserver rejected your call membership (for example, insufficient power level to send `m.call.member`). |
| Others cannot see your video | The camera portal denied access, or the toggle switched back off; look for `Could not start the camera` in the log. |
| Others cannot hear you | Check the mute toggle and the system default input; look for `Failed to publish the microphone`. |
| You still appear in a call after a crash | Your server lacks MSC4140 delayed events; the membership remains until it expires. |
| No incoming-call notification | The caller's client did not send `m.rtc.notification`, or your homeserver's push rules did not flag it. Notifications for Mandelbrot must also be enabled in GNOME Settings. |

When filing a call bug, include your homeserver software and version, the other participants'
clients, and the log from the terminal.
