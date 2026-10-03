# Overview videos

Each app has an overview video, and the suite has a trailer. They show
the running apps in GNOME's style: one segment per feature, with the
window's rounded corners and a soft shadow, on a gradient in the app's
accent colour (Letters blue, Tables green, Decks orange). Each segment is
captioned in an OSD-style pill and set in Adwaita Sans. Title and suite
cards open and close them, and crossfades join everything.

| Video | What it shows |
|---|---|
| `letters.mp4` | every Letters stop of the feature tour |
| `tables.mp4` | every Tables stop |
| `decks.mp4` | every Decks stop |
| `gtk-office-suite.mp4` | the trailer: three features of each app |

## Where they come from

They are recorded, never drawn. `tests/gui/overview_video.py` runs the
feature tour (`tests/gui/feature_tour.py`) and records the display while
the tour drives each app the way a user would. It then cuts a segment for
each feature, from the moment the window is up to just after the feature is
fully on screen. The captions are the tour's own, the same ones under the
screenshots in [docs/features](features/). A feature added to the tour
appears in its app's video at the next recording.

The apps run at 2x on a 3200×2000 virtual display, so the windows are
sharp when they are scaled to 1080p.

## Keeping them up to date

`.github/workflows/overview-videos.yml` records them every Monday, on
demand (Actions → Overview videos → Run workflow), and for every
published release. A run uploads them as the `overview-videos` artifact.
A release run also attaches them to the release page.

To record them locally:

```bash
cargo build --bin letters --bin tables --bin decks
tests/gui/overview_video.sh videos            # all four
tests/gui/overview_video.sh videos tables     # just one app
```

This needs Xvfb, xcompmgr, dbus, AT-SPI, dogtail, mss, Pillow, ffmpeg and
`rsvg-convert`. The script fetches Adwaita Sans from download.gnome.org
when there is no copy.

`tests/test_overview_video.py` keeps the video script in step with the
tour: every app has a video, the trailer names stops that exist, and every
title card has its icon and summary.
