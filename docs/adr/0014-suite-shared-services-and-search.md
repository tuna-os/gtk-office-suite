# ADR 0014: Shared services are libraries first, search is per app, and what "fully featured" is measured against

Date: 2026-11-10 · Status: **accepted** (decided on the owner's standing instruction to decide open questions; each decision names what would reverse it) · Relates to: [INTEGRATION-PLAN](../INTEGRATION-PLAN.md) "Open decisions", [ADR 0012](0012-shared-document-primitives.md), [ADR 0013](0013-one-matrix-account.md), [SANDBOX-AUDIT](../SANDBOX-AUDIT.md)

## Context

`INTEGRATION-PLAN.md` left three questions open, and the work that depends on them (A7 contacts, A9 language services, B1 search, B7 accounts, C4 evidence) cannot be designed until they are answered.

## Decision 1: shared services are libraries, not a user-level D-Bus service

Cross-app behaviour that needs no runtime state shared between running apps lives in the crates the apps already link: GTK-free models in `suite-common-core`, GTK pieces in `suite-common`. That is how the clipboard fragment (ADR 0012), `MailDraft`, the person-suggestion rule (`people::suggest`) and the search model (`search_provider`) are already built.

Why not a service:

- Each app is its own Flatpak with its own sandbox. A service the others call is another package to ship and update, and each app needs a `--talk-name` for it. That widens every sandbox, against the direction of B10 (narrow grants, one at a time).
- A service is a new process to keep private and to version against the apps that call it. A library couples release cycles, but the apps already release together (C3), so the cost is low.
- Lifafa (AGPL) and Mandelbrot (GPL) already sit behind process boundaries by design; they reach the office apps through `GApplication` actions (`open-room`, `start-call`, `share-file`), which is the one cross-process surface and stays the only one.

What would reverse it: a service that must hold state shared across running apps (one loaded language model for A9, say, too large to load per process). That case starts as an optional sidecar with its own lockfile, as `collabd` did (RFC-0005), not as a required daemon.

Consequence for A9 (shared language services): an in-tree GTK-free crate behind provider traits (spelling, grammar, translation), used by Letters and Lifafa's composer; no new process.

The licence sets the direction of any sharing. Lifafa is AGPL-3.0-or-later and its Hylki-derived code is not ours to relicense (`THIRD-PARTY-NOTICES.md`); the suite is GPL-3.0-or-later. A GPL crate may be linked into Lifafa, but Lifafa's existing grammar and translation services (`lifafa/src/services/grammar`, `translate`, with their settings, downloads and keyring use) cannot be moved into a GPL crate. So A9 is built from the suite side: a new GPL crate with the provider traits and a shared result type, an adapter in Letters for hunspell, and a thin adapter in Lifafa that implements the traits over its existing services. Lifafa adopts the contract; it does not hand over its code. Lifafa does not depend on any suite crate today, so the first step of A9 is that dependency (with the `lifafa-core` extraction of #2025), not a rewrite.

## Decision 2: search is answered by each app, from its own data

Each app ships its own `org.gnome.Shell.SearchProvider2` provider and answers from what it already keeps (recent documents for the office apps, the mail cache for Lifafa, rooms and messages for Mandelbrot). There is no shared indexer and no dependency on Tracker or LocalSearch.

Why: it is private by default (nothing is indexed outside the app that owns it), it needs no new daemon, it is what `SearchProvider2` is shaped for (one provider per app, each with its own `.search-provider.ini`), and the GTK-free half already exists on that basis (`suite_common_core::search_provider`, over `RecentItem`).

What would reverse it: a requirement to search the contents of documents the apps have not opened recently. The desktop's own file search already covers file contents without us; if that proves insufficient, the answer is a per-app content index behind the same provider, not a shared one.

## Decision 3: what "fully featured" is measured against

| App | Reference | Tracked in |
|---|---|---|
| Letters, Tables, Decks | LibreOffice Writer, Calc, Impress: the parity tables and the capability ledger that already exist | `docs/PARITY.md`, `conformance/capabilities.json` |
| Lifafa | Thunderbird and GNOME Evolution, as a checklist of what a person doing daily mail needs (accounts and protocols, search, filters, calendar and contacts, encryption, attachments) | Lifafa's own roadmap, with claims and lanes in the ledger (C4) |
| Mandelbrot | Element for what a Matrix user expects; upstream Fractal as the baseline the fork must not regress below | Mandelbrot's `ROADMAP.md`, with claims and lanes in the ledger (C4) |

Each row is a reference and a place to track it, not a promise of parity: where an app deliberately differs, the ledger says so with a waiver, as it does for the office apps today.

What would reverse it: a different reference chosen by the owner for any app. Nothing built on this ADR depends on which reference is chosen, only on there being one.

## Consequences

- A9, A7, B1 and C4 can proceed as libraries and ledger entries without waiting.
- Any proposal for a shared daemon now has to meet the reversal condition in Decision 1 and say what it does to the sandbox audit.
- The three "Open decisions" in `INTEGRATION-PLAN.md` become references to this ADR.
