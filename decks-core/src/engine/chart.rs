//! engine/chart.rs — a chart on a slide: its kind and its series, and the
//! parts that carry it in each format. GTK-free; drawn by
//! `suite_common::charts`, the renderer Tables draws its charts with.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! - pptx: a `p:graphicFrame` naming a DrawingML chart part
//!   (`ppt/charts/chartN.xml`, `c:chartSpace`). The data is written as the
//!   formula references PowerPoint writes with their caches, which is what
//!   PowerPoint and Impress draw from; no workbook is embedded.
//! - odp: a `draw:frame` holding a `draw:object`, an embedded ODF chart
//!   (`Object N/content.xml`) whose data is its own local table, as Impress
//!   writes one.
//!
//! A chart is a first series of (category, value) points, the series a
//! Tables chart draws and the inspector edits, and any further series over
//! the same categories (`more`). A bar chart's series may be stacked, each
//! value may be labelled on its bar, and the value axis may be hidden, as
//! PowerPoint and Impress write them (#2327). A scatter chart, whose series
//! need not share their x values, is read as its first series.

pub use suite_common_core::charts::ChartKind;

/// A chart's kind, series name and points.
#[derive(Clone, Debug, PartialEq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ChartData {
    pub kind: ChartKind,
    /// The series' name, its legend entry; empty for none.
    pub series: String,
    /// (category, value). For a scatter chart the category is the x value.
    pub points: Vec<(String, f64)>,
    /// The series after the first, each (name, one value per category).
    #[serde(default)]
    pub more: Vec<(String, Vec<f64>)>,
    /// A bar chart's series stacked in each category instead of side by
    /// side (`c:grouping val="stacked"`, `chart:stacked`).
    #[serde(default)]
    pub stacked: bool,
    /// Each value written on its bar or point (`c:dLbls/c:showVal`,
    /// `chart:data-label-number="value"`).
    #[serde(default)]
    pub labels: bool,
    /// The value axis hidden (`c:valAx/c:delete`, `chart:display-label`).
    #[serde(default)]
    pub hide_value_axis: bool,
    /// A doughnut's hole in percent of its radius (`c:holeSize`,
    /// `chart:ring`); 0 is a pie.
    #[serde(default)]
    pub hole: u8,
    /// A pie's slice colours, or a one-series bar chart's bar colours, as
    /// RGB by category, where the file sets them (`c:dPt`, the series'
    /// own fill, `chart:data-point`).
    #[serde(default)]
    pub point_colors: Vec<Option<[u8; 3]>>,
    /// No legend where one would be drawn: the file has none.
    #[serde(default)]
    pub no_legend: bool,
    /// No chart background: the slide shows through (`a:noFill`,
    /// `draw:fill="none"`).
    #[serde(default)]
    pub transparent: bool,
    /// A bar chart's bars lie horizontally (`c:barDir val="bar"`).
    #[serde(default)]
    pub horizontal: bool,
    /// The categories run the other way: a horizontal chart's first at the
    /// top (`c:catAx/c:scaling/c:orientation val="maxMin"`).
    #[serde(default)]
    pub reversed: bool,
    /// Values are labelled as percentages (a `0%` number format).
    #[serde(default)]
    pub percent: bool,
    /// The chart's text colour as RGB (`c:txPr`'s `a:defRPr`), where set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_color: Option<[u8; 3]>,
    /// The chart's text size in hundredths of a point (`a:defRPr sz`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_size: Option<u32>,
    /// Each series' own colour as RGB (`c:ser/c:spPr`), the first included,
    /// where the file sets one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series_colors: Vec<Option<[u8; 3]>>,
    /// A bar chart's gap between categories, in percent of one bar
    /// (`c:gapWidth`); `None` is the default 150.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<u16>,
    /// The category axis hidden (`c:catAx/c:delete`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_category_axis: bool,
}

impl ChartData {
    /// A new chart's data: PowerPoint's own sample series, four categories
    /// of one series.
    pub fn sample(kind: ChartKind) -> Self {
        let points = if kind == ChartKind::Scatter {
            vec![("1".into(), 2.7), ("2".into(), 3.2), ("3".into(), 0.8), ("4".into(), 4.5)]
        } else {
            vec![("Q1".into(), 4.3), ("Q2".into(), 2.5), ("Q3".into(), 3.5), ("Q4".into(), 4.5)]
        };
        ChartData::one(kind, "Sales", points)
    }

    /// A chart of one series and no options.
    pub fn one(kind: ChartKind, series: impl Into<String>, points: Vec<(String, f64)>) -> Self {
        ChartData {
            kind,
            series: series.into(),
            points,
            more: Vec::new(),
            stacked: false,
            labels: false,
            hide_value_axis: false,
            hole: 0,
            point_colors: Vec::new(),
            no_legend: false,
            transparent: false,
            horizontal: false,
            reversed: false,
            percent: false,
            text_color: None,
            text_size: None,
            series_colors: Vec::new(),
            gap: None,
            hide_category_axis: false,
        }
    }

    /// Every series as (name, values), the first included.
    pub fn all_series(&self) -> Vec<(&str, Vec<f64>)> {
        let mut out = vec![(self.series.as_str(), self.points.iter().map(|p| p.1).collect())];
        out.extend(self.more.iter().map(|(n, v)| (n.as_str(), v.clone())));
        out
    }

    /// The kind's name, as the accessible name and the inspector say it.
    pub fn kind_name(kind: ChartKind) -> &'static str {
        match kind {
            ChartKind::Bar => "Bar",
            ChartKind::Line => "Line",
            ChartKind::Pie => "Pie",
            ChartKind::Scatter => "Scatter",
            ChartKind::Area => "Area",
        }
    }

    /// "Bar chart: Sales, 4 values".
    pub fn describe(&self) -> String {
        let n = self.points.len();
        let values = if n == 1 { "1 value".to_string() } else { format!("{n} values") };
        if self.series.trim().is_empty() {
            format!("{} chart, {values}", Self::kind_name(self.kind))
        } else {
            format!("{} chart: {}, {values}", Self::kind_name(self.kind), self.series)
        }
    }

    /// The series name the renderer puts in the legend.
    pub fn legend(&self) -> Option<&str> {
        (!self.series.trim().is_empty()).then_some(self.series.as_str())
    }

    /// Whether the chart has more than one plain series of points: further
    /// series, or a grouping, labels or hidden axis the one-series drawing
    /// does not show.
    pub fn is_rich(&self) -> bool {
        !self.more.is_empty()
            || self.stacked
            || self.labels
            || self.hide_value_axis
            || self.hole > 0
            || self.point_colors.iter().any(Option::is_some)
            || self.no_legend
            || self.transparent
            || self.horizontal
            || self.reversed
            || self.percent
            || self.text_color.is_some()
            || self.text_size.is_some()
            || self.series_colors.iter().any(Option::is_some)
            || self.gap.is_some()
            || self.hide_category_axis
    }

    /// Whether a legend is drawn: a named series, a pie's categories or
    /// several series, unless the file has none.
    pub fn shows_legend(&self) -> bool {
        !self.no_legend && self.has_default_legend()
    }

    /// Whether this chart would carry a legend by default.
    fn has_default_legend(&self) -> bool {
        self.legend().is_some() || self.kind == ChartKind::Pie || !self.more.is_empty()
    }

    /// Every category is a number: a scatter chart's x values.
    fn numeric_categories(&self) -> Option<Vec<f64>> {
        self.points.iter().map(|(c, _)| c.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect()
    }
}

/// One edit the inspector makes to a chart: its type, its series name,
/// or one point of its data sheet. Each is one undo step.
#[derive(Clone, Debug, PartialEq)]
pub enum ChartEdit {
    Kind(ChartKind),
    Series(String),
    /// Point `i`'s category (a scatter chart's x value).
    Category(usize, String),
    /// Point `i`'s value.
    Value(usize, f64),
    /// A point after the last: the next category and the last value.
    AddPoint,
    /// Point `i` taken out; a chart keeps at least one.
    RemovePoint(usize),
}

/// The most points a data sheet is given, as many as a Tables chart
/// draws legibly.
pub const MAX_POINTS: usize = 100;

