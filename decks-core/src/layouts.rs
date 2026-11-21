// layouts.rs — slide layouts: a master's arrangements of title, subtitle
// and body placeholders, and what applying one to a slide does.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The model follows PowerPoint's: a master (background, font, decorations)
// has several layouts; each slide uses one (`Slide::layout_id`, the id of
// one of its master's `layouts`). A layout places placeholders and may add
// its own background and decorations over the master's. A text box that
// fills a placeholder says which (`TextBody::placeholder`), which is what
// lets a layout move it and a file mark it (pptx `p:ph`, odp
// `presentation:class`).
//
// Applying a layout (`apply`) moves each of the slide's placeholder boxes
// to the layout's place for its role, adds an empty box for a role the
// slide has none for, and leaves a box whose role the layout lacks where
// it is, no longer a placeholder: nothing typed is ever removed.

use crate::engine::{MasterSlide, Slide, SlideObject, TextBody};

/// What a placeholder holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayoutPlaceholder {
    /// Stable identity within its layout (DCK-005): readers assign ids
    /// once at import; 0 means not assigned yet (`ops::ensure_all_ids`).
    pub id: u64,
    pub role: Placeholder,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// The source placeholder this place was read from (DCK-006): the
    /// `p:ph` type as written (`title`, `ctrTitle`, `body`, `obj` …)
    /// and its `idx`, when the part states them. `arrange` matches a
    /// slide box carrying the same source key to this place before
    /// falling back to nth-of-role, so duplicate body placeholders
    /// (two `obj` boxes with different indices) survive a layout
    /// application on their own places instead of collapsing onto the
    /// first. Absent for authored layouts and ODP layouts, which match
    /// by role alone, as before.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_ty: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_idx: Option<u32>,
}

/// A slide text box's source placeholder (DCK-006): the `p:ph` type and
/// `idx` the part stated, kept on [`crate::engine::TextBody`] so
/// [`arrange`] can match the box back to its layout place by stable
/// source identity rather than by nth body alone.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlaceholderKey {
    pub ty: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idx: Option<u32>,
}

/// One layout of a master.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Layout {
    /// Stable identity within its master (DCK-005). Slides name their
    /// layout by this id ([`crate::engine::Slide::layout_id`]); readers
    /// assign ids once at import and 0 means not assigned yet.
    pub id: u64,
    pub name: String,
    pub kind: LayoutKind,
    /// Its own background over the master's, if it has one.
    pub background: Option<String>,
    /// Its own decorations, drawn over the master's.
    pub shapes: Vec<SlideObject>,
    pub placeholders: Vec<LayoutPlaceholder>,
}

fn ph(role: Placeholder, x: f64, y: f64, w: f64, h: f64) -> LayoutPlaceholder {
    LayoutPlaceholder { id: 0, role, x, y, w, h, source_ty: String::new(), source_idx: None }
}

