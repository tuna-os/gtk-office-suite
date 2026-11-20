// format.rs — the commands behind Decks' Format inspector.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// docs/DESIGN-UI.md, "The Format inspector": one sidebar edits whatever is
// selected. Each edit is a `FormatEdit`; `apply_edit` says what it does to
// one object (GTK-free, unit-tested), and `DecksController::format_objects`
// applies it to the whole selection as one undo step. An edit that changes
// nothing records nothing. The inspector reads what to show from
// `ObjectFormat::of`.
//
// The same edits act on a text range (DCK-007, `rich_text.rs`): each says
// what it reaches (`FormatEdit::scope`) — characters, paragraphs or the
// whole box — and `edit_style`/`edit_para` are the one definition of what
// it does to a run or a paragraph, whether the inspector formats a whole
// box or the text editor a selection.

use crate::engine::shape::{Color, ShapeKind, ShapeStyle, Stroke};
use crate::engine::theme::{ColorRef, FillSrc, StrokeSrc};
use crate::engine::{Anchor, Bullet, Insets, ParaAlign, ParaStyle, Run, RunStyle, SlideObject, Spacing};
use crate::undo::{obj_bounds, obj_rotation, set_obj_bounds, set_obj_rotation};

/// A paragraph list type, as the inspector offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    None,
    Bullet,
    Number,
}

/// One change the inspector can make.
#[derive(Clone, Debug, PartialEq)]
pub enum FormatEdit {
    // Style
    Fill(Option<Color>),
    OutlineColor(Color),
    OutlineWidth(f64),
    NoOutline,
    Kind(ShapeKind),
    // Text: characters
    FontFamily(Option<String>),
    FontSize(f64),
    Bold(bool),
    Italic(bool),
    Underline(bool),
    Strikethrough(bool),
    TextColor(Option<String>),
    // Text: paragraphs
    Align(ParaAlign),
    List(ListKind),
    /// One list level (or, outside a list, one indent step) in (`true`)
    /// or out.
    Indent(bool),
    /// Space above the paragraph, in points.
    SpaceBefore(f64),
    /// Space below the paragraph, in points.
    SpaceAfter(f64),
    // Text: the box
    Anchor(Anchor),
    /// The same inner margin on every side, in points.
    Insets(f64),
    // Arrange
    X(f64),
    Y(f64),
    Width(f64),
    Height(f64),
    Rotation(f64),
}

/// What a [`FormatEdit`] reaches inside a text box (DCK-007). A range
/// edit changes exactly this much: characters of the selection, the
/// paragraphs it touches, or the box. It never silently widens to the
/// whole box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditScope {
    /// The runs of the selected characters.
    Characters,
    /// Every paragraph the selection touches (the caret's own when
    /// collapsed).
    Paragraphs,
    /// The text box as a whole: anchor and insets.
    TextBox,
    /// Not a text edit: geometry or shape paint.
    Object,
}

impl FormatEdit {
    pub fn scope(&self) -> EditScope {
        match self {
            FormatEdit::FontFamily(_)
            | FormatEdit::FontSize(_)
            | FormatEdit::Bold(_)
            | FormatEdit::Italic(_)
            | FormatEdit::Underline(_)
            | FormatEdit::Strikethrough(_)
            | FormatEdit::TextColor(_) => EditScope::Characters,
            FormatEdit::Align(_)
            | FormatEdit::List(_)
            | FormatEdit::Indent(_)
            | FormatEdit::SpaceBefore(_)
            | FormatEdit::SpaceAfter(_) => EditScope::Paragraphs,
            FormatEdit::Anchor(_) | FormatEdit::Insets(_) => EditScope::TextBox,
            FormatEdit::Fill(_)
            | FormatEdit::OutlineColor(_)
            | FormatEdit::OutlineWidth(_)
            | FormatEdit::NoOutline
            | FormatEdit::Kind(_)
            | FormatEdit::X(_)
            | FormatEdit::Y(_)
            | FormatEdit::Width(_)
            | FormatEdit::Height(_)
            | FormatEdit::Rotation(_) => EditScope::Object,
        }
    }
}

