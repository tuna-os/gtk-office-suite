# Contributing to GTK Office Suite

Thank you for your interest in contributing to GTK Office Suite (`gtk-office-suite`)! We welcome contributions from developers of all experience levels.

## Overview

GTK Office Suite is a modern, native Linux desktop office suite built with Rust, GTK4, and libadwaita. It comprises three core applications:
- **Letters**: Document editor & processing
- **Tables**: Spreadsheet calculations & data visualizer
- **Decks**: Presentation creator & slide show designer

## Prerequisites

Before building GTK Office Suite locally, ensure you have the following installed:
- **Rust toolchain** (1.75+ recommended): Install via `rustup`
- **GTK4 & libadwaita libraries**: Install development headers via your distro's package manager
  - Fedora: `sudo dnf install gtk4-devel libadwaita-devel`
  - Ubuntu/Debian: `sudo apt install libgtk-4-dev libadwaita-1-dev`
  - Arch Linux: `sudo pacman -S gtk4 libadwaita`

## Workspace Structure

- `letters/`, `letters-core/`: Document editor UI and core document model
- `tables/`, `tables-core/`: Spreadsheet UI and calculation engine
- `decks/`, `decks-core/`: Presentation UI and slide engine
- `suite-common/`, `suite-common-core/`: Shared UI components and core data structures
- `suite-export/`: Export handlers and format converters
- `tests/`: Integration, GUI, and end-to-end test suites

## Building & Testing

### Build the Project
```bash
cargo build
```

### Run Tests
To run all unit and core integration tests:
```bash
cargo test
```

To run tests for a specific workspace crate:
```bash
cargo test -p tables-core
```

## Pull Request Workflow

1. **Find or create an issue**: Ensure an issue exists for your proposed change or bug fix.
2. **Create a topic branch**: Branch off `main` for your feature or bug fix.
3. **Commit with DCO Sign-off**: All commits require DCO sign-off (`git commit -s`).
4. **Run tests & linters**: Ensure all tests pass (`cargo test`) before pushing.
5. **Open a Pull Request**: Submit your PR targeting `main`.

Thank you for helping make native Linux office applications better!
