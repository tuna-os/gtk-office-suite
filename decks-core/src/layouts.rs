// layouts.rs — slide layouts: a master's arrangements of title, subtitle
// and body placeholders, and what applying one to a slide does.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The model follows PowerPoint's: a master (background, font, decorations)
// has several layouts; each slide uses one (`Slide::layout`, an index into
// its master's `layouts`). A layout places placeholders and may add its own
// background and decorations over the master's. A text box that fills a
// placeholder says which (`TextBody::placeholder`), which is what lets a
// layout move it and a file mark it (pptx `p:ph`, odp `presentation:class`).
//
// Applying a layout (`apply`) moves each of the slide's placeholder boxes
// to the layout's place for its role, adds an empty box for a role the
// slide has none for, and leaves a box whose role the layout lacks where
// it is, no longer a placeholder: nothing typed is ever removed.

use crate::engine::{MasterSlide, Slide, SlideObject, TextBody};

/// What a placeholder holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Placeholder {
    Title,
    Subtitle,
    Body,
}

impl Placeholder {
    /// PowerPoint's `p:ph type` (a centred title is still a title).
    pub fn to_pptx(self) -> &'static str {
        match self {
            Placeholder::Title => "title",
            Placeholder::Subtitle => "subTitle",
            Placeholder::Body => "body",
        }
    }

    /// The role of a pptx placeholder type; `obj` and an untyped
    /// placeholder with an index are content, which is a body.
    pub fn from_pptx(ty: Option<&str>) -> Option<Placeholder> {
        match ty {
            Some("title") | Some("ctrTitle") => Some(Placeholder::Title),
            Some("subTitle") => Some(Placeholder::Subtitle),
            Some("body") | Some("obj") | None => Some(Placeholder::Body),
            _ => None,
        }
    }

    /// ODF's `presentation:class` / `presentation:object`.
    pub fn to_odf(self) -> &'static str {
        match self {
            Placeholder::Title => "title",
            Placeholder::Subtitle => "subtitle",
            Placeholder::Body => "outline",
        }
    }

    pub fn from_odf(class: &str) -> Option<Placeholder> {
        match class {
            "title" => Some(Placeholder::Title),
            "subtitle" => Some(Placeholder::Subtitle),
            "outline" | "text" => Some(Placeholder::Body),
            _ => None,
        }
    }

    /// The prompt an empty placeholder shows while editing.
    pub fn prompt(self) -> &'static str {
        match self {
            Placeholder::Title => "Click to add title",
            Placeholder::Subtitle => "Click to add subtitle",
            Placeholder::Body => "Click to add text",
        }
    }
}

/// Which standard arrangement a layout is (pptx `p:sldLayout type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutKind {
    Title,
    TitleAndContent,
    SectionHeader,
    TwoContent,
    TitleOnly,
    Blank,
    /// Anything else a file names.
    Custom,
}

impl LayoutKind {
    pub fn to_pptx(self) -> &'static str {
        match self {
            LayoutKind::Title => "title",
            LayoutKind::TitleAndContent => "obj",
            LayoutKind::SectionHeader => "secHead",
            LayoutKind::TwoContent => "twoObj",
            LayoutKind::TitleOnly => "titleOnly",
            LayoutKind::Blank => "blank",
            LayoutKind::Custom => "cust",
        }
    }

    pub fn from_pptx(ty: Option<&str>) -> LayoutKind {
        match ty {
            Some("title") => LayoutKind::Title,
            Some("obj") | Some("tx") => LayoutKind::TitleAndContent,
            Some("secHead") => LayoutKind::SectionHeader,
            Some("twoObj") | Some("twoTxTwoObj") => LayoutKind::TwoContent,
            Some("titleOnly") => LayoutKind::TitleOnly,
            Some("blank") => LayoutKind::Blank,
            _ => LayoutKind::Custom,
        }
    }
}

/// One placeholder's place on a layout, in model units.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutPlaceholder {
    pub role: Placeholder,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// One layout of a master.
