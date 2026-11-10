# Third-party notices

The suite is GPL-3.0-or-later (`lifafa/` is AGPL-3.0-or-later, below). Most dependencies come from crates.io under
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

## Lifafa (`lifafa/`)

The email client. A fork of Hylki by hyprlab (<https://github.com/hyprlab/hylki>),
with native Tuta and Proton Mail accounts added, imported from
<https://github.com/tuna-os/lifafa> at commit
`a7352f0c4e4fb7b90621c8459b78a4505884d2ee` (Hylki 1.47.1).

Licence: AGPL-3.0-or-later; the full text is `lifafa/LICENSE`. Hylki's
copyright holders are hyprlab and its contributors, so the licence of the
Hylki-derived code is not ours to change. AGPLv3 section 13 and GPLv3 section 13
permit combining it with the GPL-3.0-or-later parts of the suite in one work;
the AGPL's network-use clause (section 13 of the AGPL) applies to the AGPL part.
The native SDKs it builds against, `tuna-os/tuta-unofficial-sdk` and
`tuna-os/proton-unofficial-sdk`, are git submodules at the commits `lifafa/`
pinned (see `.gitmodules`); their licences are in those repositories.

## Mandelbrot (`mandelbrot/`)

The Matrix chat and calls client. A fork of Fractal, the GNOME Matrix client
(<https://gitlab.gnome.org/World/fractal/>, Julian Sparber and the Fractal
contributors), imported from <https://github.com/tuna-os/mandelbrot> at commit
`d6f94b35c895398bbb763c38ebfd48c15de68b6b`.

Licence: GPL-3.0-or-later, the same as the rest of the suite; the full text is
`mandelbrot/LICENSE`. Its `matrixrtc/` crate is GPL-3.0-or-later as well.

## relm4 runtime (`lifafa/src/relm/`)

Lifafa's component runtime is a copy of the runtime of relm4 0.11.0
(<https://github.com/Relm4/Relm4>, by Aaron Erhardt and contributors), kept in the
tree instead of as a dependency. relm4 is MIT OR Apache-2.0; we use it under the
MIT licence, whose text follows.

```
Copyright (c) The Relm4 authors

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```
