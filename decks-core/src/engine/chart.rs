//! engine/chart.rs — a chart on a slide: its kind, categories and series,
//! and the parts that carry it in each format. GTK-free; drawn by
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
//! A chart is its categories (a scatter chart's x values) and one or more
//! series of a value per category, as PowerPoint's data sheet holds them:
//! categories down column A, a series per column after it.

pub use suite_common_core::charts::ChartKind;

/// One series: its name (its legend entry; empty for none) and a value
/// per category.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChartSeries {
    pub name: String,
    pub values: Vec<f64>,
}

/// A chart's kind, categories and series. Every series has a value per
/// category (`normalized`).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChartData {
    pub kind: ChartKind,
    pub categories: Vec<String>,
    pub series: Vec<ChartSeries>,
}

/// The most categories a chart is given, as many as a Tables chart draws
/// legibly.
pub const MAX_POINTS: usize = 100;
/// The most series: twice the theme's six accents.
pub const MAX_SERIES: usize = 12;
/// What a reader keeps of a file's chart at most, whatever it claims.
const READ_POINTS: usize = 10_000;
const READ_SERIES: usize = 64;

impl ChartData {
    /// A new chart's data: PowerPoint's own sample series, four categories
    /// of one series.
    pub fn sample(kind: ChartKind) -> Self {
        let (categories, values) = if kind == ChartKind::Scatter {
            (["1", "2", "3", "4"], vec![2.7, 3.2, 0.8, 4.5])
        } else {
            (["Q1", "Q2", "Q3", "Q4"], vec![4.3, 2.5, 3.5, 4.5])
        };
        ChartData {
            kind,
            categories: categories.iter().map(|c| c.to_string()).collect(),
            series: vec![ChartSeries { name: "Sales".into(), values }],
        }
    }

    /// A one-series chart from (category, value) points.
    pub fn from_points(kind: ChartKind, name: &str, points: &[(&str, f64)]) -> Self {
        ChartData {
            kind,
            categories: points.iter().map(|p| p.0.to_string()).collect(),
            series: vec![ChartSeries { name: name.into(), values: points.iter().map(|p| p.1).collect() }],
        }
    }

