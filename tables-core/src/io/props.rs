// props.rs — per-sheet xlsx properties: hidden rows/cols, page setup.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of io.rs (issue #247).

// io.rs — Spreadsheet file I/O: xlsx/ods/csv/tsv import, xlsx export.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Extracted from tables/src/window.rs so it is unit-testable without GTK.

use suite_common_core::zip_guard::{BoundedArchive, ZipBudget};


/// Per-sheet presentation state (#113) that calamine doesn't expose:
/// hidden rows/columns and page setup. Read directly from each sheet's
/// own XML part, resolved by name through workbook.xml + the workbook's
/// rels (not just "sheet1.xml" — unlike [[read_charts_from_xlsx]]/
/// [[read_cond_rules_from_xlsx]]'s existing single-sheet shortcut, this
/// is used by the real multi-sheet "Open File" path, where getting the
/// wrong sheet's hidden rows would be a correctness bug, not just a
/// missing-feature gap).
#[derive(Default, Debug, PartialEq)]
pub struct SheetXlsxProps {
    pub hidden_rows: std::collections::HashSet<usize>,
    pub hidden_cols: std::collections::HashSet<usize>,
    pub page_setup: Option<suite_common_core::print::PageSetup>,
    /// Explicitly-sized columns and rows, in pixels, 0-based — only the
    /// ones the file marks as custom, so a sheet of default-width columns
    /// produces an empty map rather than a row of defaults.
    pub col_widths: std::collections::HashMap<usize, f64>,
    pub row_heights: std::collections::HashMap<usize, f64>,
    /// `(frozen_rows, frozen_cols)` from a frozen `<pane>`, absent when
    /// the sheet has no split or a merely *split* (draggable) one, which
    /// this app has no model for and must not silently read as frozen.
    pub frozen: Option<(usize, usize)>,
    /// Merged ranges as `(row, col, rowspan, colspan)` — the shape
    /// `SheetModel::merges` uses, so the caller assigns it directly.
    pub merges: Vec<(usize, usize, usize, usize)>,
    /// What cells name through their style (`<c s="n">`): number format and
    /// CellStyle, 0-based `(row, col, style)`. Cells on the default style
    /// are absent.
    pub cell_styles: Vec<(usize, usize, super::XfStyle)>,
}

/// xlsx stores a column width in character units of the default font's
/// maximum digit width (MDW), and the stored number already includes the
/// cell margin padding: Excel draws `<col width="w">` as
/// `trunc(((256·w + trunc(128/MDW)) / 256) · MDW)` pixels. With Calibri 11's
/// 7 px MDW that is 7·w, so the default 9.140625 is 64 px and a width of 12
/// is 84 px. (The user-facing "8.43 characters" is w minus the padding; the
/// file holds w.) This used to subtract the padding again, drawing every
/// column from a foreign file 5 px narrow. The round-trip and Excel-value
/// tests pin real numbers rather than trusting this comment.
const PIXELS_PER_CHAR: f64 = 7.0;
/// Row heights are stored in points; 96dpi pixels are 0.75pt each.
const POINTS_PER_PIXEL: f64 = 0.75;

fn width_chars_to_pixels(chars: f64) -> f64 {
    let w = chars.max(0.0);
    ((256.0 * w + (128.0 / PIXELS_PER_CHAR).trunc()) / 256.0 * PIXELS_PER_CHAR).trunc()
}

fn height_points_to_pixels(points: f64) -> f64 {
    (points / POINTS_PER_PIXEL * 100.0).round() / 100.0
}

fn xml_attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

/// OOXML boolean attributes are legal as either "1"/"0" or "true"/"false"
/// — our own writer emits "1", but LibreOffice's re-saved xlsx uses
/// "true", so a strict `== Some("1")` check silently drops hidden
/// rows/cols on a Calc round trip.
fn xml_bool_attr(tag: &str, attr: &str) -> bool {
    matches!(xml_attr(tag, attr), Some("1") | Some("true"))
}

/// name → target for each `<Relationship Id="rIdN" Target="...">` in a
/// `.rels` part.
fn parse_rels(xml: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for tag in xml.split("<Relationship ").skip(1) {
        let tag = tag.split('>').next().unwrap_or("");
        if let (Some(id), Some(target)) = (xml_attr(tag, "Id"), xml_attr(tag, "Target")) {
            out.insert(id.to_string(), target.to_string());
        }
    }
    out
}

