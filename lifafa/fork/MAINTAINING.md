# Maintaining the Hylki soft fork

Lifafa is a soft fork of [hyprlab/hylki](https://github.com/hyprlab/hylki).
It ships as **Lifafa** (`org.tunaos.lifafa`), the email client of the TunaOS
office suite, and installs next to upstream Hylki. Its code lives in
`lifafa/` of [tuna-os/gtk-office-suite](https://github.com/tuna-os/gtk-office-suite)
and is maintained there; [tuna-os/lifafa](https://github.com/tuna-os/lifafa),
where it used to live, still publishes it until the cutover
(`docs/CUTOVER-LIFAFA-MANDELBROT.md`) moves releases, issues and the upstream
merge here, and is archived after. Where this page says "this repository" for
releases and workflows, it means tuna-os/lifafa until then. The fork exists for one
reason: upstream declined native Tuta
([#584](https://github.com/hyprlab/hylki/pull/584)) and native Proton
([#586](https://github.com/hyprlab/hylki/pull/586)) accounts, and we want
both. Everything else should be the same app as upstream, kept that way by
merging upstream continuously.

There are three rules:

1. **Upstream first.** Anything that is not Tuta, Proton or fork plumbing
   goes to hyprlab/hylki as a pull request, and reaches this fork through the
   upstream merge.
2. **Fork-only code stays in fork-only files.** Every line the fork changes in
   an upstream file is a future merge conflict.
3. **Never rewrite `main`.** Upstream is merged in, never rebased onto, so
   anyone tracking this fork can always pull.

## What belongs where

| Kind of change | Where it goes | How |
|---|---|---|
| Bug fix, security fix, UI polish, docs, translations, anything provider-neutral | **Upstream** | A branch from `upstream/main` and a PR to hyprlab/hylki (see "Sending a change upstream") |
| Security hardening that is not Tuta or Proton specific (cache encryption at rest, OpenPGP, link-tracker stripping, ...) | **Upstream** | The same route |
| A seam the fork needs in shared code (a generic hook, an outbox state, a stable message key) that has a use beyond Tuta and Proton | **Upstream**, phrased provider-neutrally | Propose it without mentioning Tuta or Proton; if upstream declines, it stays in the fork as a touchpoint |
| Native Tuta accounts | **Fork only** | [tuta-unofficial-sdk](https://github.com/tuna-os/tuta-unofficial-sdk) (the `native/tuta-unofficial-sdk` submodule), `src/worker/tuta*` |
| Native Proton accounts | **Fork only** | [proton-unofficial-sdk](https://github.com/tuna-os/proton-unofficial-sdk) (the `native/proton-unofficial-sdk` submodule), `src/worker/proton*` |
| Shared helper plumbing for Tuta and Proton | **Fork only** | `src/worker/helper*` |
| Rebranding, fork CI, release and sync workflows | **Fork only** | `fork/`, `.github/workflows/{upstream-sync,fork-release,tuta-helper,proton-helper}.yml` |

When in doubt, ask whether the change would make sense in upstream Hylki with
no Tuta or Proton account configured. If it would, it goes upstream.

## The boundary, concretely

- **Fork-only files** are files that do not exist upstream. Add as much code
  there as needed; merges never conflict on them.
- **Touchpoints** are upstream files the fork modifies (enum variants, match
  arms, provider rows, packaging steps). They are listed in
  [`touchpoints.txt`](touchpoints.txt). `fork/check-touchpoints.sh` fails when
  the fork modifies an upstream file that is not listed, and the upstream-sync
  workflow runs it after every merge.
- **Rules for touchpoints:**
  - Keep each edit as small as the language allows: one match arm that calls
    into a fork-only module, not inline logic.
  - Put fork additions at the end of lists and enums where order allows, so
    they don't sit beside the lines upstream is editing.
  - Never reformat or reorder upstream code in a touchpoint file.
  - Adding a file to `touchpoints.txt` needs a reason in the commit message.
  - The list should only shrink over time. Each provider-neutral seam
    upstream accepts removes lines from it.
- **The app identity is not a touchpoint.** Source keeps upstream's
  `co.hyprlab.Hylki`. `fork/rebrand.sh` rewrites it at build time from one
  identity block at its top (today `org.tunaos.lifafa`, "Lifafa", `lifafa`
  data directories, the fork's keyring service and icon), so the rebrand
  never conflicts with upstream and a rename edits only that block.

## Keeping up with upstream

- **Automatic:** `.github/workflows/upstream-sync.yml` runs daily, and on
  demand. It:
  1. fetches `hyprlab/hylki` `main`;
  2. merges it into our `main` with a merge commit;
  3. builds, and runs the touchpoint check;
  4. pushes if all of that passed.
- **On a conflict or failure** it pushes nothing, and opens or updates one
  issue labelled `upstream-sync` listing the files involved.
- **Resolving a sync by hand:**
  1. `git fetch upstream && git switch main && git merge upstream/main`
  2. Resolve in favour of upstream's code, then re-apply the fork's small
     touchpoint edit on top. If the conflict is in a fork-only module's
     caller, change the fork module to fit upstream rather than the other
     way round.
  3. Run the build, the tests, `fork/check-touchpoints.sh` and the
     helpers' test suites.
  4. Commit the merge and push. Close the `upstream-sync` issue.
- **When upstream merges one of our pull requests,** the next sync brings it
  in. If the fork carried an early copy, the merge resolves to upstream's
  version; delete any fork-side duplicate in the same commit.

## Sending a change upstream

1. `git fetch upstream && git switch -c fix/<topic> upstream/main`. Never
   branch from our `main`: it contains Tuta and Proton.
2. Follow upstream's `docs/CONTRIBUTING.md`:
   - Conventional Commits, with the subject at most 72 characters and no
     trailers;
   - comments that say why;
   - `i18n()` for user-facing strings;
   - no em dashes;
   - `tools/check-docs.py` passing.
3. Make sure the diff never mentions Tuta, Proton, the fork or
   the fork's app (`org.tunaos.lifafa`, Lifafa).
4. Open the PR from `tuna-os:<branch>` to `hyprlab:main`. Say what was
   verified and what was not (for example "not reproduced against a real
   account").
5. **Shipping it in the fork early (optional):** merge the same branch into
   our `main`. Don't cherry-pick, because a merge keeps the commit identity,
   so the upstream merge later resolves cleanly.

## Dependencies the fork adds

- **Tuta:** [tuna-os/tuta-unofficial-sdk](https://github.com/tuna-os/tuta-unofficial-sdk),
  the `native/tuta-unofficial-sdk` submodule: a Rust library built on Tuta's
  SDK plus a small set of reviewed SDK patches, and the `tuta-helper` JSON
  front end Hylki talks to. Every package installs `tuta-helper` under the
  name `hylki-tuta`, which the app looks for and which gives the helper its
  Hylki identity (sessions named "Hylki", `HYLKI_TUTA_DEBUG`).
  - Tuta changes (protocol, SDK patches, tests) go to that repository, with
    its own CI and live tests. The fork only bumps the submodule to a
    release tag, which `.github/workflows/tuta-helper.yml` checks.
  - Its README lists every SDK patch and why it exists, and documents the
    protocol `src/worker/tuta*` speaks.
- **Proton:** [tuna-os/proton-unofficial-sdk](https://github.com/tuna-os/proton-unofficial-sdk),
  the `native/proton-unofficial-sdk` submodule: a Go library over Proton's
  own go-proton-api and gopenpgp, with vendored modules, and the
  `proton-helper` JSON front end Hylki talks to. Every package installs
  `proton-helper` under the name `hylki-proton`, which the app looks for; it
  behaves the same under either name.
  - Proton changes (protocol, tests, dependency bumps) go to that
    repository, with its own CI. The fork only bumps the submodule to a
    release tag, which `.github/workflows/proton-helper.yml` checks.
  - A release whose `CHANGELOG.md` lists a protocol change needs the
    matching change in `src/worker/proton/wire.rs` in the same commit as the
    bump.
- **Licence:** both helpers are GPL-3.0-or-later, compatible with Hylki's
  AGPL-3.0-or-later (see [HELPERS.md](HELPERS.md#licences)).
- **Terms of service:** neither provider endorses third-party clients built
  this way. That is why upstream declined them, and users of this fork should
  know it. The fork's README says so.

## Testing and accounts

- **Unit and integration tests** run in CI with fakes. No real account is
  ever used in CI with personal credentials.
- **Live tests** are `#[ignore]` and gated on `TUTA_LIVE*` / `PROTON_LIVE*`
  environment variables:
  - run locally only, against dedicated test accounts;
  - every artifact they create is cleaned up;
  - secrets come from a password manager into the environment and are never
    printed.
- **Before a fork release:** run the full suite, both helpers' suites, the
  live Tuta test, and a Flatpak build of the rebranded tree.

## Releases

- **Tags** are `v<upstream-version>-tuna<N>`, for example `v1.47.1-tuna3`, so
  the upstream base is always visible.
- **Building:** `.github/workflows/fork-release.yml` builds the rebranded
  Flatpak (both helpers included) for x86_64 and aarch64, commits it signed
  into the Flatpak repository on GitHub Pages, and attaches bundles of the
  same commits to the GitHub release (fork/README.md, "Building" and
  "Updates"). It reuses the caches a daily build of `main` keeps warm; run it
  by hand with `clean` ticked if a cache goes bad. Before tagging, a manual
  run on the branch builds, signs and assembles the repository without
  publishing it.
- **Keys:** the repository's signing key is the `LIFAFA_GPG_PRIVATE_KEY`
  Actions secret, with an offline copy kept by the maintainers; only its
  public half, `fork/flatpak-repo.asc`, is in the tree, and the workflow
  refuses a secret that does not match it.
- **Release notes** list the upstream version merged, plus the fork-only
  changes since the last tag.
