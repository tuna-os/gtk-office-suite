//! placeholders.rs — where a pptx placeholder sits when the slide doesn't say.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! A slide's title, subtitle and body are usually *placeholders*: `p:sp`
//! elements with a `p:ph` and often no `a:xfrm` of their own. Their position
//! and size come from the slide layout's placeholder with the same `idx` (or
//! type), and failing that from the slide master's placeholder of the same
//! kind (ECMA-376 Part 1, 19.3.1.36). Reading a missing xfrm as zero put
//! every such shape at the top-left with no width, and the canvas then wrapped
//! its text one character per line (render lab `decks/title-layout`).
//!
//! Geometry here is raw EMU, exactly as the parts state it; the caller
//! scales it like any other slide coordinate.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

/// What a `p:ph` says about the placeholder: its type (`"obj"` when absent,
/// the schema default) and its index.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PhKey {
    pub ty: String,
    pub idx: Option<u32>,
}

impl PhKey {
    pub(crate) fn from_ph(e: &BytesStart) -> Self {
        let mut ty = None;
        let mut idx = None;
        for attr in e.attributes().flatten() {
            let Ok(v) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) else { continue };
            match attr.key.as_ref() {
                "type" => ty = Some(v.to_string()),
                "idx" => idx = v.parse().ok(),
                _ => {}
            }
        }
        PhKey { ty: ty.unwrap_or_else(|| "obj".into()), idx }
    }

    /// Types that stand for the same slot one level up: a layout's
    /// centred title and a master's title, a subtitle and the body, and so on.
    fn family(&self) -> &str {
        match self.ty.as_str() {
            "ctrTitle" | "title" => "title",
            "subTitle" | "body" | "obj" => "body",
            other => other,
        }
    }

    /// The same, for matching against a *layout*: a subtitle is a slot
    /// of its own there. A layout that has no placeholder for a slide's
    /// content box (say, a body pasted onto a Title Slide, with an idx the
    /// layout doesn't have) must not hand it the layout subtitle's centred,
    /// bullet-less style and position; PowerPoint and LibreOffice fall
    /// through to the master's body instead. One level further up the
    /// broad family is right: a master has only a title and a body.
    fn layout_family(&self) -> &str {
        match self.ty.as_str() {
            "subTitle" => "subTitle",
            _ => self.family(),
        }
    }
}

/// A rectangle in EMU.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EmuRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placeholder {
    pub key: PhKey,
    /// `None` when this part doesn't place it either (look one level up).
    pub rect: Option<EmuRect>,
}

