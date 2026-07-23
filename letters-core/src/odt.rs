// odt.rs — OpenDocument Text (.odt) read/write for the Letters model.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Scope (first slice, PARITY #20): paragraphs, headings 1–6, inline
// bold/italic/underline/strikethrough/highlight, font size, color,
// links, alignment, flat bullet/numbered lists, page breaks, and the
// document header/footer. Fidelity is measured against the LibreOffice
// oracle (tests/soffice_oracle.rs pattern), never ported from it.

use crate::model::*;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::{Read, Write};

const MIMETYPE: &str = "application/vnd.oasis.opendocument.text";

// ── XML helpers ──────────────────────────────────────────────────────

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── Writing ──────────────────────────────────────────────────────────

/// Collect the distinct run styles used, in first-use order.
fn collect_run_styles(doc: &Document) -> Vec<RunStyle> {
    let mut styles: Vec<RunStyle> = Vec::new();
    for p in &doc.paragraphs {
        for r in &p.runs {
            if r.style != RunStyle::default() && !styles.contains(&r.style) {
                styles.push(r.style.clone());
            }
        }
    }
    styles
}

fn run_style_props(st: &RunStyle) -> String {
    let mut props = String::new();
    if st.bold {
        props.push_str(" fo:font-weight=\"bold\"");
    }
    if st.italic {
        props.push_str(" fo:font-style=\"italic\"");
    }
    if st.underline {
        props.push_str(" style:text-underline-style=\"solid\"");
    }
    if st.strikethrough {
        props.push_str(" style:text-line-through-style=\"solid\"");
    }
    if st.highlight {
        props.push_str(" fo:background-color=\"#ffff00\"");
    }
    if let Some(hp) = st.font_size_hp {
        props.push_str(&format!(" fo:font-size=\"{}pt\"", hp as f32 / 2.0));
    }
    if let Some(color) = &st.color {
        props.push_str(&format!(" fo:color=\"#{color}\""));
    }
    if let Some(va) = st.vert_align {
        let pos = match va {
            VertAlign::Superscript => "super 58%",
            VertAlign::Subscript => "sub 58%",
        };
        props.push_str(&format!(" style:text-position=\"{pos}\""));
    }
    if st.code {
        props.push_str(" style:font-name=\"Monospace\"");
    }
    if let Some(fam) = &st.font_family {
        props.push_str(&format!(" fo:font-family=\"{}\"", esc(fam)));
    }
    props
}

fn para_style_props(st: &ParaStyle) -> String {
    let mut props = String::new();
    match st.alignment {
        Alignment::Left => {}
        Alignment::Center => props.push_str(" fo:text-align=\"center\""),
        Alignment::Right => props.push_str(" fo:text-align=\"end\""),
        Alignment::Justify => props.push_str(" fo:text-align=\"justify\""),
    }
    if st.page_break_before {
        props.push_str(" fo:break-before=\"page\"");
    }
    if (st.line_spacing - 1.0).abs() > 0.01 {
        props.push_str(&format!(" fo:line-height=\"{:.0}%\"", st.line_spacing * 100.0));
    }
    props
}

