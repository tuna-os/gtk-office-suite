// model.rs — the deck presentation data model.
use letters_core::model::Run;
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of engine.rs (issue #247).

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Deck {
    pub slides: Vec<Slide>,
    pub masters: Vec<MasterSlide>,
    /// The slide size the source package declares, in EMU (pptx `p:sldSz`,
    /// or an ODP page layout converted at 12700 EMU to the point).
    /// `None` is the size our own writer emits (10in x 5.625in,
    /// `SlideScale::DEFAULT_EMU`). Document-level since DCK-006: it used
    /// to live on every master (`MasterSlide::page_emu`), so the writer
    /// silently took the first master's and a reorder could change the
    /// size of the whole deck. The model is always 960x540 whatever this
    /// is: readers scale onto it and the writers scale back through the
    /// one conversion (`page_emu`), so a 4:3 or custom-size deck keeps
    /// its size. An ODP whose pages disagree keeps the first slide page's
    /// size here and is classified by the loss report (`mixed-page-sizes`),
    /// never silently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_size: Option<(f64, f64)>,
    /// One entry per theme part the package names, in package order
    /// (engine::theme). Each master names its theme by stable id
    /// (`MasterSlide::theme_id`); empty for formats without themes (ODP)
    /// and for new documents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub themes: Vec<super::theme::Theme>,
    /// Slide parts the PPTX reader could not read — missing from the
    /// package, corrupt, or past the zip budget — in presentation order
    /// (#1977). Those slides are absent from `slides`; this record is the
    /// only trace they existed, so the open report
    /// (`read_deck_with_report`) and the save-time loss check can name
    /// them instead of letting a save silently make the loss permanent.
    /// Empty for ODP (its slides live in parts that either read or fail
    /// the open as a whole) and for new documents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_slides: Vec<SkippedSlide>,
}

/// A slide the PPTX reader skipped because its part could not be read
/// (#1977): which part, and why the read failed, in the zip layer's words.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkippedSlide {
    pub part: String,
    pub reason: String,
}

/// Model units per inch: the model slide is 960x540 units for the sheet our
/// writer emits (9144000x5143500 EMU = 10in x 5.625in), so 96 units to the
/// inch — the same 9525 EMU per unit `SlideScale` assumes.
pub const MODEL_UNITS_PER_INCH: f64 = 96.0;

/// A line break inside a paragraph, as run text holds it: U+2028 (LINE
/// SEPARATOR), which Pango breaks the line at without starting a
/// paragraph. pptx writes it `a:br`, ODF `text:line-break`. A `'\n'` in run
/// text is a paragraph break; this is not one, so paragraph metadata
/// (`TextBody` paragraphs, bullets, spacing) is unaffected by it.
pub const LINE_BREAK: char = '\u{2028}';

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

/// The slides a show or an export shows, in order: all but the hidden
/// ones, as PowerPoint and Impress do. A deck whose every slide is hidden
/// shows them all rather than nothing.
pub fn shown_slides(slides: &[Slide]) -> Vec<usize> {
    let shown: Vec<usize> = (0..slides.len()).filter(|&i| !slides[i].hidden).collect();
    if shown.is_empty() { (0..slides.len()).collect() } else { shown }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Slide {
    pub title: String,
    /// The slide's own background (`p:bg`), as `#rrggbb`. Empty means the
    /// slide states none and inherits its layout's, then its master's
    /// (DCK-006): import never turns that absence into white, rendering
    /// falls back to [`crate::layouts::FALLBACK_BACKGROUND`] only at draw
    /// time, and the writers omit an empty background. An explicit white
    /// slide on a coloured master stays white; it does not inherit.
    pub background: String,
    /// A picture the slide's own background is filled with (pptx `p:bg`
    /// `a:blipFill`, ODF's bitmap drawing-page fill), drawn over
    /// `background`. `None` for a slide filled with a colour or inheriting.
    /// A slide that states a picture states its own background, so it
    /// draws over whatever its layout or master would have given it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_picture: Option<BackgroundPicture>,
    pub objects: Vec<SlideObject>,
    pub notes: String,
    /// The master this slide uses, by [`MasterSlide::id`]; `None` for none.
    /// Readers assign master ids once at import and resolve slides to them
    /// by name — never by position, which reordering invalidates.
    pub master_id: Option<u64>,
    /// How this slide arrives when presented (PowerPoint's model: the
    /// transition belongs to the slide it leads into).
    pub transition: Transition,
    /// Objects that build in or out, one per click, in order
    /// (decks_core::builds). Each build names its object by stable object
    /// id ([`crate::ops::SlideIds::objects`]), never by z-position, so
    /// reordering keeps every reference.
    pub builds: Vec<crate::builds::Build>,
    /// Stable identities for editing by ops (decks_core::ops): the slide's
    /// id, one id per object, and the ids of deleted objects (tombstones).
    /// Readers leave it empty; ops fill it in (`ops::ensure_ids`).
    pub ids: crate::ops::SlideIds,
    /// Which of its master's layouts the slide uses, by
    /// [`crate::layouts::Layout::id`]; `None` for none. View indices into
    /// `MasterSlide::layouts` are derived from this at the point of use.
    pub layout_id: Option<u64>,
    /// Skipped when the deck is shown or exported, as PowerPoint's and
    /// Impress's "Hide Slide" does (pptx `p:sld show="0"`, ODF
    /// `presentation:visibility="hidden"`); it stays in the deck to be
    /// edited. Older recovery payloads without it read as shown.
    #[serde(default)]
    pub hidden: bool,
}

impl Slide {
    /// The z-position of object `id` on this slide, if it is still there.
    pub fn object_index(&self, id: u64) -> Option<usize> {
        self.ids.objects.iter().position(|o| *o == id)
    }
}

/// A picture a background is filled with, stretched (`a:stretch`).
#[derive(Clone, Debug, Default, PartialEq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct BackgroundPicture {
    /// The picture's file (an imported one lives in the media cache).
    pub path: String,
    /// Where the stretched picture lies: pptx's `a:fillRect`.
    #[serde(default, skip_serializing_if = "FillRect::is_none")]
    pub fill: FillRect,
}

