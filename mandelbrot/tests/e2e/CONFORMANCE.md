# MatrixRTC conformance report

Results of running Mandelbrot's native MatrixRTC implementation against a live
stack and against Element Call. Re-run with `run-interop.sh` (client ↔ client)
and `run-interop-ec.sh <scenario> <mode>` (client ↔ Element Call).

Last run: 2026-07-24, against Element Call **v0.22.0** (`ae7ede32`, built
2026-07-20, `ghcr.io/element-hq/element-call:latest`) on synapse + livekit +
lk-jwt-service.

## Summary

Mandelbrot interoperates with deployed Element Call. Membership events are
shape-identical, both clients read each other's memberships, calls connect in
both directions, and crash cleanup works across implementations. The one
unsupported surface is the future MSC4354 sticky-event mode, which is a known
deferred item, not a regression.

## The state-key format question — resolved

Element Call v0.22.0 writes the **same device-keyed slot format we emit**:

```text
type      org.matrix.msc3401.call.member
state_key _{user_id}_{device_id}_m.call
```

The historical user-keyed format (`@user:server` with a `memberships[]` array)
could not be produced at all: even with `matrix_rtc_mode: "legacy"` _and_
`feature_use_device_session_member_events: false` (config verified as served),
EC still wrote the device-keyed key. It is not reachable by configuration in
this release.

**Conclusion:** keep emitting `m.call.member` with `SessionMembershipData` and
the device-keyed state key. Reading that format alone is sufficient for
deployed Element Call today.

## Scenario matrix

| Element Call config | EC writes | we read EC | EC reads us | result |
|---|---|---|---|---|
| default, EC joins first | device-keyed, focus `multi_sfu` | yes | yes | 8/8 pass |
| default, we join first | same | yes | yes | 7/7 pass |
| default, encrypted room, we join first | same | yes | yes | 7/7 pass |
| `legacy` + device-events off | device-keyed, focus `oldest_membership` | yes | yes | 8/8 pass |
| `matrix_2_0` | sticky timeline `org.matrix.msc4143.rtc.member` | **no** | yes | 2 fail / 6 pass |

Delayed-leave (MSC4140) interop passed in every mode: `kill -9` on our client
removed our tile from Element Call within 7–10 s; rejoining restored it; a
graceful SIGINT removed it immediately.

## Wire-level differences

Identical between implementations: `application`, `call_id`, `device_id`,
`foci_preferred[]` (type, alias, service URL), `membershipID`, `scope`, and the
absence of `created_ts`. `expires` differs only by configuration (ours 4 h,
EC's 50 h).

**One content-level divergence:** `focus_active.focus_selection`. We always
send `oldest_membership`; EC's default mode sends `multi_sfu` (its legacy mode
sends `oldest_membership`, matching us). This was harmless in every run —
consider aligning per mode.

## Media E2EE: not proven by this harness (harness limitation)

Our client logged `MissingKey` for every Element Call track, in every config
including an encrypted room. The cause was established directly rather than
inferred: Element Call made **zero `/sendToDevice/` calls**, because
`/keys/query` showed our harness client had published **no device keys** —
the `join_call` example is a bare HTTP client with no Olm stack, so EC found no
Olm-capable device and skipped key distribution entirely. The failure is
upstream of any decryption logic.

Element Call's actual requirement is only that a peer device has published
device keys; no verification or cross-signing step was reached. The Mandelbrot
app satisfies this — matrix-sdk publishes device keys and
`src/session/call/client_api.rs` sends keys with
`encrypt_and_send_raw_to_device(..., CollectStrategy::AllDevices)`. So
app-to-EC media E2EE is expected to work but **remains unproven**; proving it
requires driving the real app rather than the example client.

Element Call rendered our participant correctly (padlock, participant count 2,
audio-only avatar tile — correct, as we publish a silent microphone and no
camera).

## MSC4354 sticky events (`matrix_2_0`) — the real compatibility gap

In this mode EC sends a sticky **timeline** event instead of room state:

```text
PUT /rooms/{room}/send/org.matrix.msc4143.rtc.member/{txn}
    ?org.matrix.msc4140.delay=8000&org.matrix.msc4354.sticky_duration_ms=3600000
{"application":{"type":"m.call"}, "slot_id":"m.call#ROOM",
 "rtc_transports":[{"type":"livekit","livekit_service_url":"..."}],
 "member":{"device_id":"...","user_id":"...","id":"<uuid>"},
 "versions":[], "msc4354_sticky_key":"<uuid>"}
```

It fetches the SFU JWT from `/get_token` only (never `/sfu/get`) and uses
**hashed LiveKit identities**. EC still renders us (it reads legacy state for
back-compat) and we still reach the same SFU room and subscribe to its tracks,
but the hashed identity means our `{user}:{device}` key mapping would not match
even with keys present.

This — not the legacy user-keyed format — is the compatibility work that
matters next. The membership manager is already behind a trait so the sticky
variant can be added alongside.

## Open items

* In the `matrix_2_0` run our membership manager flapped `ProbablyLeft`
  true/false repeatedly. Most likely the 2 s local restart timeout under a
  loaded synapse, but worth confirming it is not a state-machine bug.
* Prove app-level media E2EE against Element Call by driving the real app.
* Consider matching `focus_selection` to the peer mode.
