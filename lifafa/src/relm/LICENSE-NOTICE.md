# Vendored: relm4 0.11.0 runtime

This directory is a copy of the runtime of [relm4](https://github.com/Relm4/Relm4)
0.11.0 (the `relm4` crate, without `relm4-macros`, `relm4-css`, the async
shared-state and typed-view modules), used here as an ordinary module so that
Lifafa no longer depends on the crate. Feature-gated code was resolved for the
features Lifafa used (`libadwaita`); crate paths were rewritten to `crate::relm`.

relm4 is dual-licensed under the MIT or Apache-2.0 licence, at your option:



The MIT text is in the repository's THIRD-PARTY-NOTICES.md, reproduced
from the upstream `LICENSE-MIT`.
