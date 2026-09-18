# Template catalog and distribution

**Status**: specification draft, not accepted · Consolidates #515 and #670

## Shape

A template is a directory containing the document, a thumbnail, and a manifest.
A catalog is an index of templates that a client can fetch and cache. Templates
come from first-party bundles, organization repositories, or a local directory,
and the application does not care which.

#670 specifies the data formats; #515 specifies the flow around them. They do
not conflict.

## Manifest

Per template, alongside the document:

```json
{
  "id": "org.tunaos.letters.resume-modern",
  "version": "1.0.0",
  "target_app": "letters",
  "name": "Modern Professional Resume",
  "description": "Two-column resume with a clear typographic hierarchy.",
  "author": "GTK Office Suite Core Contributors",
  "license": "CC0-1.0",
  "tags": ["resume", "career", "professional"],
  "document_file": "template.odt",
  "thumbnail_file": "thumbnail.png",
  "min_app_version": "1.0.0"
}
```

`license` is required, not optional. A template catalog that accepts
unlicensed contributions cannot ship them, and finding that out after
accepting a hundred of them is expensive.

The `id` uses the suite's real reverse-DNS prefix, `org.tunaos.*`, matching the
GSettings schemas and app IDs. #670 wrote `org.gtk_office.*`, which appears
nowhere in the repo.

## Catalog

A repository serves a `catalog.json` aggregating manifests plus a version and a
timestamp. The client fetches it, caches it under `$XDG_CACHE_HOME`, and indexes
by app and tag. Parsing, validation and caching live in `suite-common-core`; the
browser UI lives in the apps.

## Open questions

1. **Does anything fetch over the network?** A remote catalog means HTTP, TLS,
   cache invalidation, offline behaviour and a trust decision about repository
   operators. Shipping a bundled first-party set with no network path is a
   fraction of the work and covers the common case. This is the decision that
   determines the size of the feature and neither draft makes it explicitly.
2. **Trust.** If third-party repositories are supported, a template is a
   document the user opens — so the parser's robustness is the security
   boundary. That is the same boundary as opening any untrusted file, which is
   an argument that it needs no new machinery, but it should be stated rather
   than left implicit.
3. **`$schema` points at `gtk-office.org`,** a domain this project does not
   appear to control. Either register it or use a repository-relative path.
4. **Overlap with enterprise template provisioning.** #646 proposes system-wide
   template directories with fallback for fleet deployments. Same feature from
   the administrator's side; the two should share one path convention.
5. **Versioning.** `min_app_version` exists; there is no `max`, and no statement
   of what happens when a template uses a feature the running version lacks.

## Relationship to the readiness plan

Behind [#443], with one exception worth noting: a small bundled first-party
template set needs none of the above machinery and is mostly content work.

[#443]: https://github.com/tuna-os/gtk-office-suite/issues/443
