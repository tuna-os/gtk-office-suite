// save.rs — xlsx export (sheets -> xlsx bytes/file).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of io.rs (issue #247).

// io.rs — Spreadsheet file I/O: xlsx/ods/csv/tsv import, xlsx export.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Extracted from tables/src/window.rs so it is unit-testable without GTK.

use crate::engine::TablesEngine;
use crate::sheet::SheetModel;

/// Save sheet data to an XLSX file. Numbers are written as numbers,
/// everything else as strings; formulas (from `engine`, first sheet)
/// are written as real formulas so they survive into other suites.
pub fn save_sheets_to_xlsx(path: &str, sheets: &[SheetModel]) -> Result<(), String> {
    save_sheets_to_xlsx_with_engine(path, sheets, None)
}

/// Save a workbook and retain captured unsupported package members. Existing
/// generated members win, so a changed supported part cannot be overwritten
/// by stale opaque bytes.
pub fn save_sheets_to_xlsx_with_opaque(
    path: &str,
    sheets: &[SheetModel],
    engine: Option<&TablesEngine>,
    opaque: &suite_common_core::interop::OpaquePackage,
) -> Result<(), String> {
    save_sheets_to_xlsx_with_engine(path, sheets, engine)?;
    opaque.append_to(path)
}

pub fn save_sheets_to_xlsx_with_engine(
    path: &str,
    sheets: &[SheetModel],
    engine: Option<&TablesEngine>,
) -> Result<(), String> {
    let bytes = save_sheets_to_xlsx_bytes(sheets, engine)?;
    suite_common_core::atomic_save::atomic_write_bytes(std::path::Path::new(path), &bytes)
}

