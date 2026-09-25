// model.rs — the deck presentation data model.
use letters_core::model::Run;
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of engine.rs (issue #247).

#[derive(Clone, Debug)]
pub struct Deck {
    pub slides: Vec<Slide>,
    pub masters: Vec<MasterSlide>,
}

#[derive(Clone, Debug)]
pub struct Slide {
    pub title: String,
    pub background: String,
    pub objects: Vec<SlideObject>,
    pub notes: String,
    pub master_idx: Option<usize>,
    /// How this slide arrives when presented (PowerPoint's model: the
    /// transition belongs to the slide it leads into).
    pub transition: Transition,
    /// Objects that build in or out, one per click, in order
    /// (decks_core::builds).
    pub builds: Vec<crate::builds::Build>,
}

/// A slide transition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Transition {
    #[default]
    None,
    Fade,
    Push,
    Wipe,
    /// Keynote's Magic Move (PowerPoint's Morph): objects the two slides
    /// share glide from their old place, size and angle to their new one;
    /// the rest fade (decks_core::magic_move).
    MagicMove,
}

impl Transition {
    pub const ALL: [Transition; 5] = [Transition::None, Transition::Fade, Transition::Push, Transition::Wipe, Transition::MagicMove];

    pub fn label(self) -> &'static str {
        match self {
            Transition::None => "None",
            Transition::Fade => "Dissolve",
            Transition::Push => "Push",
            Transition::Wipe => "Wipe",
            Transition::MagicMove => "Magic Move",
        }
    }
}

#[derive(Clone, Debug)]
pub struct MasterSlide {
    pub name: String,
    pub background: String,
    pub default_font: String,
    pub shapes: Vec<SlideObject>,
}

impl MasterSlide {
    /// The font a master falls back to when it names none.
    ///
    /// Deliberately a generic family rather than a real face: it resolves
    /// on any system, which a named font does not.
    pub const DEFAULT_FONT: &'static str = "Sans";

    /// The font family this master's text is set in.
    ///
    /// `default_font` is a `String`, so a reader that finds no font leaves
    /// it empty rather than absent — and an empty family is not the same
    /// request as no family: it asks pango for a face with no name, and
    /// writes `typeface=""` into a theme, both of which resolve to
    /// whatever the reader likes instead of to the default we intend. One
    /// definition of that policy, because the renderer and both writers
    /// need to agree: a font carried into a package that the canvas would
    /// not have drawn is a round-trip of something nothing honours.
    pub fn font_family(&self) -> &str {
        let named = self.default_font.trim();
        if named.is_empty() { Self::DEFAULT_FONT } else { named }
    }
}

#[derive(Clone, Debug)]
pub enum SlideObject {
    TextBox {
        text: String,
        x: f64, y: f64, w: f64, h: f64,
        rotation: f64,
        /// Styled runs (shared WYSIWYG primitive with Letters). When
        /// non-empty, concatenated run text equals `text`.
        runs: Vec<Run>,
        /// Paragraph styles (alignment, level, bullet, indents, spacing),
        /// vertical anchor and insets. `TextBody::default()` is a plain box.
        body: super::text_body::TextBody,
    },
    Rect { x: f64, y: f64, w: f64, h: f64, rotation: f64 },
    /// A preset shape with its own fill and outline (engine::shape). What
    /// the pptx and odp readers produce; `Rect` and `Circle` are the
    /// editor's older unstyled shapes.
    Shape {
        kind: super::shape::ShapeKind,
        x: f64, y: f64, w: f64, h: f64,
        rotation: f64,
        style: super::shape::ShapeStyle,
    },
    Circle { x: f64, y: f64, r: f64, rotation: f64 },
    Image { path: String, x: f64, y: f64, w: f64, h: f64, rotation: f64 },
    /// A table (engine::table): a grid of styled cells in a box.
    Table {
        x: f64, y: f64, w: f64, h: f64,
        rotation: f64,
        table: super::table::TableData,
    },
}

impl SlideObject {
    pub fn x(&self) -> f64 {
        match self {
            SlideObject::TextBox { x, .. }
            | SlideObject::Rect { x, .. }
            | SlideObject::Shape { x, .. }
            | SlideObject::Table { x, .. }
            | SlideObject::Image { x, .. } => *x,
            SlideObject::Circle { x, r, .. } => *x - *r,
        }
    }
    pub fn y(&self) -> f64 {
        match self {
            SlideObject::TextBox { y, .. }
            | SlideObject::Rect { y, .. }
            | SlideObject::Shape { y, .. }
            | SlideObject::Table { y, .. }
            | SlideObject::Image { y, .. } => *y,
            SlideObject::Circle { y, r, .. } => *y - *r,
        }
    }
    pub fn rotation(&self) -> f64 {
        match self {
            SlideObject::TextBox { rotation, .. }
            | SlideObject::Rect { rotation, .. }
            | SlideObject::Circle { rotation, .. }
            | SlideObject::Shape { rotation, .. }
            | SlideObject::Table { rotation, .. }
            | SlideObject::Image { rotation, .. } => *rotation,
        }
    }
}

impl Default for Deck {
    fn default() -> Self {
        Self::new()
    }
}

impl Deck {
    pub fn new() -> Self {
        let default_master = MasterSlide {
            name: "Default".into(),
            background: "#ffffff".into(),
            default_font: MasterSlide::DEFAULT_FONT.into(),
            shapes: vec![],
        };
        Self {
            slides: vec![Slide {
                title: "Slide 1".into(),
                background: "#ffffff".into(),
                objects: vec![],
                notes: String::new(),
                master_idx: Some(0),
                transition: Default::default(),
                builds: Vec::new(),
            }],
            masters: vec![default_master],
        }
    }
}