    /// Every series a value per category (0 where it has none), none past
    /// the last, non-finite values 0, and at least one series.
    pub fn normalized(mut self) -> Self {
        let n = self.categories.len();
        if self.series.is_empty() {
            self.series.push(ChartSeries { name: String::new(), values: Vec::new() });
        }
        for s in &mut self.series {
            s.values.resize(n, 0.0);
            for v in &mut s.values {
                if !v.is_finite() {
                    *v = 0.0;
                }
            }
        }
        self
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

    /// "Bar chart: Sales, 4 values", or with several series "Bar chart:
    /// North and South, 4 values each".
    pub fn describe(&self) -> String {
        let n = self.categories.len();
        let values = if n == 1 { "1 value".to_string() } else { format!("{n} values") };
        let kind = Self::kind_name(self.kind);
        let names: Vec<&str> = self.series.iter().map(|s| s.name.trim()).filter(|s| !s.is_empty()).collect();
        match (self.series.len(), names.as_slice()) {
            (1, []) => format!("{kind} chart, {values}"),
            (1, [name]) => format!("{kind} chart: {name}, {values}"),
            (k, names) if names.len() == k => {
                let (last, rest) = names.split_last().unwrap_or((&"", &[]));
                format!("{kind} chart: {} and {last}, {values} each", rest.join(", "))
            }
            (k, _) => format!("{kind} chart, {k} series, {values} each"),
        }
    }

    /// Series `k`'s legend entry, when it has a name.
    pub fn legend(&self, k: usize) -> Option<&str> {
        self.series.get(k).map(|s| s.name.as_str()).filter(|n| !n.trim().is_empty())
    }

    /// Every category is a number: a scatter chart's x values.
    fn numeric_categories(&self) -> Option<Vec<f64>> {
        self.categories.iter().map(|c| c.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect()
    }

    fn any_legend(&self) -> bool {
        (0..self.series.len()).any(|k| self.legend(k).is_some())
    }
}

/// One edit the inspector makes to a chart: its type, a series' name, or
/// one cell, row or column of its data sheet. Each is one undo step.
#[derive(Clone, Debug, PartialEq)]
pub enum ChartEdit {
    Kind(ChartKind),
    /// Series `k`'s name.
    SeriesName(usize, String),
    /// Category `i` (a scatter chart's x value).
    Category(usize, String),
    /// Series `k`'s value for category `i`.
    Value(usize, usize, f64),
    /// A category after the last: the next one, each series its last value.
    AddPoint,
    /// Category `i` taken out, with every series' value for it; a chart
    /// keeps at least one.
    RemovePoint(usize),
    /// A series after the last, named as PowerPoint names one ("Series
    /// 2"), with the last series' values.
    AddSeries,
    /// Series `k` taken out; a chart keeps at least one.
    RemoveSeries(usize),
}

impl ChartData {
    /// `edit` applied. Whether it changed anything: an edit to a cell that
    /// isn't there, a value that isn't a number or a type the chart
    /// already has changes nothing, and so makes no undo step.
    pub fn apply(&mut self, edit: &ChartEdit) -> bool {
        let before = self.clone();
        match edit {
            ChartEdit::Kind(k) => self.kind = *k,
            ChartEdit::SeriesName(k, name) => {
                if let Some(s) = self.series.get_mut(*k) {
                    s.name = name.trim().to_string();
                }
            }
            ChartEdit::Category(i, c) => {
                if let Some(cat) = self.categories.get_mut(*i) {
                    *cat = c.trim().to_string();
                }
            }
            ChartEdit::Value(k, i, v) => {
                if let (Some(slot), true) = (self.series.get_mut(*k).and_then(|s| s.values.get_mut(*i)), v.is_finite()) {
                    *slot = *v;
                }
            }
            ChartEdit::AddPoint => {
                if self.categories.len() < MAX_POINTS {
                    let category = self.next_category();
                    self.categories.push(category);
                    for s in &mut self.series {
                        let last = s.values.last().copied().unwrap_or(0.0);
                        s.values.push(last);
                    }
                }
            }
            ChartEdit::RemovePoint(i) => {
                if self.categories.len() > 1 && *i < self.categories.len() {
                    self.categories.remove(*i);
                    for s in &mut self.series {
                        if *i < s.values.len() {
                            s.values.remove(*i);
                        }
                    }
                }
            }
            ChartEdit::AddSeries => {
                if self.series.len() < MAX_SERIES {
                    let values = self.series.last().map(|s| s.values.clone()).unwrap_or_default();
                    let name = format!("Series {}", self.series.len() + 1);
                    self.series.push(ChartSeries { name, values });
                }
            }
            ChartEdit::RemoveSeries(k) => {
                if self.series.len() > 1 && *k < self.series.len() {
                    self.series.remove(*k);
                }
            }
        }
        *self = self.clone().normalized();
        *self != before
    }

    /// The category a new point gets: the next number when they're all
    /// numbers (a scatter chart's x values, or 1, 2, 3), the next of a
    /// word and a number ("Q4" → "Q5"), else "Category n" as PowerPoint
    /// names one.
    fn next_category(&self) -> String {
        let n = self.categories.len() + 1;
        let Some(last) = self.categories.last() else { return "1".into() };
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

/// A value as a cache or a cell writes it: "4.3", "10".
fn num(v: f64) -> String {
    if v.is_finite() { format!("{v}") } else { "0".into() }
}

/// The column name of a 0-based column: A..Z, then AA...
fn col(c: usize) -> String {
    let mut c = c + 1;
    let mut s = Vec::new();
    while c > 0 {
        let r = (c - 1) % 26;
        s.push(b'A' + r as u8);
        c = (c - 1) / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
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

/// `chart` as a DrawingML chart part (`c:chartSpace`): a `c:ser` per
/// series, each naming its column of PowerPoint's data sheet.
pub fn chart_space_xml(chart: &ChartData) -> String {
    let last = chart.categories.len() + 1;
    let cats: Vec<&str> = chart.categories.iter().map(String::as_str).collect();
    let cat_ref = format!("Sheet1!$A$2:$A${last}");
    let x = match chart.numeric_categories() {
        Some(xs) => num_ref(&cat_ref, &xs),
        None => str_ref(&cat_ref, &cats),
    };
    let mut sers = String::new();
    for (k, s) in chart.series.iter().enumerate() {
        let c = col(k + 1);
        let head = format!(
            "<c:ser><c:idx val=\"{k}\"/><c:order val=\"{k}\"/><c:tx>{}</c:tx>",
            str_ref(&format!("Sheet1!${c}$1"), &[s.name.as_str()])
        );
        let val = num_ref(&format!("Sheet1!${c}$2:${c}${last}"), &s.values);
        sers.push_str(&match chart.kind {
            ChartKind::Bar => format!("{head}<c:invertIfNegative val=\"0\"/><c:cat>{}</c:cat><c:val>{val}</c:val></c:ser>", str_ref(&cat_ref, &cats)),
            ChartKind::Line => format!("{head}<c:cat>{}</c:cat><c:val>{val}</c:val><c:smooth val=\"0\"/></c:ser>", str_ref(&cat_ref, &cats)),
            ChartKind::Area | ChartKind::Pie => format!("{head}<c:cat>{}</c:cat><c:val>{val}</c:val></c:ser>", str_ref(&cat_ref, &cats)),
            ChartKind::Scatter => format!(
                "{head}<c:spPr><a:ln w=\"19050\"><a:noFill/></a:ln></c:spPr>\
                 <c:xVal>{x}</c:xVal><c:yVal>{val}</c:yVal><c:smooth val=\"0\"/></c:ser>"
            ),
        });
    }
    let ids = format!("<c:axId val=\"{AX_1}\"/><c:axId val=\"{AX_2}\"/>");
    let plot = match chart.kind {
        ChartKind::Bar => format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"0\"/>\
             {sers}<c:gapWidth val=\"150\"/>{ids}</c:barChart>{}{}",
            cat_ax(),
            val_ax(AX_2, "l", AX_1, true, "between")
        ),
        ChartKind::Line => format!(
            "<c:lineChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>\
             {sers}<c:marker val=\"1\"/>{ids}</c:lineChart>{}{}",
            cat_ax(),
            val_ax(AX_2, "l", AX_1, true, "between")
        ),
        ChartKind::Area => format!(
            "<c:areaChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>{sers}{ids}</c:areaChart>{}{}",
            cat_ax(),
            val_ax(AX_2, "l", AX_1, true, "midCat")
        ),
        ChartKind::Pie => format!("<c:pieChart><c:varyColors val=\"1\"/>{sers}<c:firstSliceAng val=\"0\"/></c:pieChart>"),
        ChartKind::Scatter => format!(
            "<c:scatterChart><c:scatterStyle val=\"lineMarker\"/><c:varyColors val=\"0\"/>{sers}{ids}</c:scatterChart>{}{}",
            val_ax(AX_1, "b", AX_2, false, "midCat"),
            val_ax(AX_2, "l", AX_1, true, "midCat")
        ),
    };
    let legend = if chart.any_legend() || chart.kind == ChartKind::Pie {
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
         <c:plotVisOnly val=\"1\"/><c:dispBlanksAs val=\"gap\"/></c:chart></c:chartSpace>"
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
    /// The first descendant, depth first, whose local name is `local`.
    fn find(&self, local: &str) -> Option<&El> {
        self.children.iter().find_map(|c| if c.local == local { Some(c) } else { c.find(local) })
    }
    /// The first descendant, depth first, named `name` (qualified).
    fn find_q(&self, name: &str) -> Option<&El> {
        self.children.iter().find_map(|c| if c.name == name { Some(c) } else { c.find_q(name) })
    }
    fn all_q<'a>(&'a self, name: &str, out: &mut Vec<&'a El>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            } else {
                c.all_q(name, out);
            }
        }
    }
    /// All the text inside, depth first.
    fn deep_text(&self) -> String {
        let mut s = self.text.clone();
        for c in &self.children {
            s.push_str(&c.deep_text());
        }
        s
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

/// The chart a DrawingML chart part draws: its first chart group (a combo
/// chart's others are left), every series in it.
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
    let sers: Vec<&El> = group.children.iter().filter(|c| c.local == "ser").take(READ_SERIES).collect();
    if sers.is_empty() {
        return None;
    }
    // The categories are the first series' that states any.
    let cats = sers
        .iter()
        .find_map(|s| s.child("cat").or_else(|| s.child("xVal")).map(cached))
        .unwrap_or_default();
    let values: Vec<Vec<Option<String>>> =
        sers.iter().map(|s| s.child("val").or_else(|| s.child("yVal")).map(cached).unwrap_or_default()).collect();
    let n = values.iter().map(Vec::len).chain([cats.len()]).max().unwrap_or(0).min(READ_POINTS);
    let categories = (0..n)
        // A category the file leaves out is its number, as PowerPoint
        // labels an axis with none.
        .map(|i| cats.get(i).cloned().flatten().unwrap_or_else(|| (i + 1).to_string()))
        .collect();
    let series = sers
        .iter()
        .zip(&values)
        .map(|(s, vals)| ChartSeries {
            name: s.child("tx").map(cached).and_then(|v| v.into_iter().next().flatten()).unwrap_or_default(),
            values: (0..n)
                .map(|i| vals.get(i).cloned().flatten().and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0))
                .collect(),
        })
        .collect();
    Some(ChartData { kind, categories, series }.normalized())
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
/// and its data as the object's local table (categories in column A, a
/// series per column after it, each under its name). The table is the
/// chart's last child, as ODF places it: beside `chart:chart` instead,
/// Impress drew the chart's kind with no data at all (the oracle's odp
/// rewrites).
pub fn odf_chart_content_xml(chart: &ChartData, w_pt: f64, h_pt: f64) -> String {
    let last = chart.categories.len() + 1;
    let class = odf_class(chart.kind);
    let range = |c: usize| format!("local-table.${}$2:.${}${last}", col(c), col(c));
    let axes = match chart.kind {
        ChartKind::Pie => String::new(),
        ChartKind::Scatter => "<chart:axis chart:dimension=\"x\" chart:name=\"primary-x\"/>\
             <chart:axis chart:dimension=\"y\" chart:name=\"primary-y\"><chart:grid chart:class=\"major\"/></chart:axis>"
            .to_string(),
        _ => format!(
            "<chart:axis chart:dimension=\"x\" chart:name=\"primary-x\">\
             <chart:categories table:cell-range-address=\"{}\"/></chart:axis>\
             <chart:axis chart:dimension=\"y\" chart:name=\"primary-y\"><chart:grid chart:class=\"major\"/></chart:axis>",
            range(0)
        ),
    };
    let domain = if chart.kind == ChartKind::Scatter {
        format!("<chart:domain table:cell-range-address=\"{}\"/>", range(0))
    } else {
        String::new()
    };
    let series: String = (0..chart.series.len())
        .map(|k| {
            format!(
                "<chart:series chart:class=\"{class}\" chart:values-cell-range-address=\"{}\" \
                 chart:label-cell-address=\"local-table.${}$1\">{domain}</chart:series>",
                range(k + 1),
                col(k + 1)
            )
        })
        .collect();
    let legend = if chart.any_legend() || chart.kind == ChartKind::Pie {
        "<chart:legend chart:legend-position=\"end\"/>"
    } else {
        ""
    };
    let numeric_x = (chart.kind == ChartKind::Scatter).then(|| chart.numeric_categories()).flatten();
    let mut rows = String::new();
    for (i, cat) in chart.categories.iter().enumerate() {
        let mut row = match &numeric_x {
            Some(xs) => format!(
                "<table:table-row><table:table-cell office:value-type=\"float\" office:value=\"{}\"><text:p>{}</text:p></table:table-cell>",
                num(xs[i]),
                esc(cat)
            ),
            None => format!("<table:table-row><table:table-cell office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>", esc(cat)),
        };
        for s in &chart.series {
            let v = num(s.values.get(i).copied().unwrap_or(0.0));
            row.push_str(&format!("<table:table-cell office:value-type=\"float\" office:value=\"{v}\"><text:p>{v}</text:p></table:table-cell>"));
        }
        row.push_str("</table:table-row>");
        rows.push_str(&row);
    }
    let names: String = chart
        .series
        .iter()
        .map(|s| format!("<table:table-cell office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>", esc(&s.name)))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <office:document-content {ODF_NS} office:version=\"1.2\"><office:automatic-styles/>\
         <office:body><office:chart>\
         <chart:chart svg:width=\"{w_pt}pt\" svg:height=\"{h_pt}pt\" chart:class=\"{class}\">{legend}\
         <chart:plot-area table:cell-range-address=\"local-table.$A$1:.${}${last}\" chart:data-source-has-labels=\"both\">\
         {axes}{series}</chart:plot-area>\
         <table:table table:name=\"local-table\">\
         <table:table-header-columns><table:table-column/></table:table-header-columns>\
         <table:table-columns><table:table-column table:number-columns-repeated=\"{}\"/></table:table-columns>\
         <table:table-header-rows><table:table-row><table:table-cell><text:p/></table:table-cell>{names}</table:table-row></table:table-header-rows>\
         <table:table-rows>{rows}</table:table-rows></table:table></chart:chart>\
         </office:chart></office:body></office:document-content>",
        col(chart.series.len()),
        chart.series.len()
    )
}

/// An embedded chart object's `styles.xml`: none of its own.
pub fn odf_chart_styles_xml() -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-styles {ODF_NS} office:version=\"1.2\"/>")
}

/// The 0-based column a cell address in a chart's local table names:
/// "local-table.$B$2:.$B$5" is 1, "$AA$1" is 26.
fn column_of(address: &str) -> Option<usize> {
    let cell = address.split(':').next()?;
    let cell = cell.rsplit('.').next()?;
    let letters: String = cell.trim_start_matches('$').chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let n = letters.to_ascii_uppercase().bytes().fold(0usize, |n, b| n * 26 + (b - b'A' + 1) as usize);
    Some(n - 1)
}

/// The chart an embedded ODF chart object's `content.xml` draws, every
/// series with its data from the object's local table.
pub fn parse_odf_chart(xml: &str) -> Option<ChartData> {
    let root = tree(xml);
    let chart = root.find_q("chart:chart")?;
    let kind = match chart.attr("chart:class").map(local_of)? {
        "bar" => ChartKind::Bar,
        "line" => ChartKind::Line,
        "circle" | "ring" => ChartKind::Pie,
        "area" => ChartKind::Area,
        "scatter" => ChartKind::Scatter,
        _ => return None,
    };
    let mut series_els = Vec::new();
    chart.all_q("chart:series", &mut series_els);
    let val_cols: Vec<usize> = series_els
        .iter()
        .take(READ_SERIES)
        .filter_map(|s| s.attr("chart:values-cell-range-address").and_then(column_of))
        .collect();
    let val_cols = if val_cols.is_empty() { vec![1] } else { val_cols };
    let cat_col = series_els
        .first()
        .and_then(|s| s.child_q("chart:domain"))
        .and_then(|d| d.attr("table:cell-range-address"))
        .or_else(|| chart.find_q("chart:categories").and_then(|c| c.attr("table:cell-range-address")))
        .and_then(column_of)
        .unwrap_or(0);
    let table = root.find_q("table:table")?;
    let cells = |row: &El| -> Vec<(String, Option<f64>)> {
        let mut out = Vec::new();
        for c in row.children.iter().filter(|c| c.name == "table:table-cell") {
            let repeat = c
                .attr("table:number-columns-repeated")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 64);
            let text: String = c.children.iter().filter(|p| p.name == "text:p").map(El::deep_text).collect::<Vec<_>>().join("\n");
            let value = c.attr_local("value").and_then(|v| v.parse::<f64>().ok()).or_else(|| text.trim().parse().ok());
            out.extend(std::iter::repeat_n((text, value), repeat));
            if out.len() > READ_SERIES + 1 {
                break;
            }
        }
        out
    };
    let header = table.child_q("table:table-header-rows").and_then(|h| h.child_q("table:table-row")).map(&cells);
    let mut rows = Vec::new();
    if let Some(body) = table.child_q("table:table-rows") {
        body.all_q("table:table-row", &mut rows);
    }
    rows.extend(table.children.iter().filter(|c| c.name == "table:table-row"));
    // A row with no value in any series' column isn't a category.
    let rows: Vec<Vec<(String, Option<f64>)>> = rows
        .into_iter()
        .take(READ_POINTS)
        .map(cells)
        .filter(|c| val_cols.iter().any(|&k| c.get(k).is_some_and(|v| v.1.is_some())))
        .collect();
    let categories = rows.iter().map(|c| c.get(cat_col).map(|c| c.0.clone()).unwrap_or_default()).collect();
    let series = val_cols
        .iter()
        .map(|&k| ChartSeries {
            name: header.as_ref().and_then(|h| h.get(k)).map(|c| c.0.clone()).unwrap_or_default(),
            values: rows.iter().map(|c| c.get(k).and_then(|v| v.1).unwrap_or(0.0)).collect(),
        })
        .collect();
    Some(ChartData { kind, categories, series }.normalized())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [ChartKind; 5] = [ChartKind::Bar, ChartKind::Line, ChartKind::Pie, ChartKind::Scatter, ChartKind::Area];

