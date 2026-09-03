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
| **Finupdate** (System Updater) | `~/dev/finupdate/` | Rust gtk4-rs + relm4 | `ShortcutsWindow`, `GtkDrawingArea`, progress bars |
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
let settings = gio::Settings::new("org.tunaos.letters");
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
<schema id="org.tunaos.letters" path="/org/tunaos/letters/">
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

**Reference implementation:** `~/dev/gtk-office-suite/tables/src/window.rs` — the parallel worker already has the Cairo grid skeleton with selection and formula bar.

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

**Verdict:** IronCalc was selected as the formula engine for Tables due to its explicit MIT/Apache 2.0 dual-licensing, simple API ergonomics, and active integration in `tables-core/src/engine.rs`.

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

---

## Tables Patterns (Spreadsheet Grid)

### 19. Column Resize via Drag on Divider

**Reference:** Gtk4 `GestureDrag` API, Excel/LibreOffice divider behavior

**Key API:** `gtk::GestureDrag::new()` with `connect_drag_begin` / `connect_drag_update` / `connect_drag_end`

```rust
use gtk::{GestureDrag, prelude::*};

// Track which column divider is being dragged
let drag_state = Rc::new(Cell::new(None::<(usize, f64)>));

let drag = GestureDrag::new();
drag.connect_drag_begin(move |_gesture, x, _y| {
    let col = hit_col_divider(x, scroll_x, &sheet);
    drag_state.set(col.map(|c| (c, sheet.col_width(c))));
});

drag.connect_drag_update(move |_gesture, dx, _dy| {
    if let Some((col, start_width)) = drag_state.get() {
        let new_width = (start_width + dx).max(30.0).min(500.0);
        sheet.set_col_width(col, new_width);
        area.queue_draw();
    }
});

drag.connect_drag_end(move |_gesture, _dx, _dy| {
    drag_state.set(None);
    // Persist to GSettings
    settings.set_double(&format!("col-width-{}", col), new_width);
});

area.add_controller(&drag);
```

**Divider hit detection:**
```rust
fn hit_col_divider(x: f64, scroll_x: f64, sheet: &SheetModel) -> Option<usize> {
    let cx = x - ROW_HEADER_WIDTH + scroll_x;
    if cx < 0.0 { return None; }
    if y < COL_HEADER_HEIGHT { /* only in header zone */ }
    let mut accum = 0.0;
    for c in 0..sheet.cols {
        accum += sheet.col_width(c);
        if (cx - accum).abs() < 5.0 { return Some(c); }
    }
    None
}
```

**Cursor feedback:** Set `area.set_cursor_from_name(Some("col-resize"))` when hovering over a divider. Use `EventControllerMotion` to detect hover position.

