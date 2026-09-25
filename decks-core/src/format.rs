// format.rs — the commands behind Decks' Format inspector.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// docs/DESIGN-UI.md, "The Format inspector": one sidebar edits whatever is
// selected. Each edit is a `FormatEdit`; `apply_edit` says what it does to
// one object (GTK-free, unit-tested), and `DecksController::format_objects`
// applies it to the whole selection as one undo step. An edit that changes
// nothing records nothing. The inspector reads what to show from
// `ObjectFormat::of`.

use suite_common_core::undo::Command;

use crate::engine::shape::{Color, ShapeKind, ShapeStyle, Stroke};
use crate::engine::{Anchor, Bullet, ParaAlign, ParaStyle, Run, RunStyle, Slide, SlideObject};
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
    // Text
    FontFamily(Option<String>),
    FontSize(f64),
    Bold(bool),
    Italic(bool),
    TextColor(Option<String>),
    Align(ParaAlign),
    List(ListKind),
    Anchor(Anchor),
    // Arrange
    X(f64),
    Y(f64),
    Width(f64),
    Height(f64),
    Rotation(f64),
}

impl FormatEdit {
    /// The undo step's name.
    pub fn description(&self) -> &'static str {
        match self {
            FormatEdit::Fill(_) => "Fill",
            FormatEdit::OutlineColor(_) | FormatEdit::OutlineWidth(_) | FormatEdit::NoOutline => "Outline",
            FormatEdit::Kind(_) => "Shape",
            FormatEdit::FontFamily(_) => "Font",
            FormatEdit::FontSize(_) => "Font Size",
            FormatEdit::Bold(_) => "Bold",
            FormatEdit::Italic(_) => "Italic",
            FormatEdit::TextColor(_) => "Text Colour",
            FormatEdit::Align(_) => "Alignment",
            FormatEdit::List(_) => "List",
            FormatEdit::Anchor(_) => "Vertical Alignment",
            FormatEdit::X(_) | FormatEdit::Y(_) => "Move",
            FormatEdit::Width(_) | FormatEdit::Height(_) => "Resize",
            FormatEdit::Rotation(_) => "Rotate",
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
    pub text_color: Option<String>,
    pub align: ParaAlign,
    pub list: ListKind,
    pub anchor: Anchor,
    pub bounds: (f64, f64, f64, f64),
    pub rotation: f64,
}

/// The size a text box's runs are drawn at when they state none.
pub const DEFAULT_FONT_PT: f64 = 18.0;

/// The fill the editor's plain rectangle and circle are drawn in, so that
/// turning one into a styled shape changes nothing on screen.
const PLAIN_RECT: Color = Color(0x4D, 0x80, 0xE6);
const PLAIN_CIRCLE: Color = Color(0xE6, 0x4D, 0x33);

fn list_of(bullet: &Bullet) -> ListKind {
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
            text_color: None,
            align: ParaAlign::Left,
            list: ListKind::None,
            anchor: Anchor::Top,
            bounds: obj_bounds(obj),
            rotation: obj_rotation(obj),
        };
        let first_run = |runs: &[Run], f: &mut ObjectFormat| {
            if let Some(r) = runs.iter().find(|r| !r.text.trim().is_empty()).or(runs.first()) {
                f.font_family = r.style.font_family.clone();
                f.font_size = r.style.font_size_hp.map(|hp| hp as f64 / 2.0);
                f.bold = r.style.bold;
                f.italic = r.style.italic;
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
                f.anchor = body.anchor;
            }
            SlideObject::Table { table, .. } => {
                f.has_text = true;
                if let Some(cell) = table.rows.iter().flatten().find(|c| !c.runs.is_empty()) {
                    first_run(&cell.runs, &mut f);
                }
            }
            SlideObject::Image { .. } => {}
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
            style: ShapeStyle { fill: Some(PLAIN_RECT), gradient: None, stroke: None },
        },
        SlideObject::Circle { x, y, r, rotation } => SlideObject::Shape {
            kind: ShapeKind::Ellipse,
            x: *x - *r,
            y: *y - *r,
            w: *r * 2.0,
            h: *r * 2.0,
            rotation: *rotation,
            style: ShapeStyle { fill: Some(PLAIN_CIRCLE), gradient: None, stroke: None },
        },
        _ => return,
    };
    *obj = promoted;
}

/// Every run of a box, the text made into one run when it has none, so
/// a style can be set on it.
fn runs_mut<'a>(text: &str, runs: &'a mut Vec<Run>) -> &'a mut Vec<Run> {
    if runs.is_empty() && !text.is_empty() {
        runs.push(Run { text: text.to_string(), style: RunStyle::default() });
    }
    runs
}

fn edit_runs(runs: &mut [Run], edit: &FormatEdit) {
    for r in runs {
        let s = &mut r.style;
        match edit {
            FormatEdit::FontFamily(f) => s.font_family = f.clone(),
            FormatEdit::FontSize(pt) => s.font_size_hp = Some((pt * 2.0).round().clamp(1.0, u16::MAX as f64) as u16),
            FormatEdit::Bold(b) => s.bold = *b,
            FormatEdit::Italic(i) => s.italic = *i,
            FormatEdit::TextColor(c) => s.color = c.clone(),
            _ => {}
        }
    }
}