    /// Three series, one unnamed, with a name and a category that need
    /// escaping and a negative value.
    fn three(kind: ChartKind) -> ChartData {
        let mut c = ChartData::sample(kind);
        c.series[0].name = "Sales & <costs>".into();
        c.series.push(ChartSeries { name: String::new(), values: vec![1.0, -2.5, 3.0, 0.5] });
        c.series.push(ChartSeries { name: "West".into(), values: vec![9.0, 8.0, 7.0, 6.0] });
        if kind != ChartKind::Scatter {
            c.categories[1] = "Q2 \"late\"".into();
        }
        c
    }

    #[test]
    fn every_kind_reads_back_from_its_drawingml_part() {
        for kind in KINDS {
            for c in [ChartData::sample(kind), three(kind)] {
                let xml = chart_space_xml(&c);
                assert_eq!(parse_chart_space(&xml), Some(c.clone()), "{kind:?}: {xml}");
            }
        }
    }

    #[test]
    fn every_kind_reads_back_from_its_odf_object() {
        for kind in KINDS {
            for c in [ChartData::sample(kind), three(kind)] {
                let xml = odf_chart_content_xml(&c, 480.0, 300.0);
                assert_eq!(parse_odf_chart(&xml), Some(c.clone()), "{kind:?}: {xml}");
            }
        }
    }

