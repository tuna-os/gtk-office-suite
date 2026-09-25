// grid_render.rs — Cairo grid drawing for the spreadsheet canvas.
use libadwaita as adw;
use gtk4::cairo::Context;
use gtk4::pango::{self, EllipsizeMode};
use std::cell::RefCell;
use std::rc::Rc;
use tables_core::sheet::{SheetModel, CellBorder, BorderStyle, SortDirection, col_label};

/// Auto-fit column width to content using PangoLayout text measurement.
/// Lives here (not tables-core) because it needs Cairo/Pango to measure.
pub fn auto_fit_column(cr: &Context, sheet: &mut SheetModel, col: usize, _scroll_x: f64) {
    suite_common::use_ui_font_rendering(cr);
    let layout = pangocairo::functions::create_layout(cr);
    let mut max_w: f64 = 30.0;
    let label = col_label(col);
    layout.set_text(&label);
    let (tw, _) = layout.pixel_size();
    max_w = max_w.max(tw as f64 + 16.0);
    for r in 0..sheet.rows {
        let val = sheet.cell(r, col);
        if val.is_empty() { continue; }
        layout.set_text(val);
        let (tw, _) = layout.pixel_size();
        max_w = max_w.max(tw as f64 + 12.0);
    }
    sheet.set_col_width(col, max_w.clamp(30.0, 500.0));
}

// Header geometry is tables-core's: hit-testing, the accessibility grid and
// the render lab's crop all measure from the same constants. A private copy
// here drifted once (50/26 vs the core's values) and put every cell 10 px
// right of where everything else thought it was.
use tables_core::sheet::{COL_HEADER_HEIGHT, ROW_HEADER_WIDTH};
const HEADER_BG: (f64, f64, f64) = (0.95, 0.95, 0.95);
const HEADER_BG_DARK: (f64, f64, f64) = (0.25, 0.25, 0.25);
const GRID_LINE: (f64, f64, f64) = (0.85, 0.85, 0.85);
const GRID_LINE_DARK: (f64, f64, f64) = (0.35, 0.35, 0.35);
const CANVAS_BG: (f64, f64, f64) = (0.5, 0.5, 0.5);
const CANVAS_BG_DARK: (f64, f64, f64) = (0.15, 0.15, 0.15);
const CELL_BG: (f64, f64, f64) = (1.0, 1.0, 1.0);
const CELL_BG_DARK: (f64, f64, f64) = (0.18, 0.18, 0.18);
const HEADER_TEXT: (f64, f64, f64) = (0.3, 0.3, 0.3);
const HEADER_TEXT_DARK: (f64, f64, f64) = (0.8, 0.8, 0.8);
const RANGE_WASH: (f64, f64, f64) = (0.8, 0.85, 0.95);
const RANGE_WASH_DARK: (f64, f64, f64) = (0.16, 0.24, 0.36);
const BORDER_LINE: (f64, f64, f64) = (0.0, 0.0, 0.0);
const BORDER_LINE_DARK: (f64, f64, f64) = (0.85, 0.85, 0.85);

pub fn draw_border_edges(cr: &Context, x: f64, y: f64, w: f64, h: f64, border: &CellBorder, is_dark: bool) {
    draw_border_line(cr, &border.top, border.color, x, y, x + w, y, is_dark);
    draw_border_line(cr, &border.bottom, border.color, x, y + h, x + w, y + h, is_dark);
    draw_border_line(cr, &border.left, border.color, x, y, x, y + h, is_dark);
    draw_border_line(cr, &border.right, border.color, x + w, y, x + w, y + h, is_dark);
}

/// Where to stroke a line `width` px wide so it lands on whole pixels
/// along the cell edge at `edge` (a whole-pixel coordinate): an odd width is
/// centred half a pixel in, on the pixel row just inside the edge, where
/// the gridline is; an even one straddles the edge. A line centred on the
/// edge itself would smear across two half-covered pixel rows.
fn crisp(edge: f64, width: f64) -> f64 {
    if (width.round() as i64) % 2 == 1 { edge.round() - 0.5 } else { edge.round() }
}

/// A header label (column letter or row number) in the UI font at 9 pt,
/// small enough for a 20 px header.
fn header_layout(cr: &Context, text: &str) -> pango::Layout {
    let layout = pangocairo::functions::create_layout(cr);
    let mut font = layout.context().font_description().unwrap_or_default();
    // Chrome, not document: the GNOME UI face, whatever the cairo
    // context's default happens to be (serif in a bare container).
    font.set_family("Adwaita Sans, Cantarell, Sans");
    font.set_size(9 * pango::SCALE);
    layout.set_font_description(Some(&font));
    layout.set_text(text);
    layout
}

