# Mandelbrot Roadmap

**Last updated**: 2026-08-29 | **Maintainer**: tuna-os (hanthor)

---

## Mission

Give the TunaOS ecosystem a first-class Matrix messaging app for GNOME:
secure, modern, and optimized for collaboration in large groups. Mandelbrot
forks [Fractal](https://gitlab.gnome.org/World/fractal/) and pushes further
into the modern Matrix feature set — native MatrixRTC voice/video calling,
simplified sliding sync, QR login, threads, polls — so users who live in
Matrix get a native desktop experience instead of a web app.

---

## Current Status

* **Published**: flatpak `org.tunaos.mandelbrot` on the tuna-os remote
  (install verified end-to-end, 2026-07-23); landing page at
  tunaos.org/mandelbrot; docs at tunaos.org/docs/mandelbrot.
* **Distribution**: OCI flatpak published to GHCR via `publish-flatpak.yml`
  on `v*` tag push. **No GitHub Releases page** — no binaries, no checksums,
  no changelog on the Releases surface (see #73).
* **Latest tag**: v14.1.1 (2026-07-24, OCI publish ran). No tag since,
  despite active development (commits through 08-14+).
* **Differentiators**: native MatrixRTC calls (Element Call v0.22.0 interop
  verified — see tests/e2e/CONFORMANCE.md), sliding sync (MSC4186) with
  classic fallback, QR login (MSC4108), threads, polls, spaces hierarchy.
* **CI health**: MatrixRTC e2e failing every nightly for 10 consecutive days
  (#66). The h2 RUSTSEC-2026-0258 advisory is closed — main is on h2 0.4.18
  via #70. The yanked `chacha20` that reddened every open PR is fixed in #87.

### Priorities

| Priority | Item | Tracking | Status |
|----------|------|----------|--------|
| P0 | Fix MatrixRTC e2e nightly (synapse container never comes up) | #66 | 🔴 10d red |
| P0 | First GitHub Release with binaries + checksums; decide release cadence | #73 | ⬜ Not started |
| P1 | Resolve fork identity — crate still `fractal` 14.1.0 upstream authors | #60 | 🟡 Open |
| P1 | h2 dependency RUSTSEC-2026 bump | #69/#70 | ✅ Done — main on h2 0.4.18 |
| P2 | ROADMAP-coverage entry in org ROADMAP tally | org `#1295` | ⬜ Not started |

---

## Quarterly Goals

### Current Quarter (2026 Q3)

**Theme**: stabilize and ship the differentiated client

| Goal | Owner | Tracking | Status |
|------|-------|----------|--------|
| Green MatrixRTC e2e nightly | hanthor | #66 | ⬜ Not started |
| First Releases-page artifact (tag + binaries + checksums) | hanthor | #73 | ⬜ Not started |
| Fork identity decision (rename vs. upstream-following) | hanthor | #60 | ⬜ Not started |

### Next Quarter (2026 Q4)

**Theme**: adoption and release cadence

| Goal | Owner | Tracking | Status |
|------|-------|----------|--------|
| Release cadence aligned with org (weekly/monthly tagged builds) | tuna-os | #73 | ⬜ Not started |
| Surface in ADOPTION-METRICS snapshot | tuna-os | org `#1174` | ⬜ Not started |
