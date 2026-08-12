# ADR-0004: Decks advanced workflows and review scope

Status: accepted for issue #117

## Decision

Decks will add advanced presentation workflows in layers. The first layer is
the presenter-state contract in `decks-core::presenter`: current slide, next
slide, speaker notes, elapsed timer, and a primary/external display target.
The GTK shell may render that state in a presenter window, but navigation and
readiness checks remain GTK-free and deterministic.

Export and print use the existing 16:9 slide geometry (960×540 model units,
16×9 cm PDF page), the slide background, and the selected master. Font
fallback is reported rather than silently substituted when a requested font is
unavailable. PDF is the release export; printer-specific settings remain a
front-end concern over the same page renderer.

## Admitted scope

| Capability | First supported behavior | Loss policy |
|---|---|---|
| Transitions | None, fade, push-left, wipe-left, cover-left, split-horizontal | Render identically in editor preview and presentation mode; unknown PPTX/ODP transitions are preserved opaquely or warned on export |
| Images | Linked images that exist at presentation time | Missing media is a structured readiness error; never a silent blank |
| Audio/video | Review only in this milestone | Preserve package parts opaquely where safe; warn before export; no playback claim |
| Comments/review markup | Review only in this milestone | Preserve opaque parts where safe; warn before destructive conversion |
| Presenter display | Primary display, explicit external display selection, fallback to primary | If external display disappears, return to primary and show a visible status |

For PPTX and ODP, text, slide order, notes, geometry, backgrounds, masters,
and supported images are must-preserve. Unsupported animation, media, and
comment parts are opaque pass-through when their package relationships can be
retained; otherwise they are warn-on-loss. A supported feature that cannot be
preserved is a hard error. No unsupported content may disappear silently.

## Release journeys

The release test set must cover:

1. Start presenter mode with one display, advance/back through a deck, verify
   notes/current/next/timer, and exit without changing the document.
2. Select an external display, disconnect it, and verify visible fallback to
   the primary display.
3. Open a deck with a missing linked image/media file and verify the file,
   slide, and repair-safe error are named before presenting/exporting.
4. Export a multi-slide deck with backgrounds, masters, fonts, notes, and an
   admitted transition; compare page geometry and ordering in the PDF.
5. Import an unsupported PPTX/ODP animation or comment, make an unrelated
   edit, and verify the opaque part survives or a structured loss warning
   blocks the save.

Anything beyond this table—timeline/keyframe animation, embedded playback,
real-time annotations, presenter remote control, and printer-specific
imposition—is explicitly deferred until it has a separate design and corpus
fixtures.