/// Draw a header label with its left edge at `x`, centred vertically on `mid_y`.
fn draw_pango_text(cr: &Context, text: &str, x: f64, mid_y: f64) {
    let layout = header_layout(cr, text);
    cr.move_to(x, mid_y - layout.pixel_size().1 as f64 / 2.0);
    pangocairo::functions::show_layout(cr, &layout);
}

fn pango_text_width(cr: &Context, text: &str) -> f64 {
    header_layout(cr, text).pixel_size().0 as f64
}

#[allow(clippy::too_many_arguments)]
fn draw_border_line(cr: &Context, style: &BorderStyle, color: (f64, f64, f64), x1: f64, y1: f64, x2: f64, y2: f64, is_dark: bool) {
    if *style == BorderStyle::None { return; }
    // The border's own colour; the default black follows the theme so a
    // black border stays visible on the dark canvas.
    let c = if color == (0.0, 0.0, 0.0) { if is_dark { BORDER_LINE_DARK } else { BORDER_LINE } } else { color };
    let width = style.width_px();
    cr.save().unwrap();
    cr.set_source_rgb(c.0, c.1, c.2);
    cr.set_line_width(width);
    match style {
        BorderStyle::Dotted => cr.set_dash(&[1.0, 2.0], 0.0),
        BorderStyle::Dashed => cr.set_dash(&[4.0, 2.0], 0.0),
        _ => cr.set_dash(&[], 0.0),
    }
    // Extend along the line to cover the corner squares a crossing edge of
    // the same weight fills, so boxed cells have closed corners.
    let (lead, trail) = ((width / 2.0).ceil(), (width / 2.0).floor());
    let (x1, y1, x2, y2) = if y1 == y2 {
        (x1.round() - lead, crisp(y1, width), x2.round() + trail, crisp(y2, width))
    } else {
        (crisp(x1, width), y1.round() - lead, crisp(x2, width), y2.round() + trail)
    };
    if *style == BorderStyle::Double {
        // Two 1 px lines with a 1 px gap, as Excel draws a double border.
        cr.set_line_width(1.0);
        let (dx, dy) = if y1 == y2 { (0.0, 1.0) } else { (1.0, 0.0) };
        for k in [-1.0, 1.0] {
            cr.move_to(x1 + k * dx, y1 + k * dy);
            cr.line_to(x2 + k * dx, y2 + k * dy);
        }
    } else {
        cr.move_to(x1, y1);
        cr.line_to(x2, y2);
    }
    cr.stroke().unwrap();
    cr.restore().unwrap();
}

/// A small filled triangle in a column header showing which way it's
/// sorted (#113's "visible criteria" — the standard spreadsheet
/// convention). `(cx, cy)` is the arrow's center.
fn draw_sort_arrow(cr: &Context, cx: f64, cy: f64, dir: SortDirection, color: (f64, f64, f64)) {
    let half_w = 4.0;
    let half_h = 3.0;
    cr.save().unwrap();
    cr.set_source_rgb(color.0, color.1, color.2);
    match dir {
        SortDirection::Ascending => {
            cr.move_to(cx - half_w, cy + half_h);
            cr.line_to(cx + half_w, cy + half_h);
            cr.line_to(cx, cy - half_h);
        }
        SortDirection::Descending => {
            cr.move_to(cx - half_w, cy - half_h);
            cr.line_to(cx + half_w, cy - half_h);
            cr.line_to(cx, cy + half_h);
        }
    }
    cr.close_path();
    cr.fill().unwrap();
    cr.restore().unwrap();
}

/// Rotating palette for formula-reference outlines (#113), same
/// convention as Excel/Sheets — each distinct reference in a formula
/// gets the next color in order of appearance.
const FORMULA_REF_COLORS: [(f64, f64, f64); 5] = [
    (0.13, 0.38, 0.77), // blue
    (0.77, 0.13, 0.13), // red
    (0.13, 0.6, 0.2),   // green
    (0.55, 0.2, 0.7),   // purple
    (0.85, 0.5, 0.0),   // orange
];