/// Every placeholder shape in a layout or master part, with the geometry
/// its own `p:spPr` gives (if complete).
pub(crate) fn parse_placeholders(xml: &str) -> Vec<Placeholder> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    // (key, x, y, w, h) for the p:sp being read.
    let mut cur: Option<(Option<PhKey>, [Option<f64>; 4])> = None;
    let mut in_sppr = false;
    loop {
        let (e, empty) = match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => (e.into_owned(), false),
            Ok(Event::Empty(e)) => (e.into_owned(), true),
            Ok(Event::End(e)) => {
                match e.name().as_ref() {
                    "p:sp" => {
                        if let Some((Some(key), g)) = cur.take() {
                            let rect = match g {
                                [Some(x), Some(y), Some(w), Some(h)] => Some(EmuRect { x, y, w, h }),
                                _ => None,
                            };
                            out.push(Placeholder { key, rect });
                        }
                    }
                    "p:spPr" => in_sppr = false,
                    _ => {}
                }
                buf.clear();
                continue;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {
                buf.clear();
                continue;
            }
        };
        match e.name().as_ref() {
            "p:sp" if !empty => cur = Some((None, [None; 4])),
            "p:spPr" if !empty => in_sppr = true,
            "p:ph" => {
                if let Some((key, _)) = cur.as_mut() {
                    *key = Some(PhKey::from_ph(&e));
                }
            }
            "a:off" | "a:ext" if in_sppr => {
                if let Some((_, g)) = cur.as_mut() {
                    let (k1, k2, i) = if e.name().as_ref() == "a:off" { ("x", "y", 0) } else { ("cx", "cy", 2) };
                    for attr in e.attributes().flatten() {
                        let v = attr
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .ok()
                            .and_then(|v| v.parse::<f64>().ok());
                        if attr.key.as_ref() == k1 {
                            g[i] = v;
                        } else if attr.key.as_ref() == k2 {
                            g[i + 1] = v;
                        }
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

/// The index of the placeholder among `keys` that `key` inherits from. Most
/// specific first: same type and index, same index within the same family,
/// same type, same family. A title with no `idx` has index 0 by the schema
/// default.
fn find_index(keys: &[&PhKey], key: &PhKey, use_idx: bool) -> Option<usize> {
    let idx = |k: &PhKey| k.idx.unwrap_or(0);
    let tiers: [&dyn Fn(&PhKey) -> bool; 4] = [
        &|p| use_idx && p.ty == key.ty && idx(p) == idx(key),
        &|p| use_idx && idx(p) == idx(key) && p.family() == key.family(),
        &|p| p.ty == key.ty,
        &|p| if use_idx { p.layout_family() == key.layout_family() } else { p.family() == key.family() },
    ];
    tiers.iter().find_map(|t| keys.iter().position(|p| t(p)))
}

fn find<'a>(list: &'a [Placeholder], key: &PhKey, use_idx: bool) -> Option<&'a Placeholder> {
    let keys: Vec<&PhKey> = list.iter().map(|p| &p.key).collect();
    find_index(&keys, key, use_idx).map(|i| &list[i])
}

/// Which layout placeholder and which master placeholder a slide
/// placeholder `key` inherits from, as indices into `layout` and `master`.
/// The layout is matched by index, then type; the master by type only,
/// through the layout's own key when the layout has one.
pub(crate) fn inherited_indices(key: &PhKey, layout: &[&PhKey], master: &[&PhKey]) -> (Option<usize>, Option<usize>) {
    let l = find_index(layout, key, true);
    let up = l.map(|i| layout[i]).unwrap_or(key);
    (l, find_index(master, up, false))
}

impl PhKey {
    /// Which of the master's `p:txStyles` this placeholder's text starts
    /// from: `titleStyle`, `bodyStyle` or `otherStyle`.
    pub(crate) fn text_style_family(&self) -> &str {
        match self.family() {
            "title" => "title",
            "body" => "body",
            _ => "other",
        }
    }
}

/// Where a slide placeholder with no geometry of its own belongs: the
/// layout's matching placeholder, else the master's placeholder of the
/// same kind. Masters are matched by type only; their indices are not
/// related to the slide's.
pub(crate) fn inherited_rect(key: &PhKey, layout: &[Placeholder], master: &[Placeholder]) -> Option<EmuRect> {
    let from_layout = find(layout, key, true);
    if let Some(rect) = from_layout.and_then(|p| p.rect) {
        return Some(rect);
    }
    let up = from_layout.map(|p| &p.key).unwrap_or(key);
    find(master, up, false).and_then(|p| p.rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp(ph: &str, xfrm: Option<(i64, i64, i64, i64)>) -> String {
        let sppr = match xfrm {
            Some((x, y, w, h)) => format!(
                "<p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm></p:spPr>"
            ),
            None => "<p:spPr/>".into(),
        };
        format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"x\"/><p:cNvSpPr/><p:nvPr>{ph}</p:nvPr></p:nvSpPr>{sppr}\
             <p:txBody><a:bodyPr/><a:p><a:r><a:t>t</a:t></a:r></a:p></p:txBody></p:sp>"
        )
    }

    fn part(shapes: &[String]) -> String {
        format!(
            "<p:sldLayout xmlns:p=\"p\" xmlns:a=\"a\"><p:cSld><p:spTree>{}</p:spTree></p:cSld></p:sldLayout>",
            shapes.concat()
        )
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Option<EmuRect> {
        Some(EmuRect { x, y, w, h })
    }

    #[test]
    fn reads_placeholder_geometry_and_skips_plain_shapes() {
        let xml = part(&[
            sp("<p:ph type=\"ctrTitle\"/>", Some((10, 20, 300, 40))),
            sp("<p:ph type=\"subTitle\" idx=\"1\"/>", None),
            sp("", Some((1, 2, 3, 4))),
        ]);
        let got = parse_placeholders(&xml);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].key, PhKey { ty: "ctrTitle".into(), idx: None });
        assert_eq!(got[0].rect, rect(10.0, 20.0, 300.0, 40.0));
        assert_eq!(got[1].key, PhKey { ty: "subTitle".into(), idx: Some(1) });
        assert_eq!(got[1].rect, None);
    }

    #[test]
    fn title_slide_inherits_from_the_layout() {
        let layout = parse_placeholders(&part(&[
            sp("<p:ph type=\"ctrTitle\"/>", Some((10, 20, 300, 40))),
            sp("<p:ph type=\"subTitle\" idx=\"1\"/>", Some((10, 80, 300, 30))),
        ]));
        let title = PhKey { ty: "ctrTitle".into(), idx: None };
        let sub = PhKey { ty: "subTitle".into(), idx: Some(1) };
        assert_eq!(inherited_rect(&title, &layout, &[]), rect(10.0, 20.0, 300.0, 40.0));
        assert_eq!(inherited_rect(&sub, &layout, &[]), rect(10.0, 80.0, 300.0, 30.0));
    }

    #[test]
    fn falls_back_to_the_master_by_kind() {
        // The layout has the placeholder but doesn't place it; the master's
        // title and body do.
        let layout = parse_placeholders(&part(&[
            sp("<p:ph type=\"ctrTitle\"/>", None),
            sp("<p:ph type=\"subTitle\" idx=\"1\"/>", None),
        ]));
        let master = parse_placeholders(&part(&[
            sp("<p:ph type=\"title\"/>", Some((5, 6, 7, 8))),
            sp("<p:ph type=\"body\" idx=\"1\"/>", Some((9, 10, 11, 12))),
        ]));
        let sub = PhKey { ty: "subTitle".into(), idx: Some(1) };
        let title = PhKey { ty: "ctrTitle".into(), idx: None };
        assert_eq!(inherited_rect(&title, &layout, &master), rect(5.0, 6.0, 7.0, 8.0));
        assert_eq!(inherited_rect(&sub, &layout, &master), rect(9.0, 10.0, 11.0, 12.0));
    }

    #[test]
    fn index_wins_over_type_within_a_family() {
        // Two body placeholders on the layout: the slide's idx picks one.
        let layout = parse_placeholders(&part(&[
            sp("<p:ph idx=\"1\"/>", Some((0, 0, 100, 100))),
            sp("<p:ph idx=\"2\"/>", Some((200, 0, 100, 100))),
        ]));
        let second = PhKey { ty: "obj".into(), idx: Some(2) };
        assert_eq!(inherited_rect(&second, &layout, &[]), rect(200.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn a_content_box_with_a_foreign_idx_skips_the_layout_subtitle() {
        // python-pptx's Title Slide layout, and a slide on it carrying a
        // content placeholder whose idx the layout doesn't have (what
        // pasting a body from another slide gives).
        let layout = parse_placeholders(&part(&[
            sp("<p:ph type=\"ctrTitle\"/>", Some((685800, 2130425, 7772400, 1470025))),
            sp("<p:ph type=\"subTitle\" idx=\"1\"/>", Some((1371600, 3886200, 6400800, 1752600))),
        ]));
        let master = parse_placeholders(&part(&[
            sp("<p:ph type=\"title\"/>", Some((457200, 274638, 8229600, 1143000))),
            sp("<p:ph type=\"body\" idx=\"1\"/>", Some((457200, 1600200, 8229600, 4525963))),
        ]));
        let content = PhKey { ty: "obj".into(), idx: Some(13) };
        assert_eq!(inherited_rect(&content, &layout, &master), rect(457200.0, 1600200.0, 8229600.0, 4525963.0));
        let lk: Vec<&PhKey> = layout.iter().map(|p| &p.key).collect();
        let mk: Vec<&PhKey> = master.iter().map(|p| &p.key).collect();
        assert_eq!(inherited_indices(&content, &lk, &mk), (None, Some(1)));
        // A subtitle still finds the layout's subtitle, and without one the
        // master's body.
        let sub = PhKey { ty: "subTitle".into(), idx: Some(1) };
        assert_eq!(inherited_indices(&sub, &lk, &mk), (Some(1), Some(1)));
        assert_eq!(inherited_indices(&sub, &lk[..1], &mk), (None, Some(1)));
    }

    #[test]
    fn a_content_box_with_a_foreign_idx_takes_the_layouts_content_slot() {
        // python-pptx's Title and Content layout: idx 1 is the content box.
        let layout = parse_placeholders(&part(&[
            sp("<p:ph type=\"title\"/>", None),
            sp("<p:ph idx=\"1\"/>", Some((10, 20, 30, 40))),
        ]));
        let content = PhKey { ty: "obj".into(), idx: Some(13) };
        assert_eq!(inherited_rect(&content, &layout, &[]), rect(10.0, 20.0, 30.0, 40.0));
        let body = PhKey { ty: "body".into(), idx: Some(13) };
        assert_eq!(inherited_rect(&body, &layout, &[]), rect(10.0, 20.0, 30.0, 40.0));
    }

    #[test]
    fn nothing_to_inherit_is_none() {
        let key = PhKey { ty: "pic".into(), idx: Some(3) };
        assert_eq!(inherited_rect(&key, &[], &[]), None);
    }
}