    /// PowerPoint's own part: a default prefix-less namespace, a sparse
    /// category cache, and a second series of literals.
    #[test]
    fn a_powerpoint_part_reads_every_series() {
        let xml = r#"<chartSpace xmlns="http://schemas.openxmlformats.org/drawingml/2006/chart"><chart><plotArea><layout/>
          <barChart><barDir val="bar"/><grouping val="clustered"/>
          <ser><idx val="0"/><order val="0"/><tx><strRef><f>Sheet1!$B$1</f><strCache><ptCount val="1"/><pt idx="0"><v>Series 1</v></pt></strCache></strRef></tx>
          <cat><strRef><f>Sheet1!$A$2:$A$4</f><strCache><ptCount val="3"/><pt idx="0"><v>Category 1</v></pt><pt idx="2"><v>Category 3</v></pt></strCache></strRef></cat>
          <val><numRef><f>Sheet1!$B$2:$B$4</f><numCache><formatCode>General</formatCode><ptCount val="3"/><pt idx="0"><v>4.3</v></pt><pt idx="1"><v>2.5</v></pt><pt idx="2"><v>3.5</v></pt></numCache></numRef></val></ser>
          <ser><idx val="1"/><order val="1"/><tx><v>Series 2</v></tx><val><numLit><ptCount val="1"/><pt idx="0"><v>9</v></pt></numLit></val></ser>
          </barChart></plotArea></chart></chartSpace>"#;
        let c = parse_chart_space(xml).unwrap();
        assert_eq!(c.kind, ChartKind::Bar);
        assert_eq!(c.categories, ["Category 1", "2", "Category 3"]);
        assert_eq!(
            c.series,
            [
                ChartSeries { name: "Series 1".into(), values: vec![4.3, 2.5, 3.5] },
                ChartSeries { name: "Series 2".into(), values: vec![9.0, 0.0, 0.0] },
            ]
        );
    }

