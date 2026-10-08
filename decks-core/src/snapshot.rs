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
    /// The bounding box's size (`SlideObject::size`), so a test can say
    /// where the canvas draws the object.
    pub w: f64,
    pub h: f64,
    /// Everything the model holds for the object (runs, style, geometry,
    /// rotation, crop), a picture by a hash of its bytes rather than the
    /// temporary file it was unpacked to: what a save and reopen must
    /// keep (decks-readiness row 11).
    pub detail: String,
}

pub struct SlideSnapshot {
    pub index: usize,
    pub title: String,
    /// The slide's speaker notes.
    pub notes: String,
    /// The slide's layout and its name.
    pub layout: Option<(usize, String)>,
    pub objects: Vec<ObjectSnapshot>,
}

pub struct DeckSnapshot {
    pub slide_count: usize,
    /// Each master's name and how many decorations it has.
    pub masters: Vec<(String, usize)>,
    /// Each master's decorations, as [`ObjectSnapshot::detail`].
    pub master_details: Vec<Vec<String>>,
    /// The master being edited, while the master view is open.
    pub editing_master: Option<usize>,
    pub slides: Vec<SlideSnapshot>,
    /// The window's current slide and selected object, which the window
    /// (not the controller) holds and fills in.
    pub selection: Option<(usize, Option<usize>)>,
}

/// `obj` in full, its picture (if any) named by a hash of the bytes.
pub fn detail(obj: &SlideObject) -> String {
    let mut obj = obj.clone();
    if let SlideObject::Image { path, .. } = &mut obj {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match std::fs::read(&*path) {
            Ok(bytes) => {
                bytes.hash(&mut h);
                *path = format!("picture:{:016x}:{}", h.finish(), bytes.len());
            }
            Err(_) => *path = format!("picture:missing:{path}"),
        }
    }
    format!("{obj:?}")
}

fn object_snapshot(index: usize, obj: &SlideObject) -> ObjectSnapshot {
    let (kind, text) = match obj {
        SlideObject::TextBox { text, .. } => ("TextBox", Some(text.clone())),
        SlideObject::Rect { .. } => ("Rect", None),
        SlideObject::Circle { .. } => ("Circle", None),
        SlideObject::Shape { .. } => ("Shape", None),
        SlideObject::Table { .. } => ("Table", None),
        SlideObject::Chart { chart, .. } => ("Chart", Some(chart.describe())),
        // A picture states its crop, as a chart states its data: the crop
        // cut from each side, left, top, right, bottom.
        SlideObject::Image { crop, .. } => (
            "Image",
            (!crop.is_none()).then(|| format!("crop {:.3} {:.3} {:.3} {:.3}", crop.left, crop.top, crop.right, crop.bottom)),
        ),
    };
    // Circle's x/y are its centre; the snapshot has always reported them so.
    let (x, y) = match obj {
        SlideObject::Circle { x, y, .. } => (*x, *y),
        o => (o.x(), o.y()),
    };
    let (w, h) = obj.size();
    ObjectSnapshot { index, kind, text, x, y, w, h, detail: detail(obj) }
}