#[derive(Clone, Debug)]
pub struct Layout {
    pub name: String,
    pub kind: LayoutKind,
    /// Its own background over the master's, if it has one.
    pub background: Option<String>,
    /// Its own decorations, drawn over the master's.
    pub shapes: Vec<SlideObject>,
    pub placeholders: Vec<LayoutPlaceholder>,
}

fn ph(role: Placeholder, x: f64, y: f64, w: f64, h: f64) -> LayoutPlaceholder {
    LayoutPlaceholder { role, x, y, w, h }
}

fn layout(name: &str, kind: LayoutKind, placeholders: Vec<LayoutPlaceholder>) -> Layout {
    Layout { name: name.into(), kind, background: None, shapes: Vec::new(), placeholders }
}

/// The standard layouts a new master gets (PowerPoint's and Keynote's
/// usual six), on the model's 960x540 slide.
pub fn standard() -> Vec<Layout> {
    use LayoutKind as K;
    use Placeholder as P;
    vec![
        layout("Title Slide", K::Title, vec![ph(P::Title, 80.0, 150.0, 800.0, 130.0), ph(P::Subtitle, 80.0, 300.0, 800.0, 60.0)]),
        layout("Title and Content", K::TitleAndContent, vec![ph(P::Title, 80.0, 40.0, 800.0, 90.0), ph(P::Body, 80.0, 150.0, 800.0, 320.0)]),
        layout("Section Header", K::SectionHeader, vec![ph(P::Title, 80.0, 200.0, 800.0, 100.0), ph(P::Body, 80.0, 310.0, 800.0, 60.0)]),
        layout(
            "Two Content",
            K::TwoContent,
            vec![ph(P::Title, 80.0, 40.0, 800.0, 90.0), ph(P::Body, 80.0, 150.0, 390.0, 320.0), ph(P::Body, 490.0, 150.0, 390.0, 320.0)],
        ),
        layout("Title Only", K::TitleOnly, vec![ph(P::Title, 80.0, 40.0, 800.0, 90.0)]),
        layout("Blank", K::Blank, vec![]),
    ]
}

/// A master's background and decorations for a slide on its layout
/// `layout`: the layout's background over the master's, and its
/// decorations after the master's.
pub fn effective_master(master: &MasterSlide, layout: Option<usize>) -> MasterSlide {
    let Some(l) = layout.and_then(|i| master.layouts.get(i)) else { return master.clone() };
    let mut m = master.clone();
    if let Some(bg) = &l.background {
        m.background = bg.clone();
    }
    m.shapes.extend(l.shapes.iter().cloned());
    m
}

/// The placeholder a slide object fills, if any.
pub fn role_of(o: &SlideObject) -> Option<Placeholder> {
    match o {
        SlideObject::TextBox { body, .. } => body.placeholder,
        _ => None,
    }
}

/// An empty text box filling `p`.
pub fn empty_placeholder(p: &LayoutPlaceholder) -> SlideObject {
    SlideObject::TextBox {
        text: String::new(),
        x: p.x,
        y: p.y,
        w: p.w,
        h: p.h,
        rotation: 0.0,
        runs: vec![],
        body: TextBody { placeholder: Some(p.role), ..Default::default() },
    }
}