/// sheet name → XML part path (e.g. "xl/worksheets/sheet2.xml"), via
/// workbook.xml's `<sheet name=".." r:id="rIdN">` and the workbook's own
/// rels part.
fn resolve_sheet_parts(
    zip: &mut zip::ZipArchive<std::fs::File>,
    budget: &mut ZipBudget,
) -> std::collections::HashMap<String, String> {
    let Ok(workbook_xml) = zip.part_to_string("xl/workbook.xml", budget) else {
        return std::collections::HashMap::new();
    };
    let Ok(rels_xml) = zip.part_to_string("xl/_rels/workbook.xml.rels", budget) else {
        return std::collections::HashMap::new();
    };
    let rels = parse_rels(&rels_xml);

    let mut out = std::collections::HashMap::new();
    let Some(sheets_block) = workbook_xml.split("<sheets>").nth(1) else {
        return out;
    };
    let sheets_block = sheets_block.split("</sheets>").next().unwrap_or("");
    for tag in sheets_block.split("<sheet ").skip(1) {
        let tag = tag.split('>').next().unwrap_or("");
        let Some(name) = xml_attr(tag, "name") else { continue };
        // The r:id attribute's literal text is "r:id" regardless of
        // namespace prefix binding — real-world xlsx writers don't vary
        // this in practice.
        let Some(rid) = xml_attr(tag, "r:id") else { continue };
        if let Some(target) = rels.get(rid) {
            let target = target.trim_start_matches("/xl/").trim_start_matches("xl/");
            out.insert(name.to_string(), format!("xl/{target}"));
        }
    }
    out
}