/// Every paragraph's style of a box `text`, one per line, for editing.
fn paras_mut(text: &str, body: &mut crate::engine::TextBody) {
    let n = text.split('\n').count();
    let full: Vec<ParaStyle> = (0..n).map(|i| body.para(i)).collect();
    body.paras = full;
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
                    }
                    FormatEdit::OutlineColor(c) => {
                        let width = style.stroke.map_or(1.0, |s| s.width);
                        style.stroke = Some(Stroke { color: *c, width });
                    }
                    FormatEdit::OutlineWidth(w) => {
                        let color = style.stroke.map_or(Color(0, 0, 0), |s| s.color);
                        style.stroke = Some(Stroke { color, width: w.max(0.0) });
                    }
                    FormatEdit::NoOutline => style.stroke = None,
                    FormatEdit::Kind(k) => *kind = k.clone(),
                    _ => {}
                }
            }
        }
        FormatEdit::FontFamily(_)
        | FormatEdit::FontSize(_)
        | FormatEdit::Bold(_)
        | FormatEdit::Italic(_)
        | FormatEdit::TextColor(_) => match obj {
            SlideObject::TextBox { text, runs, .. } => {
                // A box without runs is plain text: give it a run only if
                // the edit makes that run look different.
                if runs.is_empty() {
                    let mut probe = [Run { text: String::new(), style: RunStyle::default() }];
                    edit_runs(&mut probe, edit);
                    if probe[0].style == RunStyle::default() {
                        return;
                    }
                }
                edit_runs(runs_mut(text, runs), edit)
            }
            SlideObject::Table { table, .. } => {
                for cell in table.rows.iter_mut().flatten() {
                    edit_runs(&mut cell.runs, edit);
                }
            }
            _ => {}
        },
        FormatEdit::Align(_) | FormatEdit::List(_) | FormatEdit::Anchor(_) => {
            let SlideObject::TextBox { text, body, .. } = obj else { return };
            match edit {
                FormatEdit::Anchor(a) => body.anchor = *a,
                FormatEdit::Align(a) => {
                    paras_mut(text, body);
                    for p in &mut body.paras {
                        p.align = *a;
                    }
                }
                FormatEdit::List(kind) => {
                    paras_mut(text, body);
                    for p in &mut body.paras {
                        let was_list = p.bullet != Bullet::None;
                        p.bullet = match kind {
                            ListKind::None => Bullet::None,
                            ListKind::Bullet => Bullet::Char("•".into()),
                            ListKind::Number => Bullet::AutoNum { scheme: "arabicPeriod".into(), start: 1 },
                        };
                        // A hanging indent for the marker, 0.375in a level
                        // as PowerPoint's body style has it; taken away
                        // again when the list is.
                        let step = 36.0;
                        if *kind == ListKind::None {
                            if was_list {
                                p.margin_left = 0.0;
                                p.indent = 0.0;
                            }
                        } else if !was_list || p.indent >= 0.0 {
                            p.margin_left = step * (p.level as f64 + 1.0);
                            p.indent = -step;
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Replaces some of a slide's objects with new versions, and back.
pub struct FormatObjectsCmd {
    pub slide_idx: usize,
    pub before: Vec<(usize, SlideObject)>,
    pub after: Vec<(usize, SlideObject)>,
    pub description: &'static str,
}

impl Command<Vec<Slide>> for FormatObjectsCmd {
    fn apply(&self, slides: &mut Vec<Slide>) {
        put(slides, self.slide_idx, &self.after);
    }
    fn undo(&self, slides: &mut Vec<Slide>) {
        put(slides, self.slide_idx, &self.before);
    }
    fn description(&self) -> &str {
        self.description
    }
}

fn put(slides: &mut [Slide], slide_idx: usize, objects: &[(usize, SlideObject)]) {
    let Some(slide) = slides.get_mut(slide_idx) else { return };
    for (i, o) in objects {
        if let Some(slot) = slide.objects.get_mut(*i) {
            *slot = o.clone();
        }
    }
}

/// The command that applies `edit` to `indices` on `slide`, or `None`
/// when it changes nothing.
pub fn format_command(slide: &Slide, slide_idx: usize, indices: &[usize], edit: &FormatEdit) -> Option<FormatObjectsCmd> {
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut seen = Vec::new();
    for &i in indices {
        if seen.contains(&i) {
            continue;
        }
        seen.push(i);
        let Some(obj) = slide.objects.get(i) else { continue };
        let mut new = obj.clone();
        apply_edit(&mut new, edit);
        if format!("{new:?}") != format!("{obj:?}") {
            before.push((i, obj.clone()));
            after.push((i, new));
        }
    }
    (!after.is_empty()).then_some(FormatObjectsCmd { slide_idx, before, after, description: edit.description() })
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

    fn slide(objects: Vec<SlideObject>) -> Slide {
        Slide { title: String::new(), background: "#ffffff".into(), objects, notes: String::new(), master_idx: None, transition: Default::default() }
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
    fn a_command_covers_the_selection_and_skips_what_it_would_not_change() {
        let s = slide(vec![shape(), text_box("t"), shape()]);
        let cmd = format_command(&s, 0, &[0, 1, 2, 2], &FormatEdit::Fill(Some(Color(1, 1, 1)))).expect("a change");
        assert_eq!(cmd.after.iter().map(|(i, _)| *i).collect::<Vec<_>>(), [0, 2], "the text box has no fill; 2 once");
        assert_eq!(cmd.description(), "Fill");
        let mut slides = vec![s.clone()];
        cmd.apply(&mut slides);
        assert_eq!(ObjectFormat::of(&slides[0].objects[2]).fill, Some(Color(1, 1, 1)));
        cmd.undo(&mut slides);
        assert_eq!(format!("{:?}", slides[0]), format!("{s:?}"));
        // Already that way: no step.
        assert!(format_command(&s, 0, &[1], &FormatEdit::Bold(false)).is_none());
        assert!(format_command(&s, 0, &[0], &FormatEdit::Bold(true)).is_none(), "a shape has no text");
    }
}
