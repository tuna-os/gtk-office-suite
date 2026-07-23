// snapshot.rs — test-only state introspection (#104).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A normalized view of canonical deck state for deterministic GUI
// journeys to assert against, instead of scraping AT-SPI tree text.
// Hand-written rather than a serde derive on the real document model —
// mirrors tables_core::snapshot's reasoning: keep the test-only shape
// decoupled from Slide/SlideObject's real pptx/odp serialization
// concerns. This module ships in production builds (plain, inert
// data-building code — no I/O, no GTK), but nothing calls it unless the
// app crate wires up a test-only entry point gated behind an env var
// check that lives in the app, not here. See decks/src/window.rs.

use crate::controller::DecksController;
use crate::engine::SlideObject;

pub struct ObjectSnapshot {
    pub index: usize,
    pub kind: &'static str,
    pub text: Option<String>,
    pub x: f64,
    pub y: f64,
}

pub struct SlideSnapshot {
    pub index: usize,
    pub title: String,
    pub objects: Vec<ObjectSnapshot>,
}

pub struct DeckSnapshot {
    pub slide_count: usize,
    pub slides: Vec<SlideSnapshot>,
}

fn object_snapshot(index: usize, obj: &SlideObject) -> ObjectSnapshot {
    match obj {
        SlideObject::TextBox { text, x, y, .. } => {
            ObjectSnapshot { index, kind: "TextBox", text: Some(text.clone()), x: *x, y: *y }
        }
        SlideObject::Rect { x, y, .. } => {
            ObjectSnapshot { index, kind: "Rect", text: None, x: *x, y: *y }
        }
        SlideObject::Circle { x, y, .. } => {
            ObjectSnapshot { index, kind: "Circle", text: None, x: *x, y: *y }
        }
        SlideObject::Image { x, y, .. } => {
            ObjectSnapshot { index, kind: "Image", text: None, x: *x, y: *y }
        }
    }
}

pub fn snapshot(controller: &DecksController) -> DeckSnapshot {
    let slides = controller.slides.borrow();
    let slide_count = slides.len();
    let snapshots = slides
        .iter()
        .enumerate()
        .map(|(index, slide)| SlideSnapshot {
            index,
            title: slide.title.clone(),
            objects: slide
                .objects
                .iter()
                .enumerate()
                .map(|(i, obj)| object_snapshot(i, obj))
                .collect(),
        })
        .collect();
    DeckSnapshot { slide_count, slides: snapshots }
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn json_str(s: &str) -> String {
    format!("\"{}\"", escape_json(s))
}

fn json_opt_str(s: &Option<String>) -> String {
    match s {
        Some(v) => json_str(v),
        None => "null".to_string(),
    }
}

impl DeckSnapshot {
    pub fn to_json(&self) -> String {
        let slides = self
            .slides
            .iter()
            .map(|s| {
                let objects = s
                    .objects
                    .iter()
                    .map(|o| {
                        format!(
                            "{{\"index\":{},\"kind\":{},\"text\":{},\"x\":{},\"y\":{}}}",
                            o.index,
                            json_str(o.kind),
                            json_opt_str(&o.text),
                            o.x,
                            o.y,
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"index\":{},\"title\":{},\"objects\":[{}]}}",
                    s.index,
                    json_str(&s.title),
                    objects,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{{\"slide_count\":{},\"slides\":[{}]}}", self.slide_count, slides)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Slide;

    fn slide(title: &str) -> Slide {
        Slide { title: title.into(), background: "#fff".into(), objects: vec![], notes: String::new(), master_idx: Some(0) }
    }

    #[test]
    fn snapshot_reports_slides_and_objects() {
        let c = DecksController::new(vec![slide("S1"), slide("S2")], vec![]);
        c.add_object(0, SlideObject::Rect { x: 1.0, y: 2.0, w: 10.0, h: 10.0 });
        c.add_object(1, SlideObject::TextBox {
            text: "hi".into(), x: 0.0, y: 0.0, w: 5.0, h: 5.0, runs: vec![],
        });
        let snap = snapshot(&c);
        assert_eq!(snap.slide_count, 2);
        assert_eq!(snap.slides[0].objects.len(), 1);
        assert_eq!(snap.slides[0].objects[0].kind, "Rect");
        assert_eq!(snap.slides[1].objects[0].text.as_deref(), Some("hi"));
    }

    #[test]
    fn to_json_round_trips_shape() {
        let c = DecksController::new(vec![slide("S \"1\"")], vec![]);
        let json = snapshot(&c).to_json();
        assert!(json.contains("\\\"1\\\""));
        assert!(json.contains("\"slide_count\":1"));
    }
}