/// A cell's value through its number format, in a bounded Pango layout
/// clipped to `rect` (x, y, w, h). Clipping and ellipsizing keep long values
/// inside their cell; the text sits at the top, as the grid always drew it.
/// Pixels per indent level: three characters, as in Excel.
const INDENT_PX: f64 = 21.0;
/// Space between a cell's left edge and left-aligned text, and between
/// right-aligned text and its right edge. Measured on Calc's print of the
/// render lab's fixtures (tables/values, tables/fills): left-aligned text
/// starts about 1.5 px in, right-aligned text ends about 3 px in, of which a
/// pixel or so is the glyphs' own side bearing. A symmetric 2 px put every
/// left-aligned value 1.5 px right of Calc's.
const CELL_PAD_LEFT: f64 = 1.0;
const CELL_PAD_RIGHT: f64 = 2.0;
/// Space between a cell's bottom edge and the bottom of bottom-aligned text
/// (the layout's descent line), and the top edge and top-aligned text.
const CELL_PAD_V: f64 = 2.0;

/// The Pango layout for a cell's text in its style: font, weight, slant,
/// underline and strikethrough, alignment, and wrapping within `width`.
fn cell_layout(cr: &Context, sheet: &SheetModel, r: usize, c: usize, text: &str, width: f64) -> pango::Layout {
    use tables_core::style::HAlign;
    let style = &sheet.styles[r][c];
    let layout = pangocairo::functions::create_layout(cr);
    let mut font = layout.context().font_description().unwrap_or_default();
    font.set_family(style.font_family.as_deref().unwrap_or(&sheet.default_font_family));
    let size = style.font_size.unwrap_or(sheet.default_font_size);
    font.set_size((size * pango::SCALE as f64) as i32);
    if style.bold {
        font.set_weight(pango::Weight::Bold);
    }
    if style.italic {
        font.set_style(pango::Style::Italic);
    }
    layout.set_font_description(Some(&font));
    if style.underline || style.strikethrough {
        let attrs = pango::AttrList::new();
        if style.underline {
            attrs.insert(pango::AttrInt::new_underline(pango::Underline::Single));
        }
        if style.strikethrough {
            attrs.insert(pango::AttrInt::new_strikethrough(true));
        }
        layout.set_attributes(Some(&attrs));
    }
    layout.set_text(text);
    layout.set_width(
        ((width - CELL_PAD_LEFT - CELL_PAD_RIGHT - style.indent as f64 * INDENT_PX).max(1.0) * pango::SCALE as f64) as i32,
    );
    if style.wrap {
        layout.set_wrap(pango::WrapMode::WordChar);
    } else {
        layout.set_ellipsize(EllipsizeMode::End);
        layout.set_single_paragraph_mode(true);
    }
    layout.set_alignment(match sheet.resolved_h_align(r, c) {
        HAlign::Center => pango::Alignment::Center,
        HAlign::Right => pango::Alignment::Right,
        _ => pango::Alignment::Left,
    });
    layout
}

/// Where a cell's text layout starts, for text `text_h` px tall in a row
/// `rh` px tall at `cy`. Bottom-aligned text keeps its descent line
/// CELL_PAD_V above the cell's bottom even when the line box (ascent +
/// descent, 18 px for Calibri 11) leaves less than that above it in a 20 px
/// row: the ascent's empty top overflows into the clip, as in Calc and
/// Excel. Clamping the layout to the cell's top instead pushed every
/// default-height row's text 1–2 px down.
fn cell_text_top(v_align: tables_core::style::VAlign, cy: f64, rh: f64, text_h: f64) -> f64 {
    use tables_core::style::VAlign;
    match v_align {
        VAlign::Top => cy + CELL_PAD_V,
        VAlign::Center => cy + (rh - text_h) / 2.0,
        VAlign::Bottom => cy + rh - text_h - CELL_PAD_V,
    }
}

/// A cell's value through its number format and style, clipped to `rect`
/// (x, y, w, h). Clipping and ellipsizing keep long values inside their
/// cell; wrapped cells break across lines instead.
fn draw_cell_text(cr: &Context, sheet: &SheetModel, r: usize, c: usize, rect: (f64, f64, f64, f64), color: (f64, f64, f64)) {
    use tables_core::style::HAlign;
    let val = sheet.cell(r, c);
    if val.is_empty() {
        return;
    }
    let (cx, cy, cw, rh) = rect;
    let style = &sheet.styles[r][c];
    let (red, green, blue) = style.color.map_or(color, |c| c.to_f64());
    cr.set_source_rgb(red, green, blue);
    let formatted = sheet.formats[r][c].format(val);
    let mut layout = cell_layout(cr, sheet, r, c, &formatted, cw);
    // A number that doesn't fit is never truncated: a clipped "1,234,56…"
    // reads as a different number. Calc shows ### instead (Excel fills the
    // cell with #; Calc's form is the reference here).
    if sheet.aligns_right(r, c) && !style.wrap && layout.is_ellipsized() {
        layout = cell_layout(cr, sheet, r, c, "###", cw);
    }
    // The layout's exact logical height, not rounded up to whole pixels.
    let text_h = layout.size().1 as f64 / pango::SCALE as f64;
    let y = cell_text_top(style.v_align, cy, rh, text_h);
    let indent = style.indent as f64 * INDENT_PX;
    let x = if sheet.resolved_h_align(r, c) == HAlign::Right { cx + CELL_PAD_LEFT } else { cx + CELL_PAD_LEFT + indent };
    cr.save().unwrap();
    cr.rectangle(cx + 1.0, cy + 1.0, (cw - 2.0).max(1.0), (rh - 2.0).max(1.0));
    cr.clip();
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
    cr.restore().unwrap();
}

