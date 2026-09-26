// odp_layouts.rs — layouts (decks_core::layouts) in ODF.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// ODF has no layout part like pptx's. What it has:
// - `style:presentation-page-layout` in styles.xml's office:styles: a named
//   arrangement of placeholders (`presentation:placeholder`, each with its
//   `presentation:object` and place), which a `draw:page` names in
//   `presentation:presentation-page-layout-name`. It is where a layout's
//   placeholders and a slide's layout go. Page layouts are global in ODF;
//   ours say which master they belong to (`decks:master`), and carry the
//   layout's kind (`decks:kind`) and name (`style:display-name`).
// - A text frame's `presentation:class` (title, subtitle, outline), which
//   says which placeholder the box fills.
// A layout's own background and decorations have no place of their own, so
// a layout with a look is written as one more master page (the master's
// look with the layout's over it, which is what Impress then shows) marked
// `decks:layout-of` its master, and read back into that layout.

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::engine::MasterSlide;
use crate::layouts::{Layout, LayoutKind, LayoutPlaceholder, Placeholder};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn kind_name(k: LayoutKind) -> &'static str {
    k.to_pptx()
}

/// The page layout style name of layout `j` of master `master_style`.
pub(crate) fn page_layout_name(master_style: &str, j: usize) -> String {
    format!("{master_style}-L{}", j + 1)
}

/// The master page a layout with its own look is written as.
pub(crate) fn look_page_name(master_style: &str, j: usize) -> String {
    format!("{master_style}-Look{}", j + 1)
}

/// Whether layout `l` needs a master page of its own.
pub(crate) fn has_look(l: &Layout) -> bool {
    l.background.is_some() || !l.shapes.is_empty()
}

/// The `style:presentation-page-layout`s of master `m` (style name
/// `master_style`), for office:styles. Places go through `x`/`y`, the
/// writer's model-to-points scaling.
pub(crate) fn page_layouts_xml(m: &MasterSlide, master_style: &str, x: &dyn Fn(f64) -> f64, y: &dyn Fn(f64) -> f64) -> String {
    let pt = |v: f64| format!("{}pt", (v * 1e6).round() / 1e6);
    m.layouts
        .iter()
        .enumerate()
        .map(|(j, l)| {
            let places: String = l
                .placeholders
                .iter()
                .map(|p| {
                    format!(
                        "<presentation:placeholder presentation:object=\"{}\" svg:x=\"{}\" svg:y=\"{}\" svg:width=\"{}\" svg:height=\"{}\"/>",
                        p.role.to_odf(),
                        pt(x(p.x)),
                        pt(y(p.y)),
                        pt(x(p.w)),
                        pt(y(p.h))
                    )
                })
                .collect();
            format!(
                "<style:presentation-page-layout style:name=\"{}\" style:display-name=\"{}\" decks:master=\"{}\" decks:kind=\"{}\">{places}</style:presentation-page-layout>",
                esc(&page_layout_name(master_style, j)),
                esc(&l.name),
                esc(master_style),
                kind_name(l.kind)
            )
        })
        .collect()
}

/// The kind of one of Impress's page layouts, from the AutoLayout number
/// its name ends in ("AL2T3": 3, title and two contents). Impress has no
/// section header; a pptx one comes back from it as title and content.
fn impress_kind(name: &str) -> LayoutKind {
    let n = name.rsplit_once('T').and_then(|(_, n)| n.parse::<u32>().ok());
    match n {
        Some(0) => LayoutKind::Title,
        Some(1) => LayoutKind::TitleAndContent,
        Some(3) => LayoutKind::TwoContent,
        Some(19) => LayoutKind::TitleOnly,
        Some(20) => LayoutKind::Blank,
        _ => LayoutKind::Custom,
    }
}

/// The standard layout name for `kind`, if it is a standard one.
fn standard_name(kind: LayoutKind) -> Option<&'static str> {
    use LayoutKind as K;
    Some(match kind {
        K::Title => "Title Slide",
        K::TitleAndContent => "Title and Content",
        K::SectionHeader => "Section Header",
        K::TwoContent => "Two Content",
        K::TitleOnly => "Title Only",
        K::Blank => "Blank",
        K::Custom => return None,
    })
}