/// Render the workbook to an in-memory xlsx buffer without touching disk —
/// shared by the real save path (which then writes it atomically) and
/// autosave (which writes it into a crash-recovery snapshot slot instead).
pub fn save_sheets_to_xlsx_bytes(
    sheets: &[SheetModel],
    engine: Option<&TablesEngine>,
) -> Result<Vec<u8>, String> {
    use rust_xlsxwriter::{Formula, Workbook};
    let mut workbook = Workbook::new();
    for (si, sh) in sheets.iter().enumerate() {
        let sheet = workbook.add_worksheet();
        sheet
            .set_name(&sh.name)
            .map_err(|e| format!("Sheet name: {}", e))?;
        for r in 0..sh.rows {
            let h = sh.row_height(r);
            if (h - crate::sheet::ROW_HEIGHT).abs() > f64::EPSILON {
                sheet
                    .set_row_height_pixels(r as u32, h as u32)
                    .map_err(|e| format!("Row height: {}", e))?;
            }
        }

        // Cells covered by a merge are written by merge_range below.
        let mut merged: std::collections::HashSet<(usize, usize)> =
            std::collections::HashSet::new();
        for (mr, mc, rows, cols) in &sh.merges {
            for r in *mr..mr + *rows {
                for c in *mc..mc + *cols {
                    merged.insert((r, c));
                }
            }
        }

        for r in 0..sh.rows {
            for c in 0..sh.cols {
                if merged.contains(&(r, c)) {
                    continue;
                }
                if let Some(eng) = engine {
                    if let Some(f) = eng.formula_at(si, r, c) {
                        // Cache the computed value alongside the formula:
                        // consumers that skip recalc-on-load (LibreOffice
                        // included) show the right result immediately.
                        let cached = eng.cell_at(si, r, c);
                        sheet
                            .write_formula(r as u32, c as u16, Formula::new(&f).set_result(&cached))
                            .map_err(|e| format!("Write error: {}", e))?;
                        continue;
                    }
                }
                let val = &sh.data[r][c];
                if val.is_empty() {
                    // An empty cell can still be styled: a boxed or filled
                    // blank is part of what the sheet looks like. Most empty
                    // cells are plain, and are skipped before building a
                    // format (performance_budgets' dense save).
                    if sh.styles[r][c].is_default() && sh.borders[r][c].is_none() {
                        continue;
                    }
                    if let Some(f) = cell_format(&sh.formats[r][c], &with_sheet_font(&sh.styles[r][c], sh), &sh.borders[r][c]) {
                        sheet.write_blank(r as u32, c as u16, &f).map_err(|e| format!("Write error: {}", e))?;
                    }
                    continue;
                }
                let format = cell_format(&sh.formats[r][c], &with_sheet_font(&sh.styles[r][c], sh), &sh.borders[r][c]);
                // Rust's f64::from_str accepts "inf"/"infinity"/"nan"
                // (any case, optionally signed) as valid floats — but a
                // user typing that text almost certainly means literal
                // text, not IEEE-754 infinity/NaN, and no spreadsheet
                // app treats it as numeric by default. Writing it as a
                // number here round-trips as "INF" (calamine's own
                // float formatting) on reopen, silently corrupting the
                // cell — caught by tables-core/tests/property.rs.
                if let Some(n) = val.parse::<f64>().ok().filter(|n| n.is_finite()) {
                    match &format {
                        Some(f) => sheet.write_number_with_format(r as u32, c as u16, n, f),
                        None => sheet.write_number(r as u32, c as u16, n),
                    }
                    .map_err(|e| format!("Write error: {}", e))?;
                } else {
                    match &format {
                        Some(f) => sheet.write_string_with_format(r as u32, c as u16, val, f),
                        None => sheet.write_string(r as u32, c as u16, val),
                    }
                    .map_err(|e| format!("Write error: {}", e))?;
                }
            }
        }

        for (mr, mc, rows, cols) in &sh.merges {
            let (lr, lc) = (mr + (*rows).max(1) - 1, mc + (*cols).max(1) - 1);
            let val = sh.data[*mr][*mc].clone();
            let format = cell_format(&sh.formats[*mr][*mc], &with_sheet_font(&sh.styles[*mr][*mc], sh), &sh.borders[*mr][*mc]).unwrap_or_default();
            sheet
                .merge_range(
                    *mr as u32,
                    *mc as u16,
                    lr as u32,
                    lc as u16,
                    &val,
                    &format,
                )
                .map_err(|e| format!("Merge error: {}", e))?;
        }
        if sh.frozen_rows > 0 || sh.frozen_cols > 0 {
            sheet
                .set_freeze_panes(sh.frozen_rows as u32, sh.frozen_cols as u16)
                .map_err(|e| format!("Freeze error: {}", e))?;
        }
        for c in 0..sh.cols {
            let w = sh.col_width(c);
            if (w - tables_core_default_col_width()).abs() > 0.5 {
                // In pixels: the writer applies Excel's own character-width
                // conversion, which props.rs inverts on read.
                sheet
                    .set_column_width_pixels(c as u16, w.round().max(1.0) as u32)
                    .map_err(|e| format!("Width error: {}", e))?;
            }
        }

        // Row/column hiding (#113): filter- and manually-hidden rows are
        // indistinguishable once written — a reopened file has no filter
        // state to restore, just hidden rows, same as real Excel/Sheets
        // after a filter is "applied" and the file is saved.
        for r in 0..sh.rows {
            if sh.is_row_hidden(r) {
                sheet
                    .set_row_hidden(r as u32)
                    .map_err(|e| format!("Hide row error: {}", e))?;
            }
        }
        for c in 0..sh.cols {
            if sh.is_col_hidden(c) {
                sheet
                    .set_column_hidden(c as u16)
                    .map_err(|e| format!("Hide column error: {}", e))?;
            }
        }

        // Print area (#113).
        if let Some((pr0, pc0, pr1, pc1)) = sh.print_area {
            sheet
                .set_print_area(pr0 as u32, pc0 as u16, pr1 as u32, pc1 as u16)
                .map_err(|e| format!("Print area error: {}", e))?;
        }

        // Page setup (#113): the page-setup dialog only ever offers these
        // four sizes (see tables/src/window.rs's size_names), so Custom
        // — otherwise unreachable from the UI — falls back to A4.
        {
            use suite_common_core::print::{Orientation, PageSize};
            let paper_size = match sh.page_setup.size {
                PageSize::Letter => 1,
                PageSize::A3 => 8,
                PageSize::A4 | PageSize::Custom { .. } => 9,
                PageSize::Legal => 5,
            };
            sheet.set_paper_size(paper_size);
            match sh.page_setup.orientation {
                Orientation::Landscape => {
                    sheet.set_landscape();
                }
                Orientation::Portrait => {
                    sheet.set_portrait();
                }
            }
            let mm_to_in = |mm: f64| mm / 25.4;
            sheet.set_margins(
                mm_to_in(sh.page_setup.margin_left_mm),
                mm_to_in(sh.page_setup.margin_right_mm),
                mm_to_in(sh.page_setup.margin_top_mm),
                mm_to_in(sh.page_setup.margin_bottom_mm),
                -1.0, // header/footer margins: leave at Excel's own default
                -1.0,
            );
        }

        for rule in &sh.cond_rules {
            use crate::sheet::CondOp;
            use rust_xlsxwriter::{ConditionalFormatCell, ConditionalFormatCellRule as R, Format};
            let cf_rule = match rule.op {
                CondOp::Greater => R::GreaterThan(rule.value),
                CondOp::Less => R::LessThan(rule.value),
                CondOp::Equal => R::EqualTo(rule.value),
                CondOp::Between => {
                    R::Between(rule.value.min(rule.value2), rule.value.max(rule.value2))
                }
            };
            let fmt = Format::new().set_background_color(rust_xlsxwriter::Color::RGB(
                u32::from_str_radix(&rule.fill, 16).unwrap_or(0xFFFF00),
            ));
            let cf = ConditionalFormatCell::new()
                .set_rule(cf_rule)
                .set_format(fmt);
            let (r0, c0, r1, c1) = rule.range;
            sheet
                .add_conditional_format(r0 as u32, c0 as u16, r1 as u32, c1 as u16, &cf)
                .map_err(|e| format!("Conditional format error: {}", e))?;
        }

        for ch in &sh.charts {
            use crate::sheet::{ChartKind, LegendPosition};
            use rust_xlsxwriter::{Chart, ChartLegendPosition, ChartType as XType};
            let mut chart = Chart::new(match ch.kind {
                ChartKind::Bar => XType::Column,
                ChartKind::Line => XType::Line,
                ChartKind::Pie => XType::Pie,
                ChartKind::Scatter => XType::Scatter,
                ChartKind::Area => XType::Area,
            });

            if !ch.series.is_empty() {
                for s in &ch.series {
                    let series = chart.add_series();
                    if !s.name.is_empty() {
                        series.set_name(&s.name);
                    }
                    series.set_categories((
                        sh.name.as_str(),
                        s.cat.0 as u32,
                        s.cat.1 as u16,
                        s.cat.2 as u32,
                        s.cat.1 as u16,
                    ));
                    series.set_values((
                        sh.name.as_str(),
                        s.val.0 as u32,
                        s.val.1 as u16,
                        s.val.2 as u32,
                        s.val.1 as u16,
                    ));
                }
            } else {
                chart
                    .add_series()
                    .set_categories((
                        sh.name.as_str(),
                        ch.cat.0 as u32,
                        ch.cat.1 as u16,
                        ch.cat.2 as u32,
                        ch.cat.1 as u16,
                    ))
                    .set_values((
                        sh.name.as_str(),
                        ch.val.0 as u32,
                        ch.val.1 as u16,
                        ch.val.2 as u32,
                        ch.val.1 as u16,
                    ));
            }

            if !ch.title.is_empty() {
                chart.title().set_name(&ch.title);
            }
            if let Some(x_title) = &ch.x_axis_title {
                chart.x_axis().set_name(x_title);
            }
            if let Some(y_title) = &ch.y_axis_title {
                chart.y_axis().set_name(y_title);
            }
            match ch.legend_position {
                LegendPosition::None => chart.legend().set_hidden(),
                LegendPosition::Top => chart.legend().set_position(ChartLegendPosition::Top),
                LegendPosition::Bottom => chart.legend().set_position(ChartLegendPosition::Bottom),
                LegendPosition::Left => chart.legend().set_position(ChartLegendPosition::Left),
                LegendPosition::Right => chart.legend().set_position(ChartLegendPosition::Right),
            };

            sheet
                .insert_chart(ch.anchor.0 as u32, ch.anchor.1 as u16, &chart)
                .map_err(|e| format!("Chart error: {}", e))?;
        }

        if sh.protection.protected {
            sheet.protect();
        }
    }
    // Named ranges (#113): all workbook-scoped in this app today (no
    // per-sheet-scoped names yet), so each gets a plain global
    // definition — rust_xlsxwriter strips the leading '=' before
    // writing, matching the '=' -free form our own formula string and
    // calamine's own read-back both use.
    if let Some(eng) = engine {
        for dn in &eng.model.workbook.defined_names {
            workbook
                .define_name(&dn.name, &format!("={}", dn.formula))
                .map_err(|e| format!("Defined name error: {}", e))?;
        }
    }
    workbook
        .save_to_buffer()
        .map_err(|e| format!("Save error: {}", e))
}