    /// Impress's local table: a header row with an empty corner, each
    /// series in the column its values address names.
    #[test]
    fn an_impress_object_reads_its_series_from_the_addressed_columns() {
        let xml = r#"<office:document-content xmlns:office="o" xmlns:chart="c" xmlns:table="t" xmlns:text="x"><office:body><office:chart>
          <chart:chart chart:class="chart:line"><chart:plot-area>
          <chart:series chart:values-cell-range-address="local-table.$C$2:.$C$3" chart:label-cell-address="local-table.$C$1"/>
          <chart:series chart:values-cell-range-address="local-table.$B$2:.$B$3" chart:label-cell-address="local-table.$B$1"/>
          </chart:plot-area>
          <table:table table:name="local-table">
          <table:table-header-rows><table:table-row><table:table-cell/><table:table-cell office:value-type="string"><text:p>A</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>B</text:p></table:table-cell></table:table-row></table:table-header-rows>
          <table:table-rows>
          <table:table-row><table:table-cell office:value-type="string"><text:p>Mon</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="1"><text:p>1</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="7.5"><text:p>7.5</text:p></table:table-cell></table:table-row>
          <table:table-row><table:table-cell office:value-type="string"><text:p>Tue</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="2"><text:p>2</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="8"><text:p>8</text:p></table:table-cell></table:table-row>
          </table:table-rows></table:table></chart:chart></office:chart></office:body></office:document-content>"#;
        let c = parse_odf_chart(xml).unwrap();
        assert_eq!(c.kind, ChartKind::Line);
        assert_eq!(c.categories, ["Mon", "Tue"]);
        assert_eq!(
            c.series,
            [ChartSeries { name: "B".into(), values: vec![7.5, 8.0] }, ChartSeries { name: "A".into(), values: vec![1.0, 2.0] }]
        );
    }

