# Troubleshooting

Answers to the problems people most often hit when installing, building or
using Letters, Tables and Decks. The suite is pre-alpha, so some problems
are known limits rather than bugs. Those are listed under
[Known limits](#known-limits).

If your problem isn't here,
[file a bug](https://github.com/tuna-os/gtk-office-suite/issues/new/choose).
The bug form asks for what helps most: the steps, the file that triggers it
(or a cut-down copy without private content), the version, and your system.

## Running the apps

### An app quits at startup with a GSettings or schema error

Each app stores its preferences with GSettings and **aborts at startup if
its schema isn't installed and compiled**. The Flatpak and Nix builds
handle this for you. If you run a binary you built with Cargo, point it at
a compiled copy of the schemas:

```bash
mkdir -p /tmp/gtk-office-schemas
cp flatpak/*.gschema.xml /tmp/gtk-office-schemas/
glib-compile-schemas /tmp/gtk-office-schemas
GSETTINGS_SCHEMA_DIR=/tmp/gtk-office-schemas cargo run -p letters
```

Inside `nix develop`, `GSETTINGS_SCHEMA_DIR` already points at `flatpak/`,
so `cargo run -p letters` works as it is.

### The app crashed or was closed without saving

Each app keeps an autosave snapshot of every document with unsaved
changes. The next time you start the app **without a file argument**, it
reopens those documents with "(Recovered)" in the window title. A recovered
document is unsaved, so save it to keep it.

Snapshots are kept in the app's state directory:

- Flatpak: `~/.var/app/org.tunaos.letters/.local/state/letters` (likewise
  for `tables` and `decks`).
- Built from source: `$XDG_STATE_HOME/letters`, or
  `~/.local/state/letters` when `XDG_STATE_HOME` isn't set.

If you'd rather not keep a recovered document, close it and choose
**Discard** when the app asks to save. That clears its snapshot.

### A file won't open

When a file can't be opened, the app says so in a "Could not open" message
that names the file and the reason, and it opens nothing in its place. The
usual reasons are:

- **The file isn't really the format its name says.** For example, a
  `.docx` that is actually an old binary `.doc`, or a download that saved an
  HTML error page under the document's name. The message's reason is often
  "not a zip archive".
- **The file is damaged or incomplete.** Try opening it in another office
  suite. If that works, please attach the file (or a copy without private
  content) to a bug report.

### Saving warns that content will be lost

When a document has something the target format can't hold, Tables lists
what would be dropped and asks before saving ("… has content Tables can't
keep"). Save in the format the content came from, such as `.xlsx` for a
workbook with Excel-only features, or accept the loss knowingly.
[FORMATS.md](FORMATS.md) and [PARITY.md](PARITY.md) say what each format
keeps.

### Saving fails

If a save fails, for example because the disk is full or the folder is
read-only, your edits stay in the window, the autosave snapshot is kept,
and the app still asks before you close the document. Fix the cause or
use Save As to save somewhere else.

### The display looks wrong

- **Blurry or wrongly sized text** usually means a fractional display
  scale. The apps follow the system's scale. Please report what you see
  with your scale setting and a screenshot.
- **Missing icons (shown as placeholders)** in a build from source mean the
  Adwaita icon theme, or the SVG loader for gdk-pixbuf (`librsvg2-common` on
  Debian and Ubuntu), isn't installed. The Flatpak includes both.

## Building from source

### Which versions do I need?

Rust stable 1.80 or later, and the GTK 4.14+ and libadwaita 1.5+
development packages. On Debian or Ubuntu:

```bash
sudo apt-get install libgtk-4-dev libadwaita-1-dev libglib2.0-bin
```

[DEVELOPMENT.md](DEVELOPMENT.md) has the full setup.

### The build fails with errors about `gtk4`, `glib` or `libadwaita` versions

Pkg-config is finding older GTK or libadwaita libraries than the crates
need. Check with:

```bash
pkg-config --modversion gtk4 libadwaita-1
```

If those are older than 4.14 and 1.5, use the toolbox container
(`just setup`), the Nix dev shell (`nix develop`), or the Flatpak build.
Each of them provides the right versions.

### The wrong `cargo` or `rustc` is used

A distribution's Rust package can shadow rustup's. `which cargo` should
print `~/.cargo/bin/cargo`. If it doesn't, put `~/.cargo/bin` first on your
`PATH`.

### Nix

- `nix run .` starts Letters, and `nix run . -- tables` (or `decks`) starts
  the others.
- `nix develop` gives a shell with Rust and the GTK libraries.
- **`flake.lock` is not committed**, so the first `nix` command resolves
  nixpkgs itself. Run `nix flake lock` to pin it.
- Two crates (IronCalc and rdocx) are fetched from git with
  `allowBuiltinFetchGit`, so no hashes need updating when they change.

## Running the tests

### Tests fail with "cannot open display" or GTK init errors

The workspace has GTK widget tests, and GTK can't start without a display.
Run the tests under Xvfb:

```bash
xvfb-run -a cargo test --workspace
```

### The GUI journeys can't find the app, or time out

The AT-SPI journeys (`tests/gui/run_gui_tests.sh`) need Xvfb, a D-Bus
session and the accessibility bus. The easiest way to get all three is
`just test-gui-all`, which runs them in the toolbox container.
[GUI-TESTING-SPEC.md](GUI-TESTING-SPEC.md) explains the harness, and
[TESTING.md](TESTING.md) lists every test lane.

## Known limits

The [README's status table](../README.md#project-status) says how each app
compares with LibreOffice today, and [PARITY.md](PARITY.md) lists feature by
feature what works.

- **Formats:** each app reads and writes its OOXML and ODF formats, but not
  every feature of them. Content an app can't hold is reported when you
  save (Tables) or listed in PARITY.md.
- **Accessibility:** see [ACCESSIBILITY.md](ACCESSIBILITY.md#known-limits).
  In short, Letters can't report per-character bounds before GTK 4.16, and
  no manual screen-reader pass has been done yet.
- **Flathub:** the apps are on the TunaOS Flatpak repository, not on
  Flathub yet.

## Reporting a problem

- **Bugs:** use the
  [bug report form](https://github.com/tuna-os/gtk-office-suite/issues/new/choose).
- **Security problems**, such as a file that crashes or takes over an app:
  don't file a public issue. Follow [SECURITY.md](../SECURITY.md).
