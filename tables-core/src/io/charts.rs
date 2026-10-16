// charts.rs — read embedded charts back from xlsx.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of io.rs (issue #247).
//
// TBL-002: relationship-correct, field-complete chart persistence. Charts
// resolve through the workbook→worksheet→drawing→chart relationship chain
// and each anchor's own chart relationship ID — never ZIP order or part
// filenames. Every admitted series lands in one canonical `ChartSeries`
// vector (the legacy `cat`/`val` mirror the first series); titles, axis
// titles, legend positions, literal names, admitted colors and real anchor
// geometry round-trip. Anything else is reported, not silently dropped.

/// `xml` with every element name's namespace prefix removed (`<c:f>` →
/// `<f>`, `</xdr:from>` → `</from>`); attributes are left alone. Prefixes
/// are the producer's choice: we and Calc write `c:`/`xdr:`, openpyxl writes
/// a default namespace and no prefixes at all, and a reader matching literal
/// prefixed tags found no chart in its files (render lab `tables/chart`).
fn unprefix(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..=i]);
        rest = &rest[i + 1..];
        if let Some(r) = rest.strip_prefix('/') {
            out.push('/');
            rest = r;
        }
        let name_end = rest
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(rest.len());
        if let Some(colon) = rest[..name_end].find(':') {
            rest = &rest[colon + 1..];
        }
    }
    out.push_str(rest);
    out
}

/// `name="value"` inside one already-isolated tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    tag.split(&format!("{name}=\""))
        .nth(1)?
        .split('"')
        .next()
        .map(str::to_string)
}

/// XML text to plain text: the five named entities plus numeric references
/// (a title can hold `&#233;` where a note holds `&#10;`).
fn unescape(s: &str) -> String {
    let mut out = s
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    while let Some(i) = out.find("&#") {
        let rest = &out[i + 2..];
        let (digits, hex) = match rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
            Some(h) => (h, true),
            None => (rest, false),
        };
        let end = digits.find(';').unwrap_or(usize::MAX);
        let num = &digits[..end.min(digits.len())];
        let parsed = if hex { u32::from_str_radix(num, 16).ok() } else { num.parse::<u32>().ok() };
        let Some(code) = parsed.and_then(char::from_u32) else {
            break;
        };
        if end == usize::MAX {
            break;
        }
        out.replace_range(i..i + 2 + (if hex { 1 } else { 0 }) + num.len() + 1, &code.to_string());
    }
    out
}

/// The concatenated text of every `<t>…</t>` run inside `xml`: what a chart
/// or axis title says, without its formatting. Runs may carry attributes
/// (`<t xml:space="preserve">`, which unprefix keeps), so the tag name is
/// matched exactly rather than splitting on the literal `<t>`.
fn runs_text(xml: &str) -> String {
    let mut out = String::new();
    for seg in xml.split('<').skip(1) {
        let tag_end = seg.find('>').unwrap_or(seg.len());
        let (tag, rest) = (&seg[..tag_end], &seg[tag_end.min(seg.len())..]);
        if tag.split_whitespace().next().unwrap_or("") != "t" {
            continue;
        }
        let text = rest.strip_prefix('>').unwrap_or(rest);
        out.push_str(&unescape(text.split("</t>").next().unwrap_or(text)));
    }
    out
}

/// A sheet-qualified single range as a chart formula states it:
/// `Sheet1!$A$2:$A$5` or `'O''Brien''s!$A$1` (doubled quotes unescape to one).
struct QualifiedRange {
    sheet: String,
    first_row: usize,
    col: usize,
    last_row: usize,
}

/// Split `Sheet!$A$1:$B$3` at the qualifier `!` that is not inside a quoted
/// sheet name: the last `!` outside quotes.
fn split_qualifier(r: &str) -> Option<(&str, &str)> {
    let mut in_quotes = false;
    let mut cut = None;
    for (i, c) in r.char_indices() {
        if c == '\'' {
            in_quotes = !in_quotes;
        } else if c == '!' && !in_quotes {
            cut = Some(i);
        }
    }
    let cut = cut?;
    Some((&r[..cut], &r[cut + 1..]))
}

/// Unquote a sheet qualifier: `'O''Brien'` → `O'Brien`, `Sheet1` → `Sheet1`.
fn unquote_sheet(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        s[1..s.len() - 1].replace("''", "'")
    } else {
        s.to_string()
    }
}

fn parse_qualified_range(r: &str) -> Option<QualifiedRange> {
    use crate::sheet::parse_cell_ref;
    let (sheet, range) = split_qualifier(r.trim())?;
    let range = range.replace('$', "");
    let (a, b) = range.split_once(':').unwrap_or((range.as_str(), range.as_str()));
    let (r0, c0) = parse_cell_ref(a)?;
    let (r1, c1) = parse_cell_ref(b)?;
    if r0 >= 1_048_576 || r1 >= 1_048_576 || c0 >= 16_384 || c1 >= 16_384 {
        return None;
    }
    Some(QualifiedRange {
        sheet: unquote_sheet(sheet),
        first_row: r0.min(r1),
        col: c0,
        last_row: r0.max(r1),
    })
}

/// What the reader could not model, with the location a save question can
/// name: the chart stays (or the file keeps it for Excel/Calc) but Tables
/// does not carry this detail. The package scan (io/loss.rs) reports the
/// same content when a save would write over the source file.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartReadIssue {
    pub location: String,
    pub detail: String,
}

/// The chart kinds Tables models: the five Excel and Calc share.
fn admitted_kind(chart_xml: &str) -> Option<crate::sheet::ChartKind> {
    use crate::sheet::ChartKind;
    // The chart element present determines the kind; a part holds one.
    // `split("<barChart")` also matches `<barChart ` with attributes, and
    // unprefix has removed any namespace, so no producer's prefix matters.
    if chart_xml.contains("<barChart") {
        Some(ChartKind::Bar)
    } else if chart_xml.contains("<lineChart") {
        Some(ChartKind::Line)
    } else if chart_xml.contains("<pieChart") {
        Some(ChartKind::Pie)
    } else if chart_xml.contains("<scatterChart") {
        Some(ChartKind::Scatter)
    } else if chart_xml.contains("<areaChart") {
        Some(ChartKind::Area)
    } else {
        None
    }
}