/// Read hidden rows/columns and page setup for every sheet in `names`,
/// keyed by sheet name.
pub fn read_sheet_props_from_xlsx(
    path: &str,
    names: &[String],
) -> std::collections::HashMap<String, SheetXlsxProps> {
    let mut out = std::collections::HashMap::new();
    let Ok(f) = std::fs::File::open(path) else {
        return out;
    };
    let Ok(mut zip) = zip::ZipArchive::new(f) else {
        return out;
    };
    // Bounded reads: best-effort must not extend to decompressing whatever
    // an untrusted package asks for (#442).
    let mut budget = ZipBudget::default();
    if budget.check_entry_count(zip.len()).is_err() {
        return out;
    }
    let parts = resolve_sheet_parts(&mut zip, &mut budget);
    let xf_styles =
        super::xlsx_styles::parse_cell_styles(&zip.optional_part_to_string("xl/styles.xml", &mut budget));

    for name in names {
        let Some(part) = parts.get(name) else { continue };
        let Ok(xml) = zip.part_to_string(part, &mut budget) else {
            continue;
        };

        let mut props = SheetXlsxProps {
            cell_styles: super::numfmt::cell_style_indices(&xml)
                .into_iter()
                .filter(|&(r, c, _)| r < crate::sheet::SHEET_MAX_ROWS && c < crate::sheet::SHEET_MAX_COLS)
                .filter_map(|(r, c, s)| xf_styles.get(s).map(|x| (r, c, x.clone())))
                .filter(|(_, _, x)| *x != super::XfStyle::default())
                .collect(),
            ..SheetXlsxProps::default()
        };

        if let Some(cols_block) = xml.split("<cols>").nth(1) {
            let cols_block = cols_block.split("</cols>").next().unwrap_or("");
            for tag in cols_block.split("<col ").skip(1) {
                let tag = tag.split('>').next().unwrap_or("").trim_end_matches('/');
                let hidden = xml_bool_attr(tag, "hidden");
                // A `<col>` run carries a width, a hidden flag, or both, so
                // neither one may short-circuit the other. This loop used to
                // `continue` on anything not hidden, which is how every
                // custom column width in a saved workbook was read back as
                // the default.
                let custom_width = xml_bool_attr(tag, "customWidth");
                if !hidden && !custom_width {
                    continue;
                }
                let min: Option<usize> = xml_attr(tag, "min").and_then(|v| v.parse().ok());
                let max: Option<usize> = xml_attr(tag, "max").and_then(|v| v.parse().ok());
                if let (Some(min), Some(max)) = (min, max) {
                    // Columns are 1-based, so 0 is not a column: it used to
                    // underflow `c - 1` (a panic in debug, `usize::MAX` in
                    // release, which then recorded a nonsense hidden column).
                    // And the range is clamped to the grid's own width: a
                    // file claiming 4 billion hidden columns is not
                    // describing a spreadsheet, and looping over it hangs
                    // rather than refusing (#442).
                    let first = min.max(1);
                    let last = max.min(crate::sheet::SHEET_MAX_COLS);
                    let width = if custom_width {
                        xml_attr(tag, "width")
                            .and_then(|v| v.parse::<f64>().ok())
                            .filter(|w| w.is_finite() && *w >= 0.0)
                            .map(width_chars_to_pixels)
                    } else {
                        None
                    };
                    for c in first..=last {
                        if hidden {
                            props.hidden_cols.insert(c - 1); // 1-based → 0-based
                        }
                        if let Some(px) = width {
                            props.col_widths.insert(c - 1, px);
                        }
                    }
                }
            }
        }

        if let Some(data_block) = xml.split("<sheetData>").nth(1) {
            let data_block = data_block.split("</sheetData>").next().unwrap_or("");
            for tag in data_block.split("<row ").skip(1) {
                let tag = tag.split('>').next().unwrap_or("").trim_end_matches('/');
                let hidden = xml_bool_attr(tag, "hidden");
                let custom_height = xml_bool_attr(tag, "customHeight");
                if !hidden && !custom_height {
                    continue;
                }
                if let Some(r) = xml_attr(tag, "r").and_then(|v| v.parse::<usize>().ok()) {
                    // Rows are 1-based for the same reason, with the same
                    // underflow if a file says row 0.
                    if (1..=crate::sheet::SHEET_MAX_ROWS).contains(&r) {
                        if hidden {
                            props.hidden_rows.insert(r - 1); // 1-based → 0-based
                        }
                        if custom_height {
                            if let Some(px) = xml_attr(tag, "ht")
                                .and_then(|v| v.parse::<f64>().ok())
                                .filter(|h| h.is_finite() && *h >= 0.0)
                                .map(height_points_to_pixels)
                            {
                                props.row_heights.insert(r - 1, px);
                            }
                        }
                    }
                }
            }
        }

        // `<pane state="frozen">` — split panes are a different feature
        // (draggable, not locked) and this app models only frozen ones, so a
        // plain `<pane>` without that state is left alone rather than read
        // as a freeze the user never asked for.
        if let Some(tag) = xml.split("<pane ").nth(1) {
            let tag = tag.split('>').next().unwrap_or("").trim_end_matches('/');
            if matches!(xml_attr(tag, "state"), Some("frozen") | Some("frozenSplit")) {
                let axis = |attr: &str| {
                    xml_attr(tag, attr)
                        .and_then(|v| v.parse::<f64>().ok())
                        .filter(|n| n.is_finite() && *n >= 0.0)
                        .map(|n| n as usize)
                        .unwrap_or(0)
                };
                // ySplit counts frozen *rows*, xSplit frozen *columns* —
                // the opposite pairing to the (rows, cols) order below, and
                // an easy thing to transpose silently.
                let (rows, cols) = (axis("ySplit"), axis("xSplit"));
                if rows > 0 || cols > 0 {
                    props.frozen = Some((
                        rows.min(crate::sheet::SHEET_MAX_ROWS),
                        cols.min(crate::sheet::SHEET_MAX_COLS),
                    ));
                }
            }
        }

        if let Some(block) = xml.split("<mergeCells").nth(1) {
            let block = block.split("</mergeCells>").next().unwrap_or("");
            for tag in block.split("<mergeCell ").skip(1) {
                let tag = tag.split('>').next().unwrap_or("").trim_end_matches('/');
                let Some(reference) = xml_attr(tag, "ref") else { continue };
                let reference = reference.replace('$', "");
                let (a, b) = match reference.split_once(':') {
                    Some(pair) => pair,
                    // A one-cell "merge" spans nothing; keeping it would put
                    // a 1x1 entry in the model for no reason.
                    None => continue,
                };
                let (Some((r0, c0)), Some((r1, c1))) = (
                    crate::sheet::parse_cell_ref(a),
                    crate::sheet::parse_cell_ref(b),
                ) else {
                    continue;
                };
                // Written either way round in principle, so normalise rather
                // than trusting top-left-first.
                let (top, left) = (r0.min(r1), c0.min(c1));
                let (bottom, right) = (r0.max(r1), c0.max(c1));
                if bottom >= crate::sheet::SHEET_MAX_ROWS || right >= crate::sheet::SHEET_MAX_COLS {
                    continue;
                }
                props.merges.push((top, left, bottom - top + 1, right - left + 1));
            }
        }

        if let Some(tag) = xml.split("<pageSetup ").nth(1) {
            let tag = tag.split('>').next().unwrap_or("");
            use suite_common_core::print::{Orientation, PageSetup, PageSize};
            let mut setup = PageSetup {
                size: match xml_attr(tag, "paperSize") {
                    Some("1") => PageSize::Letter,
                    Some("5") => PageSize::Legal,
                    Some("8") => PageSize::A3,
                    _ => PageSize::A4,
                },
                orientation: match xml_attr(tag, "orientation") {
                    Some("landscape") => Orientation::Landscape,
                    _ => Orientation::Portrait,
                },
                ..PageSetup::default()
            };
            if let Some(m) = xml.split("<pageMargins ").nth(1) {
                let m = m.split('>').next().unwrap_or("");
                let in_to_mm = |v: &str| v.parse::<f64>().ok().map(|inches| inches * 25.4);
                if let Some(mm) = xml_attr(m, "left").and_then(in_to_mm) {
                    setup.margin_left_mm = mm;
                }
                if let Some(mm) = xml_attr(m, "right").and_then(in_to_mm) {
                    setup.margin_right_mm = mm;
                }
                if let Some(mm) = xml_attr(m, "top").and_then(in_to_mm) {
                    setup.margin_top_mm = mm;
                }
                if let Some(mm) = xml_attr(m, "bottom").and_then(in_to_mm) {
                    setup.margin_bottom_mm = mm;
                }
            }
            props.page_setup = Some(setup);
        }

        out.insert(name.clone(), props);
    }
    out
}

