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
//! A chart is one series of (category, value) points, the series a
//! Tables chart draws. A file's chart with more than one series is read as
//! its first.

pub use suite_common_core::charts::ChartKind;

/// A chart's kind, series name and points.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChartData {
    pub kind: ChartKind,
    /// The series' name, its legend entry; empty for none.
    pub series: String,
    /// (category, value). For a scatter chart the category is the x value.
    pub points: Vec<(String, f64)>,
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
        ChartData { kind, series: "Sales".into(), points }
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

    /// Every category is a number: a scatter chart's x values.
    fn numeric_categories(&self) -> Option<Vec<f64>> {
        self.points.iter().map(|(c, _)| c.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect()
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
    let head = format!(
        "<c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:tx>{}</c:tx>",
        str_ref("Sheet1!$B$1", &[chart.series.as_str()])
    );
    let cat = format!("<c:cat>{}</c:cat>", str_ref(&format!("Sheet1!$A$2:$A${last}"), &cats));
    let val = format!("<c:val>{}</c:val>", num_ref(&format!("Sheet1!$B$2:$B${last}"), &vals));
    let ids = format!("<c:axId val=\"{AX_1}\"/><c:axId val=\"{AX_2}\"/>");
    let plot = match chart.kind {
        ChartKind::Bar => format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"0\"/>\
             {head}<c:invertIfNegative val=\"0\"/>{cat}{val}</c:ser><c:gapWidth val=\"150\"/>{ids}</c:barChart>{}{}",
            cat_ax(),
            val_ax(AX_2, "l", AX_1, true, "between")
        ),
        ChartKind::Line => format!(
            "<c:lineChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>\
             {head}{cat}{val}<c:smooth val=\"0\"/></c:ser><c:marker val=\"1\"/>{ids}</c:lineChart>{}{}",
            cat_ax(),
            val_ax(AX_2, "l", AX_1, true, "between")
        ),
        ChartKind::Area => format!(
            "<c:areaChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>\
             {head}{cat}{val}</c:ser>{ids}</c:areaChart>{}{}",
            cat_ax(),
            val_ax(AX_2, "l", AX_1, true, "midCat")
        ),
        ChartKind::Pie => format!(
            "<c:pieChart><c:varyColors val=\"1\"/>{head}{cat}{val}</c:ser><c:firstSliceAng val=\"0\"/></c:pieChart>"
        ),
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
    let legend = if chart.legend().is_some() || chart.kind == ChartKind::Pie {
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
    let ser = group.child("ser")?;
    let series = ser.child("tx").map(cached).and_then(|v| v.into_iter().next().flatten()).unwrap_or_default();
    let cats = ser.child("cat").or_else(|| ser.child("xVal")).map(cached).unwrap_or_default();
    let vals = ser.child("val").or_else(|| ser.child("yVal")).map(cached)?;
    let points = vals
        .iter()
        .enumerate()
        .map(|(i, v)| {
            // A category the file leaves out is its number, as PowerPoint
            // labels an axis with none.
            let cat = cats.get(i).cloned().flatten().unwrap_or_else(|| (i + 1).to_string());
            (cat, v.as_deref().and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0))
        })
        .collect();
    Some(ChartData { kind, series, points })
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
/// series in B under its name).
pub fn odf_chart_content_xml(chart: &ChartData, w_pt: f64, h_pt: f64) -> String {
    let n = chart.points.len();
    let last = n + 1;
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
    let legend = if chart.legend().is_some() || chart.kind == ChartKind::Pie {
        "<chart:legend chart:legend-position=\"end\"/>"
    } else {
        ""
    };
    let numeric_x = (chart.kind == ChartKind::Scatter).then(|| chart.numeric_categories()).flatten();
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
        rows.push_str(&format!(
            "<table:table-row>{first}<table:table-cell office:value-type=\"float\" office:value=\"{v}\">\
             <text:p>{v}</text:p></table:table-cell></table:table-row>",
            v = num(*v)
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <office:document-content {ODF_NS} office:version=\"1.2\"><office:automatic-styles/>\
         <office:body><office:chart>\
         <chart:chart svg:width=\"{w_pt}pt\" svg:height=\"{h_pt}pt\" chart:class=\"{class}\">{legend}\
         <chart:plot-area table:cell-range-address=\"local-table.$A$1:.$B${last}\" chart:data-source-has-labels=\"both\">\
         {axes}<chart:series chart:class=\"{class}\" chart:values-cell-range-address=\"{}\" \
         chart:label-cell-address=\"local-table.$B$1\">{domain}</chart:series></chart:plot-area></chart:chart>\
         <table:table table:name=\"local-table\">\
         <table:table-header-columns><table:table-column/></table:table-header-columns>\
         <table:table-columns><table:table-column/></table:table-columns>\
         <table:table-header-rows><table:table-row><table:table-cell><text:p/></table:table-cell>\
         <table:table-cell office:value-type=\"string\"><text:p>{}</text:p></table:table-cell></table:table-row></table:table-header-rows>\
         <table:table-rows>{rows}</table:table-rows></table:table>\
         </office:chart></office:body></office:document-content>",
        range(1),
        esc(&chart.series)
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
    let kind = match chart.attr("chart:class").map(local_of)? {
        "bar" => ChartKind::Bar,
        "line" => ChartKind::Line,
        "circle" | "ring" => ChartKind::Pie,
        "area" => ChartKind::Area,
        "scatter" => ChartKind::Scatter,
        _ => return None,
    };
    let series = chart.find_q("chart:series");
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
    let series_name = header.map(&cells).and_then(|c| c.get(val_col).map(|c| c.0.clone())).unwrap_or_default();
    let mut rows = Vec::new();
    if let Some(body) = table.child_q("table:table-rows") {
        body.all_q("table:table-row", &mut rows);
    }
    rows.extend(table.children.iter().filter(|c| c.name == "table:table-row"));
    let points = rows
        .into_iter()
        .take(10_000)
        .filter_map(|r| {
            let c = cells(r);
            let value = c.get(val_col)?.1?;
            let cat = c.get(cat_col).map(|c| c.0.clone()).unwrap_or_default();
            Some((cat, if value.is_finite() { value } else { 0.0 }))
        })
        .collect();
    Some(ChartData { kind, series: series_name, points })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [ChartKind; 5] = [ChartKind::Bar, ChartKind::Line, ChartKind::Pie, ChartKind::Scatter, ChartKind::Area];

    #[test]
    fn every_kind_reads_back_from_its_drawingml_part() {
        for kind in KINDS {
            let mut c = ChartData::sample(kind);
            c.series = "Sales & <costs>".into();
            let xml = chart_space_xml(&c);
            assert_eq!(parse_chart_space(&xml), Some(c.clone()), "{kind:?}: {xml}");
        }
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
    fn a_chart_is_described_by_its_kind_series_and_size() {
        assert_eq!(ChartData::sample(ChartKind::Bar).describe(), "Bar chart: Sales, 4 values");
        let c = ChartData { kind: ChartKind::Pie, series: String::new(), points: vec![("a".into(), 1.0)] };
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