impl ChartData {
    /// `edit` applied. Whether it changed anything: an edit to a point
    /// that isn't there, a value that isn't a number or a type the chart
    /// already has changes nothing, and so makes no undo step.
    pub fn apply(&mut self, edit: &ChartEdit) -> bool {
        let before = self.clone();
        match edit {
            ChartEdit::Kind(k) => self.kind = *k,
            ChartEdit::Series(s) => self.series = s.trim().to_string(),
            ChartEdit::Category(i, c) => {
                if let Some(p) = self.points.get_mut(*i) {
                    p.0 = c.trim().to_string();
                }
            }
            ChartEdit::Value(i, v) => {
                if let (Some(p), true) = (self.points.get_mut(*i), v.is_finite()) {
                    p.1 = *v;
                }
            }
            ChartEdit::AddPoint => {
                if self.points.len() < MAX_POINTS {
                    let category = self.next_category();
                    let value = self.points.last().map_or(0.0, |p| p.1);
                    self.points.push((category, value));
                    for (_, values) in &mut self.more {
                        let last = values.last().copied().unwrap_or(0.0);
                        values.resize(self.points.len(), last);
                    }
                }
            }
            ChartEdit::RemovePoint(i) => {
                if self.points.len() > 1 && *i < self.points.len() {
                    self.points.remove(*i);
                    for (_, values) in &mut self.more {
                        if *i < values.len() {
                            values.remove(*i);
                        }
                    }
                }
            }
        }
        *self != before
    }

