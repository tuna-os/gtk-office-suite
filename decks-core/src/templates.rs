// templates.rs — the built-in themes a new deck can start from.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// docs/DESIGN-UI.md, "Templates that look finished": each theme is a real
// deck (a master with its background, font and decorations, and a title
// slide and a bulleted slide set in the theme), not a picture of one. The
// chooser previews them with the canvas's own renderer, and a deck made
// from one saves and reopens like any other.

use crate::engine::shape::{Color, GradientStop, LinearGradient, ShapeKind, ShapeStyle};
use crate::engine::theme::{ColorRef, FillSrc, StrokeSrc};
use crate::engine::text_body::{Anchor, Bullet, ParaAlign, ParaStyle, TextBody};
use crate::engine::{MasterSlide, Slide, SlideObject};
use crate::layouts::Placeholder;
use suite_common_core::document::{Run, RunStyle};

/// A theme as the chooser lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub name: &'static str,
    /// One line on what it looks like, for its tooltip and accessible
    /// description.
    pub description: &'static str,
}

/// How a theme looks.
struct Theme {
    template: Template,
    background: &'static str,
    font: &'static str,
    /// Title and body text, `RRGGBB`.
    title: &'static str,
    body: &'static str,
    /// The master's decorations, drawn under every slide.
    decorations: fn() -> Vec<SlideObject>,
}

fn filled(kind: ShapeKind, (x, y, w, h): (f64, f64, f64, f64), fill: Color) -> SlideObject {
    // An authored fill is an explicit local, kept as a literal reference;
    // an authored shape has no outline, and the writer materialises that
    // as an explicit no-line, so the source states it too (DCK-006).
    let fill_src = Some(FillSrc::Color(ColorRef::Rgb(fill.to_hex())));
    SlideObject::Shape { kind, x, y, w, h, rotation: 0.0, style: ShapeStyle { fill: Some(fill), gradient: None, stroke: None, fill_src, stroke_src: Some(StrokeSrc::NoLine) } }
}

fn none() -> Vec<SlideObject> {
    Vec::new()
}

fn ocean() -> Vec<SlideObject> {
    let (top, bottom) = (Color(0x0B, 0x3D, 0x6B), Color(0x13, 0x8D, 0x9C));
    let gradient = LinearGradient { stops: vec![GradientStop { pos: 0.0, color: top }, GradientStop { pos: 1.0, color: bottom }], angle: 90.0 };
    let fill = gradient.mean();
    vec![SlideObject::Shape {
        kind: ShapeKind::Rect,
        x: 0.0,
        y: 0.0,
        w: 960.0,
        h: 540.0,
        rotation: 0.0,
        style: ShapeStyle { fill, gradient: Some(gradient), stroke: None, fill_src: None, stroke_src: Some(StrokeSrc::NoLine) },
    }]
}

fn paper() -> Vec<SlideObject> {
    vec![filled(ShapeKind::Rect, (80.0, 500.0, 800.0, 3.0), Color(0xB5, 0x5A, 0x3C))]
}

fn bold() -> Vec<SlideObject> {
    vec![
        filled(ShapeKind::Rect, (0.0, 0.0, 28.0, 540.0), Color(0xE0, 0x1B, 0x24)),
        filled(ShapeKind::Ellipse, (820.0, 400.0, 220.0, 220.0), Color(0xFF, 0xD6, 0xD6)),
    ]
}

fn themes() -> Vec<Theme> {
    vec![
        Theme {
            template: Template { name: "Basic White", description: "Dark text on white, nothing else" },
            background: "#ffffff",
            font: "Sans",
            title: "1D1D1F",
            body: "3D3D40",
            decorations: none,
        },
        Theme {
            template: Template { name: "Basic Black", description: "White text on black, for dark rooms" },
            background: "#1d1d1f",
            font: "Sans",
            title: "FFFFFF",
            body: "D0D0D4",
            decorations: none,
        },
        Theme {
            template: Template { name: "Ocean", description: "White text on a deep blue to teal gradient" },
            background: "#0b3d6b",
            font: "Sans",
            title: "FFFFFF",
            body: "E6F4F5",
            decorations: ocean,
        },
        Theme {
            template: Template { name: "Paper", description: "Serif type on warm paper, with a rule" },
            background: "#f6f1e7",
            font: "Serif",
            title: "3B2F2F",
            body: "4A3F3A",
            decorations: paper,
        },
        Theme {
            template: Template { name: "Bold", description: "A red bar and a large soft circle" },
            background: "#ffffff",
            font: "Sans",
            title: "E01B24",
            body: "241F31",
            decorations: bold,
        },
    ]
}

/// The built-in themes, in the order the chooser shows them. The first is
/// the plain one a blank deck uses.
pub fn templates() -> Vec<Template> {
    themes().into_iter().map(|t| t.template).collect()
}

