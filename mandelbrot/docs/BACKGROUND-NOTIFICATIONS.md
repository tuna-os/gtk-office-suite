# Notifications while Mandelbrot is closed

Design note for #1966 (part of #1962; feeds B4/B5 in
`docs/INTEGRATION-PLAN.md`). It records the "check first" comparison, what
`NotificationClient` gives us, the E2EE limits, and what a UnifiedPush wake-up
path needs. Nothing here is built yet.

## Today

`Session::init_notifications` (`src/session/mod.rs`) calls
`Client::register_notification_handler`. The SDK runs its push rules over
events that arrive through sync and calls the handler only for events whose
actions say "notify". So notifications exist only while the app runs and
syncs. `Notifications::show_push` (`src/session/notifications/mod.rs`) then
draws the notification.

## Check first: does `NotificationClient` decide the same thing?

`show_push` makes no mention/DM decision of its own. It reads
`room.direct_member().is_some()` only to choose the body text (show the sender
name in group rooms). Whether to notify at all was already decided by the SDK's
push rules before the handler ran.

`NotificationClient::get_notification` (matrix-sdk-ui 0.19) evaluates the same
push rules and returns:

| `NotificationStatus` | What the app does |
|---|---|
| `Event(item)` | Show it. `item.actions`, `is_noisy`, `has_mention`, `is_direct_message_room`, `room_computed_display_name` replace the room lookups `show_push` does today. |
| `EventFilteredOut` | Push rules or an ignored sender said no. Show nothing. |
| `EventRedacted` | Show nothing. |
| `EventNotFound` | Show nothing, log it. |

So the two paths agree on *whether* to notify, because both use push rules.
Differences to handle when switching the closed-app path over:

- **No double notification.** While the app runs, the sync handler fires and
  a push may also arrive. Both must map to the same notification id. `show_push`
  already uses `{session_id}//{matrix uri incl. event id}` as the id, and GIO
  replaces a notification with the same id, so the rule is: always build the id
  from the event id, never from the source.
- **Invites and calls.** `show_push` also handles invites and call
  notifications through `AnySyncOrStrippedTimelineEvent`. `NotificationItem`
  carries `NotificationEvent::Invite` and `Timeline`, so the same body builders
  (`message_notification_body`, `incoming_call_notification_body`,
  `own_invite_notification_body`) can take the item's event unchanged.
- **Window-active suppression** (no notification for the open room) stays in
  the app; the SDK knows nothing about the UI.

Implementation plan: split `show_push` into a resolve step (room, sender, icon)
and a draw step that takes plain data (title, body, room uri, event id,
is_call). Both the sync handler and the `NotificationClient` path call the draw
step. The body builders are already GTK-free and have tests.

## Keeping keys current

`NotificationProcessSetup::SingleProcess { sync_service }` is the right mode:
a pushed event wakes the same process that owns the `SyncService`, whose
`EncryptionSyncService` keeps room keys current. `MultipleProcesses` (a
separate notification process) needs a cross-process store lock and is only
worth it if the wake-up runs as its own binary. Do not choose it for a
`--gapplication-service` background instance.

## E2EE limits (to publish in `PRIVACY.md` once built)

- A notification for an encrypted room is only readable when the room key is
  already in the store or arrives within the client's decryption deadline
  (`NotificationClientTimeouts`). Otherwise the SDK reports the event without
  a body; show "New message" with the room name, never a placeholder that looks
  like content.
- Keys are fetched through the encryption sync, which needs the device to be
  online and the session verified/backed up. A session that has lost its keys
  cannot preview old messages.
- The push gateway sees only room id and event id if the push format is
  `event_id_only`. Registering the pusher must use that format, or message
  text would reach the gateway.

## Background portal (B5)

`ashpd` 0.14 is already a dependency (`src/session/call/screen.rs` uses the
screencast portal). `ashpd::desktop::background::Background::request()` asks
the portal for permission to run with no window and optionally autostart.
Proposed behaviour:

1. A "Run in background" switch in preferences, off by default, matching
   Lifafa's setting (`lifafa/src/ui/preferences`).
2. Turning it on calls the portal; if denied, the switch returns to off with a
   toast. The result is stored in GSettings.
3. Closing the last window with the switch on hides rather than quits, and
   `Application::hold()` keeps the process (and the sync service) alive.
4. Autostart is requested with `--gapplication-service` so no window opens at
   login.

The Flatpak needs no extra permission for the portal. Verification needs a
real session (portal dialogs, GNOME's background-apps list); it cannot be
checked in CI.

## UnifiedPush: what a wake-up path with no polling needs

This is an evaluation only. Building it is a separate issue once the
decisions below are made.

1. **A distributor on the device** (ntfy, Sunup, a KDE/GNOME distributor
   speaking the UnifiedPush D-Bus API `org.unifiedpush.Distributor1`). The app
   registers an instance and receives an endpoint URL.
2. **A push gateway.** Matrix homeservers do not talk to UnifiedPush
   endpoints directly. They POST the Matrix Push Gateway API to a gateway URL
   set on the pusher. Matrix-to-UnifiedPush gateways exist (the public
   `matrix.gateway.unifiedpush.org` is the usual choice); it forwards the
   notification to the endpoint.
3. **Self-hosting the gateway** needs: a small HTTPS service reachable from the
   homeserver (a `/_matrix/push/v1/notify` route, TLS, a stable public name),
   the ability to reach the user's distributor endpoint, and nothing else
   stateful. Run it beside the homeserver. The cost is one more public service
   to keep patched.
4. **Pusher registration** through `Client::pusher().set(...)` with
   `PushFormat::EventIdOnly`, `kind: http`, the gateway URL, and the endpoint
   as `pushkey`. Remove it on logout.
5. **Wake-up.** The distributor activates Mandelbrot over D-Bus
   (`Message(token, message, id)`), the app resolves the event id through
   `NotificationClient`, and draws the notification. No polling.
6. **Privacy decisions for the maintainers:** whether to default to a public
   gateway (it learns that a push happened and when, not content), and
   whether the app ships a default or asks. This changes `PRIVACY.md`.

## Done-when for the build, in order

1. Refactor `show_push` into resolve + draw (unit-test the draw input).
2. Add the Background portal switch (B5).
3. Resolve through `NotificationClient` for an event id handed in by the
   push path, with the dedup rule above.
4. UnifiedPush registration, behind its own issue.

Steps 2 to 4 need a live homeserver and desktop session to verify; "a message
sent to a closed Mandelbrot raises one notification and opens the right room"
is a manual check until a harness exists.