/// The xlsx format a cell is written with: its number format and its
/// CellStyle, or `None` when both are the defaults.
/// `style` with the sheet's default font spelled out when that isn't the
/// Calibri 11 this writer's workbook declares as its default: a sheet read
/// from Calc (Liberation Sans 10) keeps its font through an xlsx save.
fn with_sheet_font<'a>(style: &'a crate::style::CellStyle, sh: &SheetModel) -> std::borrow::Cow<'a, crate::style::CellStyle> {
    use crate::sheet::{DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE};
    let family_differs = sh.default_font_family != DEFAULT_FONT_FAMILY && style.font_family.is_none();
    let size_differs = (sh.default_font_size - DEFAULT_FONT_SIZE).abs() > f64::EPSILON && style.font_size.is_none();
    if !family_differs && !size_differs {
        return std::borrow::Cow::Borrowed(style);
    }
    let mut s = style.clone();
    if family_differs {
        s.font_family = Some(sh.default_font_family.clone());
    }
    if size_differs {
        s.font_size = Some(sh.default_font_size);
    }
    std::borrow::Cow::Owned(s)
}

fn cell_format(
    nf: &suite_common_core::format::NumberFormat,
    style: &crate::style::CellStyle,
    border: &crate::sheet::CellBorder,
) -> Option<rust_xlsxwriter::Format> {
    use crate::sheet::BorderStyle;
    use crate::style::{HAlign, VAlign};
    use rust_xlsxwriter::{Color, Format, FormatAlign, FormatBorder, FormatUnderline};
    let code = xlsx_num_format(nf);
    if code.is_none() && style.is_default() && border.is_none() {
        return None;
    }
    let rgb = |c: crate::style::Rgb| Color::RGB(((c.0 as u32) << 16) | ((c.1 as u32) << 8) | c.2 as u32);
    let mut f = Format::new();
    if !border.is_none() {
        let edge = |s: &BorderStyle| match s {
            BorderStyle::None => FormatBorder::None,
            BorderStyle::Solid => FormatBorder::Thin,
            BorderStyle::Medium => FormatBorder::Medium,
            BorderStyle::Thick => FormatBorder::Thick,
            BorderStyle::Dotted => FormatBorder::Dotted,
            BorderStyle::Dashed => FormatBorder::Dashed,
            BorderStyle::Double => FormatBorder::Double,
        };
        let to_u8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let (r, g, b) = border.color;
        let color = rgb(crate::style::Rgb(to_u8(r), to_u8(g), to_u8(b)));
        f = f
            .set_border_top(edge(&border.top))
            .set_border_bottom(edge(&border.bottom))
            .set_border_left(edge(&border.left))
            .set_border_right(edge(&border.right))
            .set_border_color(color);
    }
    if let Some(code) = code {
        f = f.set_num_format(&code);
    }
    if let Some(family) = &style.font_family {
        f = f.set_font_name(family);
    }
    if let Some(size) = style.font_size {
        f = f.set_font_size(size);
    }
    if style.bold {
        f = f.set_bold();
    }
    if style.italic {
        f = f.set_italic();
    }
    if style.underline {
        f = f.set_underline(FormatUnderline::Single);
    }
    if style.strikethrough {
        f = f.set_font_strikethrough();
    }
    if let Some(c) = style.color {
        f = f.set_font_color(rgb(c));
    }
    if let Some(c) = style.fill {
        f = f.set_background_color(rgb(c));
    }
    match style.h_align {
        HAlign::General => {}
        HAlign::Left => f = f.set_align(FormatAlign::Left),
        HAlign::Center => f = f.set_align(FormatAlign::Center),
        HAlign::Right => f = f.set_align(FormatAlign::Right),
    }
    match style.v_align {
        VAlign::Bottom => {}
        VAlign::Top => f = f.set_align(FormatAlign::Top),
        VAlign::Center => f = f.set_align(FormatAlign::VerticalCenter),
    }
    if style.wrap {
        f = f.set_text_wrap();
    }
    if style.indent > 0 {
        f = f.set_indent(style.indent);
    }
    Some(f)
}