    /// The category a new point gets: the next number when they're all
    /// numbers (a scatter chart's x values, or 1, 2, 3), the next of a
    /// word and a number ("Q4" → "Q5"), else "Category n" as PowerPoint
    /// names one.
    fn next_category(&self) -> String {
        let n = self.points.len() + 1;
        let Some((last, _)) = self.points.last() else { return "1".into() };
        if let Some(xs) = self.numeric_categories() {
            let step = match xs.as_slice() {
                [.., a, b] if b > a => b - a,
                _ => 1.0,
            };
            return num(xs.last().copied().unwrap_or(0.0) + step);
        }
        let digits = last.len() - last.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 {
            let (word, number) = last.split_at(last.len() - digits);
            if let Ok(k) = number.parse::<u64>() {
                return format!("{word}{}", k + 1);
            }
        }
        format!("Category {n}")
    }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// The most series a chart keeps: more than a legend can name legibly.
pub const MAX_SERIES: usize = 16;

/// "4F81BD" as RGB.
fn hex_rgb(v: &str) -> Option<[u8; 3]> {
    let v = v.trim().trim_start_matches('#');
    if v.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&v[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// A cached point's number; a gap or text is 0.
fn number(v: Option<&str>) -> f64 {
    v.and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0)
}

/// A value as a cache or a cell writes it: "4.3", "10".
fn num(v: f64) -> String {
    if v.is_finite() { format!("{v}") } else { "0".into() }
}

/// Column letter of a 0-based column (A..Z; charts here use two).
fn col(c: usize) -> char {
    (b'A' + (c.min(25)) as u8) as char
}

// ── pptx: the DrawingML chart part ───────────────────────────────────────

pub const CHART_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
pub const CHART_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart";
pub const CHART_CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";

fn str_ref(f: &str, vals: &[&str]) -> String {
    let mut s = format!("<c:strRef><c:f>{}</c:f><c:strCache><c:ptCount val=\"{}\"/>", esc(f), vals.len());
    for (i, v) in vals.iter().enumerate() {
        s.push_str(&format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", esc(v)));
    }
    s.push_str("</c:strCache></c:strRef>");
    s
}

fn num_ref(f: &str, vals: &[f64]) -> String {
    let mut s = format!(
        "<c:numRef><c:f>{}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"{}\"/>",
        esc(f),
        vals.len()
    );
    for (i, v) in vals.iter().enumerate() {
        s.push_str(&format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", num(*v)));
    }
    s.push_str("</c:numCache></c:numRef>");
    s
}

const AX_1: &str = "111111111";
const AX_2: &str = "222222222";

fn cat_ax() -> String {
    format!(
        "<c:catAx><c:axId val=\"{AX_1}\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling>\
         <c:delete val=\"0\"/><c:axPos val=\"b\"/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/>\
         <c:majorTickMark val=\"out\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/>\
         <c:crossAx val=\"{AX_2}\"/><c:crosses val=\"autoZero\"/><c:auto val=\"1\"/>\
         <c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/><c:noMultiLvlLbl val=\"0\"/></c:catAx>"
    )
}

/// A value axis: `id` at `pos` ("l" or "b"), crossing `cross`.
fn val_ax(id: &str, pos: &str, cross: &str, grid: bool, between: &str) -> String {
    format!(
        "<c:valAx><c:axId val=\"{id}\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling>\
         <c:delete val=\"0\"/><c:axPos val=\"{pos}\"/>{}<c:numFmt formatCode=\"General\" sourceLinked=\"1\"/>\
         <c:majorTickMark val=\"out\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/>\
         <c:crossAx val=\"{cross}\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"{between}\"/></c:valAx>",
        if grid { "<c:majorGridlines/>" } else { "" }
    )
}

/// `chart` as a DrawingML chart part (`c:chartSpace`).
pub fn chart_space_xml(chart: &ChartData) -> String {
    let n = chart.points.len();
    let last = n + 1;
    let cats: Vec<&str> = chart.points.iter().map(|p| p.0.as_str()).collect();
    let vals: Vec<f64> = chart.points.iter().map(|p| p.1).collect();
    // A series' own fill, as the file coloured it.
    let ser_fill = |k: usize| match chart.series_colors.get(k).copied().flatten() {
        Some([r, g, b]) => format!("<c:spPr><a:solidFill><a:srgbClr val=\"{r:02X}{g:02X}{b:02X}\"/></a:solidFill></c:spPr>"),
        None => String::new(),
    };
    let head = format!(
        "<c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:tx>{}</c:tx>{}",
        str_ref("Sheet1!$B$1", &[chart.series.as_str()]),
        ser_fill(0)
    );
    let cat = format!("<c:cat>{}</c:cat>", str_ref(&format!("Sheet1!$A$2:$A${last}"), &cats));
    let val = format!("<c:val>{}</c:val>", num_ref(&format!("Sheet1!$B$2:$B${last}"), &vals));
    let ids = format!("<c:axId val=\"{AX_1}\"/><c:axId val=\"{AX_2}\"/>");
    // The series after the first, in columns C, D, ... of the same sheet.
    let more: String = chart
        .more
        .iter()
        .enumerate()
        .map(|(k, (name, values))| {
            let c = col(k + 2);
            let tx = str_ref(&format!("Sheet1!${c}$1"), &[name.as_str()]);
            let val = format!("<c:val>{}</c:val>", num_ref(&format!("Sheet1!${c}$2:${c}${last}"), values));
            let invert = if chart.kind == ChartKind::Bar { "<c:invertIfNegative val=\"0\"/>" } else { "" };
            let smooth = if chart.kind == ChartKind::Line { "<c:smooth val=\"0\"/>" } else { "" };
            format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:tx>{tx}</c:tx>{}{invert}{cat}{val}{smooth}</c:ser>", ser_fill(k + 1), i = k + 1)
        })
        .collect();
    let dlbls = if chart.labels {
        format!(
            "<c:dLbls>{}<c:showLegendKey val=\"0\"/><c:showVal val=\"1\"/><c:showCatName val=\"0\"/>\
             <c:showSerName val=\"0\"/><c:showPercent val=\"0\"/><c:showBubbleSize val=\"0\"/></c:dLbls>",
            if chart.percent { "<c:numFmt formatCode=\"0%\" sourceLinked=\"0\"/>" } else { "" }
        )
    } else {
        String::new()
    };
    // Each point's own colour (a pie's slices, a one-series bar chart's
    // bars), as the file set it.
    let dpts: String = chart
        .point_colors
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let [r, g, b] = (*c)?;
            Some(format!(
                "<c:dPt><c:idx val=\"{i}\"/>{}<c:bubble3D val=\"0\"/><c:spPr><a:solidFill>\
                 <a:srgbClr val=\"{r:02X}{g:02X}{b:02X}\"/></a:solidFill></c:spPr></c:dPt>",
                if chart.kind == ChartKind::Bar { "<c:invertIfNegative val=\"0\"/>" } else { "" }
            ))
        })
        .collect();
    // A horizontal chart's categories down the left and its values along
    // the bottom; reversed, the first category is at the top.
    let category_axis = || {
        let mut ax = cat_ax();
        if chart.horizontal {
            ax = ax.replacen("<c:axPos val=\"b\"/>", "<c:axPos val=\"l\"/>", 1);
        }
        if chart.reversed {
            ax = ax.replacen("<c:orientation val=\"minMax\"/>", "<c:orientation val=\"maxMin\"/>", 1);
        }
        if chart.hide_category_axis {
            ax = ax.replacen("<c:delete val=\"0\"/>", "<c:delete val=\"1\"/>", 1);
        }
        ax
    };
    let grouping = if chart.stacked { "stacked" } else { "clustered" };
    let overlap = if chart.stacked { "<c:overlap val=\"100\"/>" } else { "" };
    let value_axis = |id: &str, pos: &str, cross: &str, grid: bool, between: &str| {
        let ax = val_ax(id, pos, cross, grid, between);
        if chart.hide_value_axis { ax.replacen("<c:delete val=\"0\"/>", "<c:delete val=\"1\"/>", 1) } else { ax }
    };
    let plot = match chart.kind {
        ChartKind::Bar => format!(
            "<c:barChart><c:barDir val=\"{}\"/><c:grouping val=\"{grouping}\"/><c:varyColors val=\"0\"/>\
             {head}<c:invertIfNegative val=\"0\"/>{dpts}{cat}{val}</c:ser>{more}{dlbls}<c:gapWidth val=\"{}\"/>{overlap}{ids}</c:barChart>{}{}",
            if chart.horizontal { "bar" } else { "col" },
            chart.gap.unwrap_or(150),
            category_axis(),
            value_axis(AX_2, if chart.horizontal { "b" } else { "l" }, AX_1, true, "between")
        ),
        ChartKind::Line => format!(
            "<c:lineChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>\
             {head}{cat}{val}<c:smooth val=\"0\"/></c:ser>{more}{dlbls}<c:marker val=\"1\"/>{ids}</c:lineChart>{}{}",
            category_axis(),
            value_axis(AX_2, "l", AX_1, true, "between")
        ),
        ChartKind::Area => format!(
            "<c:areaChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>\
             {head}{cat}{val}</c:ser>{more}{dlbls}{ids}</c:areaChart>{}{}",
            category_axis(),
            value_axis(AX_2, "l", AX_1, true, "midCat")
        ),
        ChartKind::Pie => {
            if chart.hole > 0 {
                format!(
                    "<c:doughnutChart><c:varyColors val=\"1\"/>{head}{dpts}{cat}{val}</c:ser>{dlbls}\
                     <c:firstSliceAng val=\"0\"/><c:holeSize val=\"{}\"/></c:doughnutChart>",
                    chart.hole.clamp(10, 90)
                )
            } else {
                format!("<c:pieChart><c:varyColors val=\"1\"/>{head}{dpts}{cat}{val}</c:ser>{dlbls}<c:firstSliceAng val=\"0\"/></c:pieChart>")
            }
        }
        ChartKind::Scatter => {
            let x_ref = format!("Sheet1!$A$2:$A${last}");
            let x = match chart.numeric_categories() {
                Some(xs) => num_ref(&x_ref, &xs),
                None => str_ref(&x_ref, &cats),
            };
            let y = num_ref(&format!("Sheet1!$B$2:$B${last}"), &vals);
            format!(
                "<c:scatterChart><c:scatterStyle val=\"lineMarker\"/><c:varyColors val=\"0\"/>\
                 {head}<c:spPr><a:ln w=\"19050\"><a:noFill/></a:ln></c:spPr>\
                 <c:xVal>{x}</c:xVal><c:yVal>{y}</c:yVal><c:smooth val=\"0\"/></c:ser>{ids}</c:scatterChart>{}{}",
                val_ax(AX_1, "b", AX_2, false, "midCat"),
                val_ax(AX_2, "l", AX_1, true, "midCat")
            )
        }
    };
    let background = if chart.transparent { "<c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr>" } else { "" };
    let text = if chart.text_color.is_some() || chart.text_size.is_some() {
        format!(
            "<c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr{}>{}</a:defRPr></a:pPr><a:endParaRPr lang=\"en-US\"/></a:p></c:txPr>",
            chart.text_size.map(|v| format!(" sz=\"{v}\"")).unwrap_or_default(),
            chart.text_color.map(|[r, g, b]| format!("<a:solidFill><a:srgbClr val=\"{r:02X}{g:02X}{b:02X}\"/></a:solidFill>")).unwrap_or_default()
        )
    } else {
        String::new()
    };
    let legend = if chart.shows_legend() {
        "<c:legend><c:legendPos val=\"r\"/><c:overlay val=\"0\"/></c:legend>"
    } else {
        ""
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <c:chartSpace xmlns:c=\"{CHART_NS}\" \
         xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
         <c:date1904 val=\"0\"/><c:roundedCorners val=\"0\"/>\
         <c:chart><c:autoTitleDeleted val=\"1\"/><c:plotArea><c:layout/>{plot}</c:plotArea>{legend}\
         <c:plotVisOnly val=\"1\"/><c:dispBlanksAs val=\"gap\"/></c:chart>{background}{text}</c:chartSpace>"
    )
}

// ── A small element tree, prefixes kept and stripped ─────────────────────

#[derive(Debug, Default)]
struct El {
    /// The qualified name, "c:barChart".
    name: String,
    /// Without its prefix, "barChart": a DrawingML producer's prefix is
    /// its own choice (openpyxl writes none).
    local: String,
    attrs: Vec<(String, String)>,
    children: Vec<El>,
    text: String,
}

fn local_of(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

impl El {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    /// An attribute by its local name.
    fn attr_local(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| local_of(k) == name).map(|(_, v)| v.as_str())
    }
    fn child(&self, local: &str) -> Option<&El> {
        self.children.iter().find(|c| c.local == local)
    }
    fn child_q(&self, name: &str) -> Option<&El> {
        self.children.iter().find(|c| c.name == name)
    }
    /// The descendants that `matches`, depth first in document order, not
    /// descending into a match; stopping at the first when `first`. A walk
    /// with its own stack, as in `shape_xml`: a chart part is as deep as its
    /// author made it, and recursion overflows on a deep one (#1206).
    fn matching(&self, matches: impl Fn(&El) -> bool, first: bool) -> Vec<&El> {
        let mut out = Vec::new();
        let mut stack = vec![self.children.iter()];
        while let Some(level) = stack.last_mut() {
            match level.next() {
                None => {
                    stack.pop();
                }
                Some(c) if matches(c) => {
                    out.push(c);
                    if first {
                        break;
                    }
                }
                Some(c) => stack.push(c.children.iter()),
            }
        }
        out
    }
    /// The first descendant, depth first, whose local name is `local`.
    fn find(&self, local: &str) -> Option<&El> {
        self.matching(|c| c.local == local, true).into_iter().next()
    }
    /// The first descendant, depth first, named `name` (qualified).
    fn find_q(&self, name: &str) -> Option<&El> {
        self.matching(|c| c.name == name, true).into_iter().next()
    }
    fn all_q<'a>(&'a self, name: &str, out: &mut Vec<&'a El>) {
        out.extend(self.matching(|c| c.name == name, false));
    }
    /// All the text inside, depth first.
    fn deep_text(&self) -> String {
        let mut s = String::new();
        let mut stack = vec![self];
        while let Some(el) = stack.pop() {
            s.push_str(&el.text);
            stack.extend(el.children.iter().rev());
        }
        s
    }
}

/// Freed a level at a time, as `shape_xml::Node` is.
impl Drop for El {
    fn drop(&mut self) {
        let mut pending = std::mem::take(&mut self.children);
        while let Some(mut el) = pending.pop() {
            pending.append(&mut el.children);
        }
    }
}

fn tree(xml: &str) -> El {
    use quick_xml::events::{BytesStart, Event};
    use quick_xml::Reader;
    fn element(e: &BytesStart) -> El {
        let name = e.name().as_ref().to_string();
        El {
            local: local_of(&name).to_string(),
            name,
            attrs: e
                .attributes()
                .flatten()
                .filter_map(|a| {
                    let v = a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.into_owned();
                    Some((a.key.as_ref().to_string(), v))
                })
                .collect(),
            children: Vec::new(),
            text: String::new(),
        }
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut stack = vec![El::default()];
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => stack.push(element(e)),
            Ok(Event::Empty(ref e)) => {
                let el = element(e);
                if let Some(top) = stack.last_mut() {
                    top.children.push(el);
                }
            }
            Ok(Event::Text(ref t)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&super::parse::unescape_text(t));
                }
            }
            Ok(Event::GeneralRef(ref r)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&super::parse::resolve_general_ref(r));
                }
            }
            Ok(Event::End(_)) => {
                if stack.len() > 1 {
                    let el = stack.pop().unwrap_or_default();
                    if let Some(top) = stack.last_mut() {
                        top.children.push(el);
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    while stack.len() > 1 {
        let el = stack.pop().unwrap_or_default();
        if let Some(top) = stack.last_mut() {
            top.children.push(el);
        }
    }
    stack.pop().unwrap_or_default()
}

/// The points of a DrawingML data source (`c:cat`, `c:val`, `c:xVal`,
/// `c:tx`): its cache or literal, by index.
fn cached(source: &El) -> Vec<Option<String>> {
    let Some(cache) = ["strCache", "numCache", "strLit", "numLit", "lvl"].iter().find_map(|n| source.find(n)) else {
        // `c:tx` may be a bare `c:v`.
        return source.child("v").map(|v| vec![Some(v.deep_text())]).unwrap_or_default();
    };
    let pts: Vec<(usize, String)> = cache
        .children
        .iter()
        .filter(|c| c.local == "pt")
        .filter_map(|p| Some((p.attr("idx")?.parse::<usize>().ok()?, p.child("v")?.deep_text())))
        .collect();
    // As long as the points that are there reach, not as `c:ptCount`
    // says: a count is the file's claim, and a hostile one is billions.
    let len = pts.iter().map(|p| p.0 + 1).max().unwrap_or(0).min(10_000);
    let mut out = vec![None; len];
    for (i, v) in pts {
        if i < len {
            out[i] = Some(v);
        }
    }
    out
}

/// The chart a DrawingML chart part draws, as its first series.
pub fn parse_chart_space(xml: &str) -> Option<ChartData> {
    let root = tree(xml);
    let plot = root.find("plotArea")?;
    let (kind, group) = plot.children.iter().find_map(|c| {
        let kind = match c.local.as_str() {
            "barChart" | "bar3DChart" => ChartKind::Bar,
            "lineChart" | "line3DChart" => ChartKind::Line,
            "pieChart" | "pie3DChart" | "doughnutChart" | "ofPieChart" => ChartKind::Pie,
            "areaChart" | "area3DChart" => ChartKind::Area,
            "scatterChart" => ChartKind::Scatter,
            _ => return None,
        };
        Some((kind, c))
    })?;
    let sers: Vec<&El> = group.children.iter().filter(|c| c.local == "ser").collect();
    let ser = *sers.first()?;
    let name_of = |s: &El| s.child("tx").map(cached).and_then(|v| v.into_iter().next().flatten()).unwrap_or_default();
    let series = name_of(ser);
    let cats = ser.child("cat").or_else(|| ser.child("xVal")).map(cached).unwrap_or_default();
    let vals = ser.child("val").or_else(|| ser.child("yVal")).map(cached)?;
    let points = vals
        .iter()
        .enumerate()
        .map(|(i, v)| {
            // A category the file leaves out is its number, as PowerPoint
            // labels an axis with none.
            let cat = cats.get(i).cloned().flatten().unwrap_or_else(|| (i + 1).to_string());
            (cat, number(v.as_deref()))
        })
        .collect::<Vec<_>>();
    let n = points.len();
    // The series after the first, over the first's categories. A scatter
    // chart's series each carry their own x values, so it keeps its first.
    let more = if kind == ChartKind::Scatter || kind == ChartKind::Pie {
        Vec::new()
    } else {
        sers[1..]
            .iter()
            .take(MAX_SERIES - 1)
            .map(|s| {
                let vals = s.child("val").map(cached).unwrap_or_default();
                (name_of(s), (0..n).map(|i| number(vals.get(i).and_then(|v| v.as_deref()))).collect())
            })
            .collect()
    };
    let hole = if group.local == "doughnutChart" {
        group.child("holeSize").and_then(|h| h.attr("val")).and_then(|v| v.parse::<u8>().ok()).unwrap_or(10).clamp(10, 90)
    } else {
        0
    };
    // A slice's (or a one-series bar chart's bar's) colour where the file
    // gives it as RGB (`a:srgbClr`; the reader resolves theme colours to
    // RGB before this), else the series' own fill for a bar.
    let rgb_of = |sp: Option<&El>| sp.and_then(|sp| sp.child("solidFill")).and_then(|f| f.child("srgbClr")).and_then(|c| c.attr("val")).and_then(hex_rgb);
    let one_bar = kind == ChartKind::Bar && sers.len() == 1;
    let point_colors: Vec<Option<[u8; 3]>> = if kind == ChartKind::Pie || one_bar {
        let base = if one_bar { rgb_of(ser.child("spPr")) } else { None };
        let mut colors = vec![base; n.min(10_000)];
        for dpt in ser.children.iter().filter(|c| c.local == "dPt") {
            let idx = dpt.child("idx").and_then(|i| i.attr("val")).and_then(|v| v.parse::<usize>().ok());
            if let (Some(i), Some(rgb)) = (idx, rgb_of(dpt.child("spPr"))) {
                if i < colors.len() {
                    colors[i] = Some(rgb);
                }
            }
        }
        if colors.iter().any(Option::is_some) { colors } else { Vec::new() }
    } else {
        Vec::new()
    };
    let chart_el = root.find("chart");
    let has_legend = chart_el.is_some_and(|c| c.child("legend").is_some());
    let transparent = root.child("spPr").or_else(|| root.find("chartSpace").and_then(|c| c.child("spPr"))).is_some_and(|sp| sp.child("noFill").is_some());
    let grouping = group.child("grouping").and_then(|g| g.attr("val")).unwrap_or("");
    let stacked = kind == ChartKind::Bar && matches!(grouping, "stacked" | "percentStacked");
    // Labels shown for the chart group, or for every series of it.
    // A DrawingML boolean (`CT_Boolean`) with no `val` is true.
    let on = |e: &El| e.attr("val").is_none_or(|v| v == "1" || v == "true");
    let shows = |d: Option<&El>| d.and_then(|d| d.child("showVal")).is_some_and(on);
    let labels = shows(group.child("dLbls")) || (!sers.is_empty() && sers.iter().all(|s| shows(s.child("dLbls"))));
    let hide_value_axis = kind != ChartKind::Pie
        && plot
            .children
            .iter()
            .filter(|c| c.local == "valAx")
            .any(|ax| ax.child("delete").is_some_and(on))
        && kind != ChartKind::Scatter;
    let horizontal = kind == ChartKind::Bar && group.child("barDir").and_then(|d| d.attr("val")) == Some("bar");
    let reversed = kind != ChartKind::Pie
        && plot
            .children
            .iter()
            .filter(|c| c.local == "catAx")
            .any(|ax| ax.child("scaling").and_then(|s| s.child("orientation")).and_then(|o| o.attr("val")) == Some("maxMin"));
    // A percent format on the labels, or on the values the labels show.
    let pct = |code: Option<&str>| code.is_some_and(|c| c.contains('%'));
    let label_fmt = |d: Option<&El>| pct(d.and_then(|d| d.child("numFmt")).and_then(|f| f.attr("formatCode")));
    let value_fmt = ser.child("val").and_then(|v| v.find("formatCode")).map(|f| f.deep_text());
    let percent = labels && (label_fmt(group.child("dLbls")) || sers.iter().any(|s| label_fmt(s.child("dLbls"))) || pct(value_fmt.as_deref()));
    // The chart's text: each property from the first `c:txPr` that states
    // it, the chart's own, then its axes', then its data labels'.
    let axis = |name: &str| plot.children.iter().find(|c| c.local == name).and_then(|ax| ax.child("txPr"));
    let def_rprs: Vec<&El> = [
        root.find("chartSpace").unwrap_or(&root).child("txPr"),
        axis("catAx"),
        axis("valAx"),
        group.child("dLbls").and_then(|d| d.child("txPr")),
        ser.child("dLbls").and_then(|d| d.child("txPr")),
    ]
    .into_iter()
    .flatten()
    .filter_map(|t| t.find("defRPr"))
    .collect();
    let text_color = def_rprs
        .iter()
        .find_map(|d| d.child("solidFill").and_then(|f| f.child("srgbClr")).and_then(|c| c.attr("val")).and_then(hex_rgb));
    let text_size = def_rprs.iter().find_map(|d| d.attr("sz").and_then(|v| v.parse::<u32>().ok()).filter(|v| (100..=40000).contains(v)));
    let mut c = ChartData {
        kind,
        series,
        points,
        more,
        stacked,
        labels,
        hide_value_axis,
        hole,
        point_colors,
        no_legend: false,
        transparent,
        horizontal,
        reversed,
        percent,
        text_color,
        text_size,
        series_colors: {
            // A series' colour: its points' own, when every point states the
            // same one (a one-category stacked bar colours each segment so),
            // else its own fill.
            let color = |s: &El| {
                let points: Vec<Option<[u8; 3]>> = (0..n)
                    .map(|i| {
                        s.children
                            .iter()
                            .filter(|c| c.local == "dPt")
                            .find(|d| d.child("idx").and_then(|x| x.attr("val")).and_then(|v| v.parse::<usize>().ok()) == Some(i))
                            .and_then(|d| rgb_of(d.child("spPr")))
                    })
                    .collect();
                match points.first().copied().flatten() {
                    Some(c) if points.iter().all(|p| *p == Some(c)) => Some(c),
                    _ => rgb_of(s.child("spPr")),
                }
            };
            let colors: Vec<Option<[u8; 3]>> = sers.iter().take(MAX_SERIES).map(|s| color(s)).collect();
            if colors.iter().any(Option::is_some) { colors } else { Vec::new() }
        },
        gap: (kind == ChartKind::Bar).then(|| group.child("gapWidth").and_then(|g| g.attr("val")).and_then(|v| v.parse::<u16>().ok())).flatten().map(|g| g.min(500)).filter(|g| *g != 150),
        hide_category_axis: kind != ChartKind::Pie
            && plot.children.iter().filter(|c| c.local == "catAx").any(|ax| ax.child("delete").is_some_and(on)),
    };
    c.no_legend = !has_legend && c.has_default_legend();
    Some(c)
}

// ── odp: the embedded ODF chart ──────────────────────────────────────────

pub const ODF_CHART_MEDIA_TYPE: &str = "application/vnd.oasis.opendocument.chart";

fn odf_class(kind: ChartKind) -> &'static str {
    match kind {
        ChartKind::Bar => "chart:bar",
        ChartKind::Line => "chart:line",
        ChartKind::Pie => "chart:circle",
        ChartKind::Scatter => "chart:scatter",
        ChartKind::Area => "chart:area",
    }
}

const ODF_NS: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
     xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
     xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
     xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
     xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" \
     xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
     xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
     xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" \
     xmlns:chart=\"urn:oasis:names:tc:opendocument:xmlns:chart:1.0\" \
     xmlns:number=\"urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0\"";

/// An embedded chart object's `content.xml`: the chart, `w_pt` x `h_pt`,
/// and its data as the object's local table (categories in column A, the
/// series in B under its name). The table is the chart's last child, as
/// ODF places it: beside `chart:chart` instead, Impress drew the chart's
/// kind with no data at all (the oracle's odp rewrites).
pub fn odf_chart_content_xml(chart: &ChartData, w_pt: f64, h_pt: f64) -> String {
    let n = chart.points.len();
    let last = n + 1;
    let class = if chart.kind == ChartKind::Pie && chart.hole > 0 { "chart:ring" } else { odf_class(chart.kind) };
    let range = |c: usize| format!("local-table.${}$2:.${}${last}", col(c), col(c));
    let axes = match chart.kind {
        ChartKind::Pie => String::new(),
        ChartKind::Scatter => "<chart:axis chart:dimension=\"x\" chart:name=\"primary-x\"/>\
             <chart:axis chart:dimension=\"y\" chart:name=\"primary-y\"><chart:grid chart:class=\"major\"/></chart:axis>"
            .to_string(),
        _ => format!(
            "<chart:axis chart:dimension=\"x\" chart:name=\"primary-x\">\
             <chart:categories table:cell-range-address=\"{}\"/></chart:axis>\
             <chart:axis chart:dimension=\"y\" chart:name=\"primary-y\"{}><chart:grid chart:class=\"major\"/></chart:axis>",
            range(0),
            if chart.hide_value_axis { " chart:style-name=\"axis-hidden\"" } else { "" }
        ),
    };
    let domain = if chart.kind == ChartKind::Scatter {
        format!("<chart:domain table:cell-range-address=\"{}\"/>", range(0))
    } else {
        String::new()
    };
    let legend = if chart.shows_legend() { "<chart:legend chart:legend-position=\"end\"/>" } else { "" };
    // A slice's colour: one data point each, styled with its fill.
    let point_colors: &[Option<[u8; 3]>] = if chart.kind == ChartKind::Pie { &chart.point_colors } else { &[] };
    let data_points: String = if point_colors.iter().any(Option::is_some) {
        (0..chart.points.len())
            .map(|i| match point_colors.get(i).copied().flatten() {
                Some(_) => format!("<chart:data-point chart:style-name=\"point-{i}\"/>"),
                None => "<chart:data-point/>".to_string(),
            })
            .collect()
    } else {
        String::new()
    };
    let numeric_x = (chart.kind == ChartKind::Scatter).then(|| chart.numeric_categories()).flatten();
    let more = if chart.kind == ChartKind::Scatter { &[][..] } else { &chart.more[..] };
    let series_style = if chart.labels { " chart:style-name=\"series-labelled\"" } else { "" };
    let extra_series: String = (0..more.len())
        .map(|k| {
            format!(
                "<chart:series chart:class=\"{class}\" chart:values-cell-range-address=\"{}\" \
                 chart:label-cell-address=\"local-table.${}$1\"{series_style}/>",
                range(k + 2),
                col(k + 2)
            )
        })
        .collect();
    let extra_columns = "<table:table-column/>".repeat(more.len());
    let extra_names: String = more
        .iter()
        .map(|(name, _)| format!("<table:table-cell office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>", esc(name)))
        .collect();
    let mut styles = String::new();
    if chart.stacked && chart.kind == ChartKind::Bar {
        styles.push_str("<style:style style:name=\"plot\" style:family=\"chart\"><style:chart-properties chart:stacked=\"true\"/></style:style>");
    }
    if chart.labels {
        styles.push_str(
            "<style:style style:name=\"series-labelled\" style:family=\"chart\"><style:chart-properties chart:data-label-number=\"value\"/></style:style>",
        );
    }
    for (i, c) in point_colors.iter().enumerate() {
        if let Some([r, g, b]) = c {
            styles.push_str(&format!(
                "<style:style style:name=\"point-{i}\" style:family=\"chart\">\
                 <style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"#{r:02x}{g:02x}{b:02x}\"/></style:style>"
            ));
        }
    }
    if chart.transparent {
        styles.push_str(
            "<style:style style:name=\"chart-clear\" style:family=\"chart\"><style:graphic-properties draw:fill=\"none\" draw:stroke=\"none\"/></style:style>",
        );
    }
    if chart.hide_value_axis {
        styles.push_str(
            "<style:style style:name=\"axis-hidden\" style:family=\"chart\"><style:chart-properties chart:display-label=\"false\"/></style:style>",
        );
    }
    let plot_style = if chart.stacked && chart.kind == ChartKind::Bar { " chart:style-name=\"plot\"" } else { "" };
    let last_col = col(1 + more.len());
    let mut rows = String::new();
    for (i, (cat, v)) in chart.points.iter().enumerate() {
        let first = match &numeric_x {
            Some(xs) => format!(
                "<table:table-cell office:value-type=\"float\" office:value=\"{}\"><text:p>{}</text:p></table:table-cell>",
                num(xs[i]),
                esc(cat)
            ),
            None => format!("<table:table-cell office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>", esc(cat)),
        };
        let cell = |v: f64| {
            format!(
                "<table:table-cell office:value-type=\"float\" office:value=\"{v}\"><text:p>{v}</text:p></table:table-cell>",
                v = num(v)
            )
        };
        let extra: String = more.iter().map(|(_, values)| cell(values.get(i).copied().unwrap_or(0.0))).collect();
        rows.push_str(&format!("<table:table-row>{first}{}{extra}</table:table-row>", cell(*v)));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <office:document-content {ODF_NS} office:version=\"1.2\"><office:automatic-styles>{styles}</office:automatic-styles>\
         <office:body><office:chart>\
         <chart:chart svg:width=\"{w_pt}pt\" svg:height=\"{h_pt}pt\" chart:class=\"{class}\"{clear}>{legend}\
         <chart:plot-area{plot_style} table:cell-range-address=\"local-table.$A$1:.${last_col}${last}\" chart:data-source-has-labels=\"both\">\
         {axes}<chart:series chart:class=\"{class}\" chart:values-cell-range-address=\"{}\" \
         chart:label-cell-address=\"local-table.$B$1\"{series_style}>{domain}{data_points}</chart:series>{extra_series}</chart:plot-area>\
         <table:table table:name=\"local-table\">\
         <table:table-header-columns><table:table-column/></table:table-header-columns>\
         <table:table-columns><table:table-column/>{extra_columns}</table:table-columns>\
         <table:table-header-rows><table:table-row><table:table-cell><text:p/></table:table-cell>\
         <table:table-cell office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>{extra_names}</table:table-row></table:table-header-rows>\
         <table:table-rows>{rows}</table:table-rows></table:table></chart:chart>\
         </office:chart></office:body></office:document-content>",
        range(1),
        esc(&chart.series),
        clear = if chart.transparent { " chart:style-name=\"chart-clear\"" } else { "" },
    )
}

/// An embedded chart object's `styles.xml`: none of its own.
pub fn odf_chart_styles_xml() -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-styles {ODF_NS} office:version=\"1.2\"/>")
}

