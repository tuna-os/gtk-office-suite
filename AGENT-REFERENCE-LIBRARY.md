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

### 13. GTK4 Drag-and-Drop for Files and Images

GTK4 has a clean drag-and-drop API using `GtkDropTarget` and `GtkDragSource`.

**Single file drop** (from Nautilus, desktop, etc.):
```rust
let drop = gtk::DropTarget::new(
    gio::File::static_type(),   // GType for single file
    gdk::DragAction::COPY,
);
drop.connect_drop(|_target, value, _x, _y| {
    // value.get() returns Result<T, glib::BoolError>
    if let Ok(file) = value.get::<gio::File>() {
        if let Some(path) = file.path() {
            // Handle dropped file at path
            println!("Dropped file: {}", path.display());
        }
    }
    true  // return true to accept the drop
});
widget.add_controller(&drop);
```

**Multiple file drop** — use `glib::List` of `gio::File`:
```rust
// Use the GType for a GList of Gio.File
let drop = gtk::DropTarget::new(
    glib::List::<gio::File>::static_type(),
    gdk::DragAction::COPY,
);
drop.connect_drop(|_target, value, _x, _y| {
    if let Ok(files) = value.get::<glib::List<gio::File>>() {
        for file in files.iter() {
            if let Some(path) = file.path() {
                // Handle each dropped file
            }
        }
    }
    true
});
widget.add_controller(&drop);
```

**Image-specific:** Set the drop target on the GtkTextView (for Letters) or GtkDrawingArea (for Decks). On drop, insert the image at cursor or at the drop position.

**File drag OUT** (from app to file manager) — use `GtkDragSource`:
```rust
let source = gtk::DragSource::new();
source.connect_prepare(move |_source, _x, _y| {
    let file = gio::File::for_path("/path/to/file");
    Some(gdk::ContentProvider::for_value(&file))
});
widget.add_controller(&source);
```

**Reference URLs:**
- https://docs.gtk.org/gtk4/drag-and-drop.html
- https://discourse.gnome.org/t/drag-dropping-files-with-gtk4/6084
- https://docs.rs/gtk4/latest/gtk4/struct.DropTarget.html

### 14. Marko Editor — Rust GTK4 WYSIWYG Markdown Editor