/// A cell/range reference of the form `'Sheet Name'!$A$1:$C$5` or
/// `Sheet1!$A$1:$C$5`, as written into the `_xlnm.Print_Area` built-in
/// defined name — sheet name plus a 0-based inclusive rect.
pub(super) fn parse_sheet_qualified_range(s: &str) -> Option<(String, usize, usize, usize, usize)> {
    use crate::sheet::parse_cell_ref;
    let (sheet_part, range_part) = s.rsplit_once('!')?;
    let sheet_name = sheet_part.trim_matches('\'').replace("''", "'");
    let range = range_part.replace('$', "");
    let (a, b) = range.split_once(':').unwrap_or((range.as_str(), range.as_str()));
    let (r0, c0) = parse_cell_ref(a)?;
    let (r1, c1) = parse_cell_ref(b)?;
    Some((sheet_name, r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1)))
}

#[cfg(test)]
mod sheet_props_tests {
    use super::*;
    use crate::io::*;
    use crate::sheet::SheetModel;
    
    #[test]
    fn hidden_rows_and_cols_round_trip_per_sheet() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hide.xlsx");
        let mut s1 = SheetModel::new("Sheet1", 5, 5, 0);
        s1.hidden_rows_manual.insert(1);
        s1.hidden_cols.insert(2);
        let s2 = SheetModel::new("Sheet2", 5, 5, 1);
        save_sheets_to_xlsx(path.to_str().unwrap(), &[s1, s2]).unwrap();

        let names = vec!["Sheet1".to_string(), "Sheet2".to_string()];
        let props = read_sheet_props_from_xlsx(path.to_str().unwrap(), &names);
        assert!(props["Sheet1"].hidden_rows.contains(&1));
        assert!(props["Sheet1"].hidden_cols.contains(&2));
        assert!(props["Sheet2"].hidden_rows.is_empty(), "Sheet2 must not inherit Sheet1's hides");
        assert!(props["Sheet2"].hidden_cols.is_empty());
    }

    /// The pixel widths Excel shows for `<col width>` values any producer
    /// writes (openpyxl stores what the user typed): the default 9.140625 is
    /// 64 px, 12 is 84 px, 40 is 280 px. The reader used to subtract the
    /// cell padding a second time and drew each of these 5 px narrow.
    #[test]
    fn column_widths_read_as_excel_draws_them() {
        assert_eq!(width_chars_to_pixels(9.140625), 64.0);
        assert_eq!(width_chars_to_pixels(12.0), 84.0);
        assert_eq!(width_chars_to_pixels(40.0), 280.0);
        assert_eq!(width_chars_to_pixels(0.0), 0.0);
    }

    #[test]
    fn xml_bool_attr_accepts_true_as_well_as_1() {
        // LibreOffice's own xlsx export writes hidden="true" (a legal
        // OOXML boolean per the xsd:boolean spec) rather than our
        // writer's hidden="1" — a naive `== Some("1")` check silently
        // drops every hidden row/col on a Calc round trip (caught by
        // soffice_oracle.rs's hidden_rows_and_cols_survive_calc_rewrite).
        assert!(xml_bool_attr("r=\"3\" hidden=\"true\"", "hidden"));
        assert!(xml_bool_attr("r=\"3\" hidden=\"1\"", "hidden"));
        assert!(!xml_bool_attr("r=\"3\" hidden=\"false\"", "hidden"));
        assert!(!xml_bool_attr("r=\"3\"", "hidden"));
    }

    #[test]
    fn page_setup_round_trips() {
        use suite_common_core::print::{Orientation, PageSize};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("setup.xlsx");
        let mut s = SheetModel::new("Sheet1", 3, 3, 0);
        s.page_setup.size = PageSize::Legal;
        s.page_setup.orientation = Orientation::Landscape;
        s.page_setup.margin_left_mm = 10.0;
        save_sheets_to_xlsx(path.to_str().unwrap(), &[s]).unwrap();

        let names = vec!["Sheet1".to_string()];
        let props = read_sheet_props_from_xlsx(path.to_str().unwrap(), &names);
        let setup = props["Sheet1"].page_setup.as_ref().unwrap();
        assert_eq!(setup.size, PageSize::Legal);
        assert_eq!(setup.orientation, Orientation::Landscape);
        assert!((setup.margin_left_mm - 10.0).abs() < 0.1, "{}", setup.margin_left_mm);
    }

    #[test]
    fn print_area_string_parses_sheet_and_range() {
        assert_eq!(
            parse_sheet_qualified_range("'Sheet2'!$A$1:$C$5"),
            Some(("Sheet2".to_string(), 0, 0, 4, 2))
        );
        assert_eq!(
            parse_sheet_qualified_range("Sheet1!$B$2:$B$2"),
            Some(("Sheet1".to_string(), 1, 1, 1, 1))
        );
    }
}