/// How far a stretched picture's edges lie inside the box it fills, as
/// fractions of the box's width (left, right) and height (top, bottom):
/// pptx's `a:fillRect`. Negative reaches past the box, as the pay centre
/// decks' title pictures do by 1% on each side (`l="-1000" r="-1000"`):
/// what is past the edge is cut. All zero fills the box exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct FillRect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl FillRect {
    /// The picture fills the box exactly.
    pub fn is_none(&self) -> bool {
        *self == FillRect::default()
    }

    /// The picture's rectangle for a box at (`x`, `y`), `w`×`h`:
    /// (x, y, w, h). A degenerate rect (insets meeting or crossing) keeps
    /// a minimal positive size rather than drawing inverted.
    pub fn place(&self, x: f64, y: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
        let px = x + self.left * w;
        let py = y + self.top * h;
        let pw = (w * (1.0 - self.left - self.right)).max(1e-6);
        let ph = (h * (1.0 - self.top - self.bottom)).max(1e-6);
        (px, py, pw, ph)
    }
}

/// How much of a picture's source is cut from each side, as fractions of
/// its width (left, right) and height (top, bottom): pptx's `a:srcRect`
/// and ODF's `fo:clip`. The rest is stretched to the picture's box, which
/// is how PowerPoint and Impress draw a picture, cropped or not.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[derive(serde::Serialize, serde::Deserialize)]
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
#[derive(serde::Serialize, serde::Deserialize)]
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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MasterSlide {
    /// Stable identity within the session (DCK-005). Readers assign ids
    /// once at import; 0 means not assigned yet (`ops::ensure_all_ids`).
    /// Slides name this master by id ([`Slide::master_id`]), never by
    /// position in `Deck::masters`.
    pub id: u64,
    pub name: String,
    /// The master's own background (`p:bg`), as `#rrggbb`. Empty means
    /// the part states none and the master inherits: it is NOT turned
    /// into white at import (DCK-006). Rendering falls back to the
    /// documented default (`layouts::FALLBACK_BACKGROUND`) only at draw
    /// time, and the writers omit an empty background rather than
    /// freezing the fallback into the package.
    pub background: String,
    /// The font the master's theme answers with for body text, or empty
    /// when its theme names none: [`MasterSlide::font_family`] falls
    /// back to [`MasterSlide::DEFAULT_FONT`] at draw time, but import
    /// keeps the absence rather than claiming the file said Sans.
    pub default_font: String,
    pub shapes: Vec<SlideObject>,
    /// The theme this master inherits from, by [`super::theme::Theme::id`]
    /// in [`Deck::themes`]; `None` for a master whose part names no theme
    /// (what this writer used to emit) and for ODP masters. Resolution
    /// falls back to the default theme, documented on
    /// [`super::theme::Theme::default_colors`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_id: Option<u64>,
    /// The master's colour map (`p:clrMap`): presentation names (`bg1`,
    /// `tx1` …) to the theme slots they draw from. Empty is the identity
    /// mapping every package without one assumes. Retained so a save
    /// carries it back, and applied at read time by giving the master's
    /// resolving theme this remap.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub color_map: std::collections::BTreeMap<String, String>,
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

    /// The layout `id` of this master, if it still has one by that id.
    pub fn layout_by_id(&self, id: u64) -> Option<&crate::layouts::Layout> {
        self.layouts.iter().find(|l| l.id == id)
    }

    /// The view index of layout `id` within this master, for positional
    /// widgets. Derived from the stable id at the point of use.
    pub fn layout_index(&self, id: u64) -> Option<usize> {
        self.layouts.iter().position(|l| l.id == id)
    }
}