/// Grow each row to fit cells that need more than the default height (a
/// wrapped cell, or a font size of its own), unless the row already has a
/// height of its own (anything other than the default), as spreadsheets
/// auto-fit rows on open. Measured with Pango off-screen.
pub fn fit_rows_to_content(sheet: &mut SheetModel) {
    let Ok(surface) = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1) else { return };
    let Ok(cr) = Context::new(&surface) else { return };
    // Measure with the options the grid draws with, or rows fit a
    // different text than the one shown.
    suite_common::use_ui_font_rendering(&cr);
    for r in 0..sheet.rows {
        if (sheet.row_heights[r] - tables_core::sheet::ROW_HEIGHT).abs() > 0.5 {
            continue;
        }
        let mut need = sheet.row_heights[r];
        for c in 0..sheet.cols {
            let style = &sheet.styles[r][c];
            if !(style.wrap || style.font_size.is_some()) || sheet.cell(r, c).is_empty() {
                continue;
            }
            let text = sheet.formats[r][c].format(sheet.cell(r, c));
            let layout = cell_layout(&cr, sheet, r, c, &text, sheet.col_width(c));
            need = need.max(layout.pixel_size().1 as f64 + 6.0);
        }
        sheet.row_heights[r] = need;
    }
}

/// Scroll so the selection's moving end (the active cell, or the far
/// corner a Shift+arrow is extending) is on screen, frozen panes
/// included, then tell the accessibility tree where the cells now are.
pub fn follow_selection(ga: &crate::grid_area::GridArea, sheet: &SheetModel, h: &gtk4::Adjustment, v: &gtk4::Adjustment) {
    use gtk4::prelude::{AdjustmentExt, WidgetExt};
    let view = (ga.width() as f64, ga.height() as f64);
    if view.0 > 0.0 && view.1 > 0.0 {
        let target = tables_core::sheet::scroll_into_view(sheet.sel_end_row, sheet.sel_end_col, (h.value(), v.value()), view, sheet);
        for (adj, value) in [(h, target.0), (v, target.1)] {
            // The scroll range was a fixed guess; a far cell widens it.
            if value > adj.upper() - adj.page_size() {
                adj.set_upper(value + adj.page_size());
            }
            if (adj.value() - value).abs() > 0.5 {
                adj.set_value(value);
            }
        }
    }
    ga.set_geometry(sheet, (h.value(), v.value()));
}

/// Scroll to where the file's saved view left the active sheet
/// (`SheetModel::view_top_left`), once, the first time it is shown.
pub fn restore_saved_view(state: &Rc<RefCell<crate::window::AppState>>, h: &gtk4::Adjustment, v: &gtk4::Adjustment) {
    let (x, y) = {
        let st = state.borrow();
        let mut sheet = st.sheet_mut();
        let Some((r, c)) = sheet.view_top_left.take() else { return };
        tables_core::sheet::scroll_to_top_left(r, c, &sheet)
    };
    use gtk4::prelude::AdjustmentExt;
    h.set_value(x);
    v.set_value(y);
}