/// The 0-based column a cell address in a chart's local table names:
/// "local-table.$B$2:.$B$5" is 1.
fn column_of(address: &str) -> Option<usize> {
    let cell = address.split(':').next()?;
    let cell = cell.rsplit('.').next()?;
    let letter = cell.trim_start_matches('$').chars().next()?.to_ascii_uppercase();
    letter.is_ascii_uppercase().then(|| (letter as u8 - b'A') as usize)
}

/// The chart an embedded ODF chart object's `content.xml` draws, as its
/// first series, with its data from the object's local table.
pub fn parse_odf_chart(xml: &str) -> Option<ChartData> {
    let root = tree(xml);
    let chart = root.find_q("chart:chart")?;
    let class = chart.attr("chart:class").map(local_of).unwrap_or("");
    let kind = match class {
        "bar" => ChartKind::Bar,
        "line" => ChartKind::Line,
        "circle" | "ring" => ChartKind::Pie,
        "area" => ChartKind::Area,
        "scatter" => ChartKind::Scatter,
        _ => return None,
    };
    let all_series: Vec<&El> = {
        let mut v = Vec::new();
        if let Some(plot) = chart.find_q("chart:plot-area") {
            v.extend(plot.children.iter().filter(|c| c.name == "chart:series"));
        }
        v
    };
    let series = all_series.first().copied().or_else(|| chart.find_q("chart:series"));
    let val_col = series
        .and_then(|s| s.attr("chart:values-cell-range-address"))
        .and_then(column_of)
        .unwrap_or(1);
    let cat_col = series
        .and_then(|s| s.child_q("chart:domain"))
        .and_then(|d| d.attr("table:cell-range-address"))
        .or_else(|| chart.find_q("chart:categories").and_then(|c| c.attr("table:cell-range-address")))
        .and_then(column_of)
        .unwrap_or(0);
    let table = root.find_q("table:table")?;
    let cells = |row: &El| -> Vec<(String, Option<f64>)> {
        row.children
            .iter()
            .filter(|c| c.name == "table:table-cell")
            .flat_map(|c| {
                let repeat = c
                    .attr("table:number-columns-repeated")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, 64);
                let text: String = c.children.iter().filter(|p| p.name == "text:p").map(El::deep_text).collect::<Vec<_>>().join("\n");
                let value = c.attr_local("value").and_then(|v| v.parse::<f64>().ok()).or_else(|| text.trim().parse().ok());
                std::iter::repeat_n((text, value), repeat)
            })
            .collect()
    };
    let header = table.child_q("table:table-header-rows").and_then(|h| h.child_q("table:table-row"));
    let series_name = header.map(cells).and_then(|c| c.get(val_col).map(|c| c.0.clone())).unwrap_or_default();
    let mut rows = Vec::new();
    if let Some(body) = table.child_q("table:table-rows") {
        body.all_q("table:table-row", &mut rows);
    }
    rows.extend(table.children.iter().filter(|c| c.name == "table:table-row"));
    let body: Vec<Vec<(String, Option<f64>)>> = rows.into_iter().take(10_000).map(cells).collect();
    let data_rows: Vec<&Vec<(String, Option<f64>)>> = body.iter().filter(|c| c.get(val_col).is_some_and(|v| v.1.is_some())).collect();
    let points = data_rows
        .iter()
        .map(|c| {
            let value = c[val_col].1.unwrap_or(0.0);
            let cat = c.get(cat_col).map(|c| c.0.clone()).unwrap_or_default();
            (cat, if value.is_finite() { value } else { 0.0 })
        })
        .collect();
    // Automatic chart styles by name: their chart-properties.
    let style_prop = |name: Option<&str>, prop: &str| -> Option<String> {
        let name = name?;
        let styles = root.find_q("office:automatic-styles")?;
        let style = styles.children.iter().find(|st| st.name == "style:style" && st.attr("style:name") == Some(name))?;
        style.child_q("style:chart-properties")?.attr(prop).map(str::to_string)
    };
    let header_cells = header.map(cells).unwrap_or_default();
    let more = if kind == ChartKind::Scatter || kind == ChartKind::Pie {
        Vec::new()
    } else {
        all_series
            .iter()
            .skip(1)
            .take(MAX_SERIES - 1)
            .filter_map(|s| {
                let c = s.attr("chart:values-cell-range-address").and_then(column_of)?;
                let name = header_cells.get(c).map(|h| h.0.clone()).unwrap_or_default();
                Some((name, data_rows.iter().map(|r| r.get(c).and_then(|v| v.1).filter(|v| v.is_finite()).unwrap_or(0.0)).collect()))
            })
            .collect()
    };
    let plot_style = chart.find_q("chart:plot-area").and_then(|p| p.attr("chart:style-name"));
    let stacked = kind == ChartKind::Bar
        && (style_prop(plot_style, "chart:stacked").as_deref() == Some("true")
            || style_prop(plot_style, "chart:percentage").as_deref() == Some("true"));
    let labels = !all_series.is_empty()
        && all_series.iter().all(|s| {
            style_prop(s.attr("chart:style-name"), "chart:data-label-number").is_some_and(|v| v != "none")
        });
    let hide_value_axis = matches!(kind, ChartKind::Bar | ChartKind::Line | ChartKind::Area)
        && chart
            .find_q("chart:plot-area")
            .map(|p| p.children.iter().filter(|c| c.name == "chart:axis" && c.attr("chart:dimension") == Some("y")).collect::<Vec<_>>())
            .unwrap_or_default()
            .iter()
            .any(|ax| style_prop(ax.attr("chart:style-name"), "chart:display-label").as_deref() == Some("false"));
    // Impress writes no hole size a reader of ODF 1.2 keeps; a ring is
    // drawn with LibreOffice's default hole, half the radius.
    let hole = if class == "ring" { 50 } else { 0 };
    let style_graphic = |name: Option<&str>, prop: &str| -> Option<String> {
        let name = name?;
        let styles = root.find_q("office:automatic-styles")?;
        let style = styles.children.iter().find(|st| st.name == "style:style" && st.attr("style:name") == Some(name))?;
        style.child_q("style:graphic-properties")?.attr(prop).map(str::to_string)
    };
    let point_colors: Vec<Option<[u8; 3]>> = if kind == ChartKind::Pie {
        let colors: Vec<Option<[u8; 3]>> = series
            .map(|s| {
                s.children
                    .iter()
                    .filter(|c| c.name == "chart:data-point")
                    .flat_map(|dp| {
                        let repeat = dp.attr("chart:repeated").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1, 10_000);
                        let color = style_graphic(dp.attr("chart:style-name"), "draw:fill-color").as_deref().and_then(hex_rgb);
                        std::iter::repeat_n(color, repeat)
                    })
                    .take(10_000)
                    .collect()
            })
            .unwrap_or_default();
        if colors.iter().any(Option::is_some) { colors } else { Vec::new() }
    } else {
        Vec::new()
    };
    let transparent = style_graphic(chart.attr("chart:style-name"), "draw:fill").as_deref() == Some("none");
    let has_legend = chart.child_q("chart:legend").is_some();
    let mut c = ChartData {
        kind,
        series: series_name,
        points,
        more,
        stacked,
        labels,
        hide_value_axis,
        hole,
        point_colors,
        no_legend: false,
        transparent,
        horizontal: false,
        reversed: false,
        percent: false,
        text_color: None,
        text_size: None,
        series_colors: Vec::new(),
        gap: None,
        hide_category_axis: false,
    };
    c.no_legend = !has_legend && c.has_default_legend();
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [ChartKind; 5] = [ChartKind::Bar, ChartKind::Line, ChartKind::Pie, ChartKind::Scatter, ChartKind::Area];

    /// The children's nature survey's side chart, reduced: horizontal bars
    /// (first category at the top), white bars and text, percent labels.
    const HORIZONTAL: &str = r#"<c:chartSpace xmlns:c="c" xmlns:a="a"><c:chart><c:plotArea><c:barChart><c:barDir val="bar"/><c:grouping val="clustered"/>
