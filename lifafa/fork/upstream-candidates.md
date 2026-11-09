# Upstream candidates

Provider-neutral changes the fork would like hyprlab/hylki to take, drafted
here and not yet opened. Each one, once merged upstream, takes files off
[`touchpoints.txt`](touchpoints.txt) (fork/MAINTAINING.md, "Sending a change
upstream"). A draft names its branch on tuna-os/lifafa when one exists; open
the pull request from that branch to `hyprlab:main` after rebasing it onto
`upstream/main`, and move the entry to "Opened" with its number.

## Ready

### test: pass inside a bare Flatpak runtime such as the GNOME SDK

- **Branch:** `fix/sandboxed-tests` (one commit, based on an upstream
  commit; it applies to `upstream/main` as it is).
- **Touchpoints it removes once merged:** `src/app/tray/indicator.rs`,
  `src/desktop/platform.rs`, `src/mcp/server.rs`,
  `src/ui/message_view/tests/document.rs`.
- **Fork issue:** tuna-os/lifafa#6.

Body:

> Running the test suite inside the GNOME 50 SDK (`flatpak run
> --command=bash org.gnome.Sdk//50`, with the build's own dependencies
> mounted as `/app`), five tests always fail for reasons outside the code
> under test, so a real failure there can hide behind them.
>
> - `every_icon_renders_at_every_size`, `the_dot_is_red_and_only_when_asked`
>   and `a_half_fill_leaves_a_clear_margin_and_moves_the_dot` (tray) and
>   `a_card_from_your_own_mailbox_wears_that_mailboxs_face` (reader) decode
>   images. GTK does that with glycin, which inside Flatpak sandboxes its
>   loaders through the portal's `Spawn`, and the portal refuses a sandbox
>   with no `[Application]` in `/.flatpak-info` ("Key file does not have
>   group Application"). A new test helper,
>   `desktop::platform::image_loaders_unavailable`, says when that is the
>   case; those tests print the reason and stop. On the host and inside an
>   app's own sandbox they run as before.
> - `a_stale_socket_is_taken_over_and_a_live_one_is_not` (MCP) expected the
>   socket directory to be 0700, but `bind` deliberately leaves that mode to
>   Flatpak whenever `FLATPAK_ID` is set. The test now expects what `bind`
>   does.
>
> Verified: the full suite (1522 tests) in the GNOME 50 SDK passes, the five
> stopping early or passing; not verified on upstream's CI runners, where
> the image tests run in full as before.

## Drafts without a branch yet

### refactor(config): ask the protocol what it can do

Several call sites match on `Protocol` to decide a capability: whether an
account has keywords (`app/receipts.rs`), whether a message can be stored
into its folders (`app/transfer.rs`), whether it has SMTP to test
(`worker/conn_test.rs`), how snooze and undo find a moved message
(`app/snooze.rs`, `app/undo.rs`), which accounts sync on their own
(`app/workers.rs`). Methods on `Protocol` (`has_keywords`,
`can_store_messages`, `sends_through_server`, ...) keep each answer in one
place, next to the enum, so a new protocol is one edit there.

- **Touchpoints it would remove:** up to the six files named, leaving their
  arms in `src/config/accounts.rs`, which is a touchpoint already.

### refactor: build config and message structs with `..Default::default()`

`AccountConfig` literals in `services/goa.rs`, `app/demo.rs` and the tests,
and `OutgoingMessage` literals in `app/send.rs` and `app/filter_forward.rs`,
name every field, so each new field touches every one of them. Deriving
`Default` for `OutgoingMessage` and finishing these literals with
`..Default::default()` makes a new field one edit, upstream as here.

- **Touchpoints it would remove:** `src/services/goa.rs`, `src/app/send.rs`,
  `src/app/filter_forward.rs`, `src/worker/test_support.rs`, and most of
  `src/config/filter_tests.rs` and `src/app/demo.rs`.

### feat(compose): let the recipient fields mark a pill

`RecipientRow::pill_addresses` and `RecipientRow::mark_pills` (an icon, words
for the tooltip and a screen reader, and a style class on a pill), and a
`RecipientsChanged` input the fields emit on every change. Upstream could use
them to mark, say, an address with no OpenPGP key when encrypting, or one
outside the organisation.

- **Touchpoints it would remove:** `src/ui/recipient_row.rs`,
  `src/ui/compose/suggest.rs`.

### fix(compose): only real Message-IDs reach a References header

`build_message` drops the `graph-conv:` tokens Microsoft 365 threads by
before writing References. Dropping every token that is not a `<msg-id>`
covers any synthetic threading token a backend uses, with no list of
prefixes to keep.

- **Touchpoints it would remove:** `src/worker/compose/build.rs`, and the
  fork's shared prefix list in `src/worker/ids.rs`.

## Opened

None yet.