pub fn snapshot(controller: &DecksController) -> DeckSnapshot {
    let slides = controller.slides.borrow();
    let masters_ref = controller.masters.borrow();
    let slide_count = slides.len();
    let snapshots = slides
        .iter()
        .enumerate()
        .map(|(index, slide)| SlideSnapshot {
            index,
            title: slide.title.clone(),
            notes: slide.notes.clone(),
            layout: slide.layout.map(|l| {
                let name = slide
                    .master_idx
                    .and_then(|m| masters_ref.get(m))
                    .and_then(|m| m.layouts.get(l))
                    .map(|l| l.name.clone())
                    .unwrap_or_default();
                (l, name)
            }),
            objects: slide
                .objects
                .iter()
                .enumerate()
                .map(|(i, obj)| object_snapshot(i, obj))
                .collect(),
        })
        .collect();
    let masters = masters_ref.iter().map(|m| (m.name.clone(), m.shapes.len())).collect();
    let master_details = masters_ref.iter().map(|m| m.shapes.iter().map(detail).collect()).collect();
    DeckSnapshot { slide_count, masters, master_details, editing_master: controller.editing_master(), slides: snapshots, selection: None }
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
                            "{{\"index\":{},\"kind\":{},\"text\":{},\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"detail\":{}}}",
                            o.index,
                            json_str(o.kind),
                            json_opt_str(&o.text),
                            o.x,
                            o.y,
                            o.w,
                            o.h,
                            json_str(&o.detail),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"index\":{},\"title\":{},\"notes\":{},\"layout\":{},\"objects\":[{}]}}",
                    s.index,
                    json_str(&s.title),
                    json_str(&s.notes),
                    s.layout.as_ref().map_or("null".to_string(), |(i, n)| format!("{{\"index\":{i},\"name\":{}}}", json_str(n))),
                    objects,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let masters = self
            .masters
            .iter()
            .zip(self.master_details.iter().chain(std::iter::repeat(&Vec::new())))
            .map(|((name, shapes), details)| {
                let details = details.iter().map(|d| json_str(d)).collect::<Vec<_>>().join(",");
                format!("{{\"name\":{},\"shapes\":{shapes},\"details\":[{details}]}}", json_str(name))
            })
            .collect::<Vec<_>>()
            .join(",");
        let editing = self.editing_master.map_or("null".to_string(), |i| i.to_string());
        let (current, selected) = match self.selection {
            Some((c, s)) => (c.to_string(), s.map_or("null".to_string(), |s| s.to_string())),
            None => ("null".to_string(), "null".to_string()),
        };
        format!(
            "{{\"slide_count\":{},\"masters\":[{masters}],\"editing_master\":{editing},\"current_slide\":{current},\"selected\":{selected},\"selected_object\":{selected},\"slides\":[{}]}}",
            self.slide_count, slides
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Slide;

    fn slide(title: &str) -> Slide {
        Slide { title: title.into(), background: "#fff".into(), background_image: None, hidden: false, objects: vec![], notes: String::new(), master_idx: Some(0), transition: Default::default(), builds: Vec::new(), ids: Default::default(), layout: None }
    }

    #[test]
    fn snapshot_reports_slides_and_objects() {
        let c = DecksController::new(vec![slide("S1"), slide("S2")], vec![]);
        c.add_object(0, SlideObject::Rect { x: 1.0, y: 2.0, w: 10.0, h: 10.0, rotation: 0.0 });
        c.add_object(1, SlideObject::TextBox {
            text: "hi".into(), x: 0.0, y: 0.0, w: 5.0, h: 5.0, runs: vec![],
            rotation: 0.0,
            body: Default::default(),
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

    // ── escape_json / json helpers (pure string logic) ──────────────────────

    #[test]
    fn escapes_quotes_backslash_and_whitespace() {
        assert_eq!(escape_json("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(escape_json("line1\nline2\r\n\ttab"), "line1\\nline2\\r\\n\\ttab");
    }

    #[test]
    fn escapes_control_characters_as_unicode_escapes() {
        assert_eq!(escape_json("\u{01}"), "\\u0001");
        assert_eq!(escape_json("\u{1f}"), "\\u001f");
    }

    #[test]
    fn passes_through_unicode_and_plain_text() {
        assert_eq!(escape_json("héllo wörld"), "héllo wörld");
        assert_eq!(escape_json("emoji 😀 ok"), "emoji 😀 ok");
    }

    #[test]
    fn json_opt_str_none_is_null() {
        assert_eq!(json_opt_str(&None), "null");
        assert_eq!(json_opt_str(&Some("hi".into())), "\"hi\"");
    }

    #[test]
    fn object_snapshot_maps_every_kind() {
        let objs = [
            SlideObject::TextBox { text: "t".into(), x: 1.0, y: 2.0, w: 3.0, h: 4.0, rotation: 0.0, runs: vec![], body: Default::default() },
            SlideObject::Rect { x: 1.0, y: 2.0, w: 3.0, h: 4.0, rotation: 0.0 },
            SlideObject::Circle { x: 1.0, y: 2.0, r: 3.0, rotation: 0.0 },
            SlideObject::Image { path: "p.png".into(), x: 1.0, y: 2.0, w: 3.0, h: 4.0, rotation: 0.0, crop: Default::default() },
        ];
        let kinds = objs.iter().enumerate().map(|(i, o)| object_snapshot(i, o)).collect::<Vec<_>>();
        assert_eq!(kinds[0].kind, "TextBox");
        assert_eq!(kinds[0].text.as_deref(), Some("t"));
        assert_eq!(kinds[1].kind, "Rect");
        assert!(kinds[1].text.is_none());
        assert_eq!(kinds[2].kind, "Circle");
        assert_eq!(kinds[3].kind, "Image");
        for (i, k) in kinds.iter().enumerate() {
            assert_eq!(k.index, i);
            assert_eq!((k.x, k.y), (1.0, 2.0));
        }
    }
}
