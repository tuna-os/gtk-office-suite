# Telemetry and crash reporting

**Status**: strategy draft, not accepted · Consolidates #554 and #639

## Position

Off by default, opt-in only, no personal data, and an administrator can turn it
off for a fleet and lock it there. Both drafts agree on all of this and neither
proposes anything weaker.

## What would be collected

Crash and stability data, and nothing that identifies a person or a document:

- application (`letters` / `tables` / `decks`), version, Flatpak commit
- OS version, GPU driver string
- panic message and backtrace, with paths and usernames stripped
- never: document contents, file names, file paths, network addresses

Path and username stripping is the requirement most likely to be
under-implemented. A Rust backtrace contains build paths by default, and a
panic message frequently contains a file name because that is what the code was
operating on. This needs a redaction pass with tests that assert against real
panics, not a policy statement.

## How it would work

- **Consent** is a first-run prompt (an `AdwBanner` or dialog) and a permanent
  toggle in Preferences → Privacy. No dark patterns: declining is one click and
  is not asked again.
- **Collection** runs off the GTK main thread. Payloads buffer to disk under
  `$XDG_DATA_HOME` and are dispatched by a background worker.
- **Local auditability** (#554): buffered payloads are human-readable JSON, so a
  user or administrator can read exactly what would be sent before it is sent.
  This is the strongest idea in either draft — it makes the privacy claim
  checkable instead of promised — and it is worth keeping even if nothing is
  ever transmitted.
- **Crash capture** hooks `std::panic::set_hook` and serializes the backtrace
  to disk.

## Open questions

1. **Is there a receiving endpoint?** Neither draft says who runs it, where it
   is, what it retains or for how long. Without that, this is a local crash-log
   feature with a transmission story attached, and there is a good case for
   shipping only the local half first: it delivers most of the debugging value,
   needs no infrastructure, and makes no privacy claims that require trust.
2. **Settings keys.** #554 proposes `telemetry-enabled`, `crash-reports-enabled`
   and `custom-endpoint-url`. #639 proposes `org.gnome.Letters.telemetry-enabled`
   — the wrong namespace; the suite's schemas are `org.tunaos.letters` / `.tables`
   / `.decks`. These are also fleet-wide settings with no per-app meaning, which
   is an argument for the shared schema discussed in the enterprise consolidation.
3. **`custom-endpoint-url`** (#554) lets an administrator redirect telemetry. It
   also lets anyone who can write that key redirect it. If it stays, it is
   lock-only.
4. **Does a crash reporter belong here at all?** GNOME systems often already
   have `systemd-coredump` and ABRT. Duplicating that is worth a deliberate
   decision rather than an assumption.

## Relationship to the readiness plan

Behind [#443]. The local-only crash log is the piece that would genuinely help
the readiness work, and it is also the piece with no infrastructure or privacy
dependencies — which is a reason to consider it on its own merits rather than
as phase 1 of a telemetry pipeline.

[#443]: https://github.com/tuna-os/gtk-office-suite/issues/443