#[allow(clippy::too_many_arguments)]
pub fn draw_grid(
    cr: &Context, state: &Rc<RefCell<crate::window::AppState>>,
    width: f64, height: f64, scroll_x: f64, scroll_y: f64, show_gridlines: bool,
    formula_refs: &[(usize, usize, usize, usize)], accent: (f64, f64, f64),
) {
    suite_common::use_ui_font_rendering(cr);
    let st = state.borrow();
    let sheet = &st.sheets[st.active_sheet].borrow();
    let is_dark = adw::StyleManager::default().is_dark();
    let hdr_bg = if is_dark { HEADER_BG_DARK } else { HEADER_BG };
    let hdr_text = if is_dark { HEADER_TEXT_DARK } else { HEADER_TEXT };
    let grid_line = if is_dark { GRID_LINE_DARK } else { GRID_LINE };
    let cell_bg = if is_dark { CELL_BG_DARK } else { CELL_BG };
    let cell_text = suite_common::canvas_foreground(is_dark);
    // Editing chrome (selection wash, outline, fill handle) is not document
    // content; the render lab's capture leaves it out, as it does the caret.
    let show_selection = !suite_common::render_dump::active();
    let range_wash = if is_dark { RANGE_WASH_DARK } else { RANGE_WASH };
    let canvas_bg = if is_dark { CANVAS_BG_DARK } else { CANVAS_BG };

    // Background
    cr.set_source_rgb(canvas_bg.0, canvas_bg.1, canvas_bg.2);
    cr.rectangle(0.0, 0.0, width, height);
    cr.fill().unwrap();

    // Corner cell — same shade as the headers, not the canvas grey.
    cr.set_source_rgb(hdr_bg.0, hdr_bg.1, hdr_bg.2);
    cr.rectangle(0.0, 0.0, ROW_HEADER_WIDTH, COL_HEADER_HEIGHT);
    cr.fill().unwrap();

    // Column headers
    cr.save().unwrap();
    cr.rectangle(ROW_HEADER_WIDTH, 0.0, width - ROW_HEADER_WIDTH, COL_HEADER_HEIGHT);
    cr.clip();
    cr.set_source_rgb(hdr_bg.0, hdr_bg.1, hdr_bg.2);
    cr.rectangle(ROW_HEADER_WIDTH, 0.0, width, COL_HEADER_HEIGHT);
    cr.fill().unwrap();
    // Hidden columns (#113) collapse to zero width, so — like the hidden
    // rows below — this walks every column rather than a fixed-step
    // range starting at a scroll-derived index. Frozen columns' labels
    // stay put; scrolled ones are clipped where they pass under them.
    let (left, top) = (tables_core::sheet::scrolled_left(sheet), tables_core::sheet::scrolled_top(sheet));
    for c in 0..sheet.cols {
        if !tables_core::sheet::col_on_screen(c, scroll_x, sheet) { continue; }
        let cw = sheet.col_width(c);
        let cx = tables_core::sheet::col_x(c, scroll_x, sheet);
        if cx > width { break; }
        let frozen = c < sheet.frozen_cols;
        cr.save().unwrap();
        if !frozen {
            cr.rectangle(left, 0.0, width - left, COL_HEADER_HEIGHT);
            cr.clip();
        }
        let label = col_label(c);
        cr.set_source_rgb(hdr_text.0, hdr_text.1, hdr_text.2);
        let text_width = pango_text_width(cr, &label);
        draw_pango_text(cr, &label, cx + (cw - text_width) / 2.0, COL_HEADER_HEIGHT / 2.0);
        // Sort indicator (#113: "visible criteria" — a small triangle
        // pointing the sort direction, standard spreadsheet convention).
        if let Some((sc, dir)) = sheet.sorted_col {
            if sc == c {
                draw_sort_arrow(cr, cx + cw - 12.0, COL_HEADER_HEIGHT / 2.0, dir, hdr_text);
            }
        }
        cr.set_source_rgb(grid_line.0, grid_line.1, grid_line.2);
        cr.set_line_width(0.5);
        cr.move_to(cx + cw, 0.0);
        cr.line_to(cx + cw, COL_HEADER_HEIGHT);
        cr.stroke().unwrap();
        cr.restore().unwrap();
    }
    cr.restore().unwrap();

    // Row headers
    cr.save().unwrap();
    cr.rectangle(0.0, COL_HEADER_HEIGHT, ROW_HEADER_WIDTH, height - COL_HEADER_HEIGHT);
    cr.clip();
    cr.set_source_rgb(hdr_bg.0, hdr_bg.1, hdr_bg.2);
    cr.rectangle(0.0, COL_HEADER_HEIGHT, ROW_HEADER_WIDTH, height);
    cr.fill().unwrap();
    // Hidden rows (#113 filtering) collapse to zero height, so this walks
    // every row rather than a fixed-step range — cheap at sheet sizes
    // this app deals with, and the only way to know a row's true screen
    // position once earlier rows may not all be drawn.
    for r in 0..sheet.rows {
        if !tables_core::sheet::row_on_screen(r, scroll_y, sheet) { continue; }
        let ry = tables_core::sheet::row_y(r, scroll_y, sheet);
        if ry > height { break; }
        cr.save().unwrap();
        if r >= sheet.frozen_rows {
            cr.rectangle(0.0, top, ROW_HEADER_WIDTH, height - top);
            cr.clip();
        }
        cr.set_source_rgb(hdr_text.0, hdr_text.1, hdr_text.2);
        let label = (r + 1).to_string();
        let text_width = pango_text_width(cr, &label);
        draw_pango_text(cr, &label, ROW_HEADER_WIDTH - 6.0 - text_width, ry + sheet.row_height(r) / 2.0);
        cr.restore().unwrap();
    }
    cr.restore().unwrap();

    // Cells, in four panes: the scrolling one, then the frozen rows, the
    // frozen columns and their corner over it. Each pane draws only its own
    // cells, clipped to its own area, so a scrolled cell never shows
    // through a frozen one. Without frozen panes the last three are empty.
    let panes = [
        (false, false, (left, top, width - left, height - top)),
        (true, false, (left, COL_HEADER_HEIGHT, width - left, top - COL_HEADER_HEIGHT)),
        (false, true, (ROW_HEADER_WIDTH, top, left - ROW_HEADER_WIDTH, height - top)),
        (true, true, (ROW_HEADER_WIDTH, COL_HEADER_HEIGHT, left - ROW_HEADER_WIDTH, top - COL_HEADER_HEIGHT)),
    ];
    let span = |from: usize, n: usize, size: &dyn Fn(usize) -> f64| (from..from + n.max(1)).map(size).sum::<f64>();
    for (frozen_r, frozen_c, (px, py, pw, ph)) in panes {
    if pw <= 0.0 || ph <= 0.0 { continue; }
    let in_pane = |r: usize, c: usize| (r < sheet.frozen_rows) == frozen_r && (c < sheet.frozen_cols) == frozen_c;
    cr.save().unwrap();
    cr.rectangle(px, py, pw, ph);
    cr.clip();
    let mut bordered: Vec<(f64, f64, f64, f64, &CellBorder)> = Vec::new();
    for r in 0..sheet.rows {
        if (r < sheet.frozen_rows) != frozen_r || !tables_core::sheet::row_on_screen(r, scroll_y, sheet) { continue; }
        let cy = tables_core::sheet::row_y(r, scroll_y, sheet);
        let rh = sheet.row_height(r);
        if cy > height { break; }
        for c in 0..sheet.cols {
            if (c < sheet.frozen_cols) != frozen_c || !tables_core::sheet::col_on_screen(c, scroll_x, sheet) { continue; }
            let cw = sheet.col_width(c);
            let cx = tables_core::sheet::col_x(c, scroll_x, sheet);
            if cx > width { break; }
            let is_sel = show_selection && r == sheet.selected_row && c == sheet.selected_col;
            let (sr0, sc0, sr1, sc1) = sheet.selection_block();
            let in_range = show_selection && r >= sr0 && r <= sr1 && c >= sc0 && c <= sc1;
            let border = &sheet.borders[r][c];

            // Cell bg — selection wash wins; else a matching
            // conditional-formatting rule paints its fill.
            let cond_fill = sheet.cond_rules.iter().find_map(|rule| {
                if !rule.contains(r, c) {
                    return None;
                }
                let v: f64 = sheet.data[r][c].parse().ok()?;
                if rule.matches(v) { Some(rule.fill.clone()) } else { None }
            });
            if in_range {
                cr.set_source_rgb(range_wash.0, range_wash.1, range_wash.2);
            } else if let Some(hex) = cond_fill {
                let p = |i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(255) as f64 / 255.0;
                if hex.len() >= 6 {
                    cr.set_source_rgb(p(0), p(2), p(4));
                } else {
                    cr.set_source_rgb(cell_bg.0, cell_bg.1, cell_bg.2);
                }
            } else if let Some(fill) = sheet.styles[r][c].fill {
                let (fr, fg, fb) = fill.to_f64();
                cr.set_source_rgb(fr, fg, fb);
            } else {
                cr.set_source_rgb(cell_bg.0, cell_bg.1, cell_bg.2);
            }
            cr.rectangle(cx, cy, cw, rh);
            cr.fill().unwrap();

            // A merged block draws as one cell: no gridlines inside it, and
            // its anchor's value across the whole block (second pass below).
            let merge = sheet.merge_covering(r, c);
            let (right_edge, bottom_edge) = match merge {
                Some((mr, mc, rs, cs)) => (c + 1 == mc + cs.max(1), r + 1 == mr + rs.max(1)),
                None => (true, true),
            };

            // Grid line: a crisp 1 px hairline on the cell's last pixel row
            // and column. Stroked half a pixel wide on the edge itself, it
            // smeared over two pixels and the next cell's fill covered one,
            // leaving a line about 4% grey that barely showed.
            if show_gridlines {
                cr.set_source_rgb(grid_line.0, grid_line.1, grid_line.2);
                cr.set_line_width(1.0);
                if right_edge {
                    cr.move_to(crisp(cx + cw, 1.0), cy.round());
                    cr.line_to(crisp(cx + cw, 1.0), (cy + rh).round());
                    cr.stroke().unwrap();
                }
                if bottom_edge {
                    cr.move_to(cx.round(), crisp(cy + rh, 1.0));
                    cr.line_to((cx + cw).round(), crisp(cy + rh, 1.0));
                    cr.stroke().unwrap();
                }
            }

            // Cell borders are drawn after every cell's background: a 2 or
            // 3 px edge reaches into the neighbouring cell, whose fill
            // would otherwise paint over it.
            if !border.is_none() {
                bordered.push((cx, cy, cw, rh, border));
            }

            // Active cell border (a merged block's is the selection outline)
            if is_sel && sheet.merge_covering(r, c).is_none() {
                cr.set_source_rgb(accent.0, accent.1, accent.2);
                cr.set_line_width(2.0);
                cr.rectangle(cx, cy, cw, rh);
                cr.stroke().unwrap();
            }

            // Text — through the cell's number format and a bounded Pango
            // layout.  Clipping/ellipsizing keeps long values inside their
            // cell while still allowing readable text when a column grows.
            if merge.is_none() {
                draw_cell_text(cr, sheet, r, c, (cx, cy, cw, rh), cell_text);
            }
        }
    }
    for &(x, y, w, h, border) in &bordered {
        draw_border_edges(cr, x, y, w, h, border, is_dark);
    }
    // Merged blocks: the anchor's value across the whole block, drawn after
    // every cell so the covered cells' backgrounds can't paint over it.
    for &(mr, mc, rs, cs) in sheet.merges.iter().filter(|m| in_pane(m.0, m.1)) {
        let x = tables_core::sheet::col_x(mc, scroll_x, sheet);
        let y = tables_core::sheet::row_y(mr, scroll_y, sheet);
        let w = span(mc, cs, &|c| if sheet.is_col_hidden(c) { 0.0 } else { sheet.col_width(c) });
        let h = span(mr, rs, &|r| if sheet.is_row_hidden(r) { 0.0 } else { sheet.row_height(r) });
        if x > width || y > height || x + w < ROW_HEADER_WIDTH || y + h < COL_HEADER_HEIGHT {
            continue;
        }
        draw_cell_text(cr, sheet, mr, mc, (x, y, w, h), cell_text);
    }
    // Charts float above the cells at their anchor. They were read from
    // xlsx and saved back, but only ever drawn in the chart dialog's preview
    // (render lab `tables/chart`).
    for chart in sheet.charts.iter().filter(|ch| in_pane(ch.anchor.0, ch.anchor.1)) {
        let x = tables_core::sheet::col_x(chart.anchor.1, scroll_x, sheet);
        let y = tables_core::sheet::row_y(chart.anchor.0, scroll_y, sheet);
        let (w, h) = (chart.width_px.round() as i32, chart.height_px.round() as i32);
        if w < 1 || h < 1 || x > width || y > height || x + chart.width_px < ROW_HEADER_WIDTH || y + chart.height_px < COL_HEADER_HEIGHT {
            continue;
        }
        let kind = match chart.kind {
            tables_core::sheet::ChartKind::Line => crate::charts::ChartType::Line,
            tables_core::sheet::ChartKind::Pie => crate::charts::ChartType::Pie,
            _ => crate::charts::ChartType::Bar,
        };
        let name = chart.series_name();
        let surface = crate::charts::render_chart_named(&chart.points(sheet), kind, w, h, Some(&name));
        cr.set_source_surface(&surface, x.round(), y.round()).unwrap();
        cr.paint().unwrap();
        cr.set_source_rgb(grid_line.0, grid_line.1, grid_line.2);
        cr.set_line_width(1.0);
        cr.rectangle(x.round() + 0.5, y.round() + 0.5, w as f64 - 1.0, h as f64 - 1.0);
        cr.stroke().unwrap();
    }
    cr.restore().unwrap();
    }

    // The freeze lines: where the frozen panes end, through the headers,
    // as Calc draws them, so it's clear what stays put.
    cr.set_source_rgb(hdr_text.0, hdr_text.1, hdr_text.2);
    cr.set_line_width(1.0);
    if sheet.frozen_cols > 0 {
        cr.move_to(crisp(left, 1.0), 0.0);
        cr.line_to(crisp(left, 1.0), height);
        cr.stroke().unwrap();
    }
    if sheet.frozen_rows > 0 {
        cr.move_to(0.0, crisp(top, 1.0));
        cr.line_to(width, crisp(top, 1.0));
        cr.stroke().unwrap();
    }

    // Selection range outline (2px accent around the whole rectangle).
    let (sr0, sc0, sr1, sc1) = sheet.selection_block();
    let px_x = |col: usize| -> f64 { tables_core::sheet::col_x(col, scroll_x, sheet) };
    let right_of = |col: usize| -> f64 { px_x(col) + sheet.col_width(col) };
    let x0 = px_x(sc0);
    let x1 = right_of(sc1);
    let y0 = tables_core::sheet::row_y(sr0, scroll_y, sheet);
    let y1 = tables_core::sheet::row_y(sr1, scroll_y, sheet) + sheet.row_height(sr1);
    cr.save().unwrap();
    cr.rectangle(ROW_HEADER_WIDTH, COL_HEADER_HEIGHT, width - ROW_HEADER_WIDTH, height - COL_HEADER_HEIGHT);
    cr.clip();
    if show_selection {
        cr.set_source_rgb(accent.0, accent.1, accent.2);
        cr.set_line_width(2.0);
        cr.rectangle(x0, y0, x1 - x0, y1 - y0);
        cr.stroke().unwrap();

        // Fill handle (#113): a small solid square at the selection's
        // bottom-right corner, same shared geometry hit_fill_handle() tests
        // presses against — see tables_core::sheet::fill_handle_center.
        cr.rectangle(
            x1 - tables_core::sheet::FILL_HANDLE_HALF,
            y1 - tables_core::sheet::FILL_HANDLE_HALF,
            tables_core::sheet::FILL_HANDLE_HALF * 2.0,
            tables_core::sheet::FILL_HANDLE_HALF * 2.0,
        );
        cr.fill().unwrap();
    }

    // Print area (#113): a dashed border around the range PDF export
    // will use, same convention as Excel/Sheets — print_area only ever
    // affected the export bounds before this, with no on-screen way to
    // see what was set. Editing chrome like the selection: a print never
    // shows its own print-area border, so the render lab leaves it out.
    if let Some((pr0, pc0, pr1, pc1)) = sheet.print_area.filter(|_| show_selection) {
        let px0 = px_x(pc0);
        let px1 = right_of(pc1);
        let py0 = tables_core::sheet::row_y(pr0, scroll_y, sheet);
        let py1 = tables_core::sheet::row_y(pr1, scroll_y, sheet) + sheet.row_height(pr1);
        cr.set_source_rgb(hdr_text.0, hdr_text.1, hdr_text.2);
        cr.set_line_width(1.5);
        cr.set_dash(&[4.0, 3.0], 0.0);
        cr.rectangle(px0, py0, px1 - px0, py1 - py0);
        cr.stroke().unwrap();
        cr.set_dash(&[], 0.0);
    }

    // Formula reference highlighting (#113): outline each cell/range the
    // formula being edited refers to, cycling through a fixed palette in
    // order of appearance — same convention as Excel/Sheets.
    for (i, &(fr0, fc0, fr1, fc1)) in formula_refs.iter().enumerate() {
        let color = FORMULA_REF_COLORS[i % FORMULA_REF_COLORS.len()];
        let fx0 = px_x(fc0);
        let fx1 = right_of(fc1);
        let fy0 = tables_core::sheet::row_y(fr0, scroll_y, sheet);
        let fy1 = tables_core::sheet::row_y(fr1, scroll_y, sheet) + sheet.row_height(fr1);
        cr.set_source_rgb(color.0, color.1, color.2);
        cr.set_line_width(1.5);
        cr.rectangle(fx0, fy0, fx1 - fx0, fy1 - fy0);
        cr.stroke().unwrap();
    }

    cr.restore().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tables_core::style::VAlign;

    /// Calibri 11's line box (17.9 px) in a 20 px row: bottom-aligned text
    /// starts just above the row, leaving its descent line 2 px clear of the
    /// bottom, rather than being clamped down to the row's top.
    #[test]
    fn bottom_aligned_text_keeps_its_bottom_margin_in_a_default_row() {
        let top = cell_text_top(VAlign::Bottom, 100.0, 20.0, 17.9);
        assert!((top - 100.1).abs() < 1e-9, "{top}");
        assert_eq!(cell_text_top(VAlign::Top, 100.0, 20.0, 17.9), 100.0 + CELL_PAD_V);
        assert!((cell_text_top(VAlign::Center, 100.0, 20.0, 18.0) - 101.0).abs() < 1e-9);
    }
}