fn content_xml(doc: &Document) -> String {
    let run_styles = collect_run_styles(doc);

    let mut auto = String::new();
    for (i, st) in run_styles.iter().enumerate() {
        auto.push_str(&format!(
            "<style:style style:name=\"T{}\" style:family=\"text\">\
             <style:text-properties{}/></style:style>",
            i + 1,
            run_style_props(st)
        ));
    }
    // Paragraph automatic styles: one per used (alignment, break) combo.
    let mut para_autos: Vec<String> = Vec::new();
    let mut para_style_idx: Vec<Option<usize>> = Vec::new();
    for p in &doc.paragraphs {
        let props = para_style_props(&p.style);
        if props.is_empty() {
            para_style_idx.push(None);
            continue;
        }
        let pos = para_autos.iter().position(|x| *x == props).unwrap_or_else(|| {
            para_autos.push(props.clone());
            para_autos.len() - 1
        });
        para_style_idx.push(Some(pos));
    }
    for (i, props) in para_autos.iter().enumerate() {
        auto.push_str(&format!(
            "<style:style style:name=\"P{}\" style:family=\"paragraph\">\
             <style:paragraph-properties{}/></style:style>",
            i + 1,
            props
        ));
    }
    auto.push_str(
        "<text:list-style style:name=\"LB\"><text:list-level-style-bullet \
         text:level=\"1\" text:bullet-char=\"•\"/></text:list-style>\
         <text:list-style style:name=\"LN\"><text:list-level-style-number \
         text:level=\"1\" style:num-format=\"1\" style:num-suffix=\".\"/>\
         </text:list-style>",
    );

    let mut body = String::new();
    let mut open_list: Option<ListKind> = None;
    for (pi, p) in doc.paragraphs.iter().enumerate() {
        // List grouping: consecutive list paragraphs share one text:list.
        if open_list != Some(p.style.list) {
            if open_list.map(|l| l != ListKind::None).unwrap_or(false) {
                body.push_str("</text:list-item></text:list>");
            }
            open_list = Some(p.style.list);
            match p.style.list {
                ListKind::Bullet => {
                    body.push_str("<text:list text:style-name=\"LB\"><text:list-item>")
                }
                ListKind::Numbered => {
                    body.push_str("<text:list text:style-name=\"LN\"><text:list-item>")
                }
                ListKind::None => {}
            }
        } else if p.style.list != ListKind::None {
            body.push_str("</text:list-item><text:list-item>");
        }

        // LO built-in named styles win over our automatic styles; the
        // names (Title, Subtitle, Quotations) are ODF/LO conventions.
        let style_attr = if let Some(name) = &p.style.named_style {
            format!(" text:style-name=\"{}\"", esc(name))
        } else if p.style.block_quote {
            " text:style-name=\"Quotations\"".to_string()
        } else {
            match para_style_idx[pi] {
                Some(i) => format!(" text:style-name=\"P{}\"", i + 1),
                None => String::new(),
            }
        };

        let mut inner = String::new();
        for r in &p.runs {
            let mut run_xml = esc(&r.text);
            if r.style != RunStyle::default() {
                let ti = run_styles.iter().position(|s| *s == r.style).unwrap() + 1;
                run_xml = format!("<text:span text:style-name=\"T{ti}\">{run_xml}</text:span>");
            }
            if let Some(href) = &r.style.link {
                run_xml = format!(
                    "<text:a xlink:type=\"simple\" xlink:href=\"{}\">{}</text:a>",
                    esc(href),
                    run_xml
                );
            }
            inner.push_str(&run_xml);
        }

        if let Some(level) = p.style.heading {
            body.push_str(&format!(
                "<text:h text:outline-level=\"{level}\"{style_attr}>{inner}</text:h>"
            ));
        } else {
            body.push_str(&format!("<text:p{style_attr}>{inner}</text:p>"));
        }
    }
    if open_list.map(|l| l != ListKind::None).unwrap_or(false) {
        body.push_str("</text:list-item></text:list>");
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <office:document-content \
         xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
         xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" \
         office:version=\"1.2\">\
         <office:font-face-decls>\
         <style:font-face style:name=\"Monospace\" \
         svg:font-family=\"&apos;Liberation Mono&apos;, monospace\" \
         style:font-family-generic=\"modern\" style:font-pitch=\"fixed\"/>\
         </office:font-face-decls>\
         <office:automatic-styles>{auto}</office:automatic-styles>\
         <office:body><office:text>{body}</office:text></office:body>\
         </office:document-content>"
    )
}

