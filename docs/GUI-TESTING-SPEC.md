# Dogtail GUI Testing in QEMU on GitHub Actions — Specification

> End-to-end GUI test automation for all three apps (Letters, Tables, Decks)
> using Dogtail + QEMU virtual machine on GitHub CI.

> [!NOTE]
> This document specifies an architectural proposal for full-desktop QEMU VM testing on CI.
> For the current canonical GUI test execution framework, runner scripts, and Xvfb setup, see [`docs/TESTING.md`](TESTING.md).

---

## Motivation

Current CI covers unit tests (61 tests, 0 failures) and Flatpak builds. These verify logic and packaging, but do not verify that the GUI renders correctly or that common user workflows (open file, type text, format, save) function end-to-end.

Dogtail uses AT-SPI (GNOME accessibility bus) to drive GUI apps programmatically — it clicks buttons, types text, reads labels, and verifies widget states. Paired with QEMU for a virtual display, this gives us real GUI verification on every push.

---

## Architecture

```
GitHub Actions Runner (ubuntu-24.04)
  └─ QEMU VM (Fedora 43, GNOME Wayland, 2GB RAM, 2 cores)
       ├─ Boot → auto-login → GNOME session
       ├─ Install our Flatpak from CI artifact (or build from source)
       ├─ Run Dogtail test scripts (Python)
       ├─ Record screen to MP4 artifact
       └─ Exit with pass/fail
```

### Components

| Component | Purpose |
|-----------|---------|
| **QEMU** | Virtual machine with virtual display (QXL/ virtio-gpu) |
| **Fedora 43 cloud image** | Pre-built GNOME desktop VM |
| **Dogtail** | Python AT-SPI automation framework |
| **Flatpak** | Our app, installed from CI-built artifact or nightly repo |
| **Screen recording** | Debug artifact for failed test runs |

### CI Flow

```
1. Build Flatpaks (reuse existing flatpak job)
2. Upload Flatpak bundles as CI artifacts
3. Boot QEMU VM with Fedora cloud image
4. Install Flatpak + deps inside VM
5. Run Dogtail test scripts
6. Record screen → upload as artifact
7. Pass/fail based on test exit code
```

---

## Dogtail Test Scenarios

### Letters (Word Processor)

| Test | Steps | Verification |
|------|-------|-------------|
| **Create new document** | Click "New Document", type "Hello World" | Text appears in editor |
| **Bold formatting** | Type text, select it, click Bold button | Text tag changes to bold |
| **Markdown macros** | Type `**bold**` + space | Text auto-formats to bold |
| **Find & Replace** | Ctrl+F, type query, verify match count | Match count shows correct value |
| **Save as Markdown** | File → Save As → pick filename | File exists with correct content |
| **Open existing file** | File → Open → select .md file | Document loads with formatting |

### Tables (Spreadsheet)

| Test | Steps | Verification |
|------|-------|-------------|
| **Cell entry** | Click cell, type value, press Enter | Cell displays value, formula bar updates |
| **Formula evaluation** | Enter `=1+1` in cell, press Enter | Cell shows `2` |
| **Column sort** | Click column A header twice | Sort indicator appears, data reorders |
| **Number formatting** | Select cell, click format toggle | Display changes (e.g., General→$1.00) |
| **Freeze panes** | Select cell B2, View → Freeze First Row | Frozen boundary line visible |
| **XLSX import** | File → Open → sample.xlsx | Data loads, formulas evaluate |

### Decks (Presentations)

| Test | Steps | Verification |
|------|-------|-------------|
| **Create slide** | Click "Add Slide" button | New slide appears in sidebar |
| **Add text box** | Click "Add Text Box", double-click to edit | Text appears on canvas |
| **Add shape** | Click "Add Shape" twice | Rect then circle appear |
| **Navigate slides** | Press Right arrow in present mode | Next slide shown |
| **PPTX roundtrip** | Save as PPTX, close, re-open | Slides and objects preserved |
| **Undo/Redo** | Add object, Ctrl+Z, Ctrl+Shift+Z | Object removed, then restored |

---

## QEMU Configuration

```yaml
- uses: qemu-actions/run-with-qemu-vm@v1
  with:
    image: fedora-43-gnome.qcow2        # Pre-built GNOME desktop image
    memory: 2048                          # 2 GB RAM
    cpu: 2                                # 2 CPU cores
    display: gtk                          # QXL virtual display
    run: |
      # Inside VM:
      flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
      flatpak install -y org.tunaos.letters
      flatpak install -y org.tunaos.tables
      flatpak install -y org.tunaos.decks
      pip install dogtail
      python3 /tests/run_all.py
    record: screen.mp4                    # Always record for debugging
    timeout: 900                          # 15 minute timeout
```

---

## Test Runner Script (`tests/gui/run_all.py`)

```python
#!/usr/bin/env python3
"""Run all Dogtail GUI tests for gtk-office-suite."""
import subprocess, sys, os

TESTS = [
    "letters/test_create_document.py",
    "letters/test_formatting.py",
    "letters/test_markdown_macros.py",
    "letters/test_find_replace.py",
    "letters/test_file_io.py",
    "tables/test_cell_entry.py",
    "tables/test_formula.py",
    "tables/test_sort.py",
    "tables/test_format.py",
    "tables/test_freeze.py",
    "tables/test_xlsx_import.py",
    "decks/test_create_slide.py",
    "decks/test_text_box.py",
    "decks/test_shapes.py",
    "decks/test_navigate.py",
    "decks/test_pptx_roundtrip.py",
    "decks/test_undo.py",
]

failed = 0
for test in TESTS:
    print(f"Running {test}...")
    result = subprocess.run([sys.executable, test], capture_output=True, text=True)
    if result.returncode != 0:
        print(f"FAIL: {test}")
        print(result.stderr)
        failed += 1
    else:
        print(f"PASS: {test}")

print(f"\n{failed}/{len(TESTS)} tests failed")
sys.exit(failed)
```

---

## Effort Estimate

| Task | Effort |
|------|--------|
| Create Fedora 43 GNOME cloud image for QEMU | 1d |
| Set up Dogtail test framework + base utilities | 1d |
| Write Letters tests (6 scenarios) | 1d |
| Write Tables tests (6 scenarios) | 1d |
| Write Decks tests (6 scenarios) | 1d |
| Integrate into CI workflow | 0.5d |
| Test + debug + iterate | 1d |
| **Total** | **~6.5d** |

---

## Prerequisites

- [ ] Fedora 43 GNOME cloud image built and published (or use qemu-actions default)
- [ ] All 3 Flatpaks build successfully in CI (✅ done)
- [ ] AT-SPI accessibility bridge enabled in our GTK4 apps (✅ GtkDrawingArea needs `accessible-role` property set for Dogtail to see canvas content)

---

## Limitations

- **Canvas accessibility:** Dogtail cannot see inside `GtkDrawingArea` widgets. Our Tables grid and Decks canvas are rendered via Cairo — AT-SPI does not expose pixel-level content. Workaround: test via toolbar buttons, formula bar, sidebar interactions rather than canvas pixel inspection.
- **CI time:** QEMU VM boot + test run takes 10-15 minutes. Total CI time would be ~20 minutes (3 min unit tests + 15 min GUI tests). Consider running GUI tests only on PR to main, not every push.
- **Flakiness:** GUI tests are inherently more flaky than unit tests. Dogtail uses timing-dependent operations (wait for widget to appear). Test scripts must include explicit waits and retries.