    #[test]
    fn columns_are_named_past_z() {
        assert_eq!((col(0), col(25), col(26), col(27), col(701)), ("A".into(), "Z".into(), "AA".into(), "AB".into(), "ZZ".into()));
        for c in [0, 1, 25, 26, 27, 701] {
            assert_eq!(column_of(&format!("local-table.${}$2:.${}$9", col(c), col(c))), Some(c));
        }
    }

    #[test]
    fn edits_change_the_data_and_say_whether_they_did() {
        let mut c = ChartData::sample(ChartKind::Bar);
        assert!(c.apply(&ChartEdit::Kind(ChartKind::Line)));
        assert!(!c.apply(&ChartEdit::Kind(ChartKind::Line)), "already a line chart");
        assert!(c.apply(&ChartEdit::SeriesName(0, "  Revenue ".into())));
        assert!(!c.apply(&ChartEdit::SeriesName(3, "x".into())), "no such series");
        assert_eq!(c.series[0].name, "Revenue");
        assert!(c.apply(&ChartEdit::Category(0, "Jan".into())));
        assert!(c.apply(&ChartEdit::Value(0, 3, -2.5)));
        assert!(!c.apply(&ChartEdit::Value(0, 9, 1.0)), "no such point");
        assert!(!c.apply(&ChartEdit::Value(1, 0, 1.0)), "no such series");
        assert!(!c.apply(&ChartEdit::Value(0, 0, f64::NAN)), "not a number");
        assert_eq!(c.categories[0], "Jan");
        assert_eq!(c.series[0].values[3], -2.5);

        assert!(c.apply(&ChartEdit::AddSeries));
        assert_eq!(c.series[1], ChartSeries { name: "Series 2".into(), values: c.series[0].values.clone() });
        assert!(c.apply(&ChartEdit::Value(1, 0, 7.0)));
        assert!(c.apply(&ChartEdit::AddPoint));
        assert_eq!(c.categories[4], "Q5", "the next category");
        assert_eq!((c.series[0].values[4], c.series[1].values[4]), (-2.5, -2.5), "each series its last value");
        assert!(c.apply(&ChartEdit::RemovePoint(0)));
        assert_eq!((c.categories.len(), c.series[1].values.len(), c.series[1].values[0]), (4, 4, 2.5));
        assert!(c.apply(&ChartEdit::RemoveSeries(0)));
        assert_eq!(c.series[0].name, "Series 2");
        assert!(!c.apply(&ChartEdit::RemoveSeries(0)), "a chart keeps one series");
        for _ in 0..10 {
            c.apply(&ChartEdit::RemovePoint(0));
        }
        assert_eq!(c.categories.len(), 1, "a chart keeps one point");
        for _ in 0..(MAX_POINTS + 5) {
            c.apply(&ChartEdit::AddPoint);
        }
        for _ in 0..(MAX_SERIES + 5) {
            c.apply(&ChartEdit::AddSeries);
        }
        assert_eq!((c.categories.len(), c.series.len()), (MAX_POINTS, MAX_SERIES));
        assert!(c.series.iter().all(|s| s.values.len() == MAX_POINTS));
    }