/// Slide `s`'s objects on `layout`: its placeholder boxes moved to the
/// layout's places (the n-th box of a role to the n-th place of it), a
/// box added for each place left over, and a box with no place left where
/// it is, no longer a placeholder.
pub fn arrange(s: &Slide, layout: &Layout) -> Vec<SlideObject> {
    let mut used = vec![false; layout.placeholders.len()];
    let mut out: Vec<SlideObject> = s
        .objects
        .iter()
        .map(|o| {
            let Some(role) = role_of(o) else { return o.clone() };
            let place = layout.placeholders.iter().enumerate().find(|(i, p)| !used[*i] && p.role == role).map(|(i, p)| (i, p.clone()));
            let mut o = o.clone();
            if let SlideObject::TextBox { x, y, w, h, body, .. } = &mut o {
                match place {
                    Some((i, p)) => {
                        used[i] = true;
                        (*x, *y, *w, *h) = (p.x, p.y, p.w, p.h);
                    }
                    None => body.placeholder = None,
                }
            }
            o
        })
        .collect();
    for (i, p) in layout.placeholders.iter().enumerate() {
        if !used[i] {
            out.push(empty_placeholder(p));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(role: Option<Placeholder>, text: &str) -> SlideObject {
        SlideObject::TextBox {
            text: text.into(),
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
            rotation: 0.0,
            runs: vec![],
            body: TextBody { placeholder: role, ..Default::default() },
        }
    }

    fn slide(objects: Vec<SlideObject>) -> Slide {
        Slide {
            title: "s".into(),
            background: "#ffffff".into(),
            objects,
            notes: String::new(),
            master_idx: Some(0),
            transition: Default::default(),
            builds: vec![],
            ids: Default::default(),
            layout: Some(0),
        }
    }

    #[test]
    fn arranging_moves_fills_and_never_drops() {
        let layouts = standard();
        let two = &layouts[3];
        let s = slide(vec![boxed(Some(Placeholder::Title), "Hello"), boxed(None, "free text"), boxed(Some(Placeholder::Subtitle), "sub")]);
        let out = arrange(&s, two);
        // Title moved; free text untouched; the subtitle, with no place on
        // Two Content, stays where it was as a plain box; two bodies added.
        assert_eq!(out.len(), 5);
        match &out[0] {
            SlideObject::TextBox { x, y, text, body, .. } => {
                assert_eq!((*x, *y, text.as_str(), body.placeholder), (80.0, 40.0, "Hello", Some(Placeholder::Title)));
            }
            o => panic!("{o:?}"),
        }
        assert_eq!(format!("{:?}", out[1]), format!("{:?}", s.objects[1]));
        match &out[2] {
            SlideObject::TextBox { x, text, body, .. } => assert_eq!((*x, text.as_str(), body.placeholder), (1.0, "sub", None)),
            o => panic!("{o:?}"),
        }
        let added: Vec<(f64, Option<Placeholder>)> = out[3..]
            .iter()
            .map(|o| match o {
                SlideObject::TextBox { x, body, .. } => (*x, body.placeholder),
                o => panic!("{o:?}"),
            })
            .collect();
        assert_eq!(added, vec![(80.0, Some(Placeholder::Body)), (490.0, Some(Placeholder::Body))]);
    }

    #[test]
    fn a_layouts_look_goes_over_its_masters() {
        let mut m = MasterSlide {
            name: "M".into(),
            background: "#ffffff".into(),
            default_font: "Sans".into(),
            shapes: vec![SlideObject::Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0, rotation: 0.0 }],
            page_emu: None,
            layouts: standard(),
        };
        m.layouts[2].background = Some("#102030".into());
        m.layouts[2].shapes.push(SlideObject::Rect { x: 5.0, y: 5.0, w: 1.0, h: 1.0, rotation: 0.0 });
        let e = effective_master(&m, Some(2));
        assert_eq!(e.background, "#102030");
        assert_eq!(e.shapes.len(), 2);
        assert_eq!(effective_master(&m, Some(0)).background, "#ffffff");
        assert_eq!(effective_master(&m, None).shapes.len(), 1);
    }

    #[test]
    fn roles_and_kinds_map_to_both_formats() {
        for p in [Placeholder::Title, Placeholder::Subtitle, Placeholder::Body] {
            assert_eq!(Placeholder::from_pptx(Some(p.to_pptx())), Some(p));
            assert_eq!(Placeholder::from_odf(p.to_odf()), Some(p));
        }
        assert_eq!(Placeholder::from_pptx(Some("ctrTitle")), Some(Placeholder::Title));
        assert_eq!(Placeholder::from_pptx(Some("sldNum")), None);
        for l in standard() {
            assert_eq!(LayoutKind::from_pptx(Some(l.kind.to_pptx())), l.kind);
        }
    }
}
