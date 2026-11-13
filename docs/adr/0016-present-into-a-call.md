# ADR 0016: Presenting a deck into a call

Date: 2026-11-13 · Status: **accepted** (decided on the owner's standing instruction to decide open questions; decision 2, that Decks never captures itself, is the part to review) · Relates to: [INTEGRATION-PLAN](../INTEGRATION-PLAN.md) A5, [ADR 0013](0013-one-matrix-account.md), [ADR 0014](0014-suite-shared-services-and-search.md)

## Context

A5 is "Decks presents to a call as a screen share". Mandelbrot holds the call:
its MatrixRTC engine (`mandelbrot/matrixrtc`) joins a LiveKit SFU and already
publishes a microphone track and a camera track, each fed from a source the
app fills (`mandelbrot/src/session/call/media.rs`: GStreamer captures, frames go
into a `NativeVideoSource`). It has no screen share. Decks has a presenter
window and a full-screen slide window (`decks/src/presenter_window.rs`); it
knows nothing of calls and holds no Matrix account (ADR 0013: there is one,
and Mandelbrot owns it).

The question is who captures the slides and how they reach the call.

## Decisions

1. **Mandelbrot shares; the slides reach it as a screen share.** The call
   gains "Share Screen", which publishes a LiveKit track with the
   `Screenshare` source (`LivekitCallConnection::publish_screen_share_track`,
   added with this ADR) so other clients show it as a presentation, not as
   a second camera. A deck is one thing that can be shared; a spreadsheet or
   a browser is another, and the same button serves them.
2. **The capture goes through the ScreenCast portal, and the person picks
   what is shared.** Mandelbrot asks `org.freedesktop.portal.ScreenCast` for a
   window or monitor, and the desktop's own picker shows the choice. Decks never
   captures itself and never hands frames to Mandelbrot directly. A
   private channel between two apps would bypass the one consent dialog the desktop
   guarantees; inside Flatpak it would also need a grant for each pair of apps.
   The portal gives a PipeWire stream, which GStreamer's `pipewiresrc` reads.
   That is the same capture path as the camera, so it becomes the same
   `NativeVideoSource` feeding.
3. **What to pick is the slide window, not the presenter window.** The
   presenter window holds notes and the next slide; sharing it would show
   them to the call. Decks names its full-screen window "<deck> — Slide Show"
   so it is the obvious entry in the picker, and while presenting on one display
   it says, once, that the presenter view is not what to share.
4. **Decks' "Present in Call" is a shortcut, not a second path.** Once
   Mandelbrot has Share Screen, Decks may offer "Present in Call…" when
   Mandelbrot is running. It starts the slideshow, then activates
   Mandelbrot's `app.share-screen` action over the session bus
   (`org.freedesktop.Application.ActivateAction`), which opens the same
   portal picker. Inside Flatpak that needs `--talk-name=org.tunaos.mandelbrot`
   in Decks' manifest, and only that. When the call is not running, the item
   is not shown.
5. **Audio from the deck (a video's sound) is not shared** in the first
   version. `ScreenshareAudio` exists in LiveKit, but the portal gives no
   per-window audio, and sharing a monitor's sound would share everything
   the person hears.

## Order of work

1. The engine can publish a screen share track (this ADR's change).
2. Mandelbrot: Share Screen in the call view, ScreenCast portal →
   `pipewiresrc` → `NativeVideoSource`, stop on the portal session closing
   or the button.
3. Decks: the slide window's title, and "Present in Call…" behind the bus
   action.
4. A journey: two Mandelbrot accounts in a call on the test homeserver,
   one sharing a Decks slide window, the other receiving a `Screenshare`
   track whose frames change when the slide advances.

## Consequences

- Nothing in Decks depends on Matrix or LiveKit; the only coupling is one
  bus action name, and the ADR names it so both sides keep it.
- The person always sees and approves what leaves their machine, through the
  desktop's picker, as with every other screen share on GNOME.
