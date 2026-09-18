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

1. **Per-app or shared schema?** Today there are three per-app schemas and no
   shared one. Fleet-wide settings (telemetry, update policy, extension
   loading) are not per-app and have no home. Adding `org.tunaos.suite` is the
   obvious move but it is a new schema and needs deciding, not assuming.
2. **Which dconf database?** The drafts variously use `local.d` and `site.d`.
   Both work; the documentation should name one so examples are copy-pasteable.
3. **Flatpak and dconf.** Several drafts assert that host dconf policy reaches
   a Flatpak'd app "via GSettings portal forwarding". That needs verifying
   rather than asserting — it is the load-bearing claim for the whole document,
   since the suite ships as Flatpaks, and if it does not hold, fleet policy
   needs a different mechanism entirely.
4. **Keys that do not exist yet.** Telemetry, update-check and extension-loading
   policy all presuppose settings the suite does not have. Each belongs with
   the feature that introduces it.
5. **Default autosave interval.** Drafts propose 60, 120 and 300 seconds; the
   schema says 60. If 60 is wrong, change the schema and say why — do not
   document a different number.
6. **System template provisioning** (#646): standard paths for organization
   templates, with fallback when the user directory is missing or read-only.
   Reasonable and unspecified; overlaps the template-catalog drafts (#515, #670).

## Relationship to the readiness plan

Nothing here is scheduled ahead of [Roadmap to dependable daily use](readiness-2026-09/README.md)
([#443]). The immediately useful part is small: confirm the Flatpak/dconf path
works, and make the settings surface handle locked keys without crashing.

[#678]: https://github.com/tuna-os/gtk-office-suite/issues/678
[#443]: https://github.com/tuna-os/gtk-office-suite/issues/443