#[derive(Clone, Debug)]
#[derive(serde::Serialize, serde::Deserialize)]
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
        #[serde(default)]
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
    /// Width and height of the bounding box `x()`/`y()` place.
    pub fn size(&self) -> (f64, f64) {
        match self {
            SlideObject::TextBox { w, h, .. }
            | SlideObject::Rect { w, h, .. }
            | SlideObject::Shape { w, h, .. }
            | SlideObject::Table { w, h, .. }
            | SlideObject::Chart { w, h, .. }
            | SlideObject::Image { w, h, .. } => (*w, *h),
            SlideObject::Circle { r, .. } => (*r * 2.0, *r * 2.0),
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
            id: 1,
            name: "Default".into(),
            background: "#ffffff".into(),
            default_font: MasterSlide::DEFAULT_FONT.into(),
            shapes: vec![],
            theme_id: None,
            color_map: Default::default(),
            layouts: Vec::new(),
        };
        Self {
            slides: vec![Slide { hidden: false,
                title: "Slide 1".into(),
                background: "#ffffff".into(),
                background_picture: None,
                objects: vec![],
                notes: String::new(),
                master_id: Some(default_master.id),
                transition: Default::default(),
                builds: Vec::new(),
                ids: Default::default(),
                layout_id: None,
            }],
            masters: vec![default_master],
            page_size: None,
            themes: Vec::new(),
            skipped_slides: Vec::new(),
        }
    }

    /// The slide size to write and export, in EMU: the declared
    /// [`Deck::page_size`], or the size our own writer emits when the
    /// deck states none. The one conversion every format and export
    /// path uses (DCK-006) — never a first-master fallback.
    pub fn page_emu(&self) -> (f64, f64) {
        match self.page_size {
            Some((cx, cy)) if cx > 0.0 && cy > 0.0 => (cx, cy),
            _ => super::SlideScale::DEFAULT_EMU,
        }
    }

    /// The theme `master` inherits from, if the deck retains it.
    /// `None` resolves against the default theme (a master with no
    /// theme relationship, or an ODP master): see
    /// [`super::theme::Theme::default_colors`].
    pub fn theme_of(&self, master: &MasterSlide) -> Option<&super::theme::Theme> {
        master.theme_id.and_then(|id| self.themes.iter().find(|t| t.id == id))
    }

    /// The position of slide `id`, if it is still in the deck.
    pub fn slide_index(&self, id: u64) -> Option<usize> {
        self.slides.iter().position(|s| s.ids.slide == id)
    }

    /// The master `id`, if it is still in the deck.
    pub fn master_by_id(&self, id: u64) -> Option<&MasterSlide> {
        self.masters.iter().find(|m| m.id == id)
    }

    /// The master a slide uses: its [`Slide::master_id`], or the deck's
    /// first master when it names none (what a fresh slide inherits).
    /// An unknown id resolves to no master rather than a positional guess.
    pub fn master_of(&self, slide: &Slide) -> Option<&MasterSlide> {
        match slide.master_id {
            Some(id) => self.master_by_id(id),
            None => self.masters.first(),
        }
    }

    /// The layout a slide uses, resolved by stable id within its master.
    pub fn layout_of(&self, slide: &Slide) -> Option<&crate::layouts::Layout> {
        let master = self.master_of(slide)?;
        slide.layout_id.and_then(|id| master.layouts.iter().find(|l| l.id == id))
    }

    /// The view index of a slide's layout within its master, for combo rows
    /// and other positional widgets. Derived from the stable id at the
    /// point of use; `None` when the slide uses no listed layout.
    pub fn layout_index(&self, slide: &Slide) -> Option<usize> {
        let master = self.master_of(slide)?;
        slide.layout_id.and_then(|id| master.layouts.iter().position(|l| l.id == id))
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
