# Headless GUI Testing & Automated Display Verification Strategy

**Last updated**: 2026-09-04 | **Maintainer**: tuna-os (hanthor) / strategist agent

---

## Strategic Objective

Ensure robust, deterministic, and isolated headless GUI test execution across all GTK4/Libadwaita applications (Letters, Tables, Decks) in containerized CI environments and developer test runners without requiring a physical display hardware attachment.

---

## Headless Display Environment Matrix

| Execution Environment | Display Backend | Display Server / Driver | Primary Use Case | Verification Gate |
|-----------------------|-----------------|-------------------------|------------------|-------------------|
| Standard CI Runner | `WAYLAND_DISPLAY=wayland-0` | `weston --headless` / `mutter --headless` | Full Wayland GUI integration & event loop testing | Required for PR merge |
| Containerized CI | `DISPLAY=:99` | `Xvfb` (Virtual Framebuffer) | Legacy GTK4 X11 fallback verification | Required for containerized test suites |
| Offscreen Surface | `GDK_BACKEND=broadway` / offscreen | GDK Offscreen Display | Pixel snapshot & rendering verification | Nightly visual regression |
| Headless Guard Fallback | N/A | Mock GDK display guard | Unit test fallback when no display server is available | Cargo test suite fallback |

---

## Architectural Guidelines

1. **Display Guarding**: All tests instantiating `gtk::Application` or GDK surface handles must check `gtk::init()` return status or handle display initialization failures gracefully.
2. **Environment Variable Precedence**:
   - `GDK_BACKEND=wayland,x11,headless` fallback chain.
   - `GSK_RENDERER=cairo` fallback when LLVMpipe / Mesa GL drivers are absent.
3. **CI Pipeline Integration**:
   - Wrap interactive GTK integration binaries with `xvfb-run -a cargo test -p <pkg> --test <test_name>` or headless Weston sessions.
   - Ensure visual artifact outputs (PNG snapshots) are stored as workflow artifacts on failure.

---

## Verification Matrix & Release Gates

- **Gate Level 1 (Fast Unit Tests)**: Executed without GTK context; tests domain engines (`engine.rs`, `format.rs`, `undo.rs`).
- **Gate Level 2 (Headless GUI Smoke Tests)**: Executed under Xvfb/headless Wayland; verifies window instantiation, menu action wiring, and controller binding.
- **Gate Level 3 (Visual Snapshot Regression)**: Nightly execution rendering views to `cairo::ImageSurface` and matching against baseline snapshots with a controlled loss budget.

---