fn layout(name: &str, kind: LayoutKind, placeholders: Vec<LayoutPlaceholder>) -> Layout {
    Layout { id: 0, name: name.into(), kind, background: None, shapes: Vec::new(), placeholders }
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

/// A master's background and decorations for a slide on layout `layout_id`:
/// the layout's background over the master's, and its decorations after
/// the master's. The layout is resolved by stable id; an unknown id draws
/// the bare master rather than a positional guess.
pub fn effective_master(master: &MasterSlide, layout_id: Option<u64>) -> MasterSlide {
    let Some(l) = layout_id.and_then(|id| master.layout_by_id(id)) else { return master.clone() };
    let mut m = master.clone();
    if let Some(bg) = &l.background {
        m.background = bg.clone();
    }
    m.shapes.extend(l.shapes.iter().cloned());
    m
}

/// The background colour everything falls back to when no level states
/// one: white. The one documented fallback in the slide → layout →
/// master → theme precedence (DCK-006). Import never writes this into
/// the model (an absent background stays empty); renderers and writers
/// apply it at the border.
pub const FALLBACK_BACKGROUND: &str = "#ffffff";

/// A slide's background as drawn: its own when it states one, else its
/// layout's, else its master's, else [`FALLBACK_BACKGROUND`]. Empty
/// means absent at every level — never white smuggled in at import.
/// An explicit white slide on a coloured master stays white.
pub fn resolve_background<'a>(slide_bg: &'a str, layout_bg: Option<&'a str>, master_bg: &'a str) -> &'a str {
    crate::engine::resolver::stated_background(slide_bg, layout_bg, master_bg).unwrap_or(FALLBACK_BACKGROUND)
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
/// layout's places, a box added for each place left over, and a box
/// with no place left where it is, no longer a placeholder.
///
/// A box carrying its source placeholder key (`TextBody::placeholder_key`,
/// kept by the readers) matches by stable source identity first — same
/// type and index, then same type — so duplicate body placeholders
/// (two `obj` boxes with different indices) keep their own places.
/// A box with no key, or with no keyed place left, falls back to the
/// n-th box of a role to the n-th place of it, as before.
pub fn arrange(s: &Slide, layout: &Layout) -> Vec<SlideObject> {
    let key_of = |o: &SlideObject| -> Option<(Placeholder, Option<PlaceholderKey>)> {
        match o {
            SlideObject::TextBox { body, .. } => body.placeholder.map(|role| (role, body.placeholder_key.clone())),
            _ => None,
        }
    };
    // The score of place `p` for a box with source key `key`: exact
    // (type, index) beats same type; nothing else matches. `None` never
    // matches, so keyless boxes and keyless places only meet on the
    // role fallback below.
    let score = |p: &LayoutPlaceholder, key: Option<&PlaceholderKey>| -> Option<u8> {
        let key = key?;
        if key.ty.is_empty() || key.ty != p.source_ty {
            return None;
        }
        if key.idx.is_some() && key.idx == p.source_idx {
            return Some(0);
        }
        if !p.source_ty.is_empty() {
            return Some(1);
        }
        None
    };
    let mut used = vec![false; layout.placeholders.len()];
    let mut out: Vec<SlideObject> = s
        .objects
        .iter()
        .map(|o| {
            let Some((role, key)) = key_of(o) else { return o.clone() };
            // Best keyed match first; role fallback only when no keyed
            // place is left for this box.
            let mut best: Option<(u8, usize)> = None;
            for (i, p) in layout.placeholders.iter().enumerate() {
                if used[i] {
                    continue;
                }
                if let Some(q) = score(p, key.as_ref()) {
                    if best.is_none_or(|(b, _)| q < b) {
                        best = Some((q, i));
                    }
                }
            }
            let place = best
                .map(|(_, i)| (i, layout.placeholders[i].clone()))
                .or_else(|| {
                    layout.placeholders.iter().enumerate().find(|(i, p)| !used[*i] && p.role == role).map(|(i, p)| (i, p.clone()))
                });
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
        Slide { hidden: false,
            title: "s".into(),
            background: "#ffffff".into(),
            background_picture: None,
            objects,
            notes: String::new(),
            master_id: Some(7),
            transition: Default::default(),
            builds: vec![],
            ids: Default::default(),
            layout_id: Some(3),
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
        let m = MasterSlide {
            id: 7,
            name: "M".into(),
            background: "#ffffff".into(),
            default_font: "Sans".into(),
            shapes: vec![SlideObject::Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0, rotation: 0.0 }],
            theme_id: None,
            color_map: Default::default(),
            layouts: standard(),
        };
        let mut masters = vec![m];
        crate::ops::ensure_all_ids(&mut Vec::new(), &mut masters);
        let mut m = masters.pop().unwrap();
        m.layouts[2].background = Some("#102030".into());
        m.layouts[2].shapes.push(SlideObject::Rect { x: 5.0, y: 5.0, w: 1.0, h: 1.0, rotation: 0.0 });
        let third = m.layouts[2].id;
        let first = m.layouts[0].id;
        let e = effective_master(&m, Some(third));
        assert_eq!(e.background, "#102030");
        assert_eq!(e.shapes.len(), 2);
        assert_eq!(effective_master(&m, Some(first)).background, "#ffffff");
        assert_eq!(effective_master(&m, None).shapes.len(), 1);
        assert_eq!(effective_master(&m, Some(u64::MAX)).shapes.len(), 1, "an unknown layout id draws the bare master");
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

    #[test]
    fn backgrounds_resolve_slide_over_layout_over_master_over_fallback() {
        assert_eq!(resolve_background("#112233", Some("#445566"), "#778899"), "#112233");
        assert_eq!(resolve_background("", Some("#445566"), "#778899"), "#445566");
        assert_eq!(resolve_background("", None, "#778899"), "#778899");
        assert_eq!(resolve_background("", None, ""), FALLBACK_BACKGROUND);
        // An explicit white slide on a coloured master stays white.
        assert_eq!(resolve_background("#ffffff", None, "#0b3d6b"), "#ffffff");
    }

    #[test]
    fn duplicate_body_placeholders_keep_their_own_places_by_source_index() {
        // Two `obj` places with different indices, and two boxes that
        // filled them — in swapped z-order, so nth-of-role alone would
        // cross them.
        let mut layout = Layout {
            id: 3,
            name: "Two".into(),
            kind: LayoutKind::TwoContent,
            background: None,
            shapes: Vec::new(),
            placeholders: vec![
                LayoutPlaceholder { id: 1, role: Placeholder::Body, x: 10.0, y: 10.0, w: 100.0, h: 100.0, source_ty: "obj".into(), source_idx: Some(1) },
                LayoutPlaceholder { id: 2, role: Placeholder::Body, x: 200.0, y: 10.0, w: 100.0, h: 100.0, source_ty: "obj".into(), source_idx: Some(2) },
            ],
        };
        let keyed = |idx: u32, text: &str| SlideObject::TextBox {
            text: text.into(),
            x: 1.0, y: 2.0, w: 3.0, h: 4.0, rotation: 0.0, runs: vec![],
            body: TextBody {
                placeholder: Some(Placeholder::Body),
                placeholder_key: Some(PlaceholderKey { ty: "obj".into(), idx: Some(idx) }),
                ..Default::default()
            },
        };
        let out = arrange(&slide(vec![keyed(2, "second"), keyed(1, "first")]), &layout);
        let at: Vec<(&str, f64)> = out
            .iter()
            .filter_map(|o| match o {
                SlideObject::TextBox { text, x, .. } => Some((text.as_str(), *x)),
                _ => None,
            })
            .collect();
        assert_eq!(at, vec![("second", 200.0), ("first", 10.0)], "index wins over position: {at:?}");
        // Keyless boxes still match nth-of-role.
        layout.placeholders[0].source_ty.clear();
        layout.placeholders[1].source_ty.clear();
        let out = arrange(&slide(vec![boxed(Some(Placeholder::Body), "a"), boxed(Some(Placeholder::Body), "b")]), &layout);
        let texts: Vec<&str> = out.iter().filter_map(|o| match o {
            SlideObject::TextBox { text, .. } => Some(text.as_str()),
            _ => None,
        }).collect();
        assert_eq!(texts, vec!["a", "b"]);
    }
}