**Repo:** `/tmp/marko-editor/` (cloned from https://github.com/mmMike/marko-editor)

**Stack:** Rust + gtk4-rs + pulldown-cmark

**Architecture:** The best reference we have for GtkTextBuffer + Markdown integration.

| Module | What it does | Use for Letters |
|--------|-------------|-----------------|
| `textbuffermd.rs` | `assign_markdown()` — parses Markdown via pulldown-cmark and applies GtkTextTags. `to_markdown()` — walks buffer tags and generates Markdown | **Directly portable** — Markdown import/export |
| `textbufferext.rs` | Cursor helpers, image/link tag creation, paragraph formatting, text move up/down | Link/image insertion pattern |
| `texttag.rs` | Tag definitions (BOLD, ITALIC, H1-H6, CODE, MONO, STRIKE, LINK, IMAGE, colors) | Tag naming convention |
| `texttagtable.rs` | Tag factory, `create_tag()`, `md_start_tag()`/`md_end_tag()` for Markdown ↔ tag mapping | Tag↔Markdown mapping table |
| `textview.rs` | Main editor widget with key handlers, undo/redo, autocomplete, search | Full editor integration pattern |
| `textviewext.rs` | TextView helpers (scroll, insert, etc.) | Scrolling helpers |

**Key insight from `textbuffermd.rs::insert_markdown()`:**
```rust
fn insert_markdown(&self, iter: &mut gtk::TextIter, markdown: &str) {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(markdown, options);
    let pos_start = iter.offset();
    for event in parser {
        match event {
            Event::Start(CTag::Heading(_)) => pos_heading = iter.offset(),
            Event::End(CTag::Heading(level)) => {
                let tag = match level { 1 => Tag::H1, ... };
                self.apply_tag_offset(iter, tag, pos_heading);
            }
            Event::Text(text) => { self.insert(iter, text.as_ref()); }
        }
    }
}
```

**NOTE:** Marko Editor does NOT implement live markdown macros (`**bold**` → bold as you type).
Formatting is done via keyboard shortcuts (Ctrl+B, Ctrl+I) and toolbar buttons.
The `insert-text` signal auto-formatting would need to be implemented on top of this.

### 15. Formualizer — Rust Spreadsheet Formula Engine

**Repo:** https://github.com/PSU3D0/formualizer
**Docs:** https://www.formualizer.dev/

**Stack:** Pure Rust + Apache Arrow storage

**Capabilities:**
- 400+ Excel-compatible functions
- Apache Arrow columnar storage (fast cell access)
- Incremental dependency tracking (only recalculate dirty cells)
- Undo/redo with automatic action grouping
- Load/save XLSX, CSV, JSON
- Deterministic mode for reproducible evaluation
- Rust, Python, and WASM APIs

**Usage pattern:**
```rust
use formualizer_workbook::*;
let mut wb = Workbook::new();
let mut sheet = wb.new_sheet("Sheet1");
sheet.set_cell_value("A1", "Hello");
sheet.set_cell_formula("B1", "SUM(A1:A10)");
let val = sheet.evaluate("A1"); // "Hello"
let val2 = sheet.evaluate("B1"); // computed value
wb.save_xlsx("output.xlsx")?;
```

**Relevance:** Dropping Formualizer into Tables would instantly give us formula evaluation, dependency tracking, undo/redo, and XLSX I/O. It's the single biggest effort multiplier for Phase 3.

**Cargo.toml addition:**
```toml
formualizer-workbook = "0.3"  # check latest version at crates.io
```

### 16. GtkColumnView — GTK4 data grid widget

**Location:** `/tmp/gtk4-rs/examples/column_view_datagrid/`

**Pattern:** Use `GtkColumnView` + `SignalListItemFactory` + custom cell widgets for read-only/tabular data display. Not suitable for spreadsheet-like free-form grids (cell selection, merged cells, freeze panes are hard).

**Best for:** Tables that display structured data (database query results, config tables).
**Not best for:** Excel-like spreadsheet with arbitrary cell editing.

**Our approach for Tables:** Continue with custom `GtkDrawingArea` + Cairo rendering (from parallel worker). This gives full control over:
- Cell-level selection (click any cell)
- Formula bar integration (fx entry updates on selection)
- Grid line rendering (configurable color, width)
- Column headers + row numbers with freeze support
- Cell editing overlay (GtkEntry positioned over cell)
- Merged cells, custom borders, number formatting

**Reference implementation:** `/home/james/dev/hanthor/hanthor-rust/tables/src/window.rs` — the parallel worker already has the Cairo grid skeleton with selection and formula bar.

### 17. IronCalc — Alternative Rust Spreadsheet Engine

**Repo:** https://github.com/ironcalc/ironcalc
**Status:** v0.7.1 (beta), 83 functions

**Comparison vs Formualizer:**
| Feature | Formualizer | IronCalc |
|---------|-------------|----------|
| Functions | 400+ | 83 |
| Storage | Apache Arrow | Rust structs |
| XLSX I/O | Built-in | Built-in |
| License | Permissive | MIT/Apache 2.0 |
| Maturity | Production (2026) | Beta |
| API ergonomics | Cargo-idiomatic | Pythonic |

**Verdict:** Formualizer is the better choice for Tables. It's more mature, has more functions, and the Arrow backend will scale better.

### 18. Column Auto-Fit on Double-Click (Spreadsheet Pattern)

**Implementation pattern for Cairo-based grids:**

```rust
/// Detect column divider hit (within DIVIDER_SENSITIVITY pixels)
fn hit_col_divider(x: f64, scroll_x: f64, sheet: &SheetModel) -> Option<usize> {
    let cx = x - ROW_HEADER_WIDTH + scroll_x;
    if cx < 0.0 { return None; }
    let mut accum = 0.0;
    for c in 0..sheet.cols {
        accum += sheet.col_width(c);
        if (cx - accum).abs() < 5.0 { return Some(c); }
    }
    None
}

/// Auto-fit column width to content
fn auto_fit_column(cr: &Context, sheet: &mut SheetModel, col: usize) {
    let layout = pangocairo::functions::create_layout(cr);
    let mut max_w = 25.0;
    // Measure header
    let label = col_label(col);
    layout.set_text(Some(&label));
    let (tw, _) = layout.pixel_size();
    max_w = max_w.max(tw as f64 + 12.0);
    // Measure cells
    for r in 0..sheet.rows {
        let val = sheet.cell(r, col);
        if val.is_empty() { continue; }
        layout.set_text(Some(val));
        if let Ok((tw, _)) = layout.pixel_size() {
            max_w = max_w.max(tw as f64 + 12.0);
        }
    }
    sheet.set_col_width(col, max_w);
}
```

**Key API:** `pangocairo::functions::create_layout(cr)` creates a PangoLayout from a Cairo context, allowing text measurement without rendering.

**Reference:** LibreOffice, Excel, Google Sheets all implement this pattern identically.
