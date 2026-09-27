# Troubleshooting build, test, and development issues

Common problems and solutions when setting up, building, and testing the GTK Office Suite.

## Lockfile version conflicts

**Symptom:** `error: Cargo.lock is for Rust 1.80+, but you are using ... (your version)`

**Cause:** Your `cargo` is too old. The workspace requires Rust stable ≥ 1.80 (lockfile v4).

**Solution:**
```bash
rustup update stable
rustup default stable
which cargo  # should print ~/.cargo/bin/cargo, not /usr/bin/cargo
```

If `which cargo` still points to `/usr/bin/`, your distro's system `cargo` is shadowing rustup:
```bash
# Remove the system cargo
sudo apt-get remove cargo rustc  # Debian/Ubuntu
sudo dnf remove cargo rustc      # Fedora/RHEL

# Then verify rustup's cargo is used
rustup update && rustup default stable
cargo --version
```

---

## GSettings schema errors at app startup

**Symptom:** App exits with `GLib-GIO-ERROR **: could not locate schema` or similar.

**Cause:** The apps read GSettings at startup and abort if schemas are not compiled. Outside Flatpak, you must point them at compiled schemas.

**Solution:**
```bash
mkdir -p /tmp/gtk-office-schemas
cp flatpak/*.gschema.xml /tmp/gtk-office-schemas/
glib-compile-schemas /tmp/gtk-office-schemas

# Now run with the schema path set:
GSETTINGS_SCHEMA_DIR=/tmp/gtk-office-schemas cargo run -p letters
# or: GSETTINGS_SCHEMA_DIR=/tmp/gtk-office-schemas cargo run -p tables
# or: GSETTINGS_SCHEMA_DIR=/tmp/gtk-office-schemas cargo run -p decks
```

**To avoid typing it repeatedly:**
```bash
export GSETTINGS_SCHEMA_DIR=/tmp/gtk-office-schemas
cargo run -p letters
```

---

## GTK widget tests fail without a display

**Symptom:** `cargo test --workspace` fails with GTK initialization errors, or 36 tests silently stop running.

