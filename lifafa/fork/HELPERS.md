# The Tuta and Proton helpers

Lifafa reaches Tuta and Proton Mail through two helper programs that ship
beside `hylki` in every package. This page is the fork's part of upstream's
[docs/BUILDING.md](../docs/BUILDING.md) and [docs/LICENSE.md](../docs/LICENSE.md):
how the helpers are built, packaged and licensed. Using the accounts is in
[ACCOUNTS.md](ACCOUNTS.md).

Hylki looks for each helper next to the `hylki` executable first, then on
`PATH`, so every package installs them in the same `bin` directory as
`hylki`. Without a helper, everything but its kind of account works. Both
come from submodules, so check those out first:

```sh
git submodule update --init --recursive
```

## Tuta helper

Tuta accounts go through `hylki-tuta`, a separate program (GPL-3.0-or-later):
`tuta-helper` from [tuta-unofficial-sdk](https://github.com/tuna-os/tuta-unofficial-sdk),
the `native/tuta-unofficial-sdk` submodule, installed under the name
`hylki-tuta`. Started as `hylki-tuta` it names its sessions "Hylki" and reads
`HYLKI_TUTA_DEBUG`.

The helper builds against the Tuta SDK, which is generated from the
submodules inside the SDK repository (tutabridge, and the tutanota repository
inside that) plus the SDK repository's own patches. Prepare the SDK once, and
again whenever the submodule moves:

```sh
sh native/tuta-unofficial-sdk/prepare.sh
```

The script needs `git` and `python3`, no network. It runs tutabridge's
`sdk-prepare.py`, which verifies the generated tree against the digests the
submodule pins, then writes that tree with the patches applied, beside
tutabridge's `tutabridge-tuta` crate, to `native/tuta-unofficial-sdk/prepared`
(untracked), which the helper builds from. On a repeat run it only checks.
Then build the helper and put it beside `hylki` under the name the app looks
for:

```sh
(cd native/tuta-unofficial-sdk && cargo build --release --locked --bin tuta-helper)
install -Dm755 native/tuta-unofficial-sdk/target/release/tuta-helper ~/.local/bin/hylki-tuta
```

The SDK repository has its own `Cargo.lock`, tests and CI; its README
describes the protocol and lists its SDK patches. Moving to a new release is
a submodule bump to its tag. `.github/workflows/tuta-helper.yml` checks that
the submodule sits on a release tag and that the helper builds and answers
as `hylki-tuta`.

## Proton helper

Proton Mail accounts that sign in natively (rather than through Proton Mail
Bridge) go through `hylki-proton`, a separate Go program (GPL-3.0-or-later):
`proton-helper` from [proton-unofficial-sdk](https://github.com/tuna-os/proton-unofficial-sdk),
the `native/proton-unofficial-sdk` submodule, installed under the name
`hylki-proton`. It behaves the same under either name.

It needs Go 1.26.1 or newer (the `go` line in
`native/proton-unofficial-sdk/go.mod`) and nothing from the network: its
dependencies are committed in the SDK's `vendor/`.

```sh
(cd native/proton-unofficial-sdk && go build -mod=vendor -trimpath -o hylki-proton ./cmd/proton-helper)
install -Dm755 native/proton-unofficial-sdk/hylki-proton ~/.local/bin/hylki-proton
```

The SDK repository has its own tests, which run offline against
go-proton-api's in-process server, and its own CI; dependency bumps and
`go mod vendor` happen there. Its README describes the JSON it speaks.
Moving to a new release is a submodule bump to its tag.
`.github/workflows/proton-helper.yml` checks that the submodule sits on a
release tag, that the helper builds offline, and that run as `hylki-proton`
it answers every operation the app sends with the envelope the app parses.

## Flatpak

The Tuta helper's crates come from
`native/tuta-unofficial-sdk/cargo-sources.json`, which the SDK repository
commits (its `scripts/cargo-sources.sh` writes it, and its CI fails when it
is stale).

The `hylki-tuta` module builds `tuta-helper` into `/app/bin` beside `hylki`,
with `hylki-tuta` a symlink to it, and the `hylki-proton` module builds
`proton-helper` there, with `hylki-proton` a symlink to it, using the
`org.freedesktop.Sdk.Extension.golang//25.08` SDK extension (install it
beside the Rust one), offline from `native/proton-unofficial-sdk/vendor`.
Check the submodules out and prepare the Tuta SDK first, as above: neither
the submodules nor the generated tree are in the pinned git source, so the
manifest takes them from the checkout it is built in. `tools/local-manifest.sh`
builds the working tree instead of the tagged release, as upstream's
BUILDING.md describes; the release build goes through `fork/rebrand.sh`
([README.md](README.md#building)).

## RPM, Debian and AppImage

Both packages carry the Tuta helper as `/usr/bin/hylki-tuta`; the script
prepares the Tuta SDK and builds the helper itself, so the submodules have to
be checked out. They carry the Proton helper as `/usr/bin/hylki-proton` too,
so the RPM build needs `go` on the host; the Debian container installs
Debian's, and a Go older than `native/proton-unofficial-sdk/go.mod` asks for
fetches the right one (`GOTOOLCHAIN=auto`). `dpkg-shlibdeps` works out the
package's dependencies from all three binaries.

The AppImage carries both helpers beside `hylki`, built the same way as for
the packages.

## Licences

**Tuta helper.** `hylki-tuta` is `tuta-helper` from
[tuta-unofficial-sdk](https://github.com/tuna-os/tuta-unofficial-sdk) (the
`native/tuta-unofficial-sdk` submodule), installed under that name. It
originates in [Postcard](https://github.com/gxanshu/postcard) and its
[tuna-os fork](https://github.com/tuna-os/postcard). It builds on
[tutabridge](https://github.com/spartanz51/tutabridge) and the
[Tuta SDK](https://github.com/tutao/tutanota), and is licensed under the GNU
General Public License v3.0 or later (GPL-3.0-or-later), which is compatible
with the AGPL-3.0. It runs as a separate program under its own licence.
tuta-unofficial-sdk is not affiliated with or endorsed by Tuta GmbH.

**Proton helper.** `hylki-proton` is `proton-helper` from
[proton-unofficial-sdk](https://github.com/tuna-os/proton-unofficial-sdk)
(the `native/proton-unofficial-sdk` submodule), installed under that name.
It originates in [Postcard](https://github.com/gxanshu/postcard) and its
[tuna-os fork](https://github.com/tuna-os/postcard), and is licensed under
the GNU General Public License v3.0 or later (GPL-3.0-or-later), which is
compatible with the AGPL-3.0. It runs as a separate program under its own
licence. It links Proton's
[go-proton-api](https://github.com/ProtonMail/go-proton-api),
[gopenpgp](https://github.com/ProtonMail/gopenpgp) and their dependencies,
under the MIT, BSD-2-Clause and BSD-3-Clause licences; each one's licence
text is in `native/proton-unofficial-sdk/vendor`. Hylki is not affiliated
with or endorsed by Proton AG.
