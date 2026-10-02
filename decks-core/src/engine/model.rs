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

/// Model units per inch: the model slide is 960x540 units for the sheet our
/// writer emits (9144000x5143500 EMU = 10in x 5.625in), so 96 units to the
/// inch — the same 9525 EMU per unit `SlideScale` assumes.
pub const MODEL_UNITS_PER_INCH: f64 = 96.0;

/// The model slide on a PDF page, in points (10in x 5.625in at 72pt/in).
/// This is the "real slide size" the Cairo PDF export uses: the readers scale
/// every source onto the 960x540 model slide, so drawing that slide is what
/// presenting shows — unlike the fixed 16x9cm page the Typst path assumes.
pub fn slide_page_size_pt() -> (f64, f64) {
    const PT_PER_INCH: f64 = 72.0;
    (
        960.0 * PT_PER_INCH / MODEL_UNITS_PER_INCH,
        540.0 * PT_PER_INCH / MODEL_UNITS_PER_INCH,
    )
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
    /// Stable identities for editing by ops (decks_core::ops): the slide's
    /// id, one id per object, and the ids of deleted objects (tombstones).
    /// Readers leave it empty; ops fill it in (`ops::ensure_ids`).
    pub ids: crate::ops::SlideIds,
    /// Which of its master's layouts the slide uses (an index into
    /// `MasterSlide::layouts`); `None` for none.
    pub layout: Option<usize>,
}

/// How much of a picture's source is cut from each side, as fractions of
/// its width (left, right) and height (top, bottom): pptx's `a:srcRect`
/// and ODF's `fo:clip`. The rest is stretched to the picture's box, which
/// is how PowerPoint and Impress draw a picture, cropped or not.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Crop {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Crop {
    /// Nothing is cut.
    pub fn is_none(&self) -> bool {
        *self == Crop::default()
    }

    /// The part of a `iw`×`ih` source that is shown: (x, y, w, h).
    pub fn source_rect(&self, iw: f64, ih: f64) -> (f64, f64, f64, f64) {
        let (l, t) = (self.left.clamp(0.0, 1.0), self.top.clamp(0.0, 1.0));
        let w = (1.0 - l - self.right.clamp(0.0, 1.0)).max(1e-6);
        let h = (1.0 - t - self.bottom.clamp(0.0, 1.0)).max(1e-6);
        (l * iw, t * ih, w * iw, h * ih)
    }

    /// The crop that fills a `bw`×`bh` box with a `iw`×`ih` source without
    /// distorting it, cutting the overflow equally from both sides
    /// (PowerPoint's Crop to Fill).
    pub fn fill(iw: f64, ih: f64, bw: f64, bh: f64) -> Crop {
        if iw <= 0.0 || ih <= 0.0 || bw <= 0.0 || bh <= 0.0 {
            return Crop::default();
        }
        let (src, dst) = (iw / ih, bw / bh);
        if src > dst {
            let cut = (1.0 - dst / src) / 2.0;
            Crop { left: cut, right: cut, ..Default::default() }
        } else {
            let cut = (1.0 - src / dst) / 2.0;
            Crop { top: cut, bottom: cut, ..Default::default() }
        }
    }
}

/// A slide transition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// The size of the slides in the file this deck came from, in EMU
    /// (pptx `p:sldSz`, or an odp page layout converted). The model is
    /// always 960x540 whatever this is: readers scale onto it and the pptx
    /// writer scales back, so a 4:3 or custom-size deck keeps its size.
    /// `None` is the pptx writer's default, 10in x 5.625in. A pptx has one
    /// size for the whole deck; the first master's is the one written.
    pub page_emu: Option<(f64, f64)>,
    /// Its layouts (decks_core::layouts): the arrangements of placeholders
    /// its slides use, each with any look of its own. Empty for a master
    /// read from a file that has none, and for the default master.
    pub layouts: Vec<crate::layouts::Layout>,
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// A picture, stretched to its box after `crop` trims its source.
    Image {
        path: String, x: f64, y: f64, w: f64, h: f64, rotation: f64,
        #[cfg_attr(feature = "serde", serde(default))]
        crop: Crop,
    },
    /// A table (engine::table): a grid of styled cells in a box.
    Table {
        x: f64, y: f64, w: f64, h: f64,
        rotation: f64,
        table: super::table::TableData,
    },
    /// A chart (engine::chart): one series drawn by the renderer Tables
    /// draws its charts with.
    Chart {
        x: f64, y: f64, w: f64, h: f64,
        rotation: f64,
        chart: super::chart::ChartData,
    },
}

impl SlideObject {
    pub fn x(&self) -> f64 {
        match self {
            SlideObject::TextBox { x, .. }
            | SlideObject::Rect { x, .. }
            | SlideObject::Shape { x, .. }
            | SlideObject::Table { x, .. }
            | SlideObject::Chart { x, .. }
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
            | SlideObject::Chart { y, .. }
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
            | SlideObject::Chart { rotation, .. }
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
            page_emu: None,
            layouts: Vec::new(),
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
                ids: Default::default(),
                layout: None,
            }],
            masters: vec![default_master],
        }
    }
}

#[cfg(test)]
mod page_size_tests {
    use super::*;

    #[test]
    fn the_pdf_page_is_the_model_slide_in_points() {
        // The sheet our writer emits is 9144000 EMU wide for 960 model
        // units, and an inch is 914400 EMU: 914400 / (9144000 / 960) = 96
        // units to the inch, i.e. a 10in x 5.625in slide.
        let emu_per_unit = 9144000.0 / 960.0;
        assert_eq!(emu_per_unit, 9525.0);
        assert_eq!(MODEL_UNITS_PER_INCH, 914400.0 / emu_per_unit);
        assert_eq!(slide_page_size_pt(), (720.0, 405.0));
    }

    #[test]
    fn the_pdf_page_keeps_the_slide_aspect() {
        let (w, h) = slide_page_size_pt();
        assert!((w / h - 16.0 / 9.0).abs() < 1e-9, "16:9 like the model slide: {w}x{h}");
    }
}

#[cfg(test)]
mod crop_tests {
    use super::Crop;

    #[test]
    fn the_source_rect_is_what_the_crop_leaves() {
        let c = Crop { left: 0.25, top: 0.1, right: 0.0, bottom: 0.2 };
        let (x, y, w, h) = c.source_rect(400.0, 300.0);
        assert_eq!((x, w), (100.0, 300.0));
        assert!((y - 30.0).abs() < 1e-9 && (h - 210.0).abs() < 1e-9);
        assert_eq!(Crop::default().source_rect(400.0, 300.0), (0.0, 0.0, 400.0, 300.0));
    }

    #[test]
    fn fill_cuts_the_overflow_equally_and_never_distorts() {
        let tall = Crop::fill(100.0, 400.0, 100.0, 100.0);
        assert_eq!((tall.left, tall.right), (0.0, 0.0));
        assert!((tall.top - 0.375).abs() < 1e-9 && (tall.bottom - 0.375).abs() < 1e-9);
        let (_, _, w, h) = tall.source_rect(100.0, 400.0);
        assert!((w / h - 1.0).abs() < 1e-9, "what is left has the box's shape");
        assert!(Crop::fill(200.0, 100.0, 400.0, 200.0).is_none(), "same shape: nothing to cut");
    }
}
