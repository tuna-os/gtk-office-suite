# Third-party notices

The suite is GPL-3.0-or-later. Most dependencies come from crates.io under
their own licences, recorded in `Cargo.lock`. This file carries the notices
that a dependency's licence asks us to reproduce when we ship it.

## BetterOffice DrawingML (`betteroffice-drawingml`)

Used by `decks-core` for the outlines of DrawingML preset shapes our own
`preset_polygon` does not draw (`decks-core/src/engine/shape.rs`,
`borrowed_outline`). Pinned to an exact version, because the project's
contributor agreement lets later releases be relicensed.

Licence: Apache-2.0 (<https://www.apache.org/licenses/LICENSE-2.0>), which
GPL-3.0 code may include. Its NOTICE file reads:

```
BetterOffice
Copyright 2026 Elia Hilse, The OpenOOXML Project
```

Source: <https://github.com/xhayankhan/betteroffice> (`crates/ooxml-drawingml`).
