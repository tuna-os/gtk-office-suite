# Decision record: no SDK widget-driver fallback for calls

- Status: proposed (issue #1972, part of #1962)
- Compared against: `matrix-sdk` 0.19.1, the version `Cargo.lock` pins.

## Question

Should Mandelbrot add `matrix_sdk::widget` (`WidgetDriver`,
`WidgetSettings::new_virtual_element_call_widget`, behind the
`experimental-widgets` feature) as a fallback for rooms whose homeserver
publishes no LiveKit focus in `.well-known` (`org.matrix.msc4143.rtc_foci`)?

## Decision

No. Rooms with no LiveKit focus keep failing with the message
`docs/CALLING.md` already documents. The native `matrixrtc/` engine stays the
only call path.

## Reasons

1. **The driver is only half of a widget call.** `matrix_sdk::widget` is the
   Matrix side of the widget `postMessage` API. It needs a web view that loads
   Element Call and relays messages in both directions. The SDK provides no web
   view. Mandelbrot would need WebKitGTK plus a JS bridge, which breaks the
   "no embedded browser" line in `README.md`, `docs/FEATURES.md` and
   `docs/CALLING.md`. It would also bring the web view's attack surface and
   Flatpak permissions into an app that has none today.
2. **It would not fix the rooms in question.** Element Call places calls over a
   LiveKit SFU that it finds through the same MSC4143 foci (or through its own
   deployment configuration). A homeserver that advertises none leaves the
   widget with nothing to connect to, unless the app ships or points at a
   third-party SFU. That is a hosting and trust decision, not a client fix,
   and the fallback would be a second, heavier route to the same dead end.
3. **Two stacks to keep correct.** Calls would have two membership writers,
   two key-distribution paths and two UIs, with different bugs and different
   interop behaviour. The native engine is the product's differentiator and
   the one Mandelbrot tests against Element Call (`docs/CALLING.md`, interop
   section).

The alternative the issue names, saying plainly that these rooms cannot call,
is what the app does today. A homeserver admin fixes it by publishing a focus;
the troubleshooting table in `docs/CALLING.md` already says so.

## Revisit if

- the SDK ships a native (non-web) MatrixRTC session API that covers
  membership and key distribution, or
- Element Call gains a supported non-web embedding, or
- a real set of users sits on homeservers with no foci and has an SFU
  Mandelbrot can use.

## SDK overlap with `matrixrtc/` (0.19.1)

| `matrixrtc/` piece | In the SDK 0.19.1? |
|---|---|
| Membership events (`call_membership.rs`, `membership_data.rs`) | No session or membership state machine. The SDK uses ruma's `m.call.member` content types, as `matrixrtc/` does. `Client::enable_automatic_call_status` (MSC4426 `m.call` profile field) is a different feature. |
| MSC4140 delayed leave and keep-alive (`membership_manager.rs`) | Only inside the widget driver, which forwards a widget's delayed-event requests. No native API. |
| Key distribution (`encryption_manager.rs`, `key_transport.rs`) | No. The SDK only offers Olm-encrypted custom to-device sends (`experimental-send-custom-to-device`), which the engine already uses. |
| Focus discovery (`src/session/call/mod.rs`, `fetch_preferred_foci`) | **Yes.** `Client::discover_rtc_transports` (cached, uses `GET /_matrix/client/v1/rtc/transports`) and `Client::well_known_rtc_transports` replace the hand-written `.well-known` fetch. That fetch also skips the homeserver base URL the client resolved at login and assumes `https://<server name>`. |

Only the last row is a real duplicate. It is filed as a follow-up: replace
`fetch_preferred_foci` with `discover_rtc_transports`, keeping the
`.well-known` result as the fallback the SDK already provides.