// ── ODF sheet layout ────────────────────────────────────────────────────
//
// The xlsx reader learned column widths, row heights, frozen panes and
// merges in #716, after every one of them survived a save and vanished on
// reopen. The ods reader never did: `load_ods_workbook` asks calamine for
// cell values and stops, so opening a spreadsheet someone sent as .ods
// still loses its whole layout. The data is right there — a Calc-written
// ods carries `style:column-width`, `style:row-height` and the span
// attributes — which is why this reads the same struct the xlsx path
// fills, rather than inventing a second shape for the loader to merge.

/// The text after each `<name` start tag, for elements whose tag may be
/// followed by any whitespace rather than exactly one space.
///
/// `split("<table:table ")` is the obvious thing and is wrong for a
/// pretty-printed file: a producer that writes `<style:style\n  style:...`
/// matches no delimiter at all, so the scan silently finds nothing and
/// every width, height and merge reads as absent. Calc writes compact XML,
/// which is why that failed quietly rather than loudly.
fn split_elements<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let needle = format!("<{name}");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&needle) {
        let after = &rest[i + needle.len()..];
        // `<table:table` must not match `<table:table-row`: the next byte
        // has to end the name. Defensive rather than load-bearing — a
        // spurious match carries no `table:name` and is skipped anyway —
        // and said so here rather than propped up by a contrived fixture.
        match after.chars().next() {
            Some(c) if c.is_whitespace() || c == '>' || c == '/' => out.push(after),
            _ => {}
        }
        rest = after;
    }
    out
}

/// An ODF length in px at 96dpi. ODF writes an absolute unit on every
/// length, and Calc picks whichever suits the locale, so all four have to
/// be understood rather than the one that happened to appear in a fixture.
fn odf_length_to_pixels(v: &str) -> Option<f64> {
    let v = v.trim();
    let (num, per_inch) = if let Some(n) = v.strip_suffix("in") {
        (n, 1.0)
    } else if let Some(n) = v.strip_suffix("cm") {
        (n, 2.54)
    } else if let Some(n) = v.strip_suffix("mm") {
        (n, 25.4)
    } else if let Some(n) = v.strip_suffix("pt") {
        (n, 72.0)
    } else {
        // The last arm carries the `?`: a length with no unit at all is
        // not an ODF length, and ODF always writes one.
        (v.strip_suffix("px")?, 96.0)
    };
    let n: f64 = num.trim().parse().ok()?;
    let px = n / per_inch * 96.0;
    // A negative or non-finite length is a malformed file, not a shape to
    // pass on to the grid.
    (px.is_finite() && px >= 0.0).then(|| (px * 100.0).round() / 100.0)
}

/// style name → length in px, for one `style:family`.
///
/// Column and row styles live in the same `office:automatic-styles` block.
/// The family check is a guard rather than the thing doing the work: the
/// property attribute already discriminates, because only a column style
/// carries `style:column-width` and only a row style carries
/// `style:row-height`. Removing it fails no test here, and that is stated
/// rather than papered over with a fixture contrived to make it look
/// load-bearing.
fn odf_styles_by_family(xml: &str, family: &str, prop_attr: &str) -> std::collections::HashMap<String, f64> {
    let mut out = std::collections::HashMap::new();
    for block in split_elements(xml, "style:style") {
        let head = block.split('>').next().unwrap_or("");
        if xml_attr(head, "style:family") != Some(family) {
            continue;
        }
        let Some(name) = xml_attr(head, "style:name") else { continue };
        // Only this style's own body. The outer split already ends a
        // block at the next `<style:style ` — but only when that tag is
        // written with a trailing space. A pretty-printed file puts a
        // newline there instead, the split misses it, and without this
        // trim a style with no length of its own would take the next
        // one's.
        let body = block.split("</style:style>").next().unwrap_or("");
        if let Some(v) = xml_attr(body, prop_attr).and_then(odf_length_to_pixels) {
            out.insert(name.to_string(), v);
        }
    }
    out
}

/// A `table:number-*-repeated` count, defaulting to 1.
///
/// Capped because ODF's way of saying "the rest of the sheet is empty" is
/// a repeat count in the millions on a trailing row or column; taking it
/// literally would allocate for a sheet nobody has.
fn odf_repeat(tag: &str, attr: &str) -> usize {
    xml_attr(tag, attr)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .min(ODF_MAX_REPEAT)
}

const ODF_MAX_REPEAT: usize = 4096;

