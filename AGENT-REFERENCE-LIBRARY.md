# Agent Reference Library — Rust GTK4 Implementation Patterns

> Curated from SearXNG research + local cloned repos. Update this file as new
> reference implementations are discovered.

---

## Reference Projects (cloned locally)

| Project | Path | Language | Key Patterns |
|---------|------|----------|--------------|
| **Loupe** (Image Viewer) | `/tmp/loupe/` | Rust gtk4-rs | `GtkDrawingArea` + Cairo, `CompositeTemplate`, subclass pattern, fullscreen `ShyBin`, `EventControllerScroll/Motion` |
| **GNOME Tour** | `/tmp/gnome-tour/` | Rust gtk4-rs | `AdwCarousel` + `AdwCarouselIndicatorDots`, overlay buttons |
| **Papers** (Document Viewer) | `/tmp/papers/` | Rust + C | `find_sidebar.rs`, `search_box.rs`, `sidebar.rs`, undo/redo, `DocumentView` |
| **Finupdate** (System Updater) | `/home/james/dev/hanthor/finupdate/` | Rust gtk4-rs + relm4 | `ShortcutsWindow`, `GtkDrawingArea`, progress bars |
| **gtk4-rs examples** | `/tmp/` (need clone) | Rust | Official examples for DrawingArea, TextView, etc. |

---

## Pattern Catalog by Feature

### 1. Find & Replace in GtkTextView

**Reference:** Papers `find_sidebar.rs` + `search_box.rs`

**Key API:** `GtkTextBuffer::forward_search()` / `backward_search()` with `GtkTextSearchFlags`

```rust
// gtk4-rs 0.11 API:
let flags = gtk::TextSearchFlags::CASE_INSENSITIVE;
let result = buffer.forward_search(&start_iter, query, flags, None);
// Returns Option<(TextIter, TextIter)> — match start and end
```

**Pattern:**
- Connect `search-changed` on `GtkSearchEntry` → trigger search
- Highlight all matches with a `GtkTextTag` (yellow background)
- Track match positions in a `Vec<(TextIter, TextIter)>`
- Current match highlighted with different tag (orange)
- Case-sensitive + whole-word toggle buttons (see Papers `search_box.rs`)
- Match count display: "3 of 12 matches"

**Papers file reference:** `/tmp/papers/shell/src/search_box.rs` lines 60-120 show `case-sensitive` and `whole-words-only` action toggles. `/tmp/papers/shell/src/find_sidebar.rs` shows the results list.

### 2. Cairo Grid Rendering (Spreadsheet)

**Reference:** Loupe `image_view.rs`, `image.rs`

**Key API:** `GtkDrawingArea::set_draw_func()` with `cairo::Context`

```rust
let area = gtk::DrawingArea::new();
area.set_draw_func(move |_area, cr, width, height| {
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.paint().unwrap();
    // Draw grid lines
    cr.set_source_rgb(0.8, 0.8, 0.8);
    cr.set_line_width(0.5);
    for col in 0..26 {
        cr.move_to(col as f64 * COL_W, 0.0);
        cr.line_to(col as f64 * COL_W, height as f64);
    }
    cr.stroke().unwrap();
});
```

**Pattern:**
- `set_draw_func()` for all rendering — no more `connect_draw` like GTK3
- Use `GestureClick` for mouse interaction: `GtkGestureClick::new()` + `connect_pressed`
- `EventControllerMotion` for hover/highlight
- `ScrollController` for zoom
- Redraw via `area.queue_draw()` after state changes
- Store cell data in a 2D `Vec<Vec<String>>` shared via `Rc<RefCell<>>`

**Loupe file reference:** `/tmp/loupe/src/widgets/image_view.rs` (scroll/zoom), `/tmp/loupe/src/widgets/image/input_handling.rs` (gesture handling)

### 3. Rich Text in GtkTextView

**Key API:** `GtkTextBuffer::apply_tag()`, `GtkTextTagTable`

```rust
let tag = gtk::TextTag::builder()
    .name("bold")
    .weight(700)  // PANGO_WEIGHT_BOLD as i32
    .build();
buffer.tag_table().add(&tag);
buffer.apply_tag(&tag, &start, &end);
```

**Undo/Redo:**
- GtkTextBuffer has built-in undo/redo since GTK 4.10
- `buffer.can_undo()`, `buffer.undo()`, `buffer.redo()`
- `buffer.begin_user_action()` / `end_user_action()` for grouping
- Ctrl+Z/Y handled automatically if you use `GtkTextView`'s default keybindings, or wire via accelerators