fn styles_xml(doc: &Document) -> String {
    let layout = match &doc.page {
        Some(pg) => format!(
            "<style:page-layout-properties \
             fo:page-width=\"{:.2}pt\" fo:page-height=\"{:.2}pt\" \
             fo:margin-top=\"{:.2}pt\" fo:margin-bottom=\"{:.2}pt\" \
             fo:margin-left=\"{:.2}pt\" fo:margin-right=\"{:.2}pt\"/>",
            pg.width_pt, pg.height_pt, pg.margin_top_pt, pg.margin_bottom_pt,
            pg.margin_left_pt, pg.margin_right_pt
        ),
        None => "<style:page-layout-properties/>".to_string(),
    };
    let mut hf = String::new();
    if doc.header.is_some() || doc.footer.is_some() {
        hf.push_str("<office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"pm1\">");
        if let Some(h) = &doc.header {
            hf.push_str(&format!(
                "<style:header><text:p>{}</text:p></style:header>",
                esc(h)
            ));
        }
        if let Some(f) = &doc.footer {
            hf.push_str(&format!(
                "<style:footer><text:p>{}</text:p></style:footer>",
                esc(f)
            ));
        }
        hf.push_str("</style:master-page></office:master-styles>");
    } else {
        hf.push_str("<office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"pm1\"/></office:master-styles>");
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <office:document-styles \
         xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
         xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
         office:version=\"1.2\">\
         <office:automatic-styles>\
         <style:page-layout style:name=\"pm1\">{layout}\
         </style:page-layout></office:automatic-styles>{hf}\
         </office:document-styles>"
    )
}

const MANIFEST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.2\">\
<manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.text\"/>\
<manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
<manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>\
</manifest:manifest>";