/// What the inspector shows for an object.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectFormat {
    /// The Style tab applies (a shape, or one of the editor's plain ones).
    pub has_style: bool,
    /// The Text tab applies (a text box or a table).
    pub has_text: bool,
    pub kind: Option<ShapeKind>,
    pub fill: Option<Color>,
    pub outline: Option<Stroke>,
    pub font_family: Option<String>,
    /// Points; `None` is the default size.
    pub font_size: Option<f64>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub text_color: Option<String>,
    pub align: ParaAlign,
    pub list: ListKind,
    /// The first paragraph's space above and below, in points, when it
    /// states them as lengths (`None`: a fraction of the line height).
    pub space_before: Option<f64>,
    pub space_after: Option<f64>,
    pub anchor: Anchor,
    /// The box's left inner margin in points, `None` for the default.
    pub inset: Option<f64>,
    pub bounds: (f64, f64, f64, f64),
    pub rotation: f64,
    /// A chart's data, for the Chart tab.
    pub chart: Option<crate::engine::chart::ChartData>,
    /// A picture: the Picture group (crop) applies.
    pub picture: bool,
}

/// The size a text box's runs are drawn at when they state none.
pub const DEFAULT_FONT_PT: f64 = 18.0;

/// The fill the editor's plain rectangle and circle are drawn in, so that
/// turning one into a styled shape changes nothing on screen.
const PLAIN_RECT: Color = Color(0x4D, 0x80, 0xE6);
const PLAIN_CIRCLE: Color = Color(0xE6, 0x4D, 0x33);

/// The model's points-to-units factor: 96 units an inch, 72 points.
const UNITS_PER_PT: f64 = 4.0 / 3.0;

pub(crate) fn list_of(bullet: &Bullet) -> ListKind {
    match bullet {
        Bullet::None => ListKind::None,
        Bullet::Char(_) => ListKind::Bullet,
        Bullet::AutoNum { .. } => ListKind::Number,
    }
}

impl ObjectFormat {
    /// What `obj` looks like, as the inspector's rows show it. Text
    /// properties are the first run's and first paragraph's.
    pub fn of(obj: &SlideObject) -> ObjectFormat {
        let mut f = ObjectFormat {
            has_style: false,
            has_text: false,
            kind: None,
            fill: None,
            outline: None,
            font_family: None,
            font_size: None,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            text_color: None,
            align: ParaAlign::Left,
            list: ListKind::None,
            space_before: None,
            space_after: None,
            anchor: Anchor::Top,
            inset: None,
            bounds: obj_bounds(obj),
            rotation: obj_rotation(obj),
            chart: match obj {
                SlideObject::Chart { chart, .. } => Some(chart.clone()),
                _ => None,
            },
            picture: matches!(obj, SlideObject::Image { .. }),
        };
        let first_run = |runs: &[Run], f: &mut ObjectFormat| {
            if let Some(r) = runs.iter().find(|r| !r.text.trim().is_empty()).or(runs.first()) {
                f.font_family = r.style.font_family.clone();
                f.font_size = r.style.font_size_hp.map(|hp| hp as f64 / 2.0);
                f.bold = r.style.bold;
                f.italic = r.style.italic;
                f.underline = r.style.underline;
                f.strikethrough = r.style.strikethrough;
                f.text_color = r.style.color.clone();
            }
        };
        match obj {
            SlideObject::Shape { kind, style, .. } => {
                f.has_style = true;
                f.kind = Some(kind.clone());
                f.fill = style.fill.or_else(|| style.gradient.as_ref().and_then(|g| g.mean()));
                f.outline = style.stroke;
            }
            SlideObject::Rect { .. } => {
                f.has_style = true;
                f.kind = Some(ShapeKind::Rect);
                f.fill = Some(PLAIN_RECT);
            }
            SlideObject::Circle { .. } => {
                f.has_style = true;
                f.kind = Some(ShapeKind::Ellipse);
                f.fill = Some(PLAIN_CIRCLE);
            }
            SlideObject::TextBox { runs, body, .. } => {
                f.has_text = true;
                first_run(runs, &mut f);
                let p = body.para(0);
                f.align = p.align;
                f.list = list_of(&p.bullet);
                let pt = |s: Spacing| match s {
                    Spacing::Units(u) => Some(u / UNITS_PER_PT),
                    Spacing::Lines(_) => None,
                };
                f.space_before = pt(p.space_before);
                f.space_after = pt(p.space_after);
                f.anchor = body.anchor;
                f.inset = body.insets.map(|i| i.left / UNITS_PER_PT);
            }
            SlideObject::Table { table, .. } => {
                f.has_text = true;
                if let Some(cell) = table.rows.iter().flatten().find(|c| !c.runs.is_empty()) {
                    first_run(&cell.runs, &mut f);
                }
            }
            SlideObject::Image { .. } | SlideObject::Chart { .. } => {}
        }
        f
    }
}