    #[test]
    fn a_new_point_continues_the_categories() {
        let next = |cats: &[&str]| {
            let points: Vec<(&str, f64)> = cats.iter().map(|c| (*c, 1.0)).collect();
            let mut c = ChartData::from_points(ChartKind::Bar, "", &points);
            c.apply(&ChartEdit::AddPoint);
            c.categories.last().unwrap().clone()
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
        assert_eq!(ChartData::from_points(ChartKind::Pie, "", &[("a", 1.0)]).describe(), "Pie chart, 1 value");
        let mut c = ChartData::sample(ChartKind::Line);
        c.apply(&ChartEdit::AddSeries);
        assert_eq!(c.describe(), "Line chart: Sales and Series 2, 4 values each");
        c.apply(&ChartEdit::AddSeries);
        assert_eq!(c.describe(), "Line chart: Sales, Series 2 and Series 3, 4 values each");
        c.apply(&ChartEdit::SeriesName(1, String::new()));
        assert_eq!(c.describe(), "Line chart, 3 series, 4 values each");
    }

    #[test]
    fn a_hostile_point_count_allocates_only_what_is_there() {
        let xml = r#"<c:chartSpace xmlns:c="c"><c:chart><c:plotArea><c:pieChart><c:ser>
          <c:val><c:numLit><c:ptCount val="4000000000"/><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val>
          </c:ser></c:pieChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_space(xml).unwrap();
        assert_eq!((c.categories, c.series[0].values.clone()), (vec!["1".to_string()], vec![1.0]));
    }
}