/// A text box filling placeholder `role` of the slide's layout.
#[allow(clippy::too_many_arguments)]
fn text(
    role: Placeholder,
    content: &str,
    (x, y, w, h): (f64, f64, f64, f64),
    color: &str,
    size_pt: u16,
    bold: bool,
    paras: Vec<ParaStyle>,
    anchor: Anchor,
) -> SlideObject {
    let style = RunStyle { bold, font_size_hp: Some(size_pt * 2), color: Some(color.into()), ..Default::default() };
    SlideObject::TextBox {
        text: content.into(),
        x,
        y,
        w,
        h,
        rotation: 0.0,
        runs: vec![Run { text: content.into(), style }],
        body: TextBody { paras, anchor, placeholder: Some(role), ..Default::default() },
    }
}

fn centred() -> ParaStyle {
    ParaStyle { align: ParaAlign::Center, ..Default::default() }
}

fn bullet() -> ParaStyle {
    ParaStyle { bullet: Bullet::Char("\u{2022}".into()), margin_left: 36.0, indent: -28.0, ..Default::default() }
}

fn slide(title: &str, objects: Vec<SlideObject>) -> Slide {
    Slide { hidden: false,
        title: title.into(),
        // White is "unset": the slide shows its master's background.
        background: "#ffffff".into(),
        background_picture: None,
        objects,
        notes: String::new(),
        // Stamped with the template master's id below, once it has one.
        master_id: None,
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout_id: None,
    }
}

/// Template `index` as a new deck: its slides and its master. `None` past
/// the last template.
pub fn deck(index: usize) -> Option<(Vec<Slide>, Vec<MasterSlide>)> {
    let theme = themes().into_iter().nth(index)?;
    let mut masters = vec![MasterSlide {
        // Stamped with a session id below, once, with everything else.
        id: 0,
        name: theme.template.name.into(),
        background: theme.background.into(),
        default_font: theme.font.into(),
        shapes: (theme.decorations)(),
        // Authored without a theme part: resolves against the default.
        theme_id: None,
        color_map: Default::default(),
        // The standard six: the title slide is on the first, the bulleted
        // one on Title and Content, in exactly their places.
        layouts: crate::layouts::standard(),
    }];
    let points = "First point\nSecond point\nThird point";
    let mut slides = vec![
        slide(
            "Title",
            vec![
                text(Placeholder::Title, "Presentation Title", (80.0, 150.0, 800.0, 130.0), theme.title, 48, true, vec![centred()], Anchor::Bottom),
                text(Placeholder::Subtitle, "Subtitle", (80.0, 300.0, 800.0, 60.0), theme.body, 24, false, vec![centred()], Anchor::Top),
            ],
        ),
        slide(
            "Title & Bullets",
            vec![
                text(Placeholder::Title, "Slide Title", (80.0, 40.0, 800.0, 90.0), theme.title, 36, true, vec![ParaStyle::default()], Anchor::Bottom),
                text(Placeholder::Body, points, (80.0, 150.0, 800.0, 320.0), theme.body, 24, false, vec![bullet()], Anchor::Top),
            ],
        ),
    ];
    crate::ops::ensure_all_ids(&mut slides, &mut masters);
    // The title slide is on the first layout, the bulleted one on Title
    // and Content: stable ids, not the positions above.
    let master_id = masters[0].id;
    for (s, j) in slides.iter_mut().zip([0, 1]) {
        s.master_id = Some(master_id);
        s.layout_id = masters[0].layouts.get(j).map(|l| l.id);
    }
    Some((slides, masters))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_is_a_deck_with_a_title_slide_and_a_bulleted_one() {
        let names: Vec<&str> = templates().iter().map(|t| t.name).collect();
        assert_eq!(names, ["Basic White", "Basic Black", "Ocean", "Paper", "Bold"]);
        for (i, name) in names.iter().enumerate() {
            let (slides, masters) = deck(i).unwrap();
            assert_eq!(slides.len(), 2);
            assert_eq!(masters[0].name, *name);
            for s in &slides {
                for o in &s.objects {
                    if let SlideObject::TextBox { text, runs, .. } = o {
                        assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<String>(), *text, "runs spell the box's text");
                    }
                }
            }
        }
        assert!(deck(names.len()).is_none());
    }

    #[test]
    fn a_deck_from_a_template_saves_and_reopens_as_itself() {
        for i in 0..templates().len() {
            let (slides, masters) = deck(i).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("t.pptx");
            let d = crate::engine::Deck { slides, masters, page_size: None, themes: Vec::new(), skipped_slides: Vec::new() };
            crate::engine::write_pptx(path.to_str().unwrap(), &d).unwrap();
            let back = crate::engine::read_pptx(path.to_str().unwrap()).unwrap();
            assert_eq!(back.slides.len(), 2, "template {i}");
            let titles: Vec<String> = back.slides[0]
                .objects
                .iter()
                .filter_map(|o| match o {
                    SlideObject::TextBox { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            assert!(titles.iter().any(|t| t == "Presentation Title"), "template {i}: {titles:?}");
            assert_eq!(back.masters[0].font_family(), d.masters[0].font_family(), "template {i}: the theme's font");
        }
    }
}