/// A page layout as read.
#[derive(Clone, Debug)]
pub(crate) struct PageLayout {
    pub name: String,
    /// The master it belongs to (`decks:master`): `None` in a file we
    /// didn't write, where every master gets every page layout.
    pub master: Option<String>,
    pub layout: Layout,
}

fn attr(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    crate::odp::attr_of(e, name)
}

/// The page layouts of styles.xml, places scaled by `scale` (model units
/// per point, across and down).
pub(crate) fn read_page_layouts(xml: &str, scale: (f64, f64)) -> Vec<PageLayout> {
    let mut out: Vec<PageLayout> = Vec::new();
    let mut reader = Reader::from_str(xml);
    let mut cur: Option<PageLayout> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == "style:presentation-page-layout" => {
                if let Some(done) = cur.take() {
                    out.push(done);
                }
                let Some(name) = attr(&e, "style:name") else { continue };
                let kind = match attr(&e, "decks:kind") {
                    Some(k) => LayoutKind::from_pptx(Some(&k)),
                    None => impress_kind(&name),
                };
                // Impress names its page layouts after their AutoLayout
                // ("AL2T3"), not for people: they take the standard name.
                let display = attr(&e, "style:display-name").unwrap_or_else(|| standard_name(kind).unwrap_or(&name).to_string());
                cur = Some(PageLayout {
                    name,
                    master: attr(&e, "decks:master"),
                    layout: Layout { name: display, kind, background: None, shapes: Vec::new(), placeholders: Vec::new() },
                });
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == "presentation:placeholder" => {
                let Some(pl) = cur.as_mut() else { continue };
                let Some(role) = attr(&e, "presentation:object").as_deref().and_then(Placeholder::from_odf) else { continue };
                let g = |n: &str| attr(&e, n).and_then(|v| crate::odp::parse_length_pt(&v)).unwrap_or(0.0);
                pl.layout.placeholders.push(LayoutPlaceholder {
                    role,
                    x: g("svg:x") * scale.0,
                    y: g("svg:y") * scale.1,
                    w: g("svg:width") * scale.0,
                    h: g("svg:height") * scale.1,
                });
            }
            Ok(Event::End(e)) if e.name().as_ref() == "style:presentation-page-layout" => {
                if let Some(done) = cur.take() {
                    out.push(done);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    if let Some(done) = cur.take() {
        out.push(done);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_layouts_round_trip() {
        let m = MasterSlide {
            name: "M".into(),
            background: "#ffffff".into(),
            default_font: "Sans".into(),
            shapes: vec![],
            page_emu: None,
            layouts: crate::layouts::standard(),
        };
        let xml = format!(
            "<office:document-styles><office:styles>{}</office:styles></office:document-styles>",
            page_layouts_xml(&m, "M", &|v| v, &|v| v)
        );
        let read = read_page_layouts(&xml, (1.0, 1.0));
        assert_eq!(read.len(), 6);
        for (pl, l) in read.iter().zip(&m.layouts) {
            assert_eq!(pl.master.as_deref(), Some("M"));
            assert_eq!((pl.layout.name.as_str(), pl.layout.kind), (l.name.as_str(), l.kind));
            assert_eq!(pl.layout.placeholders, l.placeholders);
        }
        assert_eq!(read[3].name, page_layout_name("M", 3));
    }

    #[test]
    fn impress_page_layouts_are_known_by_their_autolayout() {
        assert_eq!(impress_kind("AL1T0"), LayoutKind::Title);
        assert_eq!(impress_kind("AL2T3"), LayoutKind::TwoContent);
        assert_eq!(impress_kind("AL7T19"), LayoutKind::TitleOnly);
        assert_eq!(impress_kind("AL0T26"), LayoutKind::Custom);
        let xml = "<office:styles><style:presentation-page-layout style:name=\"AL1T0\">\
            <presentation:placeholder presentation:object=\"title\" svg:x=\"1pt\" svg:y=\"2pt\" svg:width=\"3pt\" svg:height=\"4pt\"/>\
            </style:presentation-page-layout></office:styles>";
        let read = read_page_layouts(xml, (1.0, 1.0));
        assert_eq!((read[0].layout.name.as_str(), read[0].layout.kind), ("Title Slide", LayoutKind::Title));
        assert_eq!(read[0].master, None);
    }
}