/// Map our NumberFormat onto an xlsx number-format code, if non-default.
fn xlsx_num_format(nf: &suite_common_core::format::NumberFormat) -> Option<String> {
    use suite_common_core::format::NumberFormatKind::*;
    match &nf.kind {
        General => None,
        Number(d) => Some(if *d == 0 {
            "#,##0".to_string()
        } else {
            format!("#,##0.{}", "0".repeat(*d as usize))
        }),
        Currency(sym, d) => Some(if *d == 0 {
            format!("\"{}\"#,##0", sym)
        } else {
            format!("\"{}\"#,##0.{}", sym, "0".repeat(*d as usize))
        }),
        Percent(d) => Some(if *d == 0 {
            "0%".to_string()
        } else {
            format!("0.{}%", "0".repeat(*d as usize))
        }),
        Date(_) => Some("yyyy-mm-dd".to_string()),
        Fraction(d) => Some(format!("# {}/{}", "?".repeat(*d as usize), "?".repeat(*d as usize))),
        _ => None,
    }
}

/// The SheetModel default column width in px (COL_WIDTH).
fn tables_core_default_col_width() -> f64 {
    crate::sheet::COL_WIDTH
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::*;

    fn engine() -> TablesEngine {
        TablesEngine::new(20, 10).expect("engine")
    }

    fn write_temp(dir: &tempfile::TempDir, name: &str, content: &str) -> String {
        let p = dir.path().join(name);
        std::fs::write(&p, content).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn csv_import_basic_grid() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_temp(&dir, "t.csv", "a,b,c\n1,2,3\n");
        let mut e = engine();
        let (rows, cols) = load_file_into_engine(&path, &mut e).unwrap();
        assert_eq!((rows, cols), (2, 3));
        assert_eq!(e.cell(0, 0), "a");
        assert_eq!(e.cell(1, 2), "3");
    }

    #[test]
    fn csv_import_strips_quotes_and_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_temp(&dir, "t.csv", "\"hello\" , world\n");
        let mut e = engine();
        load_file_into_engine(&path, &mut e).unwrap();
        assert_eq!(e.cell(0, 0), "hello");
        assert_eq!(e.cell(0, 1), "world");
    }

    #[test]
    fn tsv_import_uses_tab_delimiter() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_temp(&dir, "t.tsv", "x\ty\n");
        let mut e = engine();
        let (_, cols) = load_file_into_engine(&path, &mut e).unwrap();
        assert_eq!(cols, 2);
        assert_eq!(e.cell(0, 1), "y");
    }

    #[test]
    fn csv_formulas_evaluate_on_import() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_temp(&dir, "t.csv", "1,2,=A1+B1\n");
        let mut e = engine();
        load_file_into_engine(&path, &mut e).unwrap();
        assert_eq!(e.cell(0, 2), "3");
    }

    #[test]
    fn unsupported_extension_is_an_error() {
        let mut e = engine();
        let err = load_file_into_engine("/nonexistent/file.foo", &mut e).unwrap_err();
        assert!(err.contains("Unsupported format"));
    }

    #[test]
    fn missing_xlsx_is_an_error_not_a_panic() {
        let mut e = engine();
        assert!(load_file_into_engine("/nonexistent/file.xlsx", &mut e).is_err());
    }

    #[test]
    fn resized_row_heights_write_without_error() {
        // calamine (this app's xlsx reader) has no row-height-reading API,
        // so a full write/read round trip isn't possible yet — this
        // confirms the write side succeeds and only non-default rows
        // trigger a set_row_height_pixels call (default rows are left
        // alone rather than writing a redundant explicit height for
        // every single row).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("row-height.xlsx");
        let mut sheet = SheetModel::new("Sheet1", 3, 2, 0);
        sheet.set_row_height(1, 60.0);
        save_sheets_to_xlsx(path.to_str().unwrap(), &[sheet]).unwrap();
        assert!(path.exists());
    }

    /// Font, fill, alignment and wrap survive a save and reopen: written
    /// through rust_xlsxwriter's Format, read back through styles.xml.
    #[test]
    fn cell_styles_round_trip_through_xlsx() {
        use crate::style::{CellStyle, HAlign, Rgb, VAlign};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("styles.xlsx").to_string_lossy().into_owned();
        let styled = CellStyle {
            font_family: Some("Liberation Serif".into()),
            font_size: Some(16.0),
            bold: true,
            italic: true,
            underline: true,
            color: Some(Rgb(0xC0, 0, 0)),
            fill: Some(Rgb(0xFF, 0xC7, 0xCE)),
            h_align: HAlign::Center,
            v_align: VAlign::Top,
            wrap: true,
            indent: 1,
            ..CellStyle::default()
        };
        let mut sheet = SheetModel::new("S", 3, 3, 0);
        sheet.data[0][0] = "styled".into();
        sheet.styles[0][0] = styled.clone();
        sheet.data[1][1] = "3.5".into();
        sheet.styles[1][1] = CellStyle { h_align: HAlign::Right, ..CellStyle::default() };
        sheet.data[2][2] = "plain".into();
        save_sheets_to_xlsx(&path, &[sheet]).unwrap();

        let (_, sheets) = crate::io::load_xlsx_workbook(&path).unwrap();
        let back = sheets[0].clone();
        assert_eq!(back.styles[0][0], styled);
        assert_eq!(back.styles[1][1].h_align, HAlign::Right);
        assert!(back.styles[2][2].is_default());
    }

    /// Borders were neither read nor written: a boxed table opened unboxed
    /// and saved that way. Weight, colour and an empty bordered cell all
    /// survive a save and reopen now.
    /// A sheet whose default font isn't Calibri 11 (one read from Calc)
    /// keeps its font through an xlsx save: the writer's workbook default is
    /// Calibri, so the font is spelled out on the cells that relied on it.
    #[test]
    fn a_sheets_default_font_survives_an_xlsx_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("font.xlsx").to_string_lossy().into_owned();
        let mut sheet = SheetModel::new("S", 2, 2, 0);
        (sheet.default_font_family, sheet.default_font_size) = ("Liberation Sans".into(), 10.0);
        sheet.data[0][0] = "plain".into();
        sheet.data[1][1] = "big".into();
        sheet.styles[1][1].font_size = Some(20.0);
        save_sheets_to_xlsx(&path, &[sheet]).unwrap();

        let (_, sheets) = crate::io::load_xlsx_workbook(&path).unwrap();
        let s = &sheets[0];
        let font = |r: usize, c: usize| {
            let st = &s.styles[r][c];
            (st.font_family.clone().unwrap_or(s.default_font_family.clone()), st.font_size.unwrap_or(s.default_font_size))
        };
        assert_eq!(font(0, 0), ("Liberation Sans".into(), 10.0));
        assert_eq!(font(1, 1), ("Liberation Sans".into(), 20.0));
    }

    #[test]
    fn cell_borders_round_trip_through_xlsx() {
        use crate::sheet::{BorderStyle, CellBorder};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("borders.xlsx").to_string_lossy().into_owned();
        let mut sheet = SheetModel::new("S", 3, 3, 0);
        sheet.data[0][0] = "boxed".into();
        sheet.borders[0][0] = CellBorder::outline(BorderStyle::Thick, (0.0, 0.0, 0.0));
        let mixed = CellBorder {
            top: BorderStyle::Solid,
            bottom: BorderStyle::Medium,
            left: BorderStyle::Dashed,
            right: BorderStyle::None,
            color: (1.0, 0.0, 0.0),
        };
        sheet.borders[1][1] = mixed.clone();
        save_sheets_to_xlsx(&path, &[sheet]).unwrap();

        let (_, sheets) = crate::io::load_xlsx_workbook(&path).unwrap();
        assert_eq!(sheets[0].borders[0][0], CellBorder::outline(BorderStyle::Thick, (0.0, 0.0, 0.0)));
        assert_eq!(sheets[0].borders[1][1], mixed, "an empty cell keeps its border");
        assert!(sheets[0].borders[2][2].is_none());
    }

    #[test]
    fn xlsx_round_trip_preserves_grid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rt.xlsx").to_string_lossy().into_owned();

        let mut sheet = SheetModel::new("Round Trip", 3, 3, 0);
        sheet.data[0][0] = "hello".into();
        sheet.data[0][1] = "42".into();
        sheet.data[2][2] = "3.5".into();
        save_sheets_to_xlsx(&path, &[sheet]).unwrap();

        let mut e = engine();
        let (rows, cols) = load_file_into_engine(&path, &mut e).unwrap();
        assert_eq!(e.cell(0, 0), "hello");
        assert_eq!(e.cell(0, 1), "42");
        assert_eq!(e.cell(2, 2), "3.5", "the value itself, not a 2-dp rendering of it");
        assert!(rows >= 3 && cols >= 3);
    }

    #[test]
    fn xlsx_round_trip_preserves_inf_and_nan_as_text() {
        // Rust's f64::from_str accepts "inf"/"nan" (any case) as valid
        // floats, but a user typing that text means literal text, not
        // IEEE-754 infinity/NaN — regression test for the round-trip
        // corruption ("inf" -> written as numeric infinity -> read back
        // as "INF") caught by tables-core/tests/property.rs.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inf.xlsx").to_string_lossy().into_owned();

        let mut sheet = SheetModel::new("Sheet1", 2, 2, 0);
        sheet.data[0][0] = "inf".into();
        sheet.data[0][1] = "NaN".into();
        sheet.data[1][0] = "-Infinity".into();
        save_sheets_to_xlsx(&path, &[sheet]).unwrap();

        let mut e = engine();
        load_file_into_engine(&path, &mut e).unwrap();
        assert_eq!(e.cell(0, 0), "inf");
        assert_eq!(e.cell(0, 1), "NaN");
        assert_eq!(e.cell(1, 0), "-Infinity");
    }

    #[test]
    fn formulas_survive_as_formulas() {
        use calamine::{open_workbook, Reader};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.xlsx");

        let mut e = engine();
        e.set_cell_text(0, 0, "2");
        e.set_cell_text(0, 1, "3");
        e.set_cell_text(1, 0, "=A1+B1");
        e.evaluate();
        let mut sheet = SheetModel::new("S", 3, 3, 0);
        sheet.data[0][0] = "2".into();
        sheet.data[0][1] = "3".into();
        sheet.data[1][0] = "5".into();
        save_sheets_to_xlsx_with_engine(path.to_str().unwrap(), &[sheet], Some(&e)).unwrap();

        let mut wb: calamine::Xlsx<_> = open_workbook(&path).unwrap();
        let names = wb.sheet_names().to_vec();
        let formulas = wb.worksheet_formula(&names[0]).unwrap();
        let has = formulas.rows().flatten().any(|f| f.contains("A1+B1"));
        assert!(has, "formula not written as formula");
    }

    #[test]
    fn formulas_on_each_sheet_are_written_as_formulas() {
        use calamine::{open_workbook, Reader};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi.xlsx");
        let mut engine = TablesEngine::new(3, 3).unwrap();
        engine.set_cell_text(0, 0, "2");
        engine.set_cell_text(0, 1, "=A1*2");
        engine.add_sheet("Sheet2").unwrap();
        engine.set_active_sheet(1).unwrap();
        engine.set_cell_text(0, 0, "3");
        engine.set_cell_text(0, 1, "=A1*3");

        let first = SheetModel::new("Sheet1", 3, 3, 0);
        let second = SheetModel::new("Sheet2", 3, 3, 1);
        save_sheets_to_xlsx_with_engine(path.to_str().unwrap(), &[first, second], Some(&engine))
            .unwrap();

        let mut book: calamine::Xlsx<_> = open_workbook(&path).unwrap();
        for (name, formula) in [("Sheet1", "A1*2"), ("Sheet2", "A1*3")] {
            let formulas = book.worksheet_formula(name).unwrap();
            assert!(formulas
                .rows()
                .flatten()
                .any(|value| value.contains(formula)));
        }
    }

    #[test]
    fn xlsx_workbook_loader_preserves_sheet_order_and_cross_sheet_formula() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi-load.xlsx");
        let mut engine = TablesEngine::new(3, 3).unwrap();
        engine.set_cell_text(0, 0, "4");
        engine.add_sheet("Rates").unwrap();
        engine.set_active_sheet(1).unwrap();
        engine.set_cell_text(0, 0, "=Sheet1!A1*5");
        engine.set_active_sheet(0).unwrap();
        let mut first = SheetModel::new("Sheet1", 3, 3, 0);
        first.sync_from_engine(&engine);
        engine.set_active_sheet(1).unwrap();
        let mut second = SheetModel::new("Rates", 3, 3, 1);
        second.sync_from_engine(&engine);
        let sheets = [first, second];
        save_sheets_to_xlsx_with_engine(path.to_str().unwrap(), &sheets, Some(&engine)).unwrap();

        let (mut loaded, sheets) = load_xlsx_workbook(path.to_str().unwrap()).unwrap();
        assert_eq!(
            sheets
                .iter()
                .map(|sheet| sheet.name.as_str())
                .collect::<Vec<_>>(),
            ["Sheet1", "Rates"]
        );
        loaded.set_active_sheet(1).unwrap();
        assert_eq!(loaded.formula(0, 0).as_deref(), Some("Sheet1!A1*5"));
        assert_eq!(loaded.cell(0, 0), "20");
    }

    #[test]
    fn print_area_hidden_rows_and_page_setup_round_trip_through_load_xlsx_workbook() {
        use suite_common_core::print::{Orientation, PageSize};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("props.xlsx");

        let mut s1 = SheetModel::new("Sheet1", 5, 5, 0);
        s1.print_area = Some((0, 0, 2, 1));
        s1.hidden_rows_manual.insert(3);
        s1.hidden_cols.insert(4);
        s1.page_setup.size = PageSize::A3;
        s1.page_setup.orientation = Orientation::Landscape;
        let s2 = SheetModel::new("Sheet2", 4, 4, 1);
        save_sheets_to_xlsx(path.to_str().unwrap(), &[s1, s2]).unwrap();

        let (_engine, sheets) = load_xlsx_workbook(path.to_str().unwrap()).unwrap();
        let loaded_s1 = sheets.iter().find(|s| s.name == "Sheet1").unwrap();
        let loaded_s2 = sheets.iter().find(|s| s.name == "Sheet2").unwrap();

        assert_eq!(loaded_s1.print_area, Some((0, 0, 2, 1)));
        assert!(loaded_s1.is_row_hidden(3));
        assert!(loaded_s1.is_col_hidden(4));
        assert_eq!(loaded_s1.page_setup.size, PageSize::A3);
        assert_eq!(loaded_s1.page_setup.orientation, Orientation::Landscape);

        assert_eq!(loaded_s2.print_area, None, "Sheet2 must not inherit Sheet1's print area");
        assert!(!loaded_s2.is_row_hidden(3));
        assert!(!loaded_s2.is_col_hidden(4));
    }

    #[test]
    fn defined_names_round_trip_through_xlsx() {
        // A single-cell name, used directly in a formula, exercises both
        // that names survive round-tripping AND that the round-tripped
        // name is genuinely usable, not just bookkeeping. (A range-valued
        // name in a scalar formula context, e.g. `=Total*2` where Total
        // is a 3-cell range, hits an unrelated IronCalc implicit-
        // intersection panic — an upstream engine limitation, not
        // something this persistence slice needs to work around.)
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("names-rt.xlsx");
        let mut engine = TablesEngine::new(4, 2).unwrap();
        engine.set_cell_text(0, 0, "21");
        engine
            .model
            .new_defined_name("Rate", None, "Sheet1!$A$1")
            .unwrap();
        engine.set_cell_text(1, 0, "=Rate*2");
        engine.evaluate();
        let mut sheet = SheetModel::new("Sheet1", 4, 2, 0);
        sheet.sync_from_engine(&engine);
        save_sheets_to_xlsx_with_engine(path.to_str().unwrap(), &[sheet], Some(&engine)).unwrap();

        let (mut loaded, _sheets) = load_xlsx_workbook(path.to_str().unwrap()).unwrap();
        let names = &loaded.model.workbook.defined_names;
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].name, "Rate");
        assert_eq!(names[0].formula, "Sheet1!$A$1");
        assert_eq!(loaded.formula(1, 0).as_deref(), Some("Rate*2"));
        loaded.evaluate();
        assert_eq!(loaded.cell(1, 0), "42");
    }

    #[test]
    fn number_formats_written_to_xlsx() {
        use suite_common_core::format::{NumberFormat, NumberFormatKind};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fmt.xlsx");
        let mut sheet = SheetModel::new("Fmt", 2, 2, 0);
        sheet.data[0][0] = "0.5".into();
        sheet.formats[0][0] = NumberFormat {
            kind: NumberFormatKind::Percent(1),
        };
        save_sheets_to_xlsx(path.to_str().unwrap(), &[sheet]).unwrap();
        // the format lives in styles.xml; presence check via zip
        let bytes = std::fs::read(&path).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut styles = String::new();
        use std::io::Read as _;
        zip.by_name("xl/styles.xml")
            .unwrap()
            .read_to_string(&mut styles)
            .unwrap();
        assert!(
            styles.contains("0.0%"),
            "percent format missing from styles: {styles}"
        );
    }
}