/// Column widths, row heights and merges for every sheet in an ods,
/// keyed by sheet name.
///
/// Frozen panes are deliberately not read here: they live in
/// `settings.xml`, not `content.xml`, and Calc's own headless
/// xlsx -> ods conversion does not write them at all, so there is no
/// fixture this could be checked against. Left absent rather than guessed.
pub fn read_sheet_props_from_ods(
    path: &str,
) -> std::collections::HashMap<String, SheetXlsxProps> {
    let mut out = std::collections::HashMap::new();
    let Ok(f) = std::fs::File::open(path) else {
        return out;
    };
    let Ok(mut zip) = zip::ZipArchive::new(f) else {
        return out;
    };
    let mut budget = ZipBudget::default();
    if budget.check_entry_count(zip.len()).is_err() {
        return out;
    }
    let Ok(xml) = zip.part_to_string("content.xml", &mut budget) else {
        return out;
    };

    let col_styles = odf_styles_by_family(&xml, "table-column", "style:column-width");
    let row_styles = odf_styles_by_family(&xml, "table-row", "style:row-height");
    let cell_styles = super::ods_styles::parse_ods_cell_styles(&xml);

    for table in split_elements(&xml, "table:table") {
        let head = table.split('>').next().unwrap_or("");
        let Some(name) = xml_attr(head, "table:name") else { continue };
        let body = table.split("</table:table>").next().unwrap_or("");
        let mut props = SheetXlsxProps::default();
        let mut col_cell_style: std::collections::HashMap<usize, &str> = std::collections::HashMap::new();

        // Columns: each element covers `number-columns-repeated` of them.
        let mut col = 0usize;
        for tag in split_elements(body, "table:table-column") {
            let tag = tag.split('>').next().unwrap_or("");
            let repeat = odf_repeat(tag, "table:number-columns-repeated");
            if let Some(px) = xml_attr(tag, "table:style-name").and_then(|s| col_styles.get(s)) {
                for i in 0..repeat {
                    props.col_widths.insert(col + i, *px);
                }
            }
            if let Some(s) = xml_attr(tag, "table:default-cell-style-name") {
                for i in 0..repeat {
                    col_cell_style.insert(col + i, s);
                }
            }
            col += repeat;
        }

        // Rows, and the merges the cells inside them declare. A merge is
        // written on its anchor cell as a span, and the cells it covers
        // follow as `table:covered-table-cell` — which still advance the
        // column, so they are counted rather than skipped.
        let mut row = 0usize;
        for row_block in split_elements(body, "table:table-row") {
            let head = row_block.split('>').next().unwrap_or("");
            let repeat = odf_repeat(head, "table:number-rows-repeated");
            if let Some(px) = xml_attr(head, "table:style-name").and_then(|s| row_styles.get(s)) {
                for i in 0..repeat {
                    props.row_heights.insert(row + i, *px);
                }
            }
            let cells = row_block.split("</table:table-row>").next().unwrap_or("");
            let mut c = 0usize;
            for cell in cells.split("<table:").skip(1) {
                let is_covered = cell.starts_with("covered-table-cell");
                if !cell.starts_with("table-cell") && !is_covered {
                    continue;
                }
                let tag = cell.split('>').next().unwrap_or("");
                let cspan = odf_repeat(tag, "table:number-columns-spanned");
                let rspan = odf_repeat(tag, "table:number-rows-spanned");
                if !is_covered && (cspan > 1 || rspan > 1) {
                    props.merges.push((row, c, rspan, cspan));
                }
                let crepeat = odf_repeat(tag, "table:number-columns-repeated");
                // The cell's own style, else its column's default. Only
                // cells that hold a value or stand alone are styled: a
                // repeated run of empty cells is how ODF says "the rest of
                // the row/sheet", and styling each would allocate for
                // cells nobody has.
                let has_value = tag.contains("office:value-type=");
                if has_value || (crepeat == 1 && repeat == 1) {
                    for i in 0..crepeat {
                        let style_name = xml_attr(tag, "table:style-name").or_else(|| col_cell_style.get(&(c + i)).copied());
                        let Some((style, border)) = style_name.and_then(|n| cell_styles.get(n)) else { continue };
                        if style.is_default() && border.is_none() {
                            continue;
                        }
                        for r in row..row + repeat {
                            props.cell_styles.push((
                                r,
                                c + i,
                                super::XfStyle { style: style.clone(), border: border.clone(), ..super::XfStyle::default() },
                            ));
                        }
                    }
                }
                c += crepeat;
            }
            row += repeat;
        }

        out.insert(name.to_string(), props);
    }
    out
}

#[cfg(test)]
mod odf_tests {
    use super::*;

    #[test]
    fn lengths_convert_from_every_unit_odf_uses() {
        // 96dpi: an inch is 96px, and the others are that inch restated.
        assert_eq!(odf_length_to_pixels("1in"), Some(96.0));
        assert_eq!(odf_length_to_pixels("2.54cm"), Some(96.0));
        assert_eq!(odf_length_to_pixels("25.4mm"), Some(96.0));
        assert_eq!(odf_length_to_pixels("72pt"), Some(96.0));
        assert_eq!(odf_length_to_pixels("96px"), Some(96.0));
        // Calc writes this for a 40px row; the reader must land back on it.
        assert_eq!(odf_length_to_pixels("0.4165in"), Some(39.98));
        assert_eq!(odf_length_to_pixels("garbage"), None);
        assert_eq!(odf_length_to_pixels("12"), None, "a bare number has no unit");
    }