**Important:** `GtkTextTag::weight()` takes `i32` (Pango weight value), NOT `Pango::Weight` enum. Bold = 700, Normal = 400.

### 4. Cell Selection on DrawingArea

**Reference:** Loupe `input_handling.rs`

```rust
let click = gtk::GestureClick::new();
click.connect_pressed(move |_gesture, _n_press, x, y| {
    // Convert x,y to cell coordinates
    let col = (x / COL_WIDTH) as usize;
    let row = (y / ROW_HEIGHT) as usize;
    selected_cell.set(Some((row, col)));
    area.queue_draw();
});
area.add_controller(&click);
```

**Pattern:**
- `GestureClick` for click/drag
- `EventControllerMotion` for hover
- Store `selected_cell: Cell<Option<(usize, usize)>>`
- On draw, render blue border around selected cell
- For range selection: track drag start + current position

### 5. Find & Replace Banner (GtkRevealer)

**Reference:** Papers `find_sidebar.rs`

**Pattern:**
- `GtkRevealer` containing the search/replace entry + buttons
- Slide down animation via `revealer.set_transition_type(Gtk::RevealerTransitionType::SlideDown)`
- `GtkSearchEntry` for the search field (has built-in search icon and clear button)
- Regular `GtkEntry` for replace field
- Buttons: Find Next, Find Previous, Replace, Replace All
- Toggle buttons: Case-sensitive, Whole word

**Papers file reference:** `/tmp/papers/shell/src/find_sidebar.rs` — the full sidebar implementation showing search context management and result navigation.

### 6. GSettings Binding Pattern

**Key API:** `gio::Settings`

```rust
let settings = gio::Settings::new("org.tunaos.letters-rust");
// Read
let is_dark = settings.boolean("dark-mode");
let width = settings.int("window-width");
// Write
settings.set_boolean("dark-mode", true);
// Bind (two-way)
// Not available directly in gtk4-rs 0.11 — use connect_changed signal
settings.connect_changed(Some("dark-mode"), move |settings, key| {
    let val = settings.boolean(key);
    // update UI
});
```

**Schema XML:**
```xml
<schema id="org.tunaos.letters-rust" path="/org/tunaos/letters-rust/">
  <key name="dark-mode" type="b">
    <default>false</default>
    <summary>Dark mode</summary>
  </key>
</schema>
```

### 7. AdwOverlaySplitView (Sidebar)

**Reference:** Loupe window pattern, Papers sidebar

```rust
let split = adw::OverlaySplitView::new();
split.set_sidebar(Some(&sidebar_widget));
split.set_content(Some(&content_widget));
split.set_show_sidebar(true);
split.set_collapsed(false);
// Connect to breakpoint or manual toggle
sidebar_toggle.connect_clicked(move |_| {
    split.set_show_sidebar(!split.show_sidebar());
});
```

**Breakpoint pattern (gnome-gui-spec):**
```rust
let bp = adw::Breakpoint::new(
    adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        600.0,
        adw::LengthUnit::Sp,
    )
);
bp.connect_apply(move |_| { split.set_collapsed(true); });
bp.connect_unapply(move |_| { split.set_collapsed(false); });
```

### 8. Fullscreen / Present Mode

**Reference:** Loupe `window.rs` `ShyBin` pattern, `image_window.rs`

```rust
// Enter fullscreen
window.fullscreen();
// Hide controls
header_bar.set_reveal(false);
// Listen for Escape
let controller = gtk::EventControllerKey::new();
controller.connect_key_pressed(move |_, key, _, _| {
    if key == gtk::gdk::Key::Escape {
        window.unfullscreen();
        return glib::Propagation::Stop;
    }
    glib::Propagation::Proceed
});
window.add_controller(&controller);
```

### 9. GtkDrawingArea Scroll + Zoom

**Reference:** Loupe `image.rs` zoom, scroll modules

```rust
// Scroll via GtkScrolledWindow wrapping the DrawingArea
let scroll = gtk::ScrolledWindow::new();
scroll.set_child(Some(&drawing_area));
scroll.set_vexpand(true);
scroll.set_hexpand(true);

// Zoom via Ctrl+Scroll or buttons
let scroll_controller = gtk::EventControllerScroll::new();
scroll_controller.set_flags(gtk::EventControllerScrollFlags::VERTICAL);
scroll_controller.connect_scroll(move |_, _dx, dy| {
    if /* ctrl held */ {
        zoom_level *= if dy > 0.0 { 1.1 } else { 0.9 };
        drawing_area.queue_draw();
    }
    glib::Propagation::Stop
});
```

### 10. GtkTextView Spell Check

**Key API:** `GtkTextView::set_enable_spell_checking()` (available in GTK 4.10+)

