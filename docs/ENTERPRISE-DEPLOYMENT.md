# Enterprise fleet deployment and dconf policy

**Status**: specification draft, not accepted · **Tracking**: [#678]

## Provenance

This consolidates eleven independently generated drafts: #524, #542, #617,
#628, #635, #646, #679, #681, #689, #691 and #703, written over 2026-09-11/12
into nine different file paths (including two that differ only by case,
`docs/ENTERPRISE-FLEET-DEPLOYMENT.md` and `docs/enterprise-fleet-deployment.md`,
which do not coexist on a case-insensitive filesystem).

They agree on the approach. **They also all share one factual error**, described
immediately below, which is the main reason this consolidation exists.

## The schema namespace is already decided, and the drafts got it wrong

Every one of the eleven drafts invented a GSettings hierarchy. Between them
they proposed at least five:

| Draft | Proposed namespace |
|---|---|
| #646 | `org.gnome.GtkOffice` |
| #679 | `/org/gnome/gitlab/tuna-os/gtk-office-suite/` |
| #681 | `/org/gtkoffice/suite/` |
| #691 | `org.gnome.Letters`, `org.gnome.Tables`, `org.gnome.Decks`, `org.gnome.GtkOfficeSuite.Shared` |
| #676, #680 | `org.tunaos.gtk-office-suite.plugins`, `/org/gtkoffice/suite/plugins/` |

**None of these is what the suite ships.** The schemas in `flatpak/` are:

```
org.tunaos.letters   path /org/tunaos/letters/
org.tunaos.tables    path /org/tunaos/tables/
org.tunaos.decks     path /org/tunaos/decks/
```

A policy document that names the wrong schema ID is worse than no document: an
administrator who follows it writes a keyfile and lock that silently match
nothing, `dconf update` succeeds, and the fleet is unmanaged while appearing
managed. Locks that point at a nonexistent path produce no error.

Two of the proposals are additionally wrong in kind. `org.gnome.*` is GNOME
upstream's namespace and a third-party application must not take IDs in it.
`/org/gnome/gitlab/tuna-os/...` appears to encode a forge URL into a settings
path.

The same applies to key names. The drafts write `autosave-interval-seconds`,
`default-document-format` and `telemetry-enabled`. The schemas already define
`auto-save`, `auto-save-interval` (default `60`, range 0–3600), `default-format`
(default `'odt'`) and `spell-check-enabled`. Policy is written against the keys
that exist.

## What holds up

Stripped of the invented namespaces, the drafts agree on a sound design, and
it is mostly standard GNOME practice rather than anything this suite must
invent:

- Configuration is GSettings backed by dconf, so it already works with dconf
  profiles and therefore with Ansible, Puppet, Salt, Satellite and FreeIPA.
  Nothing suite-specific is needed for fleet management to work at all.
- Administrators set defaults in a keyfile under a system dconf database and
  make them mandatory by listing the full key paths in that database's `locks/`
  directory, then running `dconf update`.
- The application must treat a locked key as read-only and not crash. Writing
  to a locked key fails, and a settings dialog that assumes writes succeed will
  misbehave. This is the one place the design imposes a real requirement on the
  code, and #646 is right to call out that it needs tests.
- Security-relevant settings should be lockable: telemetry, automatic update
  checks, outbound network access, and whether extensions may load at all.

## Worked example

Against the real schemas. A keyfile in the system database:

```ini
# /etc/dconf/db/local.d/00-office-policy
[org/tunaos/letters]
default-format='odt'
auto-save-interval=120

[org/tunaos/tables]
auto-save-interval=120
```

and the matching locks:

```
# /etc/dconf/db/local.d/locks/00-office-policy
/org/tunaos/letters/default-format
/org/tunaos/letters/auto-save-interval
/org/tunaos/tables/auto-save-interval
```

then `dconf update`.

This example only uses keys that exist today. Extending policy to telemetry or
extension loading requires those keys to be added to the schemas first — which
is work, not documentation, and is why the sections below are open rather than
specified.

## Open questions

Each question below is either **resolved** (answered, with the evidence) or
**open** (with what closes it). The document stays a draft until every open
item is closed.

| # | Question | State | Closed by |
|---|---|---|---|
| 1 | Shared schema | Resolved | `org.tunaos.suite` shipped in #2135 |
| 2 | dconf database | Resolved | `local.d` |
| 3 | Flatpak and dconf | **Open** | Verification on the GNOME 50 runtime |
| 4 | Missing keys | **Open** | Each feature that adds a key |
| 5 | Autosave default | Resolved | 60 seconds is correct |
| 6 | Template provisioning | **Open** | The template-catalog work ([#515], [#670]) |

1. **Per-app or shared schema? Resolved.** A shared schema now exists:
   `org.tunaos.suite`, path `/org/tunaos/suite/`, added in #2135 and installed
   by all three apps' Flatpak manifests. Fleet-wide settings go there. It
   currently holds only `default-mail-account`, `send-format`,
   `notification-policy` and `welcome-shown`; none of the fleet keys in
   question 4 are in it yet.
2. **Which dconf database? Resolved: `local.d`.** It is the name used in the
   dconf documentation's own examples and in the worked example above. Examples
   stay `local.d`; an administrator running a different database name
   substitutes it.
3. **Flatpak and dconf. Open.** Several drafts assert that host dconf policy
   reaches a Flatpak'd app "via GSettings portal forwarding". Nobody has
   verified that, and it is the load-bearing claim for the whole document,
   since the suite ships as Flatpaks. Acceptance criterion: on a GNOME 50
   runtime, with a keyfile and lock for `/org/tunaos/letters/auto-save-interval`
   installed in the host's `local.d` and `dconf update` run, the Flatpak'd
   Letters reads the administrator's value and a write to the key fails. If
   that does not hold, fleet policy needs another mechanism, and this document
   must be rewritten around it. Until verified, the worked example is only
   known to be correct for non-Flatpak installs.
4. **Keys that do not exist yet. Open.** Telemetry, update-check and
   extension-loading policy presuppose settings the suite does not have. Each
   belongs with the feature that introduces it, in `org.tunaos.suite` (question
   1) when it is fleet-wide. Link each key's issue or PR here as it lands; none
   is scheduled today.
5. **Default autosave interval. Resolved: 60 seconds.** The schemas define
   `auto-save-interval` with default `60` and range 0–3600. The 120 and 300
   in the drafts were values an administrator might set, not corrections to
   the default. The worked example's `120` is a policy value.
6. **System template provisioning. Open.** Standard paths for organization
   templates, with fallback when the user directory is missing or read-only
   (#646). This is the same feature as the template catalog, whose draft
   ([TEMPLATE-CATALOG.md](TEMPLATE-CATALOG.md), [#515], [#670]) lists it as an
   overlap. Specify it once, there, and link it from here.

## Relationship to the readiness plan

Nothing here is scheduled ahead of [Roadmap to dependable daily use](readiness-2026-09/README.md)
([#443]). The immediately useful part is small: confirm the Flatpak/dconf path
works, and make the settings surface handle locked keys without crashing.

[#678]: https://github.com/tuna-os/gtk-office-suite/issues/678
[#515]: https://github.com/tuna-os/gtk-office-suite/issues/515
[#670]: https://github.com/tuna-os/gtk-office-suite/issues/670
[#443]: https://github.com/tuna-os/gtk-office-suite/issues/443