    /// Lookups are family-scoped, which is what the loader relies on.
    ///
    /// This does not prove the family *check* is load-bearing — it is not,
    /// and a mutation removing it stays green, because only a column style
    /// carries `style:column-width` in the first place. Asserted as the
    /// behaviour the caller depends on, not as coverage of that guard.
    #[test]
    fn column_and_row_lengths_do_not_mix() {
        let xml = "<office:automatic-styles>\
            <style:style style:name=\"co1\" style:family=\"table-column\">\
            <style:table-column-properties style:column-width=\"2in\"/></style:style>\
            <style:style style:name=\"ro1\" style:family=\"table-row\">\
            <style:table-row-properties style:row-height=\"1in\"/></style:style>\
            </office:automatic-styles>";
        let cols = odf_styles_by_family(xml, "table-column", "style:column-width");
        let rows = odf_styles_by_family(xml, "table-row", "style:row-height");
        assert_eq!(cols.get("co1"), Some(&192.0));
        assert_eq!(rows.get("ro1"), Some(&96.0));
        assert_eq!(cols.get("ro1"), None);
        assert_eq!(rows.get("co1"), None);
    }

    /// A style with no length of its own must not take the next one's.
    ///
    /// The opening tags here are newline-separated, which is how a
    /// pretty-printing producer writes them — and the case the body trim
    /// exists for. With the tags space-separated the outer split bounds
    /// each block on its own and the trim cannot be seen to matter, which
    /// is how a first version of this test passed against its removal.
    #[test]
    fn a_style_does_not_borrow_the_next_styles_length() {
        let xml = "<office:automatic-styles>\n\
            <style:style\n style:name=\"co1\" style:family=\"table-column\">\n\
            <style:table-column-properties fo:break-before=\"auto\"/></style:style>\n\
            <style:style\n style:name=\"co2\" style:family=\"table-column\">\n\
            <style:table-column-properties style:column-width=\"3in\"/></style:style>\n\
            </office:automatic-styles>";
        let cols = odf_styles_by_family(xml, "table-column", "style:column-width");
        assert_eq!(
            cols.get("co1"),
            None,
            "co1 has no width of its own and must not inherit co2's"
        );
    }

