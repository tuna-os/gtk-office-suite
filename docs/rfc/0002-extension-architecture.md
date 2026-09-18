# RFC 0002: Extension architecture — sandboxed WASM and out-of-process IPC

Date: 2026-09-18 · Status: **draft, not accepted**

## Provenance

This RFC consolidates eight independently generated drafts of the same
proposal: #611, #657, #672, #674, #676, #680, #685 and #708. They were written
in parallel over 2026-09-11/12, each into a different file path, and three of
them claimed the RFC number 0002 at once. None was reviewed.

They agree almost entirely on the architecture. Where they differ they differ
on *numbers and names* — the memory ceiling, the execution budget, the manifest
filename, the host crate name, the runtime. Those disagreements are recorded in
[Open questions](#open-questions) rather than resolved by whichever draft was
read last. Everything below that is stated as a constraint or a decision is
common to all eight.

## Summary

Third-party extensions for Letters, Tables and Decks run in one of two tiers:
a sandboxed WebAssembly runtime for pure data transformation, and an
out-of-process IPC sidecar for anything that needs the desktop. Neither tier
can touch GTK, and neither can crash the editor.

This is a proposal. It is not scheduled, and it is not ahead of
[Roadmap to dependable daily use](../readiness-2026-09/README.md) ([#443]) —
crash safety, atomic save and format parity all come first. The reason to write
the contract down now is narrower: decisions taken in `suite-common-core` today
can foreclose a clean extension boundary tomorrow, and re-architecting the core
later is the expensive outcome this document exists to avoid.

## Constraints

These are binding on any design in this space, and all eight drafts state them.

1. **GTK-free core.** The host interface and sandbox live in `suite-common-core`
   and must build and unit-test headless — no X11, no Wayland, no GTK
   ([ADR-0001](../adr/0001-core-shell-split.md)). Plugin logic runs against the
   pure-Rust document models, never against widget code.
2. **No native in-process plugins.** Loading third-party `.so`/`.dll` into the
   editor process is rejected. It defeats Flatpak isolation, it makes a plugin
   bug a host crash, and Rust ABI instability makes such plugins fragile across
   toolchains. This is the one option every draft considered and every draft
   ruled out.
3. **No raw pointers across the boundary.** Memory handed to a guest is a value
   or a byte slice that the host owns. Internal document buffers are never
   exposed directly.
4. **Flatpak-native.** Extensions must work inside the existing sandbox without
   `--filesystem=host` or an unrestricted session bus. IPC extensions declare
   explicit bus names in the Flatpak manifest.
5. **Opt-in and non-blocking.** Plugin execution happens off the GTK main loop.
   A plugin that traps, panics or hangs is terminated by the host; the editor
   does not notice beyond a diagnostic.
6. **Save-path safety.** Any `document:pre-save` / `document:post-save` hook
   runs inside the `atomic_save` transaction ([#437]) or not at all. An
   extension must not be able to corrupt an ODF/OOXML write.

## Two tiers

**Tier 1 — in-process WASM sandbox.** For synchronous, pure computation over
document models: custom OpenFormula functions in Tables, text transformers,
linters and terminology checkers in Letters, asset generators and export
filters in Decks. Input and output are serialized records; the guest sees no
handles.

**Tier 2 — out-of-process IPC.** For extensions that need the desktop: their
own windows, network sync, system services, external databases. These are
separate processes reached over D-Bus or Varlink, managed through the portal,
and they never share an address space with the editor.

The split matters because it is what keeps tier 1 cheap. Once a plugin needs a
window, it has left the sandbox, and pretending otherwise is how UI-capable
plugin systems end up with host pointers in them.

## Capability model

A plugin declares what it needs in a manifest; the host grants nothing it did
not ask for; the user confirms the grant at install time; and an administrator
can narrow or forbid the whole thing by policy.

The capability vocabulary the drafts converge on:

| Capability | Grants |
|---|---|
| `document:read` | Read document text, AST nodes, or cell ranges |
| `document:write` | Apply edits as undoable transactions |
| `ui:action` | Register a command-palette entry, context-menu item or sidebar panel |
| `network:fetch` | Outbound HTTP. Off by default; enterprise policy can forbid it outright |
| `fs:read` | Read an explicit list of paths, inside the Flatpak sandbox |

Two rules that are easy to lose and that every draft asserts: capabilities are
declared, never inferred; and a UI capability registers a *declarative* action,
never a widget. A plugin describes a menu entry. It does not construct one.

## Enterprise policy

Fleet administrators control extensions through GSettings/dconf, alongside the
rest of the deployment policy surface:

- a global toggle for the extension runtime,
- an allowlist of permitted plugin IDs,
- a requirement that plugins carry a valid signature from a trusted key.

Signing is ed25519 in every draft that mentions it. The key-distribution
question is open and is called out below.

## Phasing

Deliberately behind the readiness plan. Horizons are indicative, not committed.

| Phase | Focus | Deliverable |
|---|---|---|
| 1 | Interface | WIT/ABI contracts and manifest schema in `suite-common-core`; headless tests for instantiation, trap recovery and timeout |
| 2 | Runtime | Sandbox host with memory and execution budgets enforced |
| 3 | Bindings | Tables custom functions and Letters text filters wired to the registry |
| 4 | Surface | Extension-manager UI, signed distribution format, IPC tier and portal integration |

Nothing before phase 1 is worth starting while [#443] is open.

## Open questions

These are the real disagreements among the eight drafts. Each needs evidence,
not a preference.

1. **Runtime: `wasmtime` or `wasmi`?** `wasmtime` JITs and is faster;
   `wasmi` is an interpreter with no JIT, which is easier to ship in a
   restrictive Flatpak and on constrained hardware. Settle with a benchmark of
   cold-start time and resident memory for a trivial plugin, on the Flatpak
   runtime we actually ship.
2. **Resource budgets.** The drafts propose memory ceilings of 16 MB, 64 MB and
   128 MB, and per-call execution budgets of 50 ms, 100 ms, 5 s and 5000 ms.
   These are not small differences — 50 ms and 5 s imply different plugin
   categories. Settle by measuring a real workload: a custom Tables function
   over a large sheet is the obvious first candidate.
3. **Manifest format and filename.** `plugin.toml`, `extension.toml` and
   `plugin.json` all appear. One name, one format, chosen once.
4. **Host crate name.** `suite-plugin-core`, `suite-plugin-api`,
   `suite-extension-api`, and `suite-common-core::plugin` all appear. This
   should follow whatever the workspace layout already implies rather than
   introduce a fourth convention.
5. **ABI style.** Raw pointer-and-length host functions (`suite_doc_get_text`)
   versus the WASM Component Model with WIT records. WIT is the direction the
   later drafts move toward and costs more to stand up now.
6. **Plugin-key distribution.** Signature verification is agreed; who holds the
   trusted keys, and how a fleet administrator adds one, is not addressed by
   any draft.

## What would make this concrete

A single tier-1 plugin, end to end, behind a feature flag: one custom Tables
function, compiled from Rust, loaded from a manifest, running under a budget,
with a test that proves a deliberate infinite loop in the guest is killed
without the host noticing. That exercise answers questions 1, 2 and 5 with
measurements, and it is small enough to throw away.

[#443]: https://github.com/tuna-os/gtk-office-suite/issues/443
[#437]: https://github.com/tuna-os/gtk-office-suite/issues/437
