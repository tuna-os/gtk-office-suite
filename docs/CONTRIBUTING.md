# Contributing — GTK Office Suite

> Guide for AI agents and developers contributing to this project.

---

## Before You Start

### Read These Files First

1. **[README.md](../README.md)** — project overview, architecture, feature status
2. **[ARCHITECTURE.md](ARCHITECTURE.md)** — module layout, data flow, LO mappings
3. **[GNOME-GUIDELINES.md](GNOME-GUIDELINES.md)** — widget rules, spacing, icon naming
4. **[AGENT-REFERENCE-LIBRARY.md](../AGENT-REFERENCE-LIBRARY.md)** — implementation patterns with code examples
5. **[AGENT-GNOME-REFERENCE.md](../AGENT-GNOME-REFERENCE.md)** — GNOME Rust reference app catalog

### Reference Repositories

| Repo | Path | When to consult |
|------|------|----------------|
| LibreOffice core | `/var/home/james/dev/libreoffice-core/` | Before implementing ANY new feature — check how LO does it |
| IronCalc | `/var/home/james/dev/ironcalc-ref/` | Tables formula engine, cell model |
| gnome-gui-spec | `https://github.com/hanthor/gnome-gui-spec` | Widget selection, GNOME HIG compliance |

### Skills Available

| Skill | When to invoke |
|-------|---------------|
| `searxng-search` | Researching implementation patterns, crates, debugging |
| `design-an-interface` | Designing new dialogs or API surface |
| `tdd` | Writing tests before implementation |
| `review` | Reviewing changes before commit |
| `to-issues` | Converting a plan into GitHub issues |
| `pi-subagents` | Parallel work on independent features |

---

## Workflow

### 1. Survey Existing Issues

```bash
gh issue list --state open --json number,title,labels
```

Check [IMPLEMENTATION-QUEUE.md](../IMPLEMENTATION-QUEUE.md) for effort estimates and dependencies.

### 2. Research Before Implementing

For any new feature:
1. **Check LO**: `grep -rn "FeatureName" /var/home/james/dev/libreoffice-core/sc/` or `sd/`
2. **Check our patterns**: Search `AGENT-REFERENCE-LIBRARY.md` for the pattern number
3. **Check crates**: Search `crates.io` before writing custom code
4. **Check GNOME HIG**: Consult `docs/GNOME-GUIDELINES.md` for widget rules

### 3. Code Conventions

#### Rust Patterns for GTK

```rust
// Shared state pattern
let state = Rc::new(RefCell::new(MyState::new()));
let state_clone = state.clone();

// GTK callback with move semantics
button.connect_clicked(move |_| {
    let mut s = state_clone.borrow_mut();
    s.do_thing();
});

// Cell for simple Copy types (bool, usize, enums with Copy)
let flag = Rc::new(Cell::new(false));
let flag2 = flag.clone();
// In callback: flag2.set(true);
```

#### Rc Clone Rules
- **Clone Rc BEFORE** the move closure, not inside it
- **One clone per closure** that needs the value
- **Use numbered suffixes** for readability: `s2`, `s3`, `s4`

#### Module Size
- `window.rs` ≤ 600 lines → refactor into `canvas.rs`, `toolbar.rs`, `sidebar.rs`
- `engine.rs` ≤ 500 lines → split by format (`read.rs`, `write.rs`)
- New features → new module file, not appended to existing

### 4. Shared vs. Per-App Code

| Put in suite-common if... | Keep in app module if... |
|---------------------------|------------------------|
| Used by 2+ apps | Specific to one app's domain model |
| Generic trait/struct | Tightly coupled to GTK widget layout |
| Matches LO's svl/ layer | Matches LO's sc/ or sd/ per-app code |
| Pure Rust (no GTK deps) | Contains GTK widget creation |

### 5. Commit Style

```
feat(tables): column resize via drag on header divider (#40)
fix(decks): handle empty slide list in present mode
refactor(decks): split window.rs into canvas/sidebar modules
docs: add architecture and contributing guides
```

Reference the GitHub issue number in parentheses.

### 6. Testing

```bash
# Fast feedback (no linking required)
cargo check -p tables
cargo check --workspace

# Full test suite (needs GTK4 runtime — run on build machine)
cargo test -p tables
cargo test -p suite-common --lib
```

**What to test:**
- `engine.rs` — data model operations (pure Rust, testable anywhere)
- `format.rs` — formatting logic (pure Rust, testable anywhere)
- `undo.rs` — command apply/undo (pure Rust)
- Window-level tests require GTK runtime — implement as integration tests

### 7. Before Committing

```bash
# Check for regressions
cargo check --workspace

# Run relevant tests
cargo test -p <package>

# Verify no unused imports
cargo fix --bin <app> -p <package> --allow-dirty
```

---

## Feature Implementation Checklist

When implementing a new feature issue, follow this order:

1. [ ] **Research** — grep LO source, search crates.io, check AGENT-REFERENCE-LIBRARY.md
2. [ ] **Design** — sketch data model changes, UI layout, user interaction
3. [ ] **Add data model** — struct/enum changes to engine or window module
4. [ ] **Implement logic** — pure Rust functions, no UI yet
5. [ ] **Add tests** — for the data model and logic
6. [ ] **Wire UI** — GTK widgets, event handlers, toolbar buttons
7. [ ] **Wire undo** — create undo command if mutation occurs
8. [ ] **Check HIG compliance** — widget choice, spacing, icon naming
9. [ ] **Build check** — `cargo check --workspace`
10. [ ] **Commit** with issue reference
11. [ ] **Close issue** with `gh issue close #N -c "message" -r completed`

---

## Common Pitfalls

| Pitfall | Solution |
|---------|----------|
| `Rc` doesn't implement `Copy` | Clone `Rc` explicitly before each closure |
| `set_draw_func` overwrites previous | Use a single draw func that checks state |
| Column header click conflicts with drag | Check y-coordinate: header zone vs cell zone |
| `GestueDrag` conflicts with `GestureClick` | They coexist — drag activates after threshold |
| `cairo::ImageSurface::create_from_png` expects `&mut Read` | Open `File` first, pass `&mut file` |
| `GtkTextView` has no `connect_key_pressed` | Use `EventControllerKey` on the text view |
| Ambiguous numeric type `{float}` | Add explicit type: `let x: f64 = ...` |
| `Cell::get()` requires `T: Copy` | Add `#[derive(Clone, Copy)]` to the enum/struct |
