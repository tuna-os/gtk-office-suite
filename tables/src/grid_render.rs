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
    let _lw = 1.5;
    draw_border_line(cr, &border.top, x, y, x + w, y, is_dark);
    draw_border_line(cr, &border.bottom, x, y + h, x + w, y + h, is_dark);
    draw_border_line(cr, &border.left, x, y, x, y + h, is_dark);
    draw_border_line(cr, &border.right, x + w, y, x + w, y + h, is_dark);
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

fn draw_border_line(cr: &Context, style: &BorderStyle, x1: f64, y1: f64, x2: f64, y2: f64, is_dark: bool) {
    if *style == BorderStyle::None { return; }
    let c = if is_dark { BORDER_LINE_DARK } else { BORDER_LINE };
    cr.save().unwrap();
    cr.set_source_rgb(c.0, c.1, c.2);
    cr.set_line_width(1.5);
    match style {
        BorderStyle::Dotted => cr.set_dash(&[2.0, 2.0], 0.0),
        BorderStyle::Dashed => cr.set_dash(&[4.0, 4.0], 0.0),
        BorderStyle::Double => { cr.set_line_width(3.0); cr.set_dash(&[1.0, 3.0], 0.0); }
        _ => cr.set_dash(&[], 0.0),
    }
    cr.move_to(x1, y1);
    cr.line_to(x2, y2);
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
/// Space between a cell's edge and its text, each side, as Excel and Calc
/// leave it.
const CELL_PAD: f64 = 2.0;

/// The Pango layout for a cell's text in its style: font, weight, slant,
/// underline and strikethrough, alignment, and wrapping within `width`.
fn cell_layout(cr: &Context, sheet: &SheetModel, r: usize, c: usize, text: &str, width: f64) -> pango::Layout {
    use tables_core::style::HAlign;
    let style = &sheet.styles[r][c];
    let layout = pangocairo::functions::create_layout(cr);
    let mut font = layout.context().font_description().unwrap_or_default();
    font.set_family(style.font_family.as_deref().unwrap_or(tables_core::sheet::DEFAULT_FONT_FAMILY));
    let size = style.font_size.unwrap_or(tables_core::sheet::DEFAULT_FONT_SIZE);
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
    layout.set_width(((width - 2.0 * CELL_PAD - style.indent as f64 * INDENT_PX).max(1.0) * pango::SCALE as f64) as i32);
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

/// A cell's value through its number format and style, clipped to `rect`
/// (x, y, w, h). Clipping and ellipsizing keep long values inside their
/// cell; wrapped cells break across lines instead.
fn draw_cell_text(cr: &Context, sheet: &SheetModel, r: usize, c: usize, rect: (f64, f64, f64, f64), color: (f64, f64, f64)) {
    use tables_core::style::{HAlign, VAlign};
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
    let text_h = layout.pixel_size().1 as f64;
    let y = match style.v_align {
        VAlign::Top => cy + 2.0,
        VAlign::Center => cy + (rh - text_h) / 2.0,
        VAlign::Bottom => cy + rh - text_h - 2.0,
    };
    let indent = style.indent as f64 * INDENT_PX;
    let x = if sheet.resolved_h_align(r, c) == HAlign::Right { cx + CELL_PAD } else { cx + CELL_PAD + indent };
    cr.save().unwrap();
    cr.rectangle(cx + 1.0, cy + 1.0, (cw - 2.0).max(1.0), (rh - 2.0).max(1.0));
    cr.clip();
    cr.move_to(x, y.max(cy + 1.0));
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

#[allow(clippy::too_many_arguments)]
pub fn draw_grid(
    cr: &Context, state: &Rc<RefCell<crate::window::AppState>>,
    width: f64, height: f64, scroll_x: f64, scroll_y: f64, show_gridlines: bool,
    formula_refs: &[(usize, usize, usize, usize)], accent: (f64, f64, f64),
) {
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
    // range starting at a scroll-derived index.
    let mut cx = ROW_HEADER_WIDTH - scroll_x;
    for c in 0..sheet.cols {
        if sheet.is_col_hidden(c) { continue; }
        let cw = sheet.col_width(c);
        if cx + cw < ROW_HEADER_WIDTH { cx += cw; continue; }
        if cx > width { break; }
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
        cx += cw;
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
        if sheet.is_row_hidden(r) { continue; }
        let ry = tables_core::sheet::row_y(r, scroll_y, sheet);
        if ry + sheet.row_height(r) < COL_HEADER_HEIGHT { continue; }
        if ry > height { break; }
        cr.set_source_rgb(hdr_text.0, hdr_text.1, hdr_text.2);
        let label = (r + 1).to_string();
        let text_width = pango_text_width(cr, &label);
        draw_pango_text(cr, &label, ROW_HEADER_WIDTH - 6.0 - text_width, ry + sheet.row_height(r) / 2.0);
    }
    cr.restore().unwrap();

    // Cells
    cr.save().unwrap();
    cr.rectangle(ROW_HEADER_WIDTH, COL_HEADER_HEIGHT, width - ROW_HEADER_WIDTH, height - COL_HEADER_HEIGHT);
    cr.clip();
    for r in 0..sheet.rows {
        if sheet.is_row_hidden(r) { continue; }
        let cy = tables_core::sheet::row_y(r, scroll_y, sheet);
        let rh = sheet.row_height(r);
        if cy + rh < COL_HEADER_HEIGHT { continue; }
        if cy > height { break; }
        cx = ROW_HEADER_WIDTH - scroll_x;
        for c in 0..sheet.cols {
            if sheet.is_col_hidden(c) { continue; }
            let cw = sheet.col_width(c);
            if cx + cw < ROW_HEADER_WIDTH { cx += cw; continue; }
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

            // Grid line
            if show_gridlines {
                cr.set_source_rgb(grid_line.0, grid_line.1, grid_line.2);
                cr.set_line_width(0.5);
                if right_edge {
                    cr.move_to(cx + cw, cy);
                    cr.line_to(cx + cw, cy + rh);
                    cr.stroke().unwrap();
                }
                if bottom_edge {
                    cr.move_to(cx, cy + rh);
                    cr.line_to(cx + cw, cy + rh);
                    cr.stroke().unwrap();
                }
            }

            // Cell border
            if border.top != BorderStyle::None || border.bottom != BorderStyle::None
                || border.left != BorderStyle::None || border.right != BorderStyle::None {
                draw_border_edges(cr, cx, cy, cw, rh, border, is_dark);
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
            cx += cw;
        }
    }
    // Merged blocks: the anchor's value across the whole block, drawn after
    // every cell so the covered cells' backgrounds can't paint over it.
    for &(mr, mc, rs, cs) in &sheet.merges {
        let x = tables_core::sheet::col_x(mc, scroll_x, sheet);
        let y = tables_core::sheet::row_y(mr, scroll_y, sheet);
        let w = tables_core::sheet::col_x(mc + cs.max(1), scroll_x, sheet) - x;
        let h = tables_core::sheet::row_y(mr + rs.max(1), scroll_y, sheet) - y;
        if x > width || y > height || x + w < ROW_HEADER_WIDTH || y + h < COL_HEADER_HEIGHT {
            continue;
        }
        draw_cell_text(cr, sheet, mr, mc, (x, y, w, h), cell_text);
    }
    // Charts float above the cells at their anchor. They were read from
    // xlsx and saved back, but only ever drawn in the chart dialog's preview
    // (render lab `tables/chart`).
    for chart in &sheet.charts {
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

    // Selection range outline (2px accent around the whole rectangle).
    let (sr0, sc0, sr1, sc1) = sheet.selection_block();
    let px_x = |col: usize| -> f64 { tables_core::sheet::col_x(col, scroll_x, sheet) };
    let x0 = px_x(sc0);
    let x1 = px_x(sc1 + 1);
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
    // see what was set.
    if let Some((pr0, pc0, pr1, pc1)) = sheet.print_area {
        let px0 = px_x(pc0);
        let px1 = px_x(pc1 + 1);
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
        let fx1 = px_x(fc1 + 1);
        let fy0 = tables_core::sheet::row_y(fr0, scroll_y, sheet);
        let fy1 = tables_core::sheet::row_y(fr1, scroll_y, sheet) + sheet.row_height(fr1);
        cr.set_source_rgb(color.0, color.1, color.2);
        cr.set_line_width(1.5);
        cr.rectangle(fx0, fy0, fx1 - fx0, fy1 - fy0);
        cr.stroke().unwrap();
    }

    cr.restore().unwrap();
}
