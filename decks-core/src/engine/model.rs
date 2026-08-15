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
}

#[derive(Clone, Debug)]
pub struct MasterSlide {
    pub name: String,
    pub background: String,
    pub default_font: String,
    pub shapes: Vec<SlideObject>,
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
    },
    Rect { x: f64, y: f64, w: f64, h: f64, rotation: f64 },
    Circle { x: f64, y: f64, r: f64, rotation: f64 },
    Image { path: String, x: f64, y: f64, w: f64, h: f64, rotation: f64 },
}

impl SlideObject {
    pub fn x(&self) -> f64 {
        match self {
            SlideObject::TextBox { x, .. }
            | SlideObject::Rect { x, .. }
            | SlideObject::Image { x, .. } => *x,
            SlideObject::Circle { x, r, .. } => *x - *r,
        }
    }
    pub fn y(&self) -> f64 {
        match self {
            SlideObject::TextBox { y, .. }
            | SlideObject::Rect { y, .. }
            | SlideObject::Image { y, .. } => *y,
            SlideObject::Circle { y, r, .. } => *y - *r,
        }
    }
    pub fn rotation(&self) -> f64 {
        match self {
            SlideObject::TextBox { rotation, .. }
            | SlideObject::Rect { rotation, .. }
            | SlideObject::Circle { rotation, .. }
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
            default_font: "Sans".into(),
            shapes: vec![],
        };
        Self {
            slides: vec![Slide {
                title: "Slide 1".into(),
                background: "#ffffff".into(),
                objects: vec![],
                notes: String::new(),
                master_idx: Some(0),
            }],
            masters: vec![default_master],
        }
    }
}
