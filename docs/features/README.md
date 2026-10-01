# Feature overviews

What each app does, feature by feature, with a screenshot of the real
running application for each one.

- [Letters](letters.md): word processor
- [Tables](tables.md): spreadsheet
- [Decks](decks.md): presentations

> [!WARNING]
> The suite is **pre-alpha** (see the [README](../../README.md#project-status)).
> These pages show what each feature does with the demo documents in
> `tests/gui/demo/`. They are not a claim that real-world files will look
> right; each page ends with the gaps these very screenshots show.

## How the screenshots are made

Nothing here is a mockup. `tests/gui/feature_tour.py` launches each app
under Xvfb with its default settings, opens a demo document, reaches
each feature the way a user does (its shortcut, menu action or button),
and captures the window. Each image is named after the stop that took it,
and the caption under each heading here is that stop's caption.

`tests/test_feature_docs.py` keeps these pages and the tour in step: every
stop's image must be embedded in its app's page, and every image a page
embeds must come from a stop. So a feature can't silently drop out of the
docs, and a page can't keep showing a screenshot nothing regenerates.

They stay current by themselves. The
[Screenshots workflow](../../.github/workflows/screenshots.yml) recaptures
them on every push to `main` that touches an app, a core crate, the demo
documents or the tour, and weekly for font and runtime updates.
`tests/gui/keep_changed_screenshots.py` then drops images whose
differences are only rendering noise (anti-aliasing, a clock or today's
date). If anything visibly changed, the workflow opens a pull request with
the new images, runs CI on it, and sets it to merge itself once CI is
green.

To recapture locally:

```bash
cargo build --bin letters --bin tables --bin decks
WALKTHROUGH=0 tests/gui/capture_walkthrough.sh /tmp/walkthrough
# or a few stops only:
WALKTHROUGH=0 FEATURE_TOUR_ONLY="letters-toc tables-" tests/gui/capture_walkthrough.sh /tmp/walkthrough
```

This needs Xvfb, dbus, AT-SPI, `python3-dogtail`, Pillow and `mss`. With
`xcompmgr` installed, popovers are drawn with their rounded corners;
without a compositor they show black corners.