```rust
// GTK 4.10+
text_view.set_enable_spell_checking(true);
// Toggle via GSettings
let spell = settings.boolean("spell-check-enabled");
text_view.set_enable_spell_checking(spell);
```

### 11. File Format Libraries

| Library | Purpose | Links |
|---------|---------|-------|
| **calamine** | Read XLSX, XLS, ODS, CSV | `https://docs.rs/calamine/latest/` |
| **rust_xlsxwriter** | Write XLSX | `https://docs.rs/rust_xlsxwriter/latest/` |
| **pulldown-cmark** | Markdown → HTML | Already in use (letters/Cargo.toml) |
| **pandoc** (CLI) | Document conversion | Via CLI subprocess |
| **typst** (CLI) | PDF compilation | Already wired in export.rs |

**calamine usage:**
```rust
use calamine::{open_workbook, Reader, Xlsx};
let mut workbook: Xlsx<_> = open_workbook("file.xlsx").unwrap();
if let Some(Ok(range)) = workbook.worksheet_range("Sheet1") {
    for row in range.rows() {
        for cell in row {
            println!("{:?}", cell);
        }
    }
}
```

**rust_xlsxwriter usage:**
```rust
use rust_xlsxwriter::*;
let mut workbook = Workbook::new();
let sheet = workbook.add_worksheet();
sheet.write_string(0, 0, "Hello")?;
sheet.write_number(1, 0, 42)?;
workbook.save("output.xlsx")?;
```

---

## Search Queries (for future research)

When stuck, search using these queries:
```
site:github.com gtk4-rs GtkDrawingArea cairo grid
site:github.com gtk4-rs TextBuffer find replace
site:discourse.gnome.org gtk4-rs AdwTabView close confirmation
site:docs.rs gtk4 latest text_iter forward_search
```

---

## Local GNOME App UI Files (for pattern reference)

The gnome-gui-spec audits at `/tmp/gnome-gui-spec/audits/` contain Blueprint/XML
widget inventories for 34 GNOME apps. Key files for our features:
- `/tmp/gnome-gui-spec/audits/loupe-image-viewer.md` — DrawingArea, fullscreen
- `/tmp/gnome-gui-spec/audits/baobab-scan.md` — OverlaySplitView, preferences
- `/tmp/gnome-gui-spec/skills/tabbed-documents/SKILL.md` — AdwTabView pattern
- `/tmp/gnome-gui-spec/skills/toast-feedback/SKILL.md` — AdwToast pattern
- `/tmp/gnome-gui-spec/skills/sidebar-navigation/SKILL.md` — AdwOverlaySplitView
- `/tmp/gnome-gui-spec/skills/preferences-dialog/SKILL.md` — Preferences pattern

---

### 12. Velotype — Block-based Markdown Editor (gpui, not GTK)

**Repo:** `/tmp/velotype/` (cloned from https://github.com/manyougz/velotype)

**Architecture uses gpui (Zed editor framework), not gtk4-rs.** UI patterns not portable.
However, the **Markdown parsing and block-model architecture** is useful:

| Module | What it does | Use for Letters |
|--------|-------------|-----------------|
| `editor/document.rs` | Markdown → editor tree deserialization | Markdown import pipeline |
| `editor/render.rs` | Block rendering with Pango layout | Not portable (gpui) |
| `editor/events.rs` | Block-level event handling (split, merge, delete, indent) | Rich text editing commands |
| `editor/history.rs` | Undo/redo stack | Reference for custom undo stack if GtkTextBuffer built-in is insufficient |
| `editor/selection.rs` | Cursor + range selection model | Selection handling pattern |
| `editor/export.rs` | Export to PDF/HTML | PDF export pipeline reference |
| `editor/persistence.rs` | Auto-save + file I/O | Auto-save timer pattern |
| `editor/table_edit.rs` | Inline table editing | Table insert/edit UI pattern |
| `components/block/element.rs` | Block element types (P, H1-H6, code, table, image) | Block-level style model |
| `components/markdown/inline.rs` | Inline markdown parsing (**bold**, *italic*, `code`, [links]) | Markdown macros reference |

**Key reference function — inline markdown parsing:**
`/tmp/velotype/src/components/markdown/inline.rs` has regex-based inline markdown parser that handles bold, italic, code, links, images, strikethrough, footnotes.

**Key reference — table insertion:**
`/tmp/velotype/src/components/block/runtime/table.rs` has table creation + cell navigation logic.

**Note:** Velotype's AST-based approach (parse markdown → block tree → render) is more sophisticated than what we need for Phase 1-2. It's a v2/v3 target for a more powerful Letters engine.