<c:ser><c:idx val="0"/><c:tx><c:v>Series 1</c:v></c:tx><c:spPr><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></c:spPr>
<c:dLbls><c:numFmt formatCode="0%" sourceLinked="0"/><c:showVal val="1"/></c:dLbls>
<c:cat><c:strLit><c:pt idx="0"><c:v>Garden</c:v></c:pt><c:pt idx="1"><c:v>Park</c:v></c:pt></c:strLit></c:cat>
<c:val><c:numLit><c:formatCode>0%</c:formatCode><c:pt idx="0"><c:v>0.81</c:v></c:pt><c:pt idx="1"><c:v>0.66</c:v></c:pt></c:numLit></c:val></c:ser>
</c:barChart><c:catAx><c:scaling><c:orientation val="maxMin"/></c:scaling><c:txPr><a:bodyPr/><a:p><a:pPr><a:defRPr sz="900" b="1"><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:defRPr></a:pPr></a:p></c:txPr></c:catAx><c:valAx/></c:plotArea></c:chart>
<c:txPr><a:bodyPr/><a:p><a:pPr><a:defRPr/></a:pPr></a:p></c:txPr></c:chartSpace>"#;

    /// An area or line chart keeps a reversed, deleted category axis on
    /// save, as a bar chart does: the reader sets both for every kind with
    /// axes.
    #[test]
    fn an_area_charts_reversed_hidden_axis_round_trips() {
        for kind in [ChartKind::Area, ChartKind::Line] {
            let mut c = ChartData::sample(kind);
            c.reversed = true;
            c.hide_category_axis = true;
            let back = parse_chart_space(&chart_space_xml(&c)).expect("its own part");
            assert!(back.reversed && back.hide_category_axis, "{kind:?}: {back:?}");
        }
    }

    #[test]
    fn a_horizontal_percent_bar_chart_reads_and_round_trips() {
        let c = parse_chart_space(HORIZONTAL).expect("a bar chart");
        assert!(c.horizontal && c.reversed && c.percent && c.labels, "{c:?}");
        assert_eq!(c.point_colors, vec![Some([255, 255, 255]); 2], "a one-series bar chart's fill colours its bars");
        assert_eq!((c.text_color, c.text_size), (Some([255, 255, 255]), Some(900)), "the axis's text, past the chart's empty txPr");
        let back = parse_chart_space(&chart_space_xml(&c)).expect("its own part");
        assert_eq!(back, c, "a save keeps it");
    }

    /// The adults' nature survey's distance bar, reduced: one category,
    /// four series stacked to 100% with no gap and no axes, each segment
    /// coloured on its one point.
    const STACKED_SEGMENTS: &str = r#"<c:chartSpace xmlns:c="c" xmlns:a="a"><c:chart><c:plotArea><c:barChart><c:barDir val="bar"/><c:grouping val="percentStacked"/>