/// The editor's plain rectangle or circle as the styled shape it looks
/// like, so the Style tab can edit it.
fn promote(obj: &mut SlideObject) {
    let promoted = match obj {
        SlideObject::Rect { x, y, w, h, rotation } => SlideObject::Shape {
            kind: ShapeKind::Rect,
            x: *x,
            y: *y,
            w: *w,
            h: *h,
            rotation: *rotation,
            style: ShapeStyle { glow: None, fill: Some(PLAIN_RECT), gradient: None, stroke: None, fill_src: Some(FillSrc::Color(ColorRef::Rgb(PLAIN_RECT.to_hex()))), stroke_src: None },
        },
        SlideObject::Circle { x, y, r, rotation } => SlideObject::Shape {
            kind: ShapeKind::Ellipse,
            x: *x - *r,
            y: *y - *r,
            w: *r * 2.0,
            h: *r * 2.0,
            rotation: *rotation,
            style: ShapeStyle { glow: None, fill: Some(PLAIN_CIRCLE), gradient: None, stroke: None, fill_src: Some(FillSrc::Color(ColorRef::Rgb(PLAIN_CIRCLE.to_hex()))), stroke_src: None },
        },
        _ => return,
    };
    *obj = promoted;
}

/// Apply a character edit to one run's style. Returns whether it changed;
/// any other edit changes nothing here.
pub fn edit_style(s: &mut RunStyle, edit: &FormatEdit) -> bool {
    let before = s.clone();
    match edit {
        FormatEdit::FontFamily(f) => s.font_family = f.clone(),
        FormatEdit::FontSize(pt) => s.font_size_hp = Some((pt * 2.0).round().clamp(1.0, u16::MAX as f64) as u16),
        FormatEdit::Bold(b) => s.bold = *b,
        FormatEdit::Italic(i) => s.italic = *i,
        FormatEdit::Underline(u) => s.underline = *u,
        FormatEdit::Strikethrough(x) => s.strikethrough = *x,
        FormatEdit::TextColor(c) => s.color = c.clone(),
        _ => {}
    }
    *s != before
}

/// One list level (0.375in, as PowerPoint's body style has it).
const LEVEL_STEP: f64 = 36.0;

/// Apply a paragraph edit to one paragraph's style; any other edit
/// changes nothing here.
pub fn edit_para(p: &mut ParaStyle, edit: &FormatEdit) {
    match edit {
        FormatEdit::Align(a) => p.align = *a,
        FormatEdit::List(kind) => {
            let was_list = p.bullet != Bullet::None;
            p.bullet = match kind {
                ListKind::None => Bullet::None,
                ListKind::Bullet => Bullet::Char("•".into()),
                ListKind::Number => Bullet::AutoNum { scheme: "arabicPeriod".into(), start: 1 },
            };
            // A hanging indent for the marker, one level step a level;
            // taken away again when the list is.
            if *kind == ListKind::None {
                if was_list {
                    p.margin_left = 0.0;
                    p.indent = 0.0;
                }
            } else if !was_list || p.indent >= 0.0 {
                p.margin_left = LEVEL_STEP * (p.level as f64 + 1.0);
                p.indent = -LEVEL_STEP;
            }
        }
        // A list paragraph moves a list level (the level both formats
        // write: pptx `lvl`, odp list nesting) with its hanging indent;
        // plain text moves one indent step, never left of the box.
        FormatEdit::Indent(more) => {
            if p.bullet != Bullet::None {
                let level = if *more { (p.level + 1).min(8) } else { p.level.saturating_sub(1) };
                p.level = level;
                p.margin_left = LEVEL_STEP * (level as f64 + 1.0);
                if p.indent >= 0.0 {
                    p.indent = -LEVEL_STEP;
                }
            } else if *more {
                p.margin_left += LEVEL_STEP;
            } else {
                p.margin_left = (p.margin_left - LEVEL_STEP).max(0.0);
            }
        }
        FormatEdit::SpaceBefore(pt) => p.space_before = Spacing::Units(pt.max(0.0) * UNITS_PER_PT),
        FormatEdit::SpaceAfter(pt) => p.space_after = Spacing::Units(pt.max(0.0) * UNITS_PER_PT),
        _ => {}
    }
}