/// Every `*Chart` element name in the part, so an unknown kind (stock,
/// surface, combo, 3-D…) is reported instead of silently skipped.
fn chart_kinds_in(chart_xml: &str) -> Vec<String> {
    let mut kinds = Vec::new();
    for tag in chart_xml.split('<').skip(1) {
        let name = tag
            .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .next()
            .unwrap_or("");
        if name.ends_with("Chart") && !name.starts_with('/') && !kinds.contains(&name.to_string()) {
            kinds.push(name.to_string());
        }
    }
    kinds
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// One anchor's chart reference: the relationship ID the drawing's own rels
/// part resolves to a chart part — never ZIP order or a filename guess.
fn anchor_chart_rid(anchor_xml: &str) -> Option<String> {
    let frame = anchor_xml.split("<graphicFrame").nth(1)?;
    let frame = frame.split("</graphicFrame>").next().unwrap_or(frame);
    let chart_tag = frame.split("<chart").nth(1)?;
    let chart_tag = chart_tag.split('>').next().unwrap_or("");
    attr(chart_tag, "r:id")
}

fn grab_num(block: &str, tag: &str) -> Option<f64> {
    block
        .split(&format!("<{tag}>"))
        .nth(1)?
        .split(&format!("</{tag}>"))
        .next()?
        .trim()
        .parse()
        .ok()
}

/// An anchor as the drawing states it, with the size in pixels resolved
/// against the owning sheet's real column/row geometry (EMU offsets folded
/// in, never ignored). `Absolute` anchors carry no cell and no size, so the
/// chart is reported rather than placed at a guessed cell.
enum Anchor {
    Placed { cell: (usize, usize), size_px: (f64, f64) },
    Absolute,
}

fn anchor_cell(cell_xml: &str) -> Option<(usize, usize, f64, f64)> {
    let col = grab_num(cell_xml, "col")?;
    let row = grab_num(cell_xml, "row")?;
    // A negative or fractional cell is malformed, not cell A1: the caller
    // reports it instead of saturating it to zero.
    if col < 0.0 || row < 0.0 || col.fract() != 0.0 || row.fract() != 0.0 {
        return None;
    }
    let (col, row) = (col as usize, row as usize);
    let col_off = grab_num(cell_xml, "colOff").unwrap_or(0.0);
    let row_off = grab_num(cell_xml, "rowOff").unwrap_or(0.0);
    if !col_off.is_finite() || !row_off.is_finite() {
        return None;
    }
    Some((row, col, col_off, row_off))
}

fn anchor_ext(anchor_xml: &str) -> Option<(f64, f64)> {
    let ext = anchor_xml.split("<ext ").nth(1)?;
    let ext = ext.split('>').next().unwrap_or("");
    let (cx, cy) = (attr(ext, "cx")?.parse::<f64>().ok()?, attr(ext, "cy")?.parse::<f64>().ok()?);
    if !cx.is_finite() || !cy.is_finite() {
        return None;
    }
    Some((cx / 9525.0, cy / 9525.0))
}

/// 9525 EMU = 1 px. Every size is validated finite and bounded; an
/// out-of-range size is reported and comes back at the default rather than
/// silently substituting it (TBL-002).
const MIN_CHART_PX: f64 = 16.0;
const MAX_CHART_PX: f64 = 20_000.0;
pub(crate) const DEFAULT_CHART_SIZE_PX: (f64, f64) = (480.0, 280.0);

fn valid_size(size_px: (f64, f64)) -> Option<(f64, f64)> {
    let (w, h) = size_px;
    if w.is_finite()
        && h.is_finite()
        && w >= MIN_CHART_PX
        && h >= MIN_CHART_PX
        && w <= MAX_CHART_PX
        && h <= MAX_CHART_PX
    {
        Some((w, h))
    } else {
        None
    }
}

/// Resolve one drawing anchor to a cell and a pixel size. `widths`/`heights`
/// are the owning sheet's real geometry; missing entries read as this app's
/// own defaults, which is also what the writer lays two-cell anchors out
/// on.
fn resolve_anchor(
    anchor_xml: &str,
    kind: &str,
    widths: &[f64],
    heights: &[f64],
    location: &str,
    issues: &mut Vec<ChartReadIssue>,
) -> Option<Anchor> {
    use crate::sheet::{COL_WIDTH, ROW_HEIGHT};
    if kind == "absoluteAnchor" {
        issues.push(ChartReadIssue {
            location: location.to_string(),
            detail: "the chart uses an absolute anchor with no cell; Tables keeps the data cells but does not place the graphic".into(),
        });
        return Some(Anchor::Absolute);
    }
    let Some(from_xml) = anchor_xml.split("<from>").nth(1).map(|f| f.split("</from>").next().unwrap_or(f)) else {
        issues.push(ChartReadIssue {
            location: location.to_string(),
            detail: "the chart's anchor states no starting cell; Tables keeps the data cells but does not place the graphic".into(),
        });
        return None;
    };
    let Some((row, col, _, _)) = anchor_cell(from_xml) else {
        issues.push(ChartReadIssue {
            location: location.to_string(),
            detail: "the chart's anchor cell is not a number; Tables keeps the data cells but does not place the graphic".into(),
        });
        return None;
    };
    if kind == "oneCellAnchor" {
        let size_px = match anchor_ext(anchor_xml) {
            Some(ext) => match valid_size(ext) {
                Some(size) => size,
                None => {
                    issues.push(ChartReadIssue {
                        location: location.to_string(),
                        detail: format!(
                            "the chart states a size of {:.0}×{:.0} px, outside the {:.0}–{:.0} px Tables draws; it comes back at the default {}×{} px",
                            ext.0,
                            ext.1,
                            MIN_CHART_PX,
                            MAX_CHART_PX,
                            DEFAULT_CHART_SIZE_PX.0,
                            DEFAULT_CHART_SIZE_PX.1,
                        ),
                    });
                    DEFAULT_CHART_SIZE_PX
                }
            },
            None => {
                if anchor_xml.contains("<ext") {
                    issues.push(ChartReadIssue {
                        location: location.to_string(),
                        detail: "the chart's anchor states no readable size; it comes back at the default 480×280 px".into(),
                    });
                }
                DEFAULT_CHART_SIZE_PX
            }
        };
        return Some(Anchor::Placed { cell: (row, col), size_px });
    }
    // Two-cell: the writer lays width/height out over the sheet's own
    // columns/rows, so inverting with the same geometry recovers them.
    let Some(to_xml) = anchor_xml.split("<to>").nth(1).map(|t| t.split("</to>").next().unwrap_or(t)) else {
        issues.push(ChartReadIssue {
            location: location.to_string(),
            detail: "the chart's two-cell anchor states no end cell; it comes back at the default size".into(),
        });
        return Some(Anchor::Placed { cell: (row, col), size_px: DEFAULT_CHART_SIZE_PX });
    };
    let Some((to_row, to_col, to_col_off, to_row_off)) = anchor_cell(to_xml) else {
        issues.push(ChartReadIssue {
            location: location.to_string(),
            detail: "the chart's two-cell anchor has no readable end cell; it comes back at the default size".into(),
        });
        return Some(Anchor::Placed { cell: (row, col), size_px: DEFAULT_CHART_SIZE_PX });
    };
    let (_, _, from_col_off, from_row_off) = anchor_cell(from_xml).unwrap_or((row, col, 0.0, 0.0));
    if to_col < col || to_row < row {
        issues.push(ChartReadIssue {
            location: location.to_string(),
            detail: "the chart's two-cell anchor ends before it starts; it comes back at the default size".into(),
        });
        return Some(Anchor::Placed { cell: (row, col), size_px: DEFAULT_CHART_SIZE_PX });
    }
    let width_of = |c: usize| {
        widths
            .get(c)
            .copied()
            .filter(|w| w.is_finite() && *w >= 0.0)
            .unwrap_or(COL_WIDTH)
    };
    let height_of = |r: usize| {
        heights
            .get(r)
            .copied()
            .filter(|h| h.is_finite() && *h >= 0.0)
            .unwrap_or(ROW_HEIGHT)
    };
    let w: f64 = (col..to_col).map(width_of).sum::<f64>() + (to_col_off - from_col_off) / 9525.0;
    let h: f64 = (row..to_row).map(height_of).sum::<f64>() + (to_row_off - from_row_off) / 9525.0;
    let size_px = match valid_size((w, h)) {
        Some(size) => size,
        None => {
            issues.push(ChartReadIssue {
                location: location.to_string(),
                detail: format!(
                    "the chart's two-cell anchor spans {:.0}×{:.0} px, outside the {:.0}–{:.0} px Tables draws; it comes back at the default {}×{} px",
                    w,
                    h,
                    MIN_CHART_PX,
                    MAX_CHART_PX,
                    DEFAULT_CHART_SIZE_PX.0,
                    DEFAULT_CHART_SIZE_PX.1,
                ),
            });
            DEFAULT_CHART_SIZE_PX
        }
    };
    Some(Anchor::Placed { cell: (row, col), size_px })
}

/// The formula (`<f>`) a category/value element carries, if it carries one
/// rather than cached literals only.
fn series_formula(side_xml: &str) -> Option<String> {
    side_xml
        .split("<f>")
        .nth(1)?
        .split("</f>")
        .next()
        .map(|f| unescape(f.trim()))
}

/// Admit one side's formula as a same-sheet single-column range, reporting
/// precisely why anything else is not carried.
fn admit_side(
    formula: &str,
    owner: &str,
    location: &str,
    issues: &mut Vec<ChartReadIssue>,
) -> Option<(usize, usize, usize)> {
    let parsed = parse_qualified_range(formula);
    let Some(parsed) = parsed else {
        issues.push(ChartReadIssue {
            location: location.to_string(),
            detail: format!("the range '{formula}' is not a readable cell range; its series is not carried"),
        });
        return None;
    };
    if parsed.sheet != owner {
        issues.push(ChartReadIssue {
            location: location.to_string(),
            detail: format!(
                "the range '{formula}' points at sheet '{}', not the chart's own sheet '{owner}'; Tables keeps single-sheet ranges only, so its series is not carried",
                parsed.sheet,
            ),
        });
        return None;
    }
    // Single-column only is what the model holds (ChartSeries); a range
    // spanning columns is row-oriented data, not a series.
    let end_col = split_qualifier(&formula.replace('$', ""))
        .and_then(|(_, r)| r.split_once(':'))
        .and_then(|(_, b)| crate::sheet::parse_cell_ref(b.trim()))
        .map(|(_, c)| c)
        .unwrap_or(parsed.col);
    if end_col != parsed.col {
        issues.push(ChartReadIssue {
            location: location.to_string(),
            detail: format!("the range '{formula}' spans several columns; Tables keeps single-column series only, so its series is not carried"),
        });
        return None;
    }
    Some((parsed.first_row, parsed.col, parsed.last_row))
}

/// Parse every admitted series in one chart part into the canonical vector.
/// The legacy `cat`/`val` mirror the first series.
fn parse_series(
    chart_xml: &str,
    kind: crate::sheet::ChartKind,
    owner: &str,
    location: &str,
    issues: &mut Vec<ChartReadIssue>,
) -> Vec<crate::sheet::ChartSeries> {
    use crate::sheet::{ChartKind, ChartSeries};
    let mut out = Vec::new();
    for ser in chart_xml.split("<ser>").skip(1) {
        let ser = ser.split("</ser>").next().unwrap_or(ser);
        // Literal name (`<tx><v>Sales</v></tx>`, what the writer emits) is
        // admitted; a cell reference names nothing Tables can resolve
        // without the grid, so it is reported and the series stays.
        let tx = ser.split("<tx>").nth(1).and_then(|t| t.split("</tx>").next()).unwrap_or("");
        let name = tx
            .split("<v>")
            .nth(1)
            .and_then(|v| v.split("</v>").next())
            .map(|v| unescape(v.trim()))
            .unwrap_or_default();
        if name.is_empty() && (tx.contains("<strRef>") || (tx.contains("<f>") && !tx.contains("<v>"))) {
            let formula = tx
                .split("<f>")
                .nth(1)
                .and_then(|f| f.split("</f>").next())
                .unwrap_or("")
                .trim();
            issues.push(ChartReadIssue {
                location: location.to_string(),
                detail: format!(
                    "a series takes its name from the cell '{formula}'; Tables keeps literal names only, so it comes back unnamed"
                ),
            });
        }
        // Admitted color: a plain sRGB fill on the series. Theme and indexed
        // colors have no hex to keep, so they are reported, not guessed.
        let sppr = ser.split("<spPr>").nth(1).and_then(|s| s.split("</spPr>").next()).unwrap_or("");
        let color = sppr
            .split("<srgbClr").nth(1)
            .and_then(|t| t.split('>').next())
            .and_then(|t| attr(t, "val"))
            .filter(|v| is_hex_color(v))
            .map(|v| v.to_ascii_uppercase());
        if color.is_none() && !sppr.is_empty() {
            issues.push(ChartReadIssue {
                location: location.to_string(),
                detail: "a series fill is a theme or indexed color, not plain sRGB; Tables keeps sRGB fills only, so it comes back uncolored".into(),
            });
        }
        let scatter = kind == ChartKind::Scatter;
        let cat_tag = if scatter { "<xVal>" } else { "<cat>" };
        let val_tag = if scatter { "<yVal>" } else { "<val>" };
        let cat_side = ser.split(cat_tag).nth(1).and_then(|s| s.split(cat_tag.replace('<', "</").as_str()).next());
        let val_side = ser.split(val_tag).nth(1).and_then(|s| s.split(val_tag.replace('<', "</").as_str()).next());
        let val_formula = val_side.and_then(series_formula);
        let Some(val_formula) = val_formula else {
            issues.push(ChartReadIssue {
                location: location.to_string(),
                detail: "a series holds cached values with no range; Tables keeps ranged series only, so it is not carried".into(),
            });
            continue;
        };
        let Some(val) = admit_side(&val_formula, owner, location, issues) else {
            continue;
        };
        let cat = cat_side
            .and_then(series_formula)
            .and_then(|f| admit_side(&f, owner, location, issues))
            .unwrap_or(val);
        if cat.2 - cat.0 != val.2 - val.0 {
            issues.push(ChartReadIssue {
                location: location.to_string(),
                detail: format!(
                    "a series plots {} categories against {} values; Tables keeps both ranges as stated",
                    cat.2 - cat.0 + 1,
                    val.2 - val.0 + 1,
                ),
            });
        }
        out.push(ChartSeries { name, cat, val, color });
    }
    out
}

/// The chart-level title: the `<title>` before `<plotArea>`, all runs joined.
fn chart_title(chart_xml: &str) -> String {
    // Split on the open tag without its bracket so attributes on
    // `<plotArea>` cannot leak an axis title into the chart title.
    let head = chart_xml.split("<plotArea").next().unwrap_or("");
    head.split("<title>")
        .nth(1)
        .and_then(|t| t.split("</title>").next())
        .map(runs_text)
        .unwrap_or_default()
}

/// Axis titles: the `<title>` inside each axis element. A bar/line/area
/// chart names x through `<catAx>` and y through `<valAx>`; a scatter names
/// both through `<valAx>` in order.
fn axis_titles(chart_xml: &str, kind: crate::sheet::ChartKind) -> (Option<String>, Option<String>) {
    use crate::sheet::ChartKind;
    let title_in = |axis: &str| {
        axis
            .split("<title>")
            .nth(1)
            .and_then(|t| t.split("</title>").next())
            .map(runs_text)
            .filter(|t| !t.trim().is_empty())
    };
    let cat_title = chart_xml
        .split("<catAx>")
        .nth(1)
        .and_then(|a| a.split("</catAx>").next())
        .and_then(title_in);
    let val_titles: Vec<String> = chart_xml
        .split("<valAx>")
        .skip(1)
        .filter_map(|a| a.split("</valAx>").next())
        .filter_map(title_in)
        .collect();
    if kind == ChartKind::Scatter {
        (
            val_titles.first().cloned(),
            val_titles.get(1).cloned(),
        )
    } else {
        (cat_title, val_titles.into_iter().next())
    }
}

/// Legend position: no `<legend>` means hidden (what the writer emits for
/// one); an unrecognized value keeps Right and is reported.
fn legend_position(chart_xml: &str, location: &str, issues: &mut Vec<ChartReadIssue>) -> crate::sheet::LegendPosition {
    use crate::sheet::LegendPosition;
    let Some(legend) = chart_xml.split("<legend>").nth(1).and_then(|l| l.split("</legend>").next()) else {
        return LegendPosition::None;
    };
    let val = legend
        .split("<legendPos")
        .nth(1)
        .and_then(|t| t.split('>').next())
        .and_then(|t| attr(t, "val"));
    // Some producers write `legendPosition`; accept both.
    let val = val.or_else(|| {
        legend
            .split("<legendPosition")
            .nth(1)
            .and_then(|t| t.split('>').next())
            .and_then(|t| attr(t, "val"))
    });
    match val.as_deref() {
        Some("t") => LegendPosition::Top,
        Some("b") => LegendPosition::Bottom,
        Some("l") => LegendPosition::Left,
        Some("r") | None => LegendPosition::Right,
        Some(other) => {
            issues.push(ChartReadIssue {
                location: location.to_string(),
                detail: format!("the legend position '{other}' is not one Tables draws; it comes back on the Right"),
            });
            LegendPosition::Right
        }
    }
}

/// Resolve one `.rels` part's `Id → Target`, with each target joined onto
/// the part's own directory: rel targets are relative (`../charts/…`).
fn rel_targets(rels_xml: &str, part_dir: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for (id, target) in super::props::parse_rels(rels_xml) {
        out.insert(id, join_part(part_dir, &target));
    }
    out
}

/// Join a relative rel target onto its part's directory (`xl/drawings` +
/// `../charts/chart1.xml` → `xl/charts/chart1.xml`).
fn join_part(dir: &str, target: &str) -> String {
    if target.starts_with('/') {
        return target.trim_start_matches('/').to_string();
    }
    let mut parts: Vec<&str> = if dir.is_empty() { Vec::new() } else { dir.split('/').collect() };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn part_dir(name: &str) -> &str {
    name.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// Read one bounded package part, or `None` where it is absent or
/// unreadable: a missing rels part means "resolve nothing", not failure.
fn read_part(
    zip: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
    budget: &mut suite_common_core::zip_guard::ZipBudget,
) -> Option<String> {
    use suite_common_core::zip_guard::BoundedArchive;
    zip.part_to_string(name, budget).ok()
}

/// sheet name → worksheet part, via workbook.xml and the workbook rels —
/// the same resolution the sheet-props reader uses, not a filename guess.
fn resolve_sheet_parts(
    zip: &mut zip::ZipArchive<std::fs::File>,
    budget: &mut suite_common_core::zip_guard::ZipBudget,
) -> Vec<(String, String)> {
    let (Some(workbook_xml), Some(rels_xml)) = (
        read_part(zip, "xl/workbook.xml", budget),
        read_part(zip, "xl/_rels/workbook.xml.rels", budget),
    ) else {
        return Vec::new();
    };
    let rels = rel_targets(&rels_xml, "xl");
    let Some(sheets_block) = workbook_xml.split("<sheets>").nth(1) else {
        return Vec::new();
    };
    let sheets_block = sheets_block.split("</sheets>").next().unwrap_or("");
    let mut out = Vec::new();
    for tag in sheets_block.split("<sheet ").skip(1) {
        let tag = tag.split('>').next().unwrap_or("");
        let (Some(name), Some(rid)) = (attr(tag, "name"), attr(tag, "r:id")) else {
            continue;
        };
        if let Some(target) = rels.get(&rid) {
            out.push((unescape(&name), target.clone()));
        }
    }
    out
}

/// The `<drawing r:id="…">` a worksheet part names, resolved through the
/// sheet's own rels part to a drawing part.
fn drawing_part_for_sheet(
    zip: &mut zip::ZipArchive<std::fs::File>,
    sheet_part: &str,
    budget: &mut suite_common_core::zip_guard::ZipBudget,
) -> Option<String> {
    let sheet_xml = read_part(zip, sheet_part, budget)?;
    let sheet_xml = unprefix(&sheet_xml);
    let drawing_tag = sheet_xml.split("<drawing").nth(1)?;
    let drawing_tag = drawing_tag.split('>').next().unwrap_or("");
    let rid = attr(drawing_tag, "r:id")?;
    let dir = part_dir(sheet_part);
    let base = sheet_part.rsplit('/').next().unwrap_or(sheet_part);
    let rels_name = format!("{dir}/_rels/{base}.rels");
    let rels_xml = read_part(zip, &rels_name, budget)?;
    rel_targets(&rels_xml, dir).get(&rid).cloned()
}

/// Read embedded charts back from an xlsx (ours or a foreign writer's),
/// with the owning sheet of each chart and what could not be modeled.
///
/// `sheets` supplies the owning sheets' real column/row geometry so
/// two-cell anchors resolve to the pixel size the producer laid out —
///
/// [`read_charts_from_xlsx`] covers callers with no model at hand (tests,
/// the oracle), which read on this app's own default geometry.
pub fn read_charts_detailed(
    path: &str,
    sheets: &[crate::sheet::SheetModel],
) -> (Vec<(String, crate::sheet::ChartSpec)>, Vec<ChartReadIssue>) {
    use crate::sheet::ChartSpec;
    let mut out = Vec::new();
    let mut issues = Vec::new();
    let Ok(f) = std::fs::File::open(path) else {
        return (out, issues);
    };
    let Ok(mut zip) = zip::ZipArchive::new(f) else {
        return (out, issues);
    };
    // Bounded reads: best-effort must not extend to decompressing whatever
    // an untrusted package asks for (#442).
    use suite_common_core::zip_guard::ZipBudget;
    let mut budget = ZipBudget::default();
    if budget.check_entry_count(zip.len()).is_err() {
        return (out, issues);
    }

    // The owning sheet's real geometry, mirroring the writer: hidden
    // rows/columns occupy zero pixels when the object moves and sizes
    // with its cells (the writer default), so a chart laid out over them
    // inverts to the written size.
    let geometry = |name: &str| -> (Vec<f64>, Vec<f64>) {
        sheets
            .iter()
            .find(|s| s.name == name)
            .map(|s| {
                (
                    (0..s.cols.max(s.col_widths.len()))
                        .map(|c| if s.is_col_hidden(c) { 0.0 } else { s.col_width(c) })
                        .collect(),
                    (0..s.rows.max(s.row_heights.len()))
                        .map(|r| if s.is_row_hidden(r) { 0.0 } else { s.row_height(r) })
                        .collect(),
                )
            })
            .unwrap_or_default()
    };

    for (sheet_name, sheet_part) in resolve_sheet_parts(&mut zip, &mut budget) {
        let Some(drawing_part) = drawing_part_for_sheet(&mut zip, &sheet_part, &mut budget) else {
            continue;
        };
        let Some(drawing_xml) = read_part(&mut zip, &drawing_part, &mut budget) else {
            continue;
        };
        let drawing_xml = unprefix(&drawing_xml);
        let dir = part_dir(&drawing_part).to_string();
        let base = drawing_part.rsplit('/').next().unwrap_or(&drawing_part).to_string();
        let rels_name = format!("{dir}/_rels/{base}.rels");
        let drawing_rels = read_part(&mut zip, &rels_name, &mut budget)
            .map(|xml| rel_targets(&xml, &dir))
            .unwrap_or_default();
        let (widths, heights) = geometry(&sheet_name);
        for anchor_kind in ["oneCellAnchor", "twoCellAnchor", "absoluteAnchor"] {
            for anchor_xml in drawing_xml.split(&format!("<{anchor_kind}")).skip(1) {
                let anchor_xml = anchor_xml.split(&format!("</{anchor_kind}>")).next().unwrap_or(anchor_xml);
                let Some(rid) = anchor_chart_rid(anchor_xml) else {
                    continue;
                };
                let Some(chart_part) = drawing_rels.get(&rid) else {
                    issues.push(ChartReadIssue {
                        location: format!("Sheet '{sheet_name}', drawing '{drawing_part}'"),
                        detail: format!("an anchor names chart relationship '{rid}' with no target; its graphic is not carried"),
                    });
                    continue;
                };
                let Some(chart_xml) = read_part(&mut zip, chart_part, &mut budget) else {
                    issues.push(ChartReadIssue {
                        location: format!("Sheet '{sheet_name}', drawing '{drawing_part}'"),
                        detail: format!("chart part '{chart_part}' cannot be read; its graphic is not carried"),
                    });
                    continue;
                };
                let chart_xml = unprefix(&chart_xml);
                let location = format!("Sheet '{sheet_name}', chart '{chart_part}'");
                let kinds = chart_kinds_in(&chart_xml);
                let unknown: Vec<&str> = kinds
                    .iter()
                    .map(String::as_str)
                    .filter(|k| {
                        !matches!(*k, "barChart" | "lineChart" | "pieChart" | "scatterChart" | "areaChart")
                    })
                    .collect();
                if !unknown.is_empty() {
                    issues.push(ChartReadIssue {
                        location: location.clone(),
                        detail: format!(
                            "chart kind{} {} are not ones Tables draws; the data cells stay, but the graphic does not come back",
                            if unknown.len() == 1 { "" } else { "s" },
                            unknown.join(", "),
                        ),
                    });
                }
                if kinds.is_empty() {
                    issues.push(ChartReadIssue {
                        location: location.clone(),
                        detail: "the chart part holds no chart Tables recognizes; the graphic is not carried".into(),
                    });
                }
                let Some(kind) = admitted_kind(&chart_xml) else {
                    continue;
                };
                let anchor = resolve_anchor(anchor_xml, anchor_kind, &widths, &heights, &location, &mut issues);
                let Some(anchor) = anchor else {
                    continue;
                };
                let Anchor::Placed { cell, size_px } = anchor else {
                    continue;
                };
                let series = parse_series(&chart_xml, kind, &sheet_name, &location, &mut issues);
                if series.is_empty() {
                    issues.push(ChartReadIssue {
                        location: location.clone(),
                        detail: "no series of the chart points at a same-sheet single-column range; the graphic is not carried".into(),
                    });
                    continue;
                }
                let (x_axis_title, y_axis_title) = axis_titles(&chart_xml, kind);
                let legend_position = legend_position(&chart_xml, &location, &mut issues);
                let first = series[0].clone();
                out.push((
                    sheet_name.clone(),
                    ChartSpec {
                        kind,
                        title: chart_title(&chart_xml),
                        x_axis_title,
                        y_axis_title,
                        legend_position,
                        series,
                        cat: first.cat,
                        val: first.val,
                        anchor: cell,
                        width_px: size_px.0,
                        height_px: size_px.1,
                    },
                ));
            }
        }
    }
    (out, issues)
}

/// Read embedded charts back from an xlsx (ours or a Calc rewrite).
/// Best-effort: unknown chart kinds and foreign anchoring are skipped.
///
/// Relationship-correct (TBL-002): charts resolve through the
/// workbook→worksheet→drawing→chart chain, so ZIP order and part names do
/// not matter. Callers with the model at hand should prefer
/// [`read_charts_detailed`], which also names the owning sheet and reports
/// what could not be modeled.
pub fn read_charts_from_xlsx(path: &str) -> Vec<crate::sheet::ChartSpec> {
    read_charts_detailed(path, &[]).0.into_iter().map(|(_, spec)| spec).collect()
}

#[cfg(test)]
mod chart_tests {
    use super::*;
    use crate::io::*;
    use crate::sheet::SheetModel;
    use crate::sheet::{ChartKind, ChartSpec, LegendPosition};

    /// One hand-built package part. Parts list in ZIP order, so a shuffled
    /// listing proves ZIP order is not identity.
    fn package(path: &std::path::Path, parts: &[(&str, &str)]) {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in parts {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(data.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    fn workbook_xml(sheets: &[(&str, &str)]) -> String {
        let entries: Vec<String> = sheets
            .iter()
            .map(|(name, rid)| format!(r#"<sheet name="{name}" r:id="{rid}"/>"#))
            .collect();
        format!(r#"<?xml version="1.0"?><workbook xmlns:r="r"><sheets>{}</sheets></workbook>"#, entries.join(""))
    }

    fn workbook_rels(rels: &[(&str, &str)]) -> String {
        let entries: Vec<String> = rels
            .iter()
            .map(|(id, target)| format!(r#"<Relationship Id="{id}" Target="{target}"/>"#))
            .collect();
        format!(r#"<?xml version="1.0"?><Relationships>{}</Relationships>"#, entries.join(""))
    }

    fn sheet_xml(rid: &str) -> String {
        format!(r#"<?xml version="1.0"?><worksheet xmlns:r="r"><sheetData/><drawing r:id="{rid}"/></worksheet>"#)
    }

    fn two_cell_anchor(from: (usize, usize), to: (usize, usize), rid: &str) -> String {
        format!(
            r#"<twoCellAnchor><from><col>{}</col><colOff>0</colOff><row>{}</row><rowOff>0</rowOff></from><to><col>{}</col><colOff>0</colOff><row>{}</row><rowOff>0</rowOff></to><graphicFrame><graphic><graphicData><chart xmlns:c="c" r:id="{rid}"/></graphicData></graphic></graphicFrame><clientData/></twoCellAnchor>"#,
            from.1, from.0, to.1, to.0,
        )
    }

    fn bar_chart(title: &str, series_xml: &str) -> String {
        format!(
            r#"<chartSpace xmlns:c="c"><chart><title><tx><rich><p><r><t>{title}</t></r></p></rich></tx><layout/></title><plotArea><barChart>{series_xml}</barChart></plotArea><legend><legendPos val="r"/><layout/></legend></chart></chartSpace>"#
        )
    }

    fn series_xml(name: &str, cat: &str, val: &str) -> String {
        let name_xml = if name.is_empty() {
            String::new()
        } else {
            format!(r#"<tx><v>{name}</v></tx>"#)
        };
        format!(
            r#"<ser><idx val="0"/>{name_xml}<cat><strRef><f>{cat}</f></strRef></cat><val><numRef><f>{val}</f></numRef></val></ser>"#
        )
    }

    #[test]
    fn element_prefixes_are_removed_and_attributes_kept() {
        assert_eq!(
            unprefix(r#"<c:chart r:id="rId1"><c:f>'S'!$A$1</c:f></chart><xdr:ext cx="1"/>"#),
            r#"<chart r:id="rId1"><f>'S'!$A$1</f></chart><ext cx="1"/>"#
        );
        assert_eq!(unprefix("<?xml v?><a b='c:d'/>"), "<?xml v?><a b='c:d'/>");
    }

    /// openpyxl writes chart and drawing parts with a default namespace
    /// (`<barChart>`, `<from>`), not `c:`/`xdr:` prefixes.
    #[test]
    fn an_unprefixed_chart_and_its_one_cell_anchor_size_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.xlsx");
        package(&path, &[
            ("xl/workbook.xml", &workbook_xml(&[("Sheet", "rId1")])),
            ("xl/_rels/workbook.xml.rels", &workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
            ("xl/worksheets/sheet1.xml", &sheet_xml("rId9")),
            ("xl/worksheets/_rels/sheet1.xml.rels", &workbook_rels(&[("rId9", "../drawings/drawing1.xml")])),
            ( // Shuffled: the drawing sorts after the chart it anchors.
                "xl/charts/chart1.xml",
                r#"<chartSpace xmlns="c"><chart><plotArea><barChart><ser><cat><numRef><f>'Sheet'!$A$1:$A$4</f></numRef></cat><val><numRef><f>'Sheet'!$B$1:$B$4</f></numRef></val></ser></barChart></plotArea><legend><legendPos val="r"/><layout/></legend></chart></chartSpace>"#,
            ),
            ("xl/drawings/drawing1.xml", r#"<wsDr xmlns="x"><oneCellAnchor><from><col>3</col><colOff>0</colOff><row>1</row><rowOff>0</rowOff></from><ext cx="3240000" cy="2160000"/><graphicFrame><graphic><graphicData><chart r:id="rId2"/></graphicData></graphic></graphicFrame></oneCellAnchor></wsDr>"#),
            ("xl/drawings/_rels/drawing1.xml.rels", &workbook_rels(&[("rId2", "../charts/chart1.xml")])),
        ]);
        let (charts, issues) = read_charts_detailed(path.to_str().unwrap(), &[]);
        assert!(issues.is_empty(), "a plain chart reports nothing: {issues:?}");
        assert_eq!(charts.len(), 1);
        let (owner, c) = &charts[0];
        assert_eq!(owner, "Sheet");
        assert_eq!((c.kind, c.cat, c.val, c.anchor), (ChartKind::Bar, (0, 0, 3), (0, 1, 3), (1, 3)));
        assert_eq!((c.width_px.round(), c.height_px.round()), (340.0, 227.0), "9 x 6 cm");
        assert_eq!(c.series.len(), 1, "the series vector holds the series");
    }

    #[test]
    fn chart_round_trips_through_xlsx() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.xlsx");
        let mut sh = SheetModel::new("Sheet1", 10, 5, 0);
        for (i, (l, v)) in [("North", 12.0), ("South", 9.5), ("East", 14.0)]
            .iter()
            .enumerate()
        {
            sh.data[i + 1][0] = l.to_string();
            sh.data[i + 1][1] = v.to_string();
        }
        sh.charts.push(ChartSpec {
            kind: ChartKind::Bar,
            title: "Regions".into(),
            x_axis_title: None,
            y_axis_title: None,
            legend_position: LegendPosition::Right,
            series: Vec::new(),
            cat: (1, 0, 3),
            val: (1, 1, 3),
            anchor: (5, 3),
            width_px: 480.0,
            height_px: 280.0,
        });
        save_sheets_to_xlsx(path.to_str().unwrap(), &[sh]).unwrap();
        let charts = read_charts_from_xlsx(path.to_str().unwrap());
        assert_eq!(charts.len(), 1, "chart part missing");
        let c = &charts[0];
        assert_eq!(c.kind, ChartKind::Bar);
        assert_eq!(c.val, (1, 1, 3), "values range lost: {c:?}");
        assert_eq!(c.cat, (1, 0, 3), "categories range lost: {c:?}");
        assert_eq!(c.canonical_series().len(), 1, "legacy cat/val normalize into the series vector");
        assert_eq!(c.title, "Regions");
        assert_eq!(c.anchor, (5, 3));
        assert_eq!(c.legend_position, LegendPosition::Right);
    }

    #[test]
    fn every_kind_round_trips_with_its_ranges() {
        for kind in [ChartKind::Line, ChartKind::Pie, ChartKind::Scatter, ChartKind::Area] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("c.xlsx");
            let mut sh = SheetModel::new("Sheet1", 6, 3, 0);
            for r in 0..4 {
                sh.data[r][0] = format!("{}", r + 1);
                sh.data[r][1] = format!("{}", (r * 7) % 5);
            }
            sh.charts.push(ChartSpec {
                kind,
                title: String::new(),
                x_axis_title: None,
                y_axis_title: None,
                legend_position: LegendPosition::Right,
                series: Vec::new(),
                cat: (0, 0, 3),
                val: (0, 1, 3),
                anchor: (2, 2),
                width_px: 480.0,
                height_px: 280.0,
            });
            save_sheets_to_xlsx(path.to_str().unwrap(), &[sh]).unwrap();
            let charts = read_charts_from_xlsx(path.to_str().unwrap());
            assert_eq!(charts.len(), 1, "{kind:?}");
            let c = &charts[0];
            assert_eq!((c.kind, c.cat, c.val, c.anchor), (kind, (0, 0, 3), (0, 1, 3), (2, 2)), "{kind:?}");
        }
    }

    /// A scatter chart's series names x and y values (`<c:xVal>`,
    /// `<c:yVal>`), not categories and values, as openpyxl and Calc write it.
    #[test]
    fn a_scatter_chart_reads_its_x_and_y_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.xlsx");
        package(&path, &[
            ("xl/workbook.xml", &workbook_xml(&[("Sheet", "rId1")])),
            ("xl/_rels/workbook.xml.rels", &workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
            ("xl/worksheets/sheet1.xml", &sheet_xml("rId5")),
            ("xl/worksheets/_rels/sheet1.xml.rels", &workbook_rels(&[("rId5", "../drawings/drawing1.xml")])),
            ("xl/drawings/drawing1.xml", &format!("<wsDr xmlns:c=\"c\"><twoCellAnchor>{}<clientData/></twoCellAnchor></wsDr>",
                "<from><col>0</col><colOff>0</colOff><row>0</row><rowOff>0</rowOff></from><to><col>2</col><colOff>0</colOff><row>4</row><rowOff>0</rowOff></to><graphicFrame><graphic><graphicData><chart r:id=\"rId6\"/></graphicData></graphic></graphicFrame>")),
            ("xl/drawings/_rels/drawing1.xml.rels", &workbook_rels(&[("rId6", "../charts/chart1.xml")])),
            ("xl/charts/chart1.xml", r#"<c:chartSpace xmlns:c="c"><c:chart><c:plotArea><c:scatterChart><c:ser>
                <c:xVal><c:numRef><c:f>'Sheet'!$A$1:$A$4</c:f></c:numRef></c:xVal>
                <c:yVal><c:numRef><c:f>'Sheet'!$B$1:$B$4</c:f></c:numRef></c:yVal>
                </c:ser></c:scatterChart></c:plotArea></c:chart></c:chartSpace>"#),
        ]);
        let charts = read_charts_from_xlsx(path.to_str().unwrap());
        assert_eq!(charts.len(), 1);
        assert_eq!((charts[0].kind, charts[0].cat, charts[0].val), (ChartKind::Scatter, (0, 0, 3), (0, 1, 3)));
    }

    /// TBL-002: two sheets, two drawings, nonstandard part names, shuffled
    /// ZIP order and several charts per drawing resolve through
    /// relationships — never ZIP order or filenames.
    #[test]
    fn tbl002_relationship_order_independent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rel.xlsx");
        let alpha_bar = bar_chart(
            "Alpha bar",
            &series_xml("First", "Alpha!$A$1:$A$3", "Alpha!$B$1:$B$3"),
        );
        let alpha_line = bar_chart("Alpha line", &series_xml("Second", "Alpha!$A$1:$A$3", "Alpha!$C$1:$C$3"))
            .replace("barChart", "lineChart");
        let beta_pie = bar_chart("Beta pie", &series_xml("", "Beta!$A$1:$A$2", "Beta!$B$1:$B$2"))
            .replace("barChart", "pieChart");
        let drawing_a = format!(
            r#"<wsDr xmlns:xdr="x" xmlns:r="r">{}{}</wsDr>"#,
            two_cell_anchor((1, 2), (4, 6), "rId11"),
            two_cell_anchor((7, 0), (9, 3), "rId12"),
        );
        let drawing_b = format!(
            r#"<wsDr xmlns:xdr="x" xmlns:r="r">{}</wsDr>"#,
            two_cell_anchor((0, 0), (2, 2), "rId21"),
        );
        package(&path, &[
            // Shuffled ZIP order, nonstandard part names throughout.
            ("xl/graphs/g2.xml", &alpha_line),
            ("xl/drawings/picB.xml", &drawing_b),
            ("xl/worksheets/sheetB.xml", &sheet_xml("rId7")),
            ("xl/graphs/g1.xml", &alpha_bar),
            ("xl/workbook.xml", &workbook_xml(&[("Alpha", "rId1"), ("Beta", "rId2")])),
            ("xl/charts/weird-name.xml", &beta_pie),
            ("xl/drawings/picA.xml", &drawing_a),
            ("xl/_rels/workbook.xml.rels", &workbook_rels(&[("rId1", "worksheets/sheetA.xml"), ("rId2", "worksheets/sheetB.xml")])),
            ("xl/worksheets/_rels/sheetA.xml.rels", &workbook_rels(&[("rId9", "../drawings/picA.xml")])),
            ("xl/drawings/_rels/picA.xml.rels", &workbook_rels(&[("rId11", "../graphs/g1.xml"), ("rId12", "../graphs/g2.xml")])),
            ("xl/worksheets/sheetA.xml", &sheet_xml("rId9")),
            ("xl/worksheets/_rels/sheetB.xml.rels", &workbook_rels(&[("rId7", "../drawings/picB.xml")])),
            ("xl/drawings/_rels/picB.xml.rels", &workbook_rels(&[("rId21", "../charts/weird-name.xml")])),
        ]);
        let (charts, issues) = read_charts_detailed(path.to_str().unwrap(), &[]);
        assert!(issues.is_empty(), "admitted content reports nothing: {issues:?}");
        assert_eq!(charts.len(), 3, "two charts on Alpha, one on Beta: {charts:?}");
        let alpha: Vec<_> = charts.iter().filter(|(o, _)| o == "Alpha").collect();
        let beta: Vec<_> = charts.iter().filter(|(o, _)| o == "Beta").collect();
        assert_eq!(alpha.len(), 2, "both charts anchor through Alpha's drawing");
        assert_eq!(beta.len(), 1, "Beta's chart anchors through Beta's drawing");
        assert_eq!(alpha[0].1.title, "Alpha bar");
        assert_eq!(alpha[1].1.title, "Alpha line");
        assert_eq!(alpha[0].1.anchor, (1, 2));
        assert_eq!(alpha[1].1.anchor, (7, 0));
        assert_eq!(beta[0].1.kind, ChartKind::Pie);
        assert_eq!(beta[0].1.series[0].name, "", "an unnamed series stays unnamed");
    }

    /// TBL-002: every admitted field survives on a later sheet — through
    /// Tables' own writer and back, with the XML an independent reader sees.
    #[test]
    fn tbl002_all_chart_fields_later_sheet() {
        use crate::sheet::{ChartSeries, SheetModel};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fields.xlsx");
        let mut engine = crate::engine::TablesEngine::new(10, 5).expect("engine");
        engine.add_sheet("Later").unwrap();
        let mut first = SheetModel::new("First", 10, 5, engine.sheet_id_at(0).unwrap());
        first.data[0][0] = "1".into();
        let mut later = SheetModel::new("Later", 10, 5, engine.sheet_id_at(1).unwrap());
        for (r, (label, a, b)) in [("North", "10", "4"), ("South", "20", "6"), ("East", "30", "2")]
            .iter()
            .enumerate()
        {
            later.data[r + 1][0] = label.to_string();
            later.data[r + 1][1] = a.to_string();
            later.data[r + 1][2] = b.to_string();
        }
        later.charts.push(ChartSpec {
            kind: ChartKind::Bar,
            title: "Later sales".into(),
            x_axis_title: Some("Region".into()),
            y_axis_title: Some("Revenue".into()),
            legend_position: LegendPosition::Bottom,
            series: vec![
                ChartSeries { name: "Sales".into(), cat: (1, 0, 3), val: (1, 1, 3), color: Some("4472C4".into()) },
                ChartSeries { name: "Returns".into(), cat: (1, 0, 3), val: (1, 2, 3), color: None },
            ],
            cat: (1, 0, 3),
            val: (1, 1, 3),
            anchor: (1, 3),
            width_px: 320.0,
            height_px: 200.0,
        });
        later.charts.push(ChartSpec {
            kind: ChartKind::Line,
            title: "Hidden key".into(),
            x_axis_title: None,
            y_axis_title: None,
            legend_position: LegendPosition::None,
            series: vec![ChartSeries { name: String::new(), cat: (1, 0, 3), val: (1, 1, 3), color: None }],
            cat: (1, 0, 3),
            val: (1, 1, 3),
            anchor: (8, 0),
            width_px: 480.0,
            height_px: 280.0,
        });
        crate::io::save_sheets_to_xlsx_bytes(&[first, later], Some(&engine)).map(|bytes| std::fs::write(&path, &bytes)).expect("save").unwrap();
        // An independent reader (not Tables' importer) sees every field.
        let bytes = std::fs::read(&path).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
        let mut read = |want: &str| {
            use std::io::Read;
            let name = names.iter().find(|n| n.contains(want)).unwrap_or_else(|| panic!("no part like {want}"));
            let mut s = String::new();
            zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
            s
        };
        let chart_xml: String = names
            .iter()
            .filter(|n| n.starts_with("xl/charts/"))
            .map(|n| read(n))
            .find(|s| s.contains("Later sales"))
            .expect("a chart part carrying the later sheet's chart");
        for marker in ["Sales", "Returns", "4472C4", "Region", "Revenue", "legendPos"] {
            assert!(chart_xml.contains(marker), "independent readers must see '{marker}'");
        }
        // Tables' own reopen keeps every admitted field on the later sheet.
        let (_engine, back) = load_workbook(path.to_str().unwrap()).expect("reopen");
        assert_eq!(back.len(), 2);
        assert!(back[0].charts.is_empty(), "no chart leaks onto the first sheet");
        assert_eq!(back[1].charts.len(), 2, "both later-sheet charts come home: {:?}", back[1].charts);
        let rich = &back[1].charts[0];
        assert_eq!(rich.title, "Later sales");
        assert_eq!(rich.series.len(), 2);
        assert_eq!(rich.series[0].name, "Sales");
        assert_eq!(rich.series[0].color.as_deref(), Some("4472C4"));
        assert_eq!(rich.series[1].name, "Returns");
        assert_eq!((rich.cat, rich.val), ((1, 0, 3), (1, 1, 3)), "legacy ranges mirror the first series");
        assert_eq!(rich.x_axis_title.as_deref(), Some("Region"));
        assert_eq!(rich.y_axis_title.as_deref(), Some("Revenue"));
        assert_eq!(rich.legend_position, LegendPosition::Bottom);
        assert_eq!(rich.anchor, (1, 3));
        assert!((rich.width_px - 320.0).abs() < 1.0 && (rich.height_px - 200.0).abs() < 1.0, "size inverts: {:?}", (rich.width_px, rich.height_px));
        let hidden = &back[1].charts[1];
        assert_eq!(hidden.legend_position, LegendPosition::None, "a hidden legend is hidden, not Right");
    }

    /// TBL-002: unsupported content is reported with its location — never
    /// silently dropped or misfiled — while the admitted chart still reads.
    #[test]
    fn tbl002_chart_unsupported_report() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unsupported.xlsx");
        let good = bar_chart("Good", &series_xml("", "Alpha!$A$1:$A$2", "Alpha!$B$1:$B$2"));
        let stock = r#"<chartSpace xmlns:c="c"><chart><plotArea><stockChart><ser><val><numRef><f>Alpha!$B$1:$B$2</f></numRef></val></ser></stockChart></plotArea></chart></chartSpace>"#;
        let cross = bar_chart("Cross", &series_xml("", "Beta!$A$1:$A$2", "Beta!$B$1:$B$2"));
        let row_oriented = bar_chart("Wide", &series_xml("", "Alpha!$A$1:$A$1", "Alpha!$A$1:$C$1"));
        let cached = r#"<chartSpace xmlns:c="c"><chart><plotArea><barChart><ser><val><numLit><ptCount val="2"/><pt idx="0"><v>1</v></pt></numLit></val></ser></barChart></plotArea></chart></chartSpace>"#;
        let named_ref = bar_chart(
            "Named",
            r#"<ser><tx><strRef><f>Alpha!$A$1</f></strRef></tx><cat><strRef><f>Alpha!$A$1:$A$2</f></strRef></cat><val><numRef><f>Alpha!$B$1:$B$2</f></numRef></val></ser>"#,
        );
        let drawing_a = format!(
            r#"<wsDr xmlns:xdr="x" xmlns:r="r">{}{}{}{}{}{}</wsDr>"#,
            two_cell_anchor((0, 0), (2, 2), "rId1"),
            two_cell_anchor((3, 0), (5, 2), "rId2"),
            two_cell_anchor((6, 0), (8, 2), "rId3"),
            two_cell_anchor((9, 0), (11, 2), "rId4"),
            two_cell_anchor((12, 0), (14, 2), "rId5"),
            two_cell_anchor((15, 0), (17, 2), "rId6"),
        );
        // An absolute anchor carries no cell: reported, never guessed.
        let drawing_b = r#"<wsDr xmlns:xdr="x" xmlns:r="r"><absoluteAnchor><pos x="100" y="200"/><ext cx="1000000" cy="1000000"/><graphicFrame><graphic><graphicData><chart xmlns:c="c" r:id="rId9"/></graphicData></graphic></graphicFrame></absoluteAnchor></wsDr>"#;
        package(&path, &[
            ("xl/workbook.xml", &workbook_xml(&[("Alpha", "rId1"), ("Beta", "rId2")])),
            ("xl/_rels/workbook.xml.rels", &workbook_rels(&[("rId1", "worksheets/sheet1.xml"), ("rId2", "worksheets/sheet2.xml")])),
            ("xl/worksheets/sheet1.xml", &sheet_xml("rIdA")),
            ("xl/worksheets/sheet2.xml", &sheet_xml("rIdB")),
            ("xl/worksheets/_rels/sheet1.xml.rels", &workbook_rels(&[("rIdA", "../drawings/drawing1.xml")])),
            ("xl/worksheets/_rels/sheet2.xml.rels", &workbook_rels(&[("rIdB", "../drawings/drawing2.xml")])),
            ("xl/drawings/drawing1.xml", &drawing_a),
            ("xl/drawings/drawing2.xml", drawing_b),
            ("xl/drawings/_rels/drawing1.xml.rels", &workbook_rels(&[("rId1", "../charts/good.xml"), ("rId2", "../charts/stock.xml"), ("rId3", "../charts/cross.xml"), ("rId4", "../charts/wide.xml"), ("rId5", "../charts/cached.xml"), ("rId6", "../charts/named.xml")])),
            ("xl/drawings/_rels/drawing2.xml.rels", &workbook_rels(&[("rId9", "../charts/good.xml")])),
            ("xl/charts/good.xml", &good),
            ("xl/charts/stock.xml", stock),
            ("xl/charts/cross.xml", &cross),
            ("xl/charts/wide.xml", &row_oriented),
            ("xl/charts/cached.xml", cached),
            ("xl/charts/named.xml", &named_ref),
        ]);
        let (charts, issues) = read_charts_detailed(path.to_str().unwrap(), &[]);
        // The admitted chart reads; the named-by-reference chart reads with
        // an unnamed series; nothing lands on the wrong sheet.
        assert_eq!(charts.len(), 2, "good + unnamed-by-reference: {charts:?}");
        assert!(charts.iter().all(|(o, _)| o == "Alpha"), "no wrong-sheet fallback: {charts:?}");
        assert_eq!(charts[0].1.title, "Good");
        assert_eq!(charts[1].1.title, "Named");
        assert_eq!(charts[1].1.series[0].name, "", "a cell-referenced name is not resolved");
        let details = issues.iter().map(|i| format!("{} | {}", i.location, i.detail)).collect::<Vec<_>>().join("\n");
        for marker in ["stockChart", "not the chart's own sheet", "spans several columns", "cached values", "takes its name from the cell", "absolute anchor"] {
            assert!(details.contains(marker), "missing report for '{marker}':\n{details}");
        }
        assert!(issues.iter().all(|i| !i.location.is_empty() && !i.detail.is_empty()), "every report names a location and consequence");
        // The save gate consents on the same file: unknown kinds and the
        // absolute anchor warn per part through the package scan.
        let gate = crate::io::loss::content_a_save_drops(
            Some(path.to_str().unwrap()),
            &[SheetModel::new("Alpha", 4, 4, 1)],
        );
        assert!(gate.features.iter().any(|f| f.id == "unsupported-charts"), "unknown kinds and the absolute anchor must warn: {:?}", gate.features);
    }
}