<c:ser><c:idx val="0"/><c:tx><c:v>A</c:v></c:tx><c:spPr><a:solidFill><a:srgbClr val="111111"/></a:solidFill></c:spPr><c:dPt><c:idx val="0"/><c:spPr><a:solidFill><a:srgbClr val="7F4F94"/></a:solidFill></c:spPr></c:dPt>
<c:cat><c:strLit><c:pt idx="0"><c:v>Response</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>0.41</c:v></c:pt></c:numLit></c:val></c:ser>
<c:ser><c:idx val="1"/><c:tx><c:v>B</c:v></c:tx><c:spPr><a:solidFill><a:srgbClr val="8D9132"/></a:solidFill></c:spPr>
<c:cat><c:strLit><c:pt idx="0"><c:v>Response</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>0.59</c:v></c:pt></c:numLit></c:val></c:ser>
<c:gapWidth val="0"/><c:overlap val="100"/></c:barChart><c:catAx><c:delete val="1"/></c:catAx><c:valAx><c:delete val="1"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;

    #[test]
    fn stacked_segments_keep_their_colours_gap_and_hidden_axes() {
        let c = parse_chart_space(STACKED_SEGMENTS).expect("a bar chart");
        assert!(c.horizontal && c.stacked && c.hide_category_axis && c.hide_value_axis, "{c:?}");
        assert_eq!(c.series_colors, vec![Some([0x7F, 0x4F, 0x94]), Some([0x8D, 0x91, 0x32])], "a point's own colour, else the series' fill");
        assert_eq!(c.gap, Some(0));
        let back = parse_chart_space(&chart_space_xml(&c)).expect("its own part");
        assert_eq!((back.series_colors, back.gap, back.hide_category_axis), (c.series_colors.clone(), Some(0), true), "a save keeps them");
    }

    #[test]
    fn a_column_chart_is_not_horizontal() {
        let c = parse_chart_space(&chart_space_xml(&ChartData::sample(ChartKind::Bar))).unwrap();
        assert!(!c.horizontal && !c.reversed && !c.percent);
    }

    #[test]
    fn every_kind_reads_back_from_its_drawingml_part() {
        for kind in KINDS {
            let mut c = ChartData::sample(kind);
            c.series = "Sales & <costs>".into();
            let xml = chart_space_xml(&c);
            assert_eq!(parse_chart_space(&xml), Some(c.clone()), "{kind:?}: {xml}");
        }
    }

    /// Two series, stacked, labelled, with the value axis hidden: the
    /// charts of the pay-centre deck in the real corpus (#2327).
    fn stacked_pair() -> ChartData {
        let mut c = ChartData::one(
            ChartKind::Bar,
            " $48/case ",
            vec![("May".into(), 951.0), ("June".into(), 3623.0), ("July".into(), 5739.0)],
        );
        c.more = vec![(" $418/case ".into(), vec![123.0, 587.0, 814.0])];
        c.stacked = true;
        c.labels = true;
        c.hide_value_axis = true;
        c
    }

    #[test]
    fn several_stacked_labelled_series_read_back_from_drawingml() {
        let c = stacked_pair();
        let xml = chart_space_xml(&c);
        assert!(xml.contains("<c:grouping val=\"stacked\"/>") && xml.contains("<c:showVal val=\"1\"/>"), "{xml}");
        assert!(xml.contains("<c:valAx><c:axId val=\"222222222\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"1\"/>"), "{xml}");
        assert_eq!(parse_chart_space(&xml), Some(c), "{xml}");
    }

    #[test]
    fn several_stacked_labelled_series_read_back_from_odf() {
        let c = stacked_pair();
        let xml = odf_chart_content_xml(&c, 480.0, 300.0);
        assert_eq!(parse_odf_chart(&xml), Some(c), "{xml}");
    }

    #[test]
    fn powerpoints_stacked_chart_keeps_both_series() {
        // As the pay-centre deck writes it: series-level labels, a deleted
        // value axis, the second series' categories left to the first's.
        let xml = "<c:chartSpace><c:chart><c:plotArea><c:barChart><c:barDir val=\"col\"/>\
            <c:grouping val=\"stacked\"/>\
            <c:ser><c:idx val=\"0\"/><c:tx><c:v>A</c:v></c:tx><c:dLbls><c:showVal val=\"1\"/></c:dLbls>\
            <c:cat><c:strLit><c:pt idx=\"0\"><c:v>May</c:v></c:pt><c:pt idx=\"1\"><c:v>June</c:v></c:pt></c:strLit></c:cat>\
            <c:val><c:numLit><c:pt idx=\"0\"><c:v>1</c:v></c:pt><c:pt idx=\"1\"><c:v>2</c:v></c:pt></c:numLit></c:val></c:ser>\
            <c:ser><c:idx val=\"1\"/><c:tx><c:v>B</c:v></c:tx><c:dLbls><c:showVal val=\"1\"/></c:dLbls>\
            <c:val><c:numLit><c:pt idx=\"1\"><c:v>5</c:v></c:pt></c:numLit></c:val></c:ser>\
            <c:overlap val=\"100\"/></c:barChart><c:catAx/><c:valAx><c:delete val=\"1\"/></c:valAx>\
            </c:plotArea></c:chart></c:chartSpace>";
        let c = parse_chart_space(xml).unwrap();
        assert_eq!(c.points, vec![("May".to_string(), 1.0), ("June".to_string(), 2.0)]);
        assert_eq!(c.more, vec![("B".to_string(), vec![0.0, 5.0])]);
        assert!(c.stacked && c.labels && c.hide_value_axis);
    }

    /// A doughnut with its slices coloured by the file, labelled, no
    /// legend and no background: the aiva deck's chart (render parity
    /// Phase 3).
    fn coloured_ring(hole: u8) -> ChartData {
        let mut c = ChartData::one(ChartKind::Pie, "Intake", vec![("Completed".into(), 7984.0), ("Forecast".into(), 152016.0)]);
        c.hole = hole;
        c.point_colors = vec![Some([0xd9, 0xe7, 0xf3]), Some([0x00, 0xb0, 0xf0])];
        c.labels = true;
        c.no_legend = true;
        c.transparent = true;
        c
    }

    #[test]
    fn a_coloured_doughnut_reads_back_from_drawingml() {
        let c = coloured_ring(55);
        let xml = chart_space_xml(&c);
        assert!(xml.contains("<c:holeSize val=\"55\"/>") && xml.contains("<a:srgbClr val=\"00B0F0\"/>") && !xml.contains("<c:legend>"), "{xml}");
        assert_eq!(parse_chart_space(&xml), Some(c), "{xml}");
    }

    #[test]
    fn a_coloured_ring_reads_back_from_odf() {
        // ODF keeps no hole size; a ring reads back with the default half.
        let c = coloured_ring(50);
        let xml = odf_chart_content_xml(&c, 480.0, 300.0);
        assert_eq!(parse_odf_chart(&xml), Some(c), "{xml}");
    }

    #[test]
    fn a_chart_without_its_default_legend_says_so() {
        // A pie draws its categories' legend unless the file has none.
        let with = chart_space_xml(&ChartData::sample(ChartKind::Pie));
        assert!(!parse_chart_space(&with).unwrap().no_legend);
        let without = with.replace("<c:legend><c:legendPos val=\"r\"/><c:overlay val=\"0\"/></c:legend>", "");
        assert!(parse_chart_space(&without).unwrap().no_legend);
    }

    #[test]
    fn adding_and_removing_points_keeps_every_series_aligned() {
        let mut c = stacked_pair();
        assert!(c.apply(&ChartEdit::AddPoint));
        assert_eq!(c.more[0].1, vec![123.0, 587.0, 814.0, 814.0]);
        assert!(c.apply(&ChartEdit::RemovePoint(0)));
        assert_eq!(c.more[0].1, vec![587.0, 814.0, 814.0]);
        assert_eq!(c.points.len(), 3);
    }

    #[test]
    fn every_kind_reads_back_from_its_odf_object() {
        for kind in KINDS {
            let mut c = ChartData::sample(kind);
            c.series = "Sales & <costs>".into();
            let xml = odf_chart_content_xml(&c, 480.0, 300.0);
            assert_eq!(parse_odf_chart(&xml), Some(c.clone()), "{kind:?}: {xml}");
        }
    }

    /// PowerPoint's own part: a default prefix-less namespace would read
    /// the same, and a second series is left for the first.
    #[test]
    fn a_powerpoint_part_with_two_series_reads_as_its_first() {
        let xml = r#"<chartSpace xmlns="http://schemas.openxmlformats.org/drawingml/2006/chart"><chart><plotArea><layout/>
          <barChart><barDir val="bar"/><grouping val="clustered"/>
          <ser><idx val="0"/><order val="0"/><tx><strRef><f>Sheet1!$B$1</f><strCache><ptCount val="1"/><pt idx="0"><v>Series 1</v></pt></strCache></strRef></tx>
          <cat><strRef><f>Sheet1!$A$2:$A$4</f><strCache><ptCount val="3"/><pt idx="0"><v>Category 1</v></pt><pt idx="2"><v>Category 3</v></pt></strCache></strRef></cat>
          <val><numRef><f>Sheet1!$B$2:$B$4</f><numCache><formatCode>General</formatCode><ptCount val="3"/><pt idx="0"><v>4.3</v></pt><pt idx="1"><v>2.5</v></pt><pt idx="2"><v>3.5</v></pt></numCache></numRef></val></ser>
          <ser><idx val="1"/><order val="1"/><val><numLit><ptCount val="1"/><pt idx="0"><v>9</v></pt></numLit></val></ser>
          </barChart></plotArea></chart></chartSpace>"#;
        let c = parse_chart_space(xml).unwrap();
        assert_eq!(c.kind, ChartKind::Bar);
        assert_eq!(c.series, "Series 1");
        assert_eq!(
            c.points,
            vec![("Category 1".into(), 4.3), ("2".into(), 2.5), ("Category 3".into(), 3.5)]
        );
    }

    /// Impress's local table: a header row with an empty corner, the
    /// series in the column its values address names.
    #[test]
    fn an_impress_object_reads_its_series_from_the_addressed_column() {
        let xml = r#"<office:document-content xmlns:office="o" xmlns:chart="c" xmlns:table="t" xmlns:text="x"><office:body><office:chart>
          <chart:chart chart:class="chart:line"><chart:plot-area>
          <chart:series chart:values-cell-range-address="local-table.$C$2:.$C$3" chart:label-cell-address="local-table.$C$1"/>
          </chart:plot-area></chart:chart>
          <table:table table:name="local-table">
          <table:table-header-rows><table:table-row><table:table-cell/><table:table-cell office:value-type="string"><text:p>A</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>B</text:p></table:table-cell></table:table-row></table:table-header-rows>
          <table:table-rows>
          <table:table-row><table:table-cell office:value-type="string"><text:p>Mon</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="1"><text:p>1</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="7.5"><text:p>7.5</text:p></table:table-cell></table:table-row>
          <table:table-row><table:table-cell office:value-type="string"><text:p>Tue</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="2"><text:p>2</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="8"><text:p>8</text:p></table:table-cell></table:table-row>
          </table:table-rows></table:table></office:chart></office:body></office:document-content>"#;
        let c = parse_odf_chart(xml).unwrap();
        assert_eq!(c.kind, ChartKind::Line);
        assert_eq!(c.series, "B");
        assert_eq!(c.points, vec![("Mon".into(), 7.5), ("Tue".into(), 8.0)]);
    }

    #[test]
    fn edits_change_the_data_and_say_whether_they_did() {
        let mut c = ChartData::sample(ChartKind::Bar);
        assert!(c.apply(&ChartEdit::Kind(ChartKind::Line)));
        assert!(!c.apply(&ChartEdit::Kind(ChartKind::Line)), "already a line chart");
        assert!(c.apply(&ChartEdit::Series("  Revenue ".into())));
        assert_eq!(c.series, "Revenue");
        assert!(c.apply(&ChartEdit::Category(0, "Jan".into())));
        assert!(c.apply(&ChartEdit::Value(3, -2.5)));
        assert!(!c.apply(&ChartEdit::Value(9, 1.0)), "no such point");
        assert!(!c.apply(&ChartEdit::Value(0, f64::NAN)), "not a number");
        assert_eq!(c.points[0].0, "Jan");
        assert_eq!(c.points[3].1, -2.5);
        assert!(c.apply(&ChartEdit::AddPoint));
        assert_eq!(c.points[4], ("Q5".into(), -2.5), "the next category, the last value");
        assert!(c.apply(&ChartEdit::RemovePoint(0)));
        assert_eq!(c.points.len(), 4);
        for _ in 0..10 {
            c.apply(&ChartEdit::RemovePoint(0));
        }
        assert_eq!(c.points.len(), 1, "a chart keeps one point");
        for _ in 0..(MAX_POINTS + 5) {
            c.apply(&ChartEdit::AddPoint);
        }
        assert_eq!(c.points.len(), MAX_POINTS);
    }

    #[test]
    fn a_new_point_continues_the_categories() {
        let next = |cats: &[&str]| {
            let mut c = ChartData::one(ChartKind::Bar, "", cats.iter().map(|s| (s.to_string(), 1.0)).collect());
            c.apply(&ChartEdit::AddPoint);
            c.points.last().unwrap().0.clone()
        };
        assert_eq!(next(&["Q1", "Q2"]), "Q3");
        assert_eq!(next(&["2024", "2025"]), "2026");
        assert_eq!(next(&["0.5", "1"]), "1.5");
        assert_eq!(next(&["Apples", "Pears"]), "Category 3");
        assert_eq!(next(&[]), "1");
    }

    #[test]
    fn a_chart_is_described_by_its_kind_series_and_size() {
        assert_eq!(ChartData::sample(ChartKind::Bar).describe(), "Bar chart: Sales, 4 values");
        let c = ChartData::one(ChartKind::Pie, "", vec![("a".into(), 1.0)]);
        assert_eq!(c.describe(), "Pie chart, 1 value");
    }

    #[test]
    fn a_hostile_point_count_allocates_only_what_is_there() {
        let xml = r#"<c:chartSpace xmlns:c="c"><c:chart><c:plotArea><c:pieChart><c:ser>
          <c:val><c:numLit><c:ptCount val="4000000000"/><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val>
          </c:ser></c:pieChart></c:plotArea></c:chart></c:chartSpace>"#;
        assert_eq!(parse_chart_space(xml).unwrap().points, vec![("1".into(), 1.0)]);
    }
}