/// Write the document as .odt. Built fully in memory, then placed
/// atomically (see suite_common_core::atomic_save) — a rename before the
/// ZipWriter flushes its central directory would leave a corrupt archive.
pub fn write(doc: &Document, path: &str) -> Result<(), String> {
    let buf = std::io::Cursor::new(Vec::new());
    let mut z = zip::ZipWriter::new(buf);
    // Per ODF spec the mimetype entry comes first and uncompressed.
    z.start_file(
        "mimetype",
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .map_err(|e| e.to_string())?;
    z.write_all(MIMETYPE.as_bytes()).map_err(|e| e.to_string())?;
    let opt = zip::write::SimpleFileOptions::default();
    z.start_file("META-INF/manifest.xml", opt).map_err(|e| e.to_string())?;
    z.write_all(MANIFEST.as_bytes()).map_err(|e| e.to_string())?;
    z.start_file("content.xml", opt).map_err(|e| e.to_string())?;
    z.write_all(content_xml(doc).as_bytes()).map_err(|e| e.to_string())?;
    z.start_file("styles.xml", opt).map_err(|e| e.to_string())?;
    z.write_all(styles_xml(doc).as_bytes()).map_err(|e| e.to_string())?;
    let bytes = z.finish().map_err(|e| e.to_string())?.into_inner();
    suite_common_core::atomic_save::atomic_write_bytes(std::path::Path::new(path), &bytes)
}

// ── Reading ──────────────────────────────────────────────────────────

fn attr_val(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    e.attributes().filter_map(|a| a.ok()).find_map(|a| {
        if a.key.as_ref() == name.as_bytes() {
            Some(String::from_utf8_lossy(&a.value).to_string())
        } else {
            None
        }
    })
}

/// Parse the style:text-properties / paragraph-properties of automatic
/// styles into model styles keyed by style name.
struct AutoStyles {
    text: std::collections::HashMap<String, RunStyle>,
    para: std::collections::HashMap<String, (Alignment, bool, f32)>,
    /// Automatic paragraph style → its style:parent-style-name (LO
    /// rewrites named styles as autos inheriting from the built-in).
    para_parent: std::collections::HashMap<String, String>,
}

fn parse_auto_styles(xml: &str) -> AutoStyles {
    let mut out = AutoStyles { text: Default::default(), para: Default::default(), para_parent: Default::default() };
    let mut reader = Reader::from_str(xml);
    let mut cur_name: Option<String> = None;
    let mut cur_family = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                match e.name().as_ref() {
                    b"style:style" => {
                        cur_name = attr_val(&e, "style:name");
                        cur_family = attr_val(&e, "style:family").unwrap_or_default();
                        if cur_family == "paragraph" {
                            if let (Some(n), Some(parent)) =
                                (cur_name.clone(), attr_val(&e, "style:parent-style-name"))
                            {
                                out.para_parent.insert(n, parent);
                            }
                        }
                    }
                    b"style:text-properties" => {
                        if let (Some(name), "text") = (cur_name.clone(), cur_family.as_str()) {
                            let mut st = RunStyle::default();
                            if attr_val(&e, "fo:font-weight").as_deref() == Some("bold") {
                                st.bold = true;
                            }
                            if attr_val(&e, "fo:font-style").as_deref() == Some("italic") {
                                st.italic = true;
                            }
                            if attr_val(&e, "style:text-underline-style")
                                .map(|v| v != "none")
                                .unwrap_or(false)
                            {
                                st.underline = true;
                            }
                            if attr_val(&e, "style:text-line-through-style")
                                .map(|v| v != "none")
                                .unwrap_or(false)
                            {
                                st.strikethrough = true;
                            }
                            if let Some(bg) = attr_val(&e, "fo:background-color") {
                                if bg.to_lowercase() == "#ffff00" {
                                    st.highlight = true;
                                }
                            }
                            if let Some(sz) = attr_val(&e, "fo:font-size") {
                                if let Ok(pt) = sz.trim_end_matches("pt").parse::<f32>() {
                                    st.font_size_hp = Some((pt * 2.0).round() as u16);
                                }
                            }
                            if let Some(c) = attr_val(&e, "fo:color") {
                                st.color = Some(c.trim_start_matches('#').to_lowercase());
                            }
                            if let Some(fam) = attr_val(&e, "fo:font-family") {
                                let f = fam.to_lowercase();
                                if f.contains("mono") || f.contains("courier") {
                                    st.code = true;
                                } else {
                                    st.font_family =
                                        Some(fam.trim_matches('\'').to_string());
                                }
                            }
                            if let Some(fname) = attr_val(&e, "style:font-name") {
                                // Exactly what our writer emits for code
                                // spans; LO preserves or maps it to a
                                // mono face.
                                let f = fname.to_lowercase();
                                if f.contains("monospace")
                                    || f.contains("courier")
                                    || f.contains("liberation mono")
                                {
                                    st.code = true;
                                    st.font_family = None;
                                }
                            }
                            if let Some(tp) = attr_val(&e, "style:text-position") {
                                if tp.starts_with("super") {
                                    st.vert_align = Some(VertAlign::Superscript);
                                } else if tp.starts_with("sub") {
                                    st.vert_align = Some(VertAlign::Subscript);
                                }
                            }
                            out.text.insert(name, st);
                        }
                    }
                    b"style:paragraph-properties" => {
                        if let (Some(name), "paragraph") = (cur_name.clone(), cur_family.as_str()) {
                            let align = match attr_val(&e, "fo:text-align").as_deref() {
                                Some("center") => Alignment::Center,
                                Some("end") | Some("right") => Alignment::Right,
                                Some("justify") => Alignment::Justify,
                                _ => Alignment::Left,
                            };
                            let brk = attr_val(&e, "fo:break-before").as_deref() == Some("page");
                            let spacing = attr_val(&e, "fo:line-height")
                                .and_then(|v| v.trim_end_matches('%').parse::<f32>().ok())
                                .map(|pct| pct / 100.0)
                                .unwrap_or(1.0);
                            out.para.insert(name, (align, brk, spacing));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Parse an ODF length ("595.3pt", "21cm", "210mm", "8.5in") to points.
fn parse_length_pt(v: &str) -> Option<f64> {
    let v = v.trim();
    let (num, unit) = v.split_at(v.find(|c: char| c.is_ascii_alphabetic())?);
    let n: f64 = num.parse().ok()?;
    Some(match unit {
        "pt" => n,
        "cm" => n * 72.0 / 2.54,
        "mm" => n * 72.0 / 25.4,
        "in" => n * 72.0,
        _ => return None,
    })
}

/// Read an .odt into the model.
pub fn read(path: &str) -> Result<Document, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut content = String::new();
    zip.by_name("content.xml")
        .map_err(|_| "no content.xml — not an ODT?".to_string())?
        .read_to_string(&mut content)
        .map_err(|e| e.to_string())?;
    let mut styles = String::new();
    if let Ok(mut f) = zip.by_name("styles.xml") {
        let _ = f.read_to_string(&mut styles);
    }

    let auto = parse_auto_styles(&content);

    let mut doc = Document { paragraphs: Vec::new(), footnotes: Vec::new(), header: None, footer: None, page: None };
    let mut reader = Reader::from_str(&content);
    let mut in_body = false;
    let mut para: Option<Paragraph> = None;
    // Span/link style stack: (style, depth marker)
    let mut span_stack: Vec<RunStyle> = Vec::new();
    let mut link_stack: Vec<String> = Vec::new();
    let mut list_kind = ListKind::None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"office:text" => in_body = true,
                b"text:p" | b"text:h" if in_body => {
                    let mut style = ParaStyle::default();
                    if e.name().as_ref() == b"text:h" {
                        let lvl = attr_val(&e, "text:outline-level")
                            .and_then(|v| v.parse::<u8>().ok())
                            .unwrap_or(1);
                        style.heading = Some(lvl.clamp(1, 6));
                    }
                    if let Some(name) = attr_val(&e, "text:style-name") {
                        if let Some((align, brk, spacing)) = auto.para.get(&name) {
                            style.alignment = *align;
                            style.page_break_before = *brk;
                            style.line_spacing = *spacing;
                        }
                        // Direct built-in name, or an automatic style
                        // inheriting from one (LO's rewrite pattern).
                        let base = auto
                            .para_parent
                            .get(&name)
                            .cloned()
                            .unwrap_or_else(|| name.clone());
                        // ODF display names use _20_ for spaces.
                        let base = base.replace("_20_", " ");
                        match base.as_str() {
                            "Title" | "Subtitle" => style.named_style = Some(base),
                            "Quotations" => style.block_quote = true,
                            _ => {}
                        }
                    }
                    style.list = list_kind;
                    para = Some(Paragraph { style, runs: Vec::new() });
                }
                b"text:span" => {
                    let st = attr_val(&e, "text:style-name")
                        .and_then(|n| auto.text.get(&n).cloned())
                        .unwrap_or_default();
                    span_stack.push(st);
                }
                b"text:a" => {
                    link_stack.push(attr_val(&e, "xlink:href").unwrap_or_default());
                }
                b"text:list" if in_body => {
                    // Bullet vs numbered comes from the list style name we
                    // write; LO-authored lists fall back to bullet.
                    let name = attr_val(&e, "text:style-name").unwrap_or_default();
                    list_kind = if name.contains('N') && !name.contains("LB") {
                        ListKind::Numbered
                    } else {
                        ListKind::Bullet
                    };
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"text:s" if para.is_some() => {
                    let n = attr_val(&e, "text:c")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1);
                    push_text(&mut para, &span_stack, &link_stack, &" ".repeat(n));
                }
                b"text:tab" if para.is_some() => {
                    push_text(&mut para, &span_stack, &link_stack, "\t");
                }
                b"text:p" | b"text:h" if in_body => {
                    let style = ParaStyle { list: list_kind, ..Default::default() };
                    doc.paragraphs.push(Paragraph { style, runs: Vec::new() });
                }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if para.is_some() {
                    let txt = t.unescape().unwrap_or_default().to_string();
                    push_text(&mut para, &span_stack, &link_stack, &txt);
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"text:p" | b"text:h" => {
                    if let Some(p) = para.take() {
                        doc.paragraphs.push(p);
                    }
                }
                b"text:span" => {
                    span_stack.pop();
                }
                b"text:a" => {
                    link_stack.pop();
                }
                b"text:list" => list_kind = ListKind::None,
                b"office:text" => in_body = false,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error: {e}")),
            _ => {}
        }
    }

    // Header/footer and page geometry from styles.xml.
    if !styles.is_empty() {
        let mut reader = Reader::from_str(&styles);
        let mut in_header = false;
        let mut in_footer = false;
        let mut read_page_layout = |e: &quick_xml::events::BytesStart| {
            let w = attr_val(e, "fo:page-width").and_then(|v| parse_length_pt(&v));
            let h = attr_val(e, "fo:page-height").and_then(|v| parse_length_pt(&v));
            if let (Some(width_pt), Some(height_pt)) = (w, h) {
                let d = PageGeometry::default();
                let m = |name: &str, fallback: f64| {
                    attr_val(e, name).and_then(|v| parse_length_pt(&v)).unwrap_or(fallback)
                };
                doc.page = Some(PageGeometry {
                    width_pt,
                    height_pt,
                    margin_top_pt: m("fo:margin-top", d.margin_top_pt),
                    margin_bottom_pt: m("fo:margin-bottom", d.margin_bottom_pt),
                    margin_left_pt: m("fo:margin-left", d.margin_left_pt),
                    margin_right_pt: m("fo:margin-right", d.margin_right_pt),
                });
            }
        };
        loop {
            match reader.read_event() {
                Ok(Event::Empty(e)) if e.name().as_ref() == b"style:page-layout-properties" => {
                    read_page_layout(&e);
                }
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"style:header" => in_header = true,
                    b"style:footer" => in_footer = true,
                    b"style:page-layout-properties" => read_page_layout(&e),
                    _ => {}
                },
                Ok(Event::End(e)) => match e.name().as_ref() {
                    b"style:header" => in_header = false,
                    b"style:footer" => in_footer = false,
                    _ => {}
                },
                Ok(Event::Text(t)) => {
                    let txt = t.unescape().unwrap_or_default().to_string();
                    if in_header && !txt.trim().is_empty() {
                        doc.header.get_or_insert_with(String::new).push_str(&txt);
                    }
                    if in_footer && !txt.trim().is_empty() {
                        doc.footer.get_or_insert_with(String::new).push_str(&txt);
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
        }
    }

    doc.ensure_non_empty();
    Ok(doc)
}

fn push_text(
    para: &mut Option<Paragraph>,
    span_stack: &[RunStyle],
    link_stack: &[String],
    text: &str,
) {
    if text.is_empty() {
        return;
    }
    if let Some(p) = para.as_mut() {
        let mut style = span_stack.last().cloned().unwrap_or_default();
        if let Some(href) = link_stack.last() {
            if !href.is_empty() {
                style.link = Some(href.clone());
            }
        }
        // Merge with the previous run when styles match (normalizes the
        // reader output so round-trip comparisons are stable).
        if let Some(last) = p.runs.last_mut() {
            if last.style == style {
                last.text.push_str(text);
                return;
            }
        }
        p.runs.push(Run { text: text.to_string(), style });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(doc: &Document) -> Document {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.odt");
        write(doc, path.to_str().unwrap()).expect("write odt");
        read(path.to_str().unwrap()).expect("read odt")
    }

    #[test]
    fn plain_paragraphs() {
        let d = Document::from_plain_text("first\nsecond\n\nfourth");
        assert_eq!(round_trip(&d).to_plain_text(), d.to_plain_text());
    }

    #[test]
    fn headings_survive() {
        let mut d = Document::from_plain_text("Title\nSection\nbody");
        d.set_heading(0, Some(1));
        d.set_heading(1, Some(3));
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].style.heading, Some(1));
        assert_eq!(rt.paragraphs[1].style.heading, Some(3));
        assert_eq!(rt.paragraphs[2].style.heading, None);
        assert_eq!(rt.to_plain_text(), d.to_plain_text());
    }

    #[test]
    fn inline_styles_survive() {
        let mut d = Document::from_plain_text("normal bold italic under strike");
        d.apply_run_style(7, 11, &StylePatch::set_bold(true));
        d.apply_run_style(12, 18, &StylePatch::set_italic(true));
        d.apply_run_style(19, 24, &StylePatch::set_underline(true));
        d.apply_run_style(25, 31, &StylePatch::set_strikethrough(true));
        let rt = round_trip(&d);
        assert_eq!(rt.to_plain_text(), d.to_plain_text());
        assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
    }

    #[test]
    fn font_size_color_highlight_survive() {
        let mut d = Document::from_plain_text("");
        d.paragraphs[0].runs = vec![
            Run {
                text: "sized".into(),
                style: RunStyle { font_size_hp: Some(28), ..Default::default() },
            },
            Run::plain(" "),
            Run {
                text: "colored".into(),
                style: RunStyle { color: Some("ff0000".into()), ..Default::default() },
            },
            Run::plain(" "),
            Run {
                text: "marked".into(),
                style: RunStyle { highlight: true, ..Default::default() },
            },
        ];
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
    }

    #[test]
    fn links_survive() {
        let mut d = Document::from_plain_text("");
        d.paragraphs[0].runs = vec![
            Run::plain("visit "),
            Run {
                text: "gnome".into(),
                style: RunStyle { link: Some("https://gnome.org".into()), ..Default::default() },
            },
            Run::plain(" now"),
        ];
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
    }

    #[test]
    fn alignment_and_page_break_survive() {
        let mut d = Document::from_plain_text("centered\nright\njustified\nbroken");
        d.paragraphs[0].style.alignment = Alignment::Center;
        d.paragraphs[1].style.alignment = Alignment::Right;
        d.paragraphs[2].style.alignment = Alignment::Justify;
        d.paragraphs[3].style.page_break_before = true;
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].style.alignment, Alignment::Center);
        assert_eq!(rt.paragraphs[1].style.alignment, Alignment::Right);
        assert_eq!(rt.paragraphs[2].style.alignment, Alignment::Justify);
        assert!(rt.paragraphs[3].style.page_break_before);
    }

    #[test]
    fn lists_survive() {
        let mut d = Document::from_plain_text("intro\none\ntwo\nfirst\nsecond\noutro");
        d.paragraphs[1].style.list = ListKind::Bullet;
        d.paragraphs[2].style.list = ListKind::Bullet;
        d.paragraphs[3].style.list = ListKind::Numbered;
        d.paragraphs[4].style.list = ListKind::Numbered;
        let rt = round_trip(&d);
        let kinds: Vec<ListKind> = rt.paragraphs.iter().map(|p| p.style.list).collect();
        assert_eq!(
            kinds,
            vec![
                ListKind::None,
                ListKind::Bullet,
                ListKind::Bullet,
                ListKind::Numbered,
                ListKind::Numbered,
                ListKind::None
            ]
        );
        assert_eq!(rt.to_plain_text(), d.to_plain_text());
    }

    #[test]
    fn header_footer_survive() {
        let mut d = Document::from_plain_text("body");
        d.header = Some("Report — {page}".into());
        d.footer = Some("Confidential".into());
        let rt = round_trip(&d);
        assert_eq!(rt.header.as_deref(), Some("Report — {page}"));
        assert_eq!(rt.footer.as_deref(), Some("Confidential"));
    }

    #[test]
    fn special_chars_escaped() {
        let d = Document::from_plain_text("a < b & c > \"d\"");
        assert_eq!(round_trip(&d).to_plain_text(), d.to_plain_text());
    }

    #[test]
    fn page_geometry_survives() {
        let mut d = Document::from_plain_text("body");
        d.page = Some(PageGeometry {
            width_pt: 612.0,   // US Letter
            height_pt: 792.0,
            margin_top_pt: 36.0,
            margin_bottom_pt: 54.0,
            margin_left_pt: 90.0,
            margin_right_pt: 45.0,
        });
        let rt = round_trip(&d);
        let pg = rt.page.expect("page geometry lost");
        assert!(pg.approx_eq(&d.page.unwrap()), "geometry drifted: {pg:?}");
    }

    #[test]
    fn no_page_geometry_reads_none() {
        let d = Document::from_plain_text("body");
        assert_eq!(round_trip(&d).page, None);
    }

    #[test]
    fn font_family_survives() {
        let mut d = Document::from_plain_text("");
        d.paragraphs[0].runs = vec![
            Run::plain("sans "),
            Run {
                text: "serif".into(),
                style: RunStyle {
                    font_family: Some("Liberation Serif".into()),
                    ..Default::default()
                },
            },
        ];
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
    }

    #[test]
    fn line_spacing_survives() {
        let mut d = Document::from_plain_text("single\ndouble");
        d.paragraphs[1].style.line_spacing = 2.0;
        let rt = round_trip(&d);
        assert!((rt.paragraphs[0].style.line_spacing - 1.0).abs() < 0.01);
        assert!((rt.paragraphs[1].style.line_spacing - 2.0).abs() < 0.01);
    }

    #[test]
    fn empty_paragraphs_preserved() {
        let d = Document::from_plain_text("a\n\n\nb");
        assert_eq!(round_trip(&d).to_plain_text(), "a\n\n\nb");
    }
}