/// Apply a box edit (anchor, insets) to a text body. Returns whether it
/// changed.
pub fn edit_body(body: &mut crate::engine::TextBody, edit: &FormatEdit) -> bool {
    let (anchor, insets) = (body.anchor, body.insets);
    match edit {
        FormatEdit::Anchor(a) => body.anchor = *a,
        FormatEdit::Insets(pt) => {
            let u = pt.max(0.0) * UNITS_PER_PT;
            body.insets = Some(Insets { left: u, top: u, right: u, bottom: u });
        }
        _ => {}
    }
    body.anchor != anchor || body.insets != insets
}

/// Apply `edit` to `obj`. Edits that don't apply to it (a fill on a text
/// box, a font on a picture) leave it alone.
pub fn apply_edit(obj: &mut SlideObject, edit: &FormatEdit) {
    match edit {
        FormatEdit::X(v) | FormatEdit::Y(v) | FormatEdit::Width(v) | FormatEdit::Height(v) => {
            let (mut x, mut y, mut w, mut h) = obj_bounds(obj);
            match edit {
                FormatEdit::X(_) => x = *v,
                FormatEdit::Y(_) => y = *v,
                FormatEdit::Width(_) => w = v.max(1.0),
                _ => h = v.max(1.0),
            }
            set_obj_bounds(obj, x, y, w, h);
        }
        FormatEdit::Rotation(a) => set_obj_rotation(obj, a.rem_euclid(360.0)),
        FormatEdit::Fill(_)
        | FormatEdit::OutlineColor(_)
        | FormatEdit::OutlineWidth(_)
        | FormatEdit::NoOutline
        | FormatEdit::Kind(_) => {
            promote(obj);
            if let SlideObject::Shape { kind, style, .. } = obj {
                match edit {
                    FormatEdit::Fill(c) => {
                        style.fill = *c;
                        style.gradient = None;
                        // A user-picked fill is an explicit local: a colour
                        // keeps its literal reference, no-fill is explicit.
                        style.fill_src = Some(match c {
                            Some(color) => FillSrc::Color(ColorRef::Rgb(color.to_hex())),
                            None => FillSrc::NoFill,
                        });
                    }
                    FormatEdit::OutlineColor(c) => {
                        let width = style.stroke.map_or(1.0, |s| s.width);
                        style.stroke = Some(Stroke { color: *c, width });
                        style.stroke_src = Some(StrokeSrc::Color(ColorRef::Rgb(c.to_hex())));
                    }
                    FormatEdit::OutlineWidth(w) => {
                        let color = style.stroke.map_or(Color(0, 0, 0), |s| s.color);
                        style.stroke = Some(Stroke { color, width: w.max(0.0) });
                        // A user-touched outline is a local override: keep
                        // a carried reference, otherwise pin the colour.
                        if style.stroke_src.is_none() {
                            style.stroke_src = Some(StrokeSrc::Color(ColorRef::Rgb(color.to_hex())));
                        }
                    }
                    FormatEdit::NoOutline => {
                        style.stroke = None;
                        style.stroke_src = Some(StrokeSrc::NoLine);
                    }
                    FormatEdit::Kind(k) => *kind = k.clone(),
                    _ => {}
                }
            }
        }
        FormatEdit::FontFamily(_)
        | FormatEdit::FontSize(_)
        | FormatEdit::Bold(_)
        | FormatEdit::Italic(_)
        | FormatEdit::Underline(_)
        | FormatEdit::Strikethrough(_)
        | FormatEdit::TextColor(_) => match obj {
            SlideObject::TextBox { text, runs, .. } => {
                // A box without runs is plain text: give it a run only if
                // the edit makes that run look different. An empty box
                // keeps the format as its typing style: one empty run.
                if runs.is_empty() {
                    let mut probe = RunStyle::default();
                    if !edit_style(&mut probe, edit) {
                        return;
                    }
                    runs.push(Run { text: text.clone(), style: RunStyle::default() });
                }
                for r in runs.iter_mut() {
                    edit_style(&mut r.style, edit);
                }
            }
            SlideObject::Table { table, .. } => {
                for cell in table.rows.iter_mut().flatten() {
                    for r in &mut cell.runs {
                        edit_style(&mut r.style, edit);
                    }
                }
            }
            _ => {}
        },
        FormatEdit::Align(_)
        | FormatEdit::List(_)
        | FormatEdit::Indent(_)
        | FormatEdit::SpaceBefore(_)
        | FormatEdit::SpaceAfter(_) => {
            let SlideObject::TextBox { text, body, .. } = obj else { return };
            let n = text.split('\n').count();
            let before: Vec<ParaStyle> = (0..n).map(|i| body.para(i)).collect();
            let mut paras = before.clone();
            for p in &mut paras {
                edit_para(p, edit);
            }
            if paras != before {
                body.paras = paras;
            }
        }
        FormatEdit::Anchor(_) | FormatEdit::Insets(_) => {
            if let SlideObject::TextBox { body, .. } = obj {
                edit_body(body, edit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::TextBody;

    fn text_box(text: &str) -> SlideObject {
        SlideObject::TextBox {
            text: text.into(),
            x: 10.0, y: 20.0, w: 300.0, h: 100.0,
            rotation: 0.0,
            runs: vec![],
            body: TextBody::default(),
        }
    }

    fn shape() -> SlideObject {
        SlideObject::Shape { kind: ShapeKind::Rect, x: 0.0, y: 0.0, w: 100.0, h: 50.0, rotation: 0.0, style: ShapeStyle::default() }
    }


    #[test]
    fn a_fill_edit_on_a_plain_rectangle_makes_it_a_styled_shape_that_looks_the_same_until_then() {
        let mut o = SlideObject::Rect { x: 1.0, y: 2.0, w: 3.0, h: 4.0, rotation: 0.0 };
        assert_eq!(ObjectFormat::of(&o).fill, Some(PLAIN_RECT));
        apply_edit(&mut o, &FormatEdit::Fill(Some(Color(1, 2, 3))));
        let SlideObject::Shape { kind, x, w, style, .. } = &o else { panic!("{o:?}") };
        assert_eq!((kind, *x, *w, style.fill), (&ShapeKind::Rect, 1.0, 3.0, Some(Color(1, 2, 3))));
        // A circle keeps its box, not its centre.
        let mut c = SlideObject::Circle { x: 50.0, y: 50.0, r: 10.0, rotation: 0.0 };
        apply_edit(&mut c, &FormatEdit::Kind(ShapeKind::Diamond));
        assert_eq!(obj_bounds(&c), (40.0, 40.0, 20.0, 20.0));
        assert_eq!(ObjectFormat::of(&c).kind, Some(ShapeKind::Diamond));
    }

    #[test]
    fn outline_edits_keep_the_other_half() {
        let mut o = shape();
        apply_edit(&mut o, &FormatEdit::NoOutline);
        assert_eq!(ObjectFormat::of(&o).outline, None);
        apply_edit(&mut o, &FormatEdit::OutlineWidth(3.0));
        assert_eq!(ObjectFormat::of(&o).outline, Some(Stroke { color: Color(0, 0, 0), width: 3.0 }));
        apply_edit(&mut o, &FormatEdit::OutlineColor(Color(9, 9, 9)));
        assert_eq!(ObjectFormat::of(&o).outline, Some(Stroke { color: Color(9, 9, 9), width: 3.0 }));
        // A gradient fill is replaced by a solid one.
        apply_edit(&mut o, &FormatEdit::Fill(None));
        assert_eq!(ObjectFormat::of(&o).fill, None);
    }

    #[test]
    fn text_edits_style_every_run_of_a_plain_box() {
        let mut o = text_box("one\ntwo");
        apply_edit(&mut o, &FormatEdit::Bold(true));
        apply_edit(&mut o, &FormatEdit::FontSize(24.0));
        apply_edit(&mut o, &FormatEdit::FontFamily(Some("Cantarell".into())));
        let SlideObject::TextBox { runs, text, .. } = &o else { panic!() };
        assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<String>(), *text, "runs still spell the text");
        let f = ObjectFormat::of(&o);
        assert!(f.bold && f.has_text && !f.has_style);
        assert_eq!((f.font_size, f.font_family.as_deref()), (Some(24.0), Some("Cantarell")));
    }

    #[test]
    fn a_list_gives_every_paragraph_a_marker_on_a_hanging_indent_and_back() {
        let mut o = text_box("a\nb\nc");
        apply_edit(&mut o, &FormatEdit::List(ListKind::Bullet));
        let SlideObject::TextBox { body, .. } = &o else { panic!() };
        assert_eq!(body.paras.len(), 3);
        assert!(body.paras.iter().all(|p| p.bullet == Bullet::Char("•".into()) && p.indent < 0.0 && p.margin_left > 0.0));
        apply_edit(&mut o, &FormatEdit::List(ListKind::Number));
        assert_eq!(ObjectFormat::of(&o).list, ListKind::Number);
        apply_edit(&mut o, &FormatEdit::List(ListKind::None));
        let SlideObject::TextBox { body, .. } = &o else { panic!() };
        assert!(body.paras.iter().all(|p| p.bullet == Bullet::None && p.margin_left == 0.0 && p.indent == 0.0));
        apply_edit(&mut o, &FormatEdit::Align(ParaAlign::Center));
        apply_edit(&mut o, &FormatEdit::Anchor(Anchor::Middle));
        let f = ObjectFormat::of(&o);
        assert_eq!((f.align, f.anchor), (ParaAlign::Center, Anchor::Middle));
    }

    #[test]
    fn arrange_edits_move_resize_and_rotate() {
        let mut o = shape();
        apply_edit(&mut o, &FormatEdit::X(5.0));
        apply_edit(&mut o, &FormatEdit::Width(40.0));
        apply_edit(&mut o, &FormatEdit::Rotation(-90.0));
        let f = ObjectFormat::of(&o);
        assert_eq!((f.bounds, f.rotation), ((5.0, 0.0, 40.0, 50.0), 270.0));
        // A circle stays round: its box is square on the smaller side.
        let mut c = SlideObject::Circle { x: 50.0, y: 50.0, r: 10.0, rotation: 0.0 };
        apply_edit(&mut c, &FormatEdit::X(0.0));
        assert_eq!(obj_bounds(&c), (0.0, 40.0, 20.0, 20.0));
        let mut t = text_box("t");
        apply_edit(&mut t, &FormatEdit::Height(-5.0));
        assert!(obj_bounds(&t).3 > 0.0, "never collapses");
    }


    #[test]
    fn indent_spacing_insets_and_scope_reach_what_they_name() {
        let mut o = text_box("a\nb");
        apply_edit(&mut o, &FormatEdit::List(ListKind::Bullet));
        apply_edit(&mut o, &FormatEdit::Indent(true));
        let SlideObject::TextBox { body, .. } = &o else { panic!() };
        assert!(body.paras.iter().all(|p| p.level == 1 && p.margin_left == 72.0 && p.indent < 0.0), "{body:?}");
        apply_edit(&mut o, &FormatEdit::Indent(false));
        apply_edit(&mut o, &FormatEdit::Indent(false));
        let SlideObject::TextBox { body, .. } = &o else { panic!() };
        assert!(body.paras.iter().all(|p| p.level == 0 && p.margin_left == 36.0), "never below level 0");
        // Outside a list an indent step moves the text, never left of the box.
        let mut plain = ParaStyle::default();
        edit_para(&mut plain, &FormatEdit::Indent(true));
        assert_eq!((plain.level, plain.margin_left), (0, 36.0));
        edit_para(&mut plain, &FormatEdit::Indent(false));
        edit_para(&mut plain, &FormatEdit::Indent(false));
        assert_eq!(plain.margin_left, 0.0);
        apply_edit(&mut o, &FormatEdit::SpaceBefore(12.0));
        apply_edit(&mut o, &FormatEdit::Insets(9.0));
        let f = ObjectFormat::of(&o);
        assert!((f.space_before.unwrap() - 12.0).abs() < 1e-9 && (f.inset.unwrap() - 9.0).abs() < 1e-9, "{f:?}");
        assert_eq!(FormatEdit::Underline(true).scope(), EditScope::Characters);
        assert_eq!(FormatEdit::Indent(true).scope(), EditScope::Paragraphs);
        assert_eq!(FormatEdit::Insets(1.0).scope(), EditScope::TextBox);
        assert_eq!(FormatEdit::X(1.0).scope(), EditScope::Object);
    }

    #[test]
    fn a_whole_box_format_on_an_empty_box_is_its_typing_style() {
        let mut o = text_box("");
        apply_edit(&mut o, &FormatEdit::Strikethrough(false));
        let SlideObject::TextBox { runs, .. } = &o else { panic!() };
        assert!(runs.is_empty(), "no visible change, no run");
        apply_edit(&mut o, &FormatEdit::Underline(true));
        let SlideObject::TextBox { runs, .. } = &o else { panic!() };
        assert_eq!(runs.len(), 1);
        assert!(runs[0].text.is_empty() && runs[0].style.underline);
        assert!(ObjectFormat::of(&o).underline);
    }
}