**Cause:** The workspace contains GTK widget tests. GTK requires a display server to initialize — these tests **fail** rather than skip without one (see TESTING.md #241). This is deliberate: tests that silently skip can rot without anyone noticing.

**Solution — if you have Xvfb:**
```bash
xvfb-run -a cargo test --workspace
```

**Solution — if you cannot provide a display (e.g., CI without Xvfb, or a constrained environment):**
```bash
SUITE_GTK_TESTS=skip cargo test --workspace
```

This skips only the widget tests; logic tests in GTK-free core crates still run:
```bash
# Alternative: test only core crates, no GTK required
cargo test -p suite-common-core
cargo test -p letters-core
cargo test -p tables-core
cargo test -p decks-core
```

---

## GUI smoke tests fail to start display

**Symptom:** `tests/gui/run_gui_tests.sh test_smoke.py` fails with `no X11 display`, `Cannot find X11 socket`, or timeout waiting for Xvfb.

**Cause:** The test runner allocates its own Xvfb display and needs permissions or dependencies.

**Solution — install dependencies (Ubuntu/Debian):**
```bash
sudo apt-get install xvfb dbus at-spi2-core matchbox-window-manager \
  python3-dogtail python3-pytest python3-pil python3-requests xwayland

python3 -m pip install --break-system-packages mss
```

**Solution — watch the test on your own display (slow, not recommended for CI):**
```bash
GUI_TEST_REUSE_DISPLAY=1 tests/gui/run_gui_tests.sh test_smoke.py
```

**Solution — adjust timeouts if your machine is slow:**
```bash
# Increase startup budget (default 60s)
GUI_TEST_READY_SECONDS=120 tests/gui/run_gui_tests.sh test_smoke.py

# Increase Xvfb server-start budget (default 60s)
GUI_TEST_XVFB_SECONDS=120 tests/gui/run_gui_tests.sh test_smoke.py
```

---

## Python dogtail import errors in GUI tests

**Symptom:** `ImportError: cannot import name '_gi'` or similar when running GUI tests.

**Cause:** System apt's dogtail (0.9.11) lives in `dist-packages` and is compiled for a specific Python version. If you're using a different version, the compiled extension won't load.

**Solution:**
```bash
# Find which Python version dogtail was compiled for
ls /usr/lib/python3/dist-packages/gi/_gi.cpython-*.so

# You'll see something like: ...-312-... (for Python 3.12)
# Point the test runner at that version:
GUI_TEST_PYTHON=/usr/bin/python3.12 tests/gui/run_gui_tests.sh test_smoke.py
```

---

## LibreOffice oracle tests skip or hang

**Symptom:** Oracle tests skip or hang with timeout during `REQUIRE_SOFFICE=1 cargo test`.

**Cause:** Two issues:
1. LibreOffice is not installed, or
2. LibreOffice is sandboxed (Flathub version) and cannot access temp files.

**Solution — install LibreOffice (if not already):**
```bash
# Debian/Ubuntu
sudo apt-get install libreoffice-writer libreoffice-impress libreoffice-calc

# Fedora/RHEL
sudo dnf install libreoffice-writer libreoffice-impress libreoffice-calc
```

**Solution — if using Flathub LibreOffice (sandboxed):**
The sandbox may not honour `--filesystem=host`, preventing LibreOffice from reading temp files. Point `TMPDIR` to a directory the sandbox can see:

```bash
TMPDIR=~/.var/app/org.libreoffice.LibreOffice/data/tmp \
REQUIRE_SOFFICE=1 cargo test --test-threads=1
```

**Solution — run tests serially:**
Concurrent `soffice` instances contend over the user profile and silently produce no output, causing spurious failures:

```bash
REQUIRE_SOFFICE=1 cargo test --test-threads=1
```

---

## LibreOffice oracle tests claim to pass but conversion fails silently

**Symptom:** `soffice --version` works, but tests fail with "No such file or directory" or zero output.

**Cause:** LibreOffice is installed but misconfigured. The skip-guard checks only `soffice --version`; it does not verify that file conversion works.

**Solution:**
```bash
# Test that LibreOffice can actually convert files
soffice --headless --convert-to pdf /tmp/test.txt --outdir /tmp/

# If this fails or hangs, LibreOffice is broken. Reinstall:
sudo apt-get remove --purge libreoffice* && sudo apt-get install libreoffice-writer

# Then retry the oracle tests serially
REQUIRE_SOFFICE=1 cargo test -p letters-core -p decks-core --test-threads=1
```

---

## Clippy warns about many existing issues

**Symptom:** `cargo clippy --workspace` reports warnings in existing code.

**Cause:** The codebase has pre-existing clippy violations (see AGENTS.md).

**Solution:** For PRs, ensure your changes do not *introduce* new warnings:
```bash
cargo clippy --workspace -- -D warnings
```

This will fail if your code adds a warning, but allows existing ones. The project gates on this in CI.

---

## Flatpak build fails locally

**Symptom:** `flatpak run org.flatpak.Builder ...` fails with missing dependencies or build errors.

**Cause:** Flatpak builds require the builder and GNOME 50 runtime.

**Solution:**
```bash
# Install Flatpak builder (if not already)
sudo apt-get install flatpak flatpak-builder
# or: sudo dnf install flatpak flatpak-builder

# Add the Flathub remote for runtimes
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo

# Install the GNOME 50 runtime (once)
flatpak install flathub org.gnome.Platform/x86_64/50 org.gnome.Sdk/x86_64/50

# Build
flatpak run org.flatpak.Builder --state-dir=.flatpak-builder \
    build-dir flatpak/org.tunaos.letters.json
```

For detailed instructions, see the `README.md` [Flatpak section](../README.md#flatpak).

---

## Unit tests pass locally but fail in CI

**Symptom:** Green test run locally, but CI gates fail.

**Cause:** CI runs with stricter settings:
- Unit tests run under `xvfb-run` (display server required)
- `cargo clippy` runs with `-D warnings` (all warnings are errors)
- `REQUIRE_SOFFICE=1` is set nightly, not on every push

**Solution:**
```bash
# Run the exact CI checks locally
xvfb-run -a cargo test --workspace
cargo clippy --workspace -- -D warnings

# For Oracle tests (only in nightly):
REQUIRE_SOFFICE=1 cargo test --test-threads=1
```

---

## Render parity test fails: "expected X, got Y"

**Symptom:** A render-lab fixture fails during CI, showing a screenshot mismatch.

**Cause:** Your change altered rendering. This is not necessarily a bug — if the change is intentional, the baseline needs updating.

**Solution:**
1. Review the diff in `render-lab-out/report.html` in the CI job artifacts.
2. If the change is correct, download the proposed baseline from the CI job:
   ```bash
   gh run download <run-id> -n render-parity-report
   # This extracts report.html, baseline.proposed.json, and images
   ```
3. Review the images carefully against LibreOffice's rendering of the same file.
4. Commit the new baseline:
   ```bash
   mv baseline.proposed.json tools/render-lab/baseline.json
   git add tools/render-lab/baseline.json
   git commit -s -m "fix(render-lab): update baseline for [feature]"
   ```

See [RENDER-PARITY-ROADMAP.md](RENDER-PARITY-ROADMAP.md) for more details.

---

## Build complains about missing libgtk4 or libadwaita

**Symptom:** `cargo check` fails with linking errors about GTK4 or libadwaita.

**Cause:** Development headers are not installed.

**Solution (Debian/Ubuntu):**
```bash
sudo apt-get install libgtk-4-dev libadwaita-1-dev libglib2.0-bin
```

**Solution (Fedora/RHEL):**
```bash
sudo dnf install gtk4-devel libadwaita-devel
```

**Solution (Nix):**
```bash
nix develop
```

---

## Contributing a fix

If you find a recurring issue not covered here, consider adding it. See [`docs/CONTRIBUTING.md`](CONTRIBUTING.md) for the workflow.