    /// A pretty-printed file yields its layout at all.
    ///
    /// Not a refinement: `split("<style:style ")` matched no delimiter in
    /// a file whose tags are newline-formatted, so every width, height and
    /// merge read as absent and the sheet opened with defaults. Silent,
    /// because Calc writes compact XML and every fixture came from Calc.
    #[test]
    fn a_pretty_printed_ods_is_read_rather_than_silently_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pretty.ods");
        write_ods_fixture(
            &path,
            "<office:automatic-styles>\n\
             <style:style\n  style:name=\"co1\"\n  style:family=\"table-column\">\n\
             <style:table-column-properties\n  style:column-width=\"1.5in\"/>\n\
             </style:style>\n</office:automatic-styles>\n\
             <office:body>\n<office:spreadsheet>\n\
             <table:table\n  table:name=\"S\">\n\
             <table:table-column\n  table:style-name=\"co1\"/>\n\
             <table:table-row>\n<table:table-cell><text:p>x</text:p></table:table-cell>\n\
             </table:table-row>\n</table:table>\n\
             </office:spreadsheet>\n</office:body>",
        );
        let props = read_sheet_props_from_ods(path.to_str().unwrap());
        let s = props.get("S").expect("the sheet was not found at all");
        assert_eq!(s.col_widths.get(&0), Some(&144.0), "1.5in is 144px");
    }

    /// ODF says "these forty columns are all like this" with a repeat
    /// count, so a reader that treats each element as one column puts
    /// every later width on the wrong index.
    #[test]
    fn repeated_columns_and_rows_land_on_every_index_they_cover() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.ods");
        write_ods_fixture(
            &path,
            "<office:automatic-styles>\
             <style:style style:name=\"co1\" style:family=\"table-column\">\
             <style:table-column-properties style:column-width=\"1in\"/></style:style>\
             <style:style style:name=\"co2\" style:family=\"table-column\">\
             <style:table-column-properties style:column-width=\"2in\"/></style:style>\
             <style:style style:name=\"ro1\" style:family=\"table-row\">\
             <style:table-row-properties style:row-height=\"0.5in\"/></style:style>\
             </office:automatic-styles>\
             <office:body><office:spreadsheet>\
             <table:table table:name=\"S\">\
             <table:table-column table:style-name=\"co1\" table:number-columns-repeated=\"3\"/>\
             <table:table-column table:style-name=\"co2\"/>\
             <table:table-row table:style-name=\"ro1\" table:number-rows-repeated=\"2\">\
             <table:table-cell office:value-type=\"string\"><text:p>x</text:p></table:table-cell>\
             </table:table-row>\
             </table:table></office:spreadsheet></office:body>",
        );
        let props = read_sheet_props_from_ods(path.to_str().unwrap());
        let s = props.get("S").expect("sheet S");
        for c in 0..3 {
            assert_eq!(s.col_widths.get(&c), Some(&96.0), "column {c}");
        }
        assert_eq!(s.col_widths.get(&3), Some(&192.0), "the fourth column");
        assert_eq!(s.row_heights.get(&0), Some(&48.0));
        assert_eq!(s.row_heights.get(&1), Some(&48.0), "the repeated row");
    }

    /// A cell takes its own style, else its column's default cell style;
    /// repeated runs of empty cells are left alone.
    #[test]
    fn cell_styles_land_on_their_cells() {
        use crate::sheet::BorderStyle;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("styles.ods");
        write_ods_fixture(
            &path,
            "<office:automatic-styles>\
             <style:style style:name=\"ce1\" style:family=\"table-cell\">\
             <style:text-properties fo:font-weight=\"bold\"/></style:style>\
             <style:style style:name=\"ce2\" style:family=\"table-cell\">\
             <style:table-cell-properties fo:background-color=\"#c6efce\" fo:border=\"0.74pt solid #000000\"/></style:style>\
             </office:automatic-styles>\
             <office:body><office:spreadsheet><table:table table:name=\"S\">\
             <table:table-column table:default-cell-style-name=\"ce2\" table:number-columns-repeated=\"2\"/>\
             <table:table-column/>\
             <table:table-row>\
             <table:table-cell table:style-name=\"ce1\" office:value-type=\"string\"><text:p>b</text:p></table:table-cell>\
             <table:table-cell office:value-type=\"float\" office:value=\"2\"><text:p>2</text:p></table:table-cell>\
             <table:table-cell table:style-name=\"ce2\" table:number-columns-repeated=\"1000\"/>\
             </table:table-row>\
             </table:table></office:spreadsheet></office:body>",
        );
        let props = read_sheet_props_from_ods(path.to_str().unwrap());
        let styles = &props["S"].cell_styles;
        let at = |r: usize, c: usize| styles.iter().find(|(sr, sc, _)| (*sr, *sc) == (r, c)).map(|(_, _, x)| x);
        assert!(at(0, 0).unwrap().style.bold, "the cell's own style");
        assert!(at(0, 0).unwrap().border.is_none(), "its own style wins over the column's");
        let b = at(0, 1).expect("the column default");
        assert_eq!(b.style.fill, Some(crate::style::Rgb(0xC6, 0xEF, 0xCE)));
        assert_eq!(b.border.top, BorderStyle::Solid);
        assert_eq!(styles.len(), 2, "the repeated empty run is not expanded: {styles:?}");
    }

    /// A merge is written as a span on its anchor and `covered-table-cell`
    /// for the rest. The covered ones still advance the column, so a
    /// reader that skips them puts later merges at the wrong column.
    #[test]
    fn a_merge_is_anchored_where_the_span_is_declared() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.ods");
        write_ods_fixture(
            &path,
            "<office:body><office:spreadsheet><table:table table:name=\"S\">\
             <table:table-row>\
             <table:table-cell/>\
             <table:table-cell table:number-columns-spanned=\"2\" \
             table:number-rows-spanned=\"3\"><text:p>a</text:p></table:table-cell>\
             <table:covered-table-cell/>\
             <table:table-cell table:number-columns-spanned=\"2\"><text:p>b</text:p></table:table-cell>\
             </table:table-row>\
             </table:table></office:spreadsheet></office:body>",
        );
        let props = read_sheet_props_from_ods(path.to_str().unwrap());
        let s = props.get("S").expect("sheet S");
        assert_eq!(
            s.merges,
            vec![(0, 1, 3, 2), (0, 3, 1, 2)],
            "the second merge is at column 3 only if the covered cell was counted"
        );
    }

    /// ODF ends a sheet with a repeat count in the millions; taking it
    /// literally would allocate for a grid nobody has.
    #[test]
    fn an_enormous_repeat_count_is_capped() {
        assert_eq!(
            odf_repeat("table:number-columns-repeated=\"16384000\"", "table:number-columns-repeated"),
            ODF_MAX_REPEAT
        );
        assert_eq!(odf_repeat("<x/>", "table:number-columns-repeated"), 1);
    }

    fn write_ods_fixture(path: &std::path::Path, body: &str) {
        use std::io::Write;
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <office:document-content xmlns:office=\"o\" xmlns:table=\"t\" \
             xmlns:style=\"s\" xmlns:text=\"x\" xmlns:fo=\"f\">{body}</office:document-content>"
        );
        let f = std::fs::File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        w.start_file("content.xml", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(xml.as_bytes()).unwrap();
        w.finish().unwrap();
    }
}