**Double-click to auto-fit:** On `GestureClick` with `n_press == 2`, if click hits a divider zone, auto-fit the column (see Pattern #18).

---

### 20. Sort and Filter on Column Headers

**Pattern:** Click column header → sort ascending. Click again → sort descending. Click again → remove sort. Context menu or right-click → filter dialog.

**Sort implementation (in-place on SheetModel data):**
```rust
enum SortDirection { Ascending, Descending, None }

fn sort_by_column(sheet: &mut SheetModel, col: usize, dir: SortDirection) {
    if col >= sheet.cols { return; }
    let mut indices: Vec<usize> = (0..sheet.rows).collect();
    match dir {
        SortDirection::Ascending => {
            indices.sort_by(|a, b| {
                sheet.data[*a][col].cmp(&sheet.data[*b][col])
            });
        }
        SortDirection::Descending => {
            indices.sort_by(|a, b| {
                sheet.data[*b][col].cmp(&sheet.data[*a][col])
            });
        }
        SortDirection::None => return, // remove filter
    }
    // Reorder rows
    let old_data = sheet.data.clone();
    for (new_row, &old_row) in indices.iter().enumerate() {
        sheet.data[new_row] = old_data[old_row].clone();
    }
}
```

**Sort indicator in header:** Draw a small ▲ or ▼ arrow next to the column label. Store `sorted_col: Option<(usize, SortDirection)>` in SheetModel.

**Filter pattern:** Filter operates on a column, hiding rows that don't match. Store `filters: Vec<Option<FilterRule>>` per column.

```rust
enum FilterRule {
    TextContains(String),
    TextEquals(String),
    NumberGreaterThan(f64),
    NumberLessThan(f64),
    NumberBetween(f64, f64),
    NotEmpty,
    Custom(Box<dyn Fn(&str) -> bool>),
}

fn filtered_rows(sheet: &SheetModel) -> Vec<usize> {
    (0..sheet.rows).filter(|&r| {
        sheet.filters.iter().enumerate().all(|(c, filter)| {
            match filter {
                Some(FilterRule::TextContains(q)) => sheet.data[r][c].contains(q),
                Some(FilterRule::NumberGreaterThan(n)) => {
                    sheet.data[r][c].parse::<f64>().map(|v| v > *n).unwrap_or(false)
                }
                // ... other filter types
                None => true,
            }
        })
    }).collect()
}
```

**UI for filter:** AdwDialog with a popover-style list of filter options. Excel builds a unique-values dropdown; we can start with text match and numeric range.

**Reference:** Excel/LibreOffice column header click-to-sort behavior. Filter dialog from Google Sheets.

---

### 21. Merge Cells — Spanning a Rect Over Multiple Cells

**Pattern:** Store merge regions in `SheetModel` as `Vec<(row_start, col_start, row_end, col_end)>`. Render merged cells as a single painted rectangle with centered content from the top-left cell.

**Data model extension:**
```rust
pub struct SheetModel {
    // ... existing fields
    pub merges: Vec<(usize, usize, usize, usize)>, // (r1, c1, r2, c2)
}

impl SheetModel {
    fn is_merged(&self, r: usize, c: usize) -> Option<(usize, usize, usize, usize)> {
        self.merges.iter().find(|&&(r1, c1, r2, c2)| {
            r >= r1 && r <= r2 && c >= c1 && c <= c2
        }).copied()
    }

    fn merge_cells(&mut self, r1: usize, c1: usize, r2: usize, c2: usize) {
        // Remove any overlapping merges first
        self.merges.retain(|&(mr1, mc1, mr2, mc2)| {
            r2 < mr1 || r1 > mr2 || c2 < mc1 || c1 > mc2
        });
        self.merges.push((r1, c1, r2, c2));
    }

    fn unmerge(&mut self, r: usize, c: usize) {
        self.merges.retain(|&(r1, c1, r2, c2)| {
            !(r >= r1 && r <= r2 && c >= c1 && c <= c2)
        });
    }
}
```

**Rendering merged cells:**
```rust
fn draw_merged_cell(cr: &Context, sheet: &SheetModel, merge: (usize, usize, usize, usize),
                    scroll_x: f64, scroll_y: f64) {
    let (r1, c1, r2, c2) = merge;
    // Calculate total width/height
    let x = ROW_HEADER_WIDTH + c1 as f64 * COL_WIDTH - scroll_x;
    let y = COL_HEADER_HEIGHT + r1 as f64 * ROW_HEIGHT - scroll_y;
    let total_w: f64 = (c1..=c2).map(|c| sheet.col_width(c)).sum();
    let total_h = (r2 - r1 + 1) as f64 * ROW_HEIGHT;

    cr.rectangle(x, y, total_w, total_h);
    cr.stroke().unwrap();
    // Draw content from top-left cell
    cr.move_to(x + 4.0, y + ROW_HEIGHT - 8.0);
    cr.show_text(sheet.cell(r1, c1)).unwrap();
}
```

**In draw loop:** Skip grid lines for cells inside a merge region. Draw merged regions as a separate layer on top.

**Reference:** Excel merge & center, LibreOffice Format → Merge Cells.

---

### 22. Freeze Panes — Lock Row/Column Headers During Scroll

**Pattern:** Split the Cairo rendering into fixed and scrollable regions. Draw frozen rows/columns with a separate offset, using thicker border lines at the freeze boundary.

**Data model:**
```rust
pub struct SheetModel {
    // ... existing fields
    pub frozen_rows: usize,  // 0 = no freeze, 1+ = first N rows frozen
    pub frozen_cols: usize,  // 0 = no freeze, 1+ = first N columns frozen
}
```

**Rendering approach (Cairo clip regions):**
```rust
fn draw_grid_with_freeze(cr: &Context, state: &AppState, width: f64, height: f64,
                         scroll_x: f64, scroll_y: f64) {
    let sh = state.sheet();
    let fr = sh.frozen_rows;
    let fc = sh.frozen_cols;

    // 1. Draw frozen row headers (top-left corner)
    let freeze_y = COL_HEADER_HEIGHT + fr as f64 * ROW_HEIGHT;
    let freeze_x = ROW_HEADER_WIDTH + fc as f64 * COL_WIDTH; // using uniform width for freeze cols

    // 2. Top-left fixed corner (header intersection)
    cr.save().unwrap();
    cr.rectangle(0.0, 0.0, freeze_x, freeze_y);
    cr.clip();
    draw_cells(cr, sh, 0.0, 0.0, 0, fc, 0, fr); // no scroll offset frozen area
    cr.restore().unwrap();

    // 3. Top scrollable row header band
    cr.save().unwrap();
    cr.rectangle(freeze_x, 0.0, width - freeze_x, freeze_y);
    cr.clip();
    draw_cells(cr, sh, scroll_x, 0.0, fc, sh.cols, 0, fr);
    cr.restore().unwrap();

    // 4. Left fixed column band
    cr.save().unwrap();
    cr.rectangle(0.0, freeze_y, freeze_x, height - freeze_y);
    cr.clip();
    draw_cells(cr, sh, 0.0, scroll_y, 0, fc, fr, sh.rows);
    cr.restore().unwrap();

    // 5. Scrollable area
    cr.save().unwrap();
    cr.rectangle(freeze_x, freeze_y, width - freeze_x, height - freeze_y);
    cr.clip();
    draw_cells(cr, sh, scroll_x, scroll_y, fc, sh.cols, fr, sh.rows);
    cr.restore().unwrap();

    // 6. Draw freeze boundary lines (thicker, darker)
    cr.set_source_rgb(0.3, 0.3, 0.3);
    cr.set_line_width(2.0);
    if fr > 0 { cr.move_to(0.0, freeze_y); cr.line_to(width, freeze_y); cr.stroke().unwrap(); }
    if fc > 0 { cr.move_to(freeze_x, 0.0); cr.line_to(freeze_x, height); cr.stroke().unwrap(); }
}
```

**Scrolling behavior:** The scroll offset only applies to the unfrozen portion. Use `v_adj`/`h_adj` for position within the scrollable area only.

**UI toggle:** Menu items "Freeze First Row", "Freeze First Column", "Freeze Panes" (at selection). Set `SheetModel.frozen_rows`/`frozen_cols` accordingly.

**Reference:** Excel View → Freeze Panes, LibreOffice View → Freeze Cells.

---

### 23. Number Formatting (Currency, Date, Percent, etc.)

**Pattern:** Each cell has a format string (or enum). On display, format the raw value through the format before rendering. On edit, show the raw value or formula.

**Rust crate:** `num-format` — locale-aware number formatting with grouping separators.

**Cargo.toml:**
```toml
num-format = "0.4"
chrono = "0.4"  # Date parsing/formatting
```

**Format enum and display logic:**
```rust
use num_format::{Locale, ToFormattedString};

#[derive(Clone, Debug, PartialEq)]
pub enum NumberFormat {
    General,               // Default, no special formatting
    Number(u8),            // Decimal places: Number(2) = "1,234.56"
    Currency(String, u8),  // Currency("$", 2) = "$1,234.56"
    Percent(u8),           // Percent(1) = "12.3%" (value 0.123)
    Date(String),          // Date("%Y-%m-%d") = "2026-06-23"
    DateTime(String),      // DateTime("%Y-%m-%d %H:%M")
    Scientific(u8),        // Scientific(2) = "1.23e3"
    Text,                  // Displayed as-is
}

impl NumberFormat {
    fn format(&self, raw: &str) -> String {
        // First try to parse as number
        let num = raw.parse::<f64>();
        match self {
            NumberFormat::General => raw.to_string(),
            NumberFormat::Number(dp) => {
                if let Ok(n) = num {
                    format!("{:.*}", *dp as usize, n)
                        .parse::<f64>().unwrap()
                        .to_formatted_string(&Locale::en)
                } else { raw.to_string() }
            }
            NumberFormat::Currency(sym, dp) => {
                if let Ok(n) = num {
                    format!("{}{:.*}", sym, *dp as usize, n)
                } else { raw.to_string() }
            }
            NumberFormat::Percent(dp) => {
                if let Ok(n) = num {
                    format!("{:.*}%", *dp as usize, n * 100.0)
                } else { raw.to_string() }
            }
            NumberFormat::Date(fmt) => {
                // Parse Excel serial date number or ISO string
                if let Ok(serial) = raw.parse::<f64>() {
                    excel_serial_to_date(serial).format(fmt).to_string()
                } else if let Ok(dt) = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
                    dt.format(fmt).to_string()
                } else { raw.to_string() }
            }
            _ => raw.to_string(),
        }
    }
}

/// Convert Excel serial date number (days since 1899-12-30) to chrono NaiveDate
fn excel_serial_to_date(serial: f64) -> chrono::NaiveDate {
    let epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).unwrap();
    epoch + chrono::Duration::days(serial as i64)
}
```

**Per-cell format storage:**
```rust
pub struct SheetModel {
    // ... existing fields
    pub formats: Vec<Vec<NumberFormat>>, // Format per cell
}
```

**UI:** Toolbar dropdown with format options. Right-click → Format Cells dialog (AdwPreferencesDialog-style for per-cell formatting, borders, fill).

**Reference:** Excel Format Cells dialog, Google Sheets Format → Number menu.

---

### 24. Cell Borders UI

**Pattern:** Draw borders via Cairo strokes. Store border state per-cell or per-edge.

**Border data model:**
```rust
#[derive(Clone, Debug, Default)]
pub struct CellBorder {
    pub top: Option<BorderStyle>,
    pub bottom: Option<BorderStyle>,
    pub left: Option<BorderStyle>,
    pub right: Option<BorderStyle>,
}

#[derive(Clone, Debug)]
pub struct BorderStyle {
    pub color: (f64, f64, f64),
    pub width: f64,
    pub pattern: BorderPattern,
}

#[derive(Clone, Debug)]
pub enum BorderPattern {
    Solid, Dashed, Dotted, Double, None,
}
```

**Cairo border rendering:**
```rust
fn draw_cell_borders(cr: &Context, x: f64, y: f64, w: f64, h: f64, border: &CellBorder) {
    for (edge, style, ex, ey, ew, eh) in [
        (&border.top, TOP_OFFSET, x, y, w, 0.0),
        (&border.bottom, BOTTOM_OFFSET, x, y + h, w, 0.0),
        (&border.left, LEFT_OFFSET, x, y, 0.0, h),
        (&border.right, RIGHT_OFFSET, x + w, y, 0.0, h),
    ] {
        if let Some(style) = edge {
            cr.set_source_rgb(style.color.0, style.color.1, style.color.2);
            cr.set_line_width(style.width);
            match style.pattern {
                BorderPattern::Dashed => cr.set_dash(&[4.0, 4.0], 0.0),
                BorderPattern::Dotted => cr.set_dash(&[1.0, 3.0], 0.0),
                BorderPattern::Double => {
                    cr.move_to(x, y); cr.line_to(x + w, y);
                    cr.move_to(x, y + 2.0); cr.line_to(x + w, y + 2.0);
                    cr.stroke().unwrap();
                    continue;
                }
                _ => cr.set_dash(&[], 0.0),
            }
            cr.move_to(x, y + h); // adjust for edge
            cr.stroke().unwrap();
        }
    }
}
```

**Borders toolbar:** Toggle buttons for each edge (top, bottom, left, right, all, outer, none), color picker, line style dropdown. Examples: Excel borders dropdown, LibreOffice Format → Cells → Borders tab.

**Reference:** Excel Home → Borders dropdown, LibreOffice Format Cells dialog.

---

### 25. Data Validation (Dropdown Lists, Number Ranges)

**Pattern:** Associate validation rules with cells. Show a dropdown picker or warning on invalid input.

**Data model:**
```rust
#[derive(Clone, Debug)]
pub enum ValidationRule {
    List(Vec<String>),                          // Dropdown from fixed list
    RangeList { sheet: usize, col: usize, rows: (usize, usize) },  // List from range
    WholeNumber { min: Option<i64>, max: Option<i64> },
    Decimal { min: Option<f64>, max: Option<f64> },
    TextLength { min: Option<usize>, max: Option<usize> },
    Custom(Box<dyn Fn(&str) -> bool>),          // Not serializable, use for runtime
    Regex(String),                              // Pattern match
}

pub struct SheetModel {
    // ... existing fields
    pub validations: Vec<Vec<Option<ValidationRule>>>,
}

impl ValidationRule {
    fn validate(&self, value: &str) -> bool {
        match self {
            ValidationRule::List(items) => items.iter().any(|i| i == value),
            ValidationRule::WholeNumber { min, max } => {
                value.parse::<i64>().ok().map_or(false, |v| {
                    min.map_or(true, |m| v >= m) && max.map_or(true, |m| v <= m)
                })
            }
            ValidationRule::Decimal { min, max } => {
                value.parse::<f64>().ok().map_or(false, |v| {
                    min.map_or(true, |m| v >= m) && max.map_or(true, |m| v <= m)
                })
            }
            ValidationRule::Regex(pattern) => {
                regex::Regex::new(pattern).map_or(false, |re| re.is_match(value))
            }
            _ => true,
        }
    }
}
```

**Dropdown rendering:** For `List` validation, when a cell is selected, show a `GtkPopover` or inline `GtkDropDown` with the options. This is the inline editing pattern (Pattern #4 on DrawingArea, using a positioned overlay widget).

**Invalid input warning:** On cell commit (Enter or focus loss), if validation fails, show an `AdwToast` or `AdwAlertDialog`.

```rust
// After cell edit commit:
if let Some(rule) = &sheet.validations[row][col] {
    if !rule.validate(&new_value) {
        let toast = adw::Toast::new("Invalid input");
        toast.set_timeout(3);
        toast_overlay.add_toast(toast);
        // Revert to previous value
        sheet.data[row][col] = old_value.clone();
    }
}
```

**Input message:** Optional tooltip shown when cell is selected, explaining what input is expected.

**Reference:** Excel Data → Data Validation dialog, Google Sheets Data → Data validation.

---

### 26. Charts Dialog Integration

**Status:** `charts.rs` engine exists (Pattern #18 in existing file). Renders Bar/Line/Pie to `ImageSurface`. Missing: dialog for selecting data ranges, chart type, labels, titles before rendering.

**Dialog design (AdwDialog with stacked pages):**
- **Page 1 — Data:** Column range selector (two `GtkSpinButton` for start/end column), row range, series name entry
- **Page 2 — Type:** Radio buttons for Bar/Line/Pie with preview thumbnails
- **Page 3 — Labels:** Chart title entry, axis labels, legend toggle
- **Preview:** Live Cairo `DrawingArea` updating as parameters change

**Widget tree:**
```rust
let dialog = adw::Dialog::builder()
    .title("Insert Chart")
    .content_width(500).content_height(450)
    .build();

let toolbar_view = adw::ToolbarView::new();
// Add header bar with Cancel/Insert buttons

let stack = gtk::Stack::new();
// Page 0: Data range selectors
// Page 1: Chart type picker  
// Page 2: Labels & title
// Page 3: Preview

let data_preview = gtk::DrawingArea::new();
data_preview.set_draw_func(move |_, cr, w, h| {
    let chart_surface = charts::render_chart(&preview_data, chart_type, w, h);
    cr.set_source_surface(&chart_surface, 0.0, 0.0).unwrap();
    cr.paint().unwrap();
});

toolbar_view.set_content(Some(&stack));
dialog.set_child(Some(&toolbar_view));
dialog.present(parent_window);
```

**Data extraction for chart:** From the currently selected range (or ask user to select range first), extract labels from first column and values from the selected column.

**Reference:** Excel Insert → Chart dialog, Google Sheets Insert → Chart sidebar.

---

## Decks Patterns (Presentation Canvas)

### 27. Canvas Undo/Redo with Command Pattern

**Reference:** Command pattern from GoF, Rust trait-based implementation, GTK TextBuffer undo stack

**Pattern:** Store a stack of `CanvasCommand` trait objects. Each command knows how to apply and revert itself. Commands are pushed after each canvas mutation.

```rust
use std::rc::Rc;
use std::cell::RefCell;

trait CanvasCommand {
    fn apply(&self, slides: &mut Vec<Slide>);
    fn undo(&self, slides: &mut Vec<Slide>);
    fn description(&self) -> &str;
}

// Concrete commands
struct AddObject { slide_idx: usize, object: SlideObject }
struct DeleteObject { slide_idx: usize, index: usize, object: SlideObject }
struct MoveObject { slide_idx: usize, index: usize, dx: f64, dy: f64 }
struct ResizeObject { slide_idx: usize, index: usize, old_w: f64, old_h: f64, new_w: f64, new_h: f64 }
struct ChangeText { slide_idx: usize, index: usize, old_text: String, new_text: String }
struct ReorderSlides { from: usize, to: usize }
struct AddSlide { index: usize }
struct DeleteSlide { index: usize, slide: Slide }

impl CanvasCommand for MoveObject {
    fn apply(&self, slides: &mut Vec<Slide>) {
        apply_offset(&mut slides[self.slide_idx].objects[self.index], self.dx, self.dy);
    }
    fn undo(&self, slides: &mut Vec<Slide>) {
        apply_offset(&mut slides[self.slide_idx].objects[self.index], -self.dx, -self.dy);
    }
    fn description(&self) -> &str { "Move Object" }
}

fn apply_offset(obj: &mut SlideObject, dx: f64, dy: f64) {
    match obj {
        SlideObject::TextBox { x, y, .. } | SlideObject::Rect { x, y, .. } => {
            *x += dx; *y += dy;
        }
        SlideObject::Circle { x, y, .. } => { *x += dx; *y += dy; }
        SlideObject::Image { x, y, .. } => { *x += dx; *y += dy; }
    }
}
```

**Undo stack manager:**
```rust
pub struct UndoManager {
    undo_stack: Vec<Box<dyn CanvasCommand>>,
    redo_stack: Vec<Box<dyn CanvasCommand>>,
    slides: Rc<RefCell<Vec<Slide>>>,
}

impl UndoManager {
    pub fn execute(&mut self, cmd: Box<dyn CanvasCommand>) {
        cmd.apply(&mut self.slides.borrow_mut());
        self.undo_stack.push(cmd);
        self.redo_stack.clear(); // Invalidate redo after new action
    }

    pub fn undo(&mut self) -> bool {
        if let Some(cmd) = self.undo_stack.pop() {
            cmd.undo(&mut self.slides.borrow_mut());
            self.redo_stack.push(cmd);
            true
        } else { false }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(cmd) = self.redo_stack.pop() {
            cmd.apply(&mut self.slides.borrow_mut());
            self.undo_stack.push(cmd);
            true
        } else { false }
    }
}
```

**Keyboard bindings:** Ctrl+Z → `undo_manager.undo()`, Ctrl+Shift+Z → `undo_manager.redo()`. Wire to `EventControllerKey` on the canvas.

**Integration with GtkTextBuffer undo:** For text editing within text boxes, we can use a separate approach — `GtkTextBuffer` has its own undo stack. When a text box is double-clicked, show an inline `GtkTextView` with built-in undo. On commit, generate a `ChangeText` command for the canvas-level undo stack.

**Granularity:** Each discrete user action creates one command. Continuous operations (dragging an object) create a single command with the net offset, not per-pixel commands.

**Reference:** Papers undo/redo, GIMP command pattern, GoF Command pattern. `https://en.wikipedia.org/wiki/Command_pattern`

---

### 28. Object Drag on Canvas (Move + Resize)

**Reference:** Loupe `input_handling.rs`, GTK4 `GestureDrag`

**Pattern:** Hit-test objects on `GestureDrag::drag_begin`. On `drag_update`, offset the object. On `drag_end`, commit to undo stack.

```rust
// Drag state
let drag_state = Rc::new(Cell::new(None::<(usize, f64, f64)>)); // (object_index, orig_x, orig_y)

let drag = gtk::GestureDrag::new();
let slides = slides.clone();
let current_slide = current_slide.clone();
let canvas = canvas.clone();
let undo = undo_manager.clone();

drag.connect_drag_begin(move |_gesture, x, y| {
    let idx = current_slide.get();
    let sl = slides.borrow();
    if idx >= sl.len() { return; }
    // Convert canvas coords to slide coords (accounting for 16:9 scaling)
    let (sx, sy) = canvas_to_slide(x, y, canvas_w, canvas_h);
    if let Some(oi) = hit_test_object(&sl[idx].objects, sx, sy) {
        let pos = object_position(&sl[idx].objects[oi]);
        drag_state.set(Some((oi, pos.0, pos.1)));
    }
});

drag.connect_drag_update(move |_gesture, dx, dy| {
    if let Some((oi, orig_x, orig_y)) = drag_state.get() {
        let idx = current_slide.get();
        let mut sl = slides.borrow_mut();
        if idx < sl.len() && oi < sl[idx].objects.len() {
            set_object_position(&mut sl[idx].objects[oi], orig_x + dx, orig_y + dy);
            canvas.queue_draw();
        }
    }
});

drag.connect_drag_end(move |_gesture, dx, dy| {
    if let Some((oi, _, _)) = drag_state.get() {
        // Commit to undo stack with net offset
        undo.borrow_mut().execute(Box::new(
            MoveObject { slide_idx: current_slide.get(), index: oi, dx, dy }
        ));
    }
    drag_state.set(None);
});

canvas.add_controller(&drag);
```

**Coordinate conversion:** The canvas draws slides at 16:9 aspect ratio within the DrawingArea. Convert screen coordinates to slide coordinate system (960×540 for HD slides).

```rust
fn canvas_to_slide(x: f64, y: f64, canvas_w: f64, canvas_h: f64) -> (f64, f64) {
    let slide_w = canvas_w * 0.85;
    let slide_h = slide_w * 9.0 / 16.0;
    let ox = (canvas_w - slide_w) / 2.0;
    let oy = (canvas_h - slide_h) / 2.0;
    let sx = (x - ox) / slide_w * 960.0;  // 960 = standard slide width
    let sy = (y - oy) / slide_h * 540.0;  // 540 = standard slide height
    (sx, sy)
}
```

**Resize handles:** Draw 8 small squares at the corners and edge midpoints of the selected object. A separate `GestureDrag` detects if the drag starts on a handle rather than the object body.

**Reference:** Loupe, Inkscape selection handles, PowerPoint resize handles.

---

### 29. Slide Transitions (Fade, Slide, Zoom)

**Reference:** `AdwAnimation` / `AdwTimedAnimation` with `AdwCallbackAnimationTarget`

**Approach 1 — GSK Render Nodes (hardware-accelerated):** Use `GtkSnapshot` + `GskTransform` to animate slide changes. More complex but smoother.

**Approach 2 — Cairo double-buffering with timer (simpler, works now):** Render old slide to an `ImageSurface`, render new slide to another, crossfade using `glib::timeout_add()` for animation frames.

```rust
use glib::{timeout_add_local, ControlFlow};

struct TransitionState {
    from_surface: Option<cairo::ImageSurface>,
    to_surface: Option<cairo::ImageSurface>,
    progress: f64,  // 0.0 → 1.0
    active: bool,
}

fn start_fade_transition(
    state: Rc<RefCell<TransitionState>>,
    from_slide: &Slide, to_slide: &Slide,
    area: &gtk::DrawingArea,
) {
    // Pre-render both slides to surfaces
    let from_surf = render_slide_to_surface(from_slide, 960, 540);
    let to_surf = render_slide_to_surface(to_slide, 960, 540);
    state.borrow_mut().from_surface = Some(from_surf);
    state.borrow_mut().to_surface = Some(to_surf);
    state.borrow_mut().progress = 0.0;
    state.borrow_mut().active = true;

    let ts = state.clone();
    let da = area.clone();
    timeout_add_local(std::time::Duration::from_millis(16), move || {
        let mut ts = ts.borrow_mut();
        ts.progress += 0.04;  // ~25 frames for 400ms transition
        if ts.progress >= 1.0 {
            ts.progress = 1.0;
            ts.active = false;
            ts.from_surface = None;
            ts.to_surface = None;
            da.queue_draw();
            return ControlFlow::Break;
        }
        da.queue_draw();
        ControlFlow::Continue
    });
}

// In drawing function, when transition is active:
fn draw_transition(cr: &cairo::Context, state: &TransitionState) {
    if !state.active { return; }
    let t = state.progress;
    // Ease-out cubic for smoother feel
    let eased = 1.0 - (1.0 - t).powi(3);

    // Draw "from" slide at opacity (1.0 - eased)
    if let Some(ref surf) = state.from_surface {
        cr.save().unwrap();
        cr.set_source_surface(surf, 0.0, 0.0).unwrap();
        cr.paint_with_alpha(1.0 - eased).unwrap();
        cr.restore().unwrap();
    }

    // Draw "to" slide at opacity eased
    if let Some(ref surf) = state.to_surface {
        cr.save().unwrap();
        cr.set_source_surface(surf, 0.0, 0.0).unwrap();
        cr.paint_with_alpha(eased).unwrap();
        cr.restore().unwrap();
    }
}
```

**Slide transition (push left/right):** Offset the surfaces instead of crossfading.
```rust
// For slide-left transition:
cr.set_source_surface(&from_surf, -slide_w * eased, 0.0).unwrap();
cr.paint().unwrap();
cr.set_source_surface(&to_surf, slide_w * (1.0 - eased), 0.0).unwrap();
cr.paint().unwrap();
```

**Zoom transition:** Scale and translate the surfaces.

**AdwAnimation approach (for widget-level, not canvas):**
```rust
// AdwAnimation is for widget properties, not Cairo surfaces.
// Use for animating sidebar collapse, dialog entry, etc.
let target = adw::CallbackAnimationTarget::new(move |value| {
    widget.set_opacity(value);
});
let animation = adw::TimedAnimation::builder()
    .widget(&widget)
    .duration(300)
    .value_from(0.0).value_to(1.0)
    .target(&target)
    .build();
animation.play();
```

**Reference:** PowerPoint transitions, `iovxw/gtk-rs-animation` examples, GNOME Builder transition animations.

---

### 30. Snap-to-Grid and Alignment Guides

**Reference:** Inkscape grid/guides, PowerPoint snap-to-grid, GNOME Builder snapping

**Pattern:** Define a grid spacing (e.g., 20px). During object drag, snap the object position to the nearest grid point. Draw faint grid lines on the canvas.

```rust
const GRID_SPACING: f64 = 20.0;  // In slide coordinates (960x540)

fn snap_to_grid(value: f64, grid: f64) -> f64 {
    (value / grid).round() * grid
}

// During drag update:
let snapped_x = snap_to_grid(orig_x + dx, GRID_SPACING);
let snapped_y = snap_to_grid(orig_y + dy, GRID_SPACING);
set_object_position(&mut objects[oi], snapped_x, snapped_y);
```

**Smart alignment guides:** When an object's edges or center align with another object or the slide center, draw temporary magenta lines.

```rust
fn find_alignments(obj: &SlideObject, others: &[SlideObject]) -> Vec<(f64, f64, f64, f64)> {
    // Returns list of guide lines as (x1, y1, x2, y2)
    let (ox, oy, ow, oh) = object_bounds(obj);
    let ocx = ox + ow / 2.0;
    let ocy = oy + oh / 2.0;
    let mut guides = Vec::new();

    // Slide center alignment
    if (ocx - 480.0).abs() < 5.0 {
        guides.push((480.0, 0.0, 480.0, 540.0));
    }
    if (ocy - 270.0).abs() < 5.0 {
        guides.push((0.0, 270.0, 960.0, 270.0));
    }

    // Edge alignment with other objects
    for other in others.iter().filter(|o| !std::ptr::eq(*o, obj)) {
        let (oox, ooy, oow, ooh) = object_bounds(other);
        // Left edge match
        if (ox - oox).abs() < 5.0 { guides.push((ox, 0.0, ox, 540.0)); }
        // Right edge match
        if ((ox + ow) - (oox + oow)).abs() < 5.0 { guides.push((ox + ow, 0.0, ox + ow, 540.0)); }
        // Center X match
        if (ocx - (oox + oow / 2.0)).abs() < 5.0 { guides.push((ocx, 0.0, ocx, 540.0)); }
    }
    guides
}
```

**Rendering guides:** In the draw function, after drawing objects, draw guides as thin magenta lines.

**Grid rendering:** Draw faint gray dots or lines at grid intervals on the canvas background (behind the slide).

**UI toggle:** Menu item "View → Show Grid" and "View → Snap to Grid". Store in GSettings.

**Reference:** Inkscape document properties grid, PowerPoint View → Grid and Guides, GNOME Builder blog post on snapping (2026-05-28).

---

### 31. Speaker Notes

**Pattern:** Each slide has a `notes: String` field. In present mode, show notes on a secondary window or in a bottom panel. During editing, show notes in a collapsible bottom pane.

**Data model:**
```rust
pub struct Slide {
    pub title: String,
    pub background: String,
    pub objects: Vec<SlideObject>,
    pub notes: String,  // NEW: speaker notes
}
```

**Editing UI:** Add a `GtkExpander` or collapsible pane below the canvas.
```rust
let notes_expander = gtk::Expander::new(Some("Speaker Notes"));
let notes_buffer = gtk::TextBuffer::new(None);
let notes_view = gtk::TextView::with_buffer(&notes_buffer);
notes_view.set_wrap_mode(gtk::WrapMode::Word);
notes_view.set_min_content_height(80);
notes_expander.set_child(Some(&notes_view));

// On slide change, load notes
connect_slide_changed(move |idx| {
    let slides = slides.borrow();
    if let Some(slide) = slides.get(idx) {
        notes_buffer.set_text(&slide.notes);
    }
});

// On notes change, save to slide
notes_buffer.connect_changed(move |buf| {
    let idx = current_slide.get();
    let mut slides = slides.borrow_mut();
    if let Some(slide) = slides.get_mut(idx) {
        slide.notes = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
    }
});
```

**Present mode:** In fullscreen presentation, show a popup window or use a dual-monitor setup (one monitor for audience, one for presenter with notes + next slide preview). This is a v2 feature.

**PPTX roundtrip:** Speaker notes are stored in `<p:notes>` elements in the PPTX ZIP. Extend `read_pptx`/`write_pptx` to handle the notes slide relationship. For MVP, notes can be stored as custom XML properties or simply omitted from PPTX roundtrip.

**Reference:** PowerPoint speaker notes pane, Google Slides "Speaker notes" bottom panel.

---

### 32. Master Slides & Templates

**Pattern:** A master slide defines default background, placeholder positions, fonts, and colors. Each content slide references a master and inherits its properties but can override them.

**Data model:**
```rust
pub struct Deck {
    pub slides: Vec<Slide>,
    pub masters: Vec<MasterSlide>,         // NEW
    pub slide_layouts: Vec<SlideLayout>,   // NEW: references a master + defines placeholders
}

pub struct MasterSlide {
    pub name: String,
    pub background: String,
    pub default_font: String,
    pub default_color: (f64, f64, f64),
    pub shapes: Vec<SlideObject>,  // Persistent shapes (logos, headers, footers)
}

pub struct SlideLayout {
    pub name: String,
    pub master_idx: usize,
    pub placeholders: Vec<Placeholder>,
}

pub struct Placeholder {
    pub kind: PlaceholderKind,  // Title, Body, Image, Footer, SlideNumber
    pub x: f64, pub y: f64, pub w: f64, pub h: f64,
}

pub enum PlaceholderKind {
    Title, Body, Subtitle, Image, Footer, SlideNumber, Date,
}

pub struct Slide {
    // ... existing fields
    pub layout_idx: Option<usize>,  // Reference to a layout in Deck
}
```

**Rendering:** First draw master slide shapes (background, logos), then draw slide content on top.

```rust
fn draw_slide_with_master(cr: &cairo::Context, slide: &Slide, master: Option<&MasterSlide>) {
    // 1. Master background
    if let Some(m) = master {
        cr.set_source_rgb(/* parse m.background */);
        cr.paint().unwrap();
        // Draw master shapes (logos, headers)
        for obj in &m.shapes {
            draw_object(cr, obj);
        }
    } else {
        // Default white background
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.paint().unwrap();
    }

    // 2. Slide content
    for obj in &slide.objects {
        draw_object(cr, obj);
    }
}
```

**UI:** "Slide → Edit Master" enters master view. Similar to PowerPoint's Slide Master view with a separate master/layout list on the left.

**PPTX support:** Master slides map to `ppt/slideMasters/slideMaster1.xml` and layouts to `ppt/slideLayouts/slideLayout1.xml`. For MVP, add a single default master and layout; full PPTX master roundtrip is a v2 feature.

**Reference:** PowerPoint Slide Master view, LibreOffice Impress View → Master Slide.

---

### 33. Text Editing on Slides (Inline TextView)

**Pattern:** Double-click a `SlideObject::TextBox` to open an inline `GtkTextView` overlay positioned over the text box. On commit (Enter or focus loss), update the text box content.

**Implementation (similar to Tables inline edit — Pattern #4):**
```rust
let dbl_click = gtk::GestureClick::new();
dbl_click.set_button(1);
dbl_click.connect_pressed(move |_g, n, x, y| {
    if n < 2 { return; }
    let (sx, sy) = canvas_to_slide(x, y, canvas_w, canvas_h);
    let idx = current_slide.get();
    let slides_ref = slides.borrow();
    if let Some(slide) = slides_ref.get(idx) {
        // Find text box under click
        for (oi, obj) in slide.objects.iter().enumerate() {
            if let SlideObject::TextBox { text, x: ox, y: oy, w: ow, h: oh } = obj {
                if sx >= *ox && sx <= *ox + *ow && sy >= *oy && sy <= *oy + *oh {
                    // Position overlay
                    let (cvx, cvy) = slide_to_canvas(*ox, *oy, canvas_w, canvas_h);
                    let (_, cvh) = slide_to_canvas(0.0, *oh, canvas_w, canvas_h);

                    let text_view = gtk::TextView::new();
                    text_view.buffer().set_text(text);
                    text_view.set_size_request(
                        (*ow / 960.0 * canvas_w * 0.85) as i32,
                        (*oh / 540.0 * canvas_h * 0.85) as i32,
                    );

                    let overlay = gtk::Fixed::new();
                    overlay.put(&text_view, cvx, cvy);

                    // Commit on focus loss
                    let slides_clone = slides.clone();
                    let current_slide_clone = current_slide.clone();
                    let canvas_clone = canvas.clone();
                    let focus_ctrl = gtk::EventControllerFocus::new();
                    let tv_clone = text_view.clone();
                    focus_ctrl.connect_leave(move |_| {
                        let buf = tv_clone.buffer();
                        let new_text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
                        let idx = current_slide_clone.get();
                        let mut sl = slides_clone.borrow_mut();
                        if let Some(slide) = sl.get_mut(idx) {
                            if let Some(SlideObject::TextBox { ref mut text, .. }) = slide.objects.get_mut(oi) {
                                *text = new_text.to_string();
                            }
                        }
                        tv_clone.parent().map(|p| { p.unparent(); });
                        canvas_clone.queue_draw();
                    });
                    text_view.add_controller(focus_ctrl);
                    return;
                }
            }
        }
    }
});
canvas.add_controller(&dbl_click);
```

**Rich text formatting toolbar:** The toolbar (Bold/Italic/Underline buttons already exist — see `build_decks_toolbar` in `window.rs`) should apply `GtkTextTag`s to the selected text when a text box is being edited. Wire toolbar buttons to the active `GtkTextBuffer`.

**Reference:** Marko Editor `textview.rs`, Tables inline edit (existing code in `tables/src/window.rs`).

---

### 34. Image Loading via Cairo ImageSurface

**Reference:** `cairo::ImageSurface::create_from_png()`, `gio::File` pattern

**Problem:** Current Decks code shows gray placeholder rectangles for images. Need actual image rendering on the canvas.

**Solution:** Load images as `cairo::ImageSurface` during draw, with caching to avoid re-loading every frame.

```rust
use std::collections::HashMap;

// Image cache (avoid re-loading on every draw)
thread_local! {
    static IMAGE_CACHE: RefCell<HashMap<String, cairo::ImageSurface>> = RefCell::new(HashMap::new());
}

fn load_image_surface(path: &str) -> Result<cairo::ImageSurface, cairo::Error> {
    // Check cache first
    let cached = IMAGE_CACHE.with(|cache| {
        cache.borrow().get(path).cloned()
    });
    if let Some(surf) = cached { return Ok(surf); }

    // Load and cache
    let surface = cairo::ImageSurface::create_from_png(path)?;
    IMAGE_CACHE.with(|cache| {
        cache.borrow_mut().insert(path.to_string(), surface.clone());
    });
    Ok(surface)
}

// In draw function for Image objects:
SlideObject::Image { path, x, y, w, h } => {
    if let Ok(img_surf) = load_image_surface(path) {
        let img_w = img_surf.width() as f64;
        let img_h = img_surf.height() as f64;
        let scale_x = sw / img_w;
        let scale_y = sh / img_h;
        let scale = scale_x.min(scale_y); // Fit within bounds

        cr.save().unwrap();
        cr.translate(sx, sy);
        cr.scale(scale, scale);
        cr.set_source_surface(&img_surf, 0.0, 0.0).unwrap();
        cr.paint().unwrap();
        cr.restore().unwrap();
    }
}
```

**Non-PNG images:** Use the `image` crate to decode JPEG, WebP, GIF, etc. into RGBA bytes, then create a `cairo::ImageSurface` from the raw data.

```toml
# Cargo.toml
image = "0.25"
```

```rust
use image::GenericImageView;

fn load_any_image(path: &str) -> Result<cairo::ImageSurface, String> {
    if path.ends_with(".png") {
        return cairo::ImageSurface::create_from_png(path)
            .map_err(|e| e.to_string());
    }
    let img = image::open(path).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let stride = cairo::Format::ARgb32.stride_for_width(w).unwrap();
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32)
        .map_err(|e| e.to_string())?;
    {
        let mut data = surface.data().map_err(|e| e.to_string())?;
        // Convert RGBA (image crate) to ARGB (Cairo ARgb32)
        for (i, pixel) in rgba.chunks(4).enumerate() {
            let offset = i * 4;
            data[offset] = pixel[2];     // B
            data[offset + 1] = pixel[1]; // G
            data[offset + 2] = pixel[0]; // R
            data[offset + 3] = pixel[3]; // A
        }
    }
    Ok(surface)
}
```

**Reference:** `cairo-rs` docs, `image` crate docs, Loupe image loading via glycin.

---

## Crate Reference — Additional Dependencies

### Tables crates (in addition to existing)

| Crate | Version | Purpose |
|-------|---------|---------|
| `num-format` | 0.4 | Locale-aware number formatting (thousands separator) |
| `chrono` | 0.4 | Date parsing/formatting for date-formatted cells |
| `regex` | 1 | Pattern matching for data validation rules |
| `ordering` | 0.2 | Total ordering for f64 (sorting numeric columns) |

### Decks crates (in addition to existing)

| Crate | Version | Purpose |
|-------|---------|---------|
| `image` | 0.25 | Decode JPEG/WebP/GIF images for canvas display |
| `quick-xml` | 0.37 | Already in use for PPTX I/O |
| `zip` | 2 | Already in use for PPTX I/O |

---

## Implementation Priority Matrix

### Tables — ordered by effort vs. impact

| Pattern | Effort | Impact | Dependencies |
|---------|--------|--------|--------------|
| #19 Column drag resize | 0.5d | High | GestureDrag |
| #23 Number formatting | 1d | High | num-format, chrono |
| #20 Sort/filter headers | 1.5d | High | None (pure Rust) |
| #22 Freeze panes | 1d | Medium | Cairo clip regions |
| #21 Merge cells | 1d | Medium | Data model change |
| #24 Cell borders | 1d | Medium | Cairo strokes |
| #26 Charts dialog | 1.5d | Medium | Existing charts.rs |
| #25 Data validation | 1d | Low | regex crate |

### Decks — ordered by effort vs. impact

| Pattern | Effort | Impact | Dependencies |
|---------|--------|--------|--------------|
| #34 Image loading | 0.5d | High | image crate |
| #33 Text editing | 1d | High | GtkTextView overlay |
| #28 Object drag | 1d | High | GestureDrag |
| #27 Canvas undo/redo | 1d | High | Command pattern |
| #30 Snap-to-grid | 0.5d | Medium | Cairo grid lines |
| #29 Slide transitions | 1d | Medium | Cairo surfaces + timer |
| #31 Speaker notes | 0.5d | Low | GtkExpander + TextBuffer |
| #32 Master slides | 2d | Low | Data model change, PPTX roundtrip |

---

## Search Queries (for future research)

```
site:docs.rs gtk4 GestureDrag
docs.gtk.org gtk4 class.GestureDrag
site:gtk-rs.org gtk4-rs book animation
site:gnome.pages.gitlab.gnome.org libadwaita AdwAnimation
site:crates.io num-format chrono
site:github.com rust gtk4 drawing canvas undo command
site:discourse.gnome.org gtk4 cairo animate
```

---

## Reference Projects Catalog — Extended

### Rnote — Rust GTK4 Vector Drawing App (Decks reference)

**Repo:** `https://github.com/flxzt/rnote`
**Stack:** Rust + GTK4 + libadwaita + Cairo
**Relevance to Decks:** Canvas-based drawing, object manipulation (shapes, selection, resize, move), undo/redo, pen/tool system, infinite canvas zoom/pan.

**Architecture (from rnote-engine crate):**
```
rnote-engine/
├── pens/          # Brush, Pen, Eraser, Shape tools — each implements a trait
├── strokes/       # Stroke data model (vector paths)
├── selection.rs   # Object selection + manipulation
├── engine.rs      # Core engine: manages strokes, undo/redo, rendering
└── document.rs    # Document model with pages
```

**Key patterns we can borrow:**

| Rnote feature | Decks equivalent |
|---------------|-----------------|
| Shape tool (rect, circle, line) | SlideObject insertion (already done) |
| Object selection with handles | Selected object highlight + resize handles |
| Move/rotate/resize dragged objects | Object drag on canvas (#28) |
| Undo/redo via engine transaction system | Canvas undo/redo (#27) |
| Zoom/pan infinite canvas | Slide canvas zoom during editing |
| Pen abstraction (trait-based tool system) | Drawing tool abstraction |
| Pressure-sensitive stylus | Not needed for office suite |

**Key difference:** Rnote stores vector strokes (paths), while Decks stores discrete objects (TextBox, Rect, Circle, Image). Rnote's undo system operates on `engine::Engine` transactions — essentially a command pattern with `apply()`/`undo()`.

**Files to study (if cloning):**
- `rnote-engine/src/engine/mod.rs` — Undo/redo transaction system
- `rnote-engine/src/pens/mod.rs` — Tool abstraction pattern
- `rnote-engine/src/selection.rs` — Object selection with handles

---

### LibreOffice Calc — Full Feature Reference for Tables

**Homepage:** `https://www.libreoffice.org/discover/calc/`
**Documentation:** `https://books.libreoffice.org/en/GS74/GS7403-GettingStartedWithCalc.html`

**Complete feature list (target catalog for Tables):**

#### Spreadsheet Engine
- [x] Cell value entry (text, numbers, formulas) — **Done** (`engine.rs`)
- [x] Formula evaluation (=SUM, =AVERAGE, etc.) — **Done** (IronCalc, 83 functions)
- [x] Multi-sheet workbook — **Done** (SheetModel + DropDown switcher)
- [ ] Cell references (A1, $A$1, Sheet2.A1) — Partial (IronCalc supports, not tested)
- [ ] Named ranges — Not started
- [ ] Array formulas — Not started (IronCalc may support)
- [ ] Goal seek / solver — v2
- [ ] Pivot tables — v2

#### Grid & Selection
- [x] Row/column headers with labels — **Done** (A..Z, 1..n)
- [x] Cell selection (click to select) — **Done**
- [x] Active cell indicator (green border) — **Done**
- [ ] Range selection (Shift+click or drag) — Not started
- [ ] Column resize by drag — **Pattern #19**
- [ ] Row resize by drag — Same pattern as col resize
- [ ] Auto-fit column width (double-click divider) — **Pattern #18**
- [ ] Freeze panes — **Pattern #22**
- [ ] Split window — v2
- [ ] Hide/unhide rows/columns — Not started

#### Formatting
- [ ] Font family, size, bold, italic, underline — Not started (needs per-cell format struct)
- [ ] Text color, cell background color — Not started
- [ ] Text alignment (left, center, right, justify) — Not started
- [ ] Text wrapping (wrap, clip, overflow) — Not started
- [ ] Merge cells (merge, merge & center, unmerge) — **Pattern #21**
- [ ] Number format (General, Number, Currency, Date, Percent, Scientific, Text) — **Pattern #23**
- [ ] Cell borders (top, bottom, left, right, all, outer, none) — **Pattern #24**
- [ ] Conditional formatting (highlight rules, color scales, data bars) — v2
- [ ] Cell styles (named style presets) — v2

#### Data Management
- [ ] Sort (ascending, descending, multi-column) — **Pattern #20**
- [ ] Filter (AutoFilter, Standard Filter, Advanced Filter) — **Pattern #20**
- [ ] Data validation (list, number range, text length, regex) — **Pattern #25**
- [ ] Find & Replace — Not started (see Letters Pattern #1 for approach)
- [ ] Go to cell (Ctrl+G) — Not started
- [ ] Fill handle (drag to fill series) — Not started
- [ ] Data grouping / outlining — v2

#### Insert
- [x] Charts (bar, line, pie) — **Done** (engine), **Pattern #26** (dialog)
- [ ] Images — Not started
- [ ] Hyperlinks — Not started
- [ ] Comments / notes on cells — v2
- [ ] Shapes / drawing objects — v2

#### File I/O
- [x] Open XLSX, XLS, ODS — **Done** (calamine)
- [x] Open CSV/TSV — **Done**
- [x] Save XLSX — **Done** (rust_xlsxwriter)
- [ ] Save CSV — Not started
- [ ] Save ODS — Not started (need ODS writer crate)
- [ ] Export PDF — **Done** (Typst CLI)
- [ ] Print / Page setup — Not started

#### UI / Navigation
- [x] Formula bar (fx entry) — **Done**
- [x] Sheet tabs (add, switch, rename) — **Done** (basic)
- [ ] Sheet tab rename via double-click — Not started
- [ ] Sheet tab color — Not started
- [ ] Zoom slider — Not started
- [ ] Status bar (sum/count/avg of selection) — Not started
- [ ] Fullscreen / distraction-free — Not started

#### Keyboard / Productivity
- [x] Delete cell content (Delete/Backspace) — **Done**
- [ ] Cut/Copy/Paste — Partial (clipboard stubs may exist in suite-common)
- [ ] Undo/Redo — Not started (needs engine-level undo, IronCalc may have this)
- [ ] Tab/Enter to navigate cells — Not started
- [ ] Ctrl+Home/End to navigate sheet edges — Not started
- [ ] Autofill (type "Jan" and drag to fill months) — v2

---

### LibreOffice Impress — Full Feature Reference for Decks

**Homepage:** `https://www.libreoffice.org/discover/impress/`
**Documentation:** `https://books.libreoffice.org/en/GS71/GS7106-GettingStartedWithImpress.html`

**Complete feature list (target catalog for Decks):**

#### Slide Management
- [x] Add/delete/reorder slides — **Done**
- [x] Slide sidebar with thumbnails — **Done** (ListBox with labels, no thumbnails yet)
- [ ] Slide thumbnail rendering — Not started (render mini previews)
- [ ] Duplicate slide — Not started
- [ ] Hide slide — Not started
- [ ] Slide sections / grouping — v2
- [ ] Custom slide show (subset of slides) — v2

#### Slide Content
- [x] Text boxes — **Done** (insert + basic display)
- [x] Shapes (rect, circle) — **Done**
- [x] Image insertion — **Done** (path stored, needs rendering — **Pattern #34**)
- [ ] Lines, arrows, connectors — Not started
- [ ] Tables on slides — Not started (reuse Tables engine?)
- [ ] Charts on slides — Not started (reuse charts.rs?)
- [ ] Media (audio/video) — v2
- [ ] SmartArt / diagrams — v2

#### Object Manipulation
- [ ] Object drag (move) — **Pattern #28**
- [ ] Object resize via handles — **Pattern #28**
- [ ] Object rotate — v2
- [ ] Object delete (Delete key) — **Done**
- [ ] Object selection (click) — **Done**
- [ ] Multi-select (Shift+click, rubber band) — Not started
- [ ] Bring to front / send to back (z-order) — Not started
- [ ] Align objects (left, center, right, top, middle, bottom) — **Pattern #30**
- [ ] Distribute objects (horizontal, vertical) — **Pattern #30**
- [ ] Group / ungroup objects — v2
- [ ] Copy/paste objects — Not started

#### Text Formatting
- [x] Bold/Italic/Underline toolbar buttons — **Done** (stubs, not wired to text)
- [ ] Text editing (inline) — **Pattern #33**
- [ ] Font family, size, color — Not started
- [ ] Text alignment (left, center, right, justify) — Not started
- [ ] Bullet/numbered lists — Not started
- [ ] Line spacing — Not started

#### Slide Design
- [ ] Slide background (solid color, gradient, image) — Partial (background field exists, only solid color)
- [ ] Slide transitions — **Pattern #29**
- [ ] Object animations (entrance, emphasis, exit) — v2
- [ ] Slide layout / placeholders — **Pattern #32**
- [ ] Master slides — **Pattern #32**
- [ ] Themes / templates — v2

#### Present Mode
- [x] Fullscreen presentation — **Done** (basic)
- [x] Keyboard navigation (arrows, Home, End) — **Done**
- [x] Escape to exit — **Done**
- [ ] Slide transitions during presentation — **Pattern #29**
- [ ] Presenter console (notes, timer, next slide preview) — **Pattern #31**
- [ ] Presenter pointer / pen annotations — v2
- [ ] Slide number display — **Done** (badge in corner)
- [ ] Blank screen (B/W keys) — Not started
- [ ] Jump to slide (type number + Enter) — Not started

#### File I/O
- [x] Open PPTX — **Done** (zip + quick-xml)
- [x] Save PPTX — **Done**
- [x] Save-as PPTX — **Done**
- [ ] Open ODP — Not started
- [ ] Save ODP — Not started
- [ ] Export PDF — **Done** (Typst CLI)
- [ ] Export as images (PNG per slide) — Not started

#### Undo / Redo
- [ ] Canvas undo/redo — **Pattern #27**
- [ ] Slide-level undo (add/delete/reorder slides) — **Pattern #27**

---

### PowerPoint Transition Types — Complete Catalog

**Source:** `https://support.microsoft.com/en-us/powerpoint` + Vedantu reference

These are the 48 standard PowerPoint transitions. We should implement a minimum viable subset.

**Subtle (implement first):**
| Transition | Description | Cairo implementation |
|------------|-------------|---------------------|
| None | Instant cut | No animation |
| Fade | Crossfade from old to new | `cr.paint_with_alpha()` on two surfaces |
| Push | Old slides out, new slides in | `cr.set_source_surface()` with x-offset |
| Wipe | New slides over old (L→R, R→L, T→B, B→T) | Clip region moving across surface |
| Cover | New covers old (same directions) | Same as wipe, reversed order |
| Uncover | Old reveals new (same directions) | Same as wipe |
| Split | Slide splits from center (horiz/vert) | Two clip regions expanding outward |
| Reveal | Old slides away, revealing new | Reverse of Cover |

**Exciting (implement v2):**
| Transition | Description | Difficulty |
|------------|-------------|------------|
| Dissolve | Old dissolves into noise, resolves to new | Hard (needs noise shader) |
| Random Bars | Horizontal/vertical bars appear | Medium |
| Shape (Circle/Diamond) | New grows from center as shape | Medium |
| Clock | Radial wipe like clock hands | Medium |
| Ripple | Center ripple outward | Hard |
| Honeycomb | Hexagon tiles | Hard |
| Glitter | Diamond pattern dissolve | Hard |
| Vortex | Spinning tiles | Hard |
| Shred | Paper shred strips | Medium |
| Switch | Like flip board | Hard |
| Flip | 3D card flip | Hard |
| Gallery | Slide out to center | Medium |
| Cube | 3D cube rotate | Hard |
| Doors | Vertical doors open | Medium |
| Box | 3D box rotate | Hard |
| Comb | Like window blinds | Medium |
| Zoom | Zoom in/out of new slide | Medium |
| Random | Random transition | Pick from implemented set |
| Pan | Camera pan | Medium |
| Ferris Wheel | Rotating wheel | Hard |
| Conveyor | Like conveyor belt | Medium |
| Rotate | 3D rotate | Hard |
| Window | Like window opening | Medium |
| Orbit | 3D orbit | Hard |
| Fly Through | 3D fly through | Hard |

**Dynamic Content (v2):**
| Transition | Description |
|------------|-------------|
| Pan | Background pans, content stays |
| Ferris Wheel | Content rotates in |
| Conveyor | Content slides in |
| Rotate | Content rotates in 3D |
| Window | Content appears through window |
| Orbit | Content orbits in 3D |
| Fly Through | Content flies in 3D |

**Implementation plan for v1.0:**
1. None (already default)
2. Fade — Crossfade surfaces with ease-out (#29)
3. Push (L→R only) — Slide surfaces horizontally
4. Wipe (L→R only) — Clip region sweep
5. Cover (L→R only) — Same as wipe, reversed
6. Split (horiz/vert) — Two expanding clip regions

**Total v1 effort: ~1d** (extend TransitionState enum, add wipe/clip logic)

---

### LibreOffice Sidebar Decks — Impress UI Reference

Impress has a right sidebar with 7 decks (property panels). This is our UI target for side panels in both Tables and Decks.

**Impress sidebar decks:**
| Deck | Shortcut | Content | Decks equivalent |
|------|----------|---------|-----------------|
| Properties | Alt+1 | Object properties (position, size, rotation, text formatting) | Properties panel for selected object |
| Slide Transition | Alt+6 | Transition type, speed, auto-advance timing | Slide transition settings |
| Animation | Alt+7 | Object animation effects (entrance, emphasis, exit) | v2: object animations |
| Master Slides | — | Master slide picker | Pattern #32 |
| Gallery | — | Clip art / media browser | Not needed for MVP |
| Navigator | — | Slide outline / structure navigation | Slide list (already in left sidebar) |
| Styles | — | Text styles (heading, body, etc.) | v2: text styles |

**Tables equivalent sidebar decks:**
| Deck | Content |
|------|---------|
| Properties | Cell formatting (font, alignment, number format, borders, background) |
| Functions | Function picker / formula help |
| Charts | Chart data selector + type picker |
| Navigator | Sheet outline, named ranges |

---

## Implementation Status Update (Post-Research)

### Tables — Feature Completion Map

| Category | Done | This Phase | v2+ |
|----------|------|-----------|-----|
| Spreadsheet Engine | 3 | 1 (cell refs) | 3 |
| Grid & Selection | 3 | 4 (range sel, col resize, freeze) | 3 |
| Formatting | 0 | 6 (fonts, colors, align, merge, numbers, borders) | 2 |
| Data Management | 0 | 4 (sort, filter, validation, find) | 2 |
| Insert | 1 (charts engine) | 1 (charts dialog) | 3 |
| File I/O | 4 | 2 (CSV save, ODS save) | 1 |
| UI / Navigation | 2 | 3 (tab rename, zoom, status bar) | 1 |
| Keyboard/Productivity | 1 | 3 (undo, tab nav, clipboard) | 2 |

### Decks — Feature Completion Map

| Category | Done | This Phase | v2+ |
|----------|------|-----------|-----|
| Slide Management | 3 | 2 (thumbnails, duplicate) | 3 |
| Slide Content | 3 | 2 (image render, lines) | 4 |
| Object Manipulation | 2 | 4 (drag, resize, multi-select, z-order) | 4 |
| Text Formatting | 3 (stubs) | 3 (inline edit, font, alignment) | 2 |
| Slide Design | 0 | 3 (transitions, master, grid) | 3 |
| Present Mode | 4 | 2 (transitions, presenter console) | 3 |
| File I/O | 4 | 2 (ODP roundtrip, image export) | 0 |
| Undo/Redo | 0 | 1 (canvas undo) | 0 |

---

## Search Queries (for future research)

```
site:docs.rs gtk4 GestureDrag
docs.gtk.org gtk4 class.GestureDrag
site:gtk-rs.org gtk4-rs book animation
site:gnome.pages.gitlab.gnome.org libadwaita AdwAnimation
site:crates.io num-format chrono
site:github.com rust gtk4 drawing canvas undo command
site:discourse.gnome.org gtk4 cairo animate
site:github.com flxzt rnote rnote-engine
books.libreoffice.org calc getting started formatting
books.libreoffice.org impress getting started slide show
```
