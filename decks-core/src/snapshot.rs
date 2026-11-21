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
    /// The object's stable id: what selection, undo and collaboration
    /// name. The view index above is draw order only.
    pub id: u64,
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
    /// A text box's runs and paragraph styles as JSON arrays (DCK-007):
    /// what range formatting must split and keep.
    pub rich: Option<(String, String)>,
}

pub struct SlideSnapshot {
    pub index: usize,
    /// The slide's stable id.
    pub id: u64,
    pub title: String,
    /// The slide's speaker notes.
    pub notes: String,
    /// The slide's master, by stable id.
    pub master_id: Option<u64>,
    /// The slide's layout id and its name.
    pub layout: Option<(u64, String)>,
    pub objects: Vec<ObjectSnapshot>,
}

pub struct DeckSnapshot {
    pub slide_count: usize,
    /// Each master's stable id, name and how many decorations it has.
    pub masters: Vec<(u64, String, usize)>,
    /// Each master's decorations, as [`ObjectSnapshot::detail`].
    pub master_details: Vec<Vec<String>>,
    /// The master being edited, by stable id, while the master view is open.
    pub editing_master: Option<u64>,
    pub slides: Vec<SlideSnapshot>,
    /// The controller-owned selection (stable ids, never positions),
    /// filled in by [`snapshot`]. View indices for it are derived in
    /// [`DeckSnapshot::to_json`] alongside the legacy positional keys.
    pub selection: crate::controller::SelectionState,
    /// The open in-place text edit as JSON, if any: its box, working
    /// text, runs, selection and whether it changed anything. The session
    /// is not the document; journeys use this to see that typing and IME
    /// preedit stay out of the slide until commit.
    pub text_session: Option<String>,
}

/// `runs` and paragraph `paras` as JSON arrays.
fn rich_json(runs: &[crate::engine::Run], paras: &[crate::engine::ParaStyle]) -> (String, String) {
    (
        serde_json::to_string(runs).unwrap_or_else(|_| "[]".into()),
        serde_json::to_string(paras).unwrap_or_else(|_| "[]".into()),
    )
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

fn object_snapshot(index: usize, id: u64, obj: &SlideObject) -> ObjectSnapshot {
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
    let rich = match obj {
        SlideObject::TextBox { runs, body, .. } => Some(rich_json(runs, &body.paras)),
        _ => None,
    };
    ObjectSnapshot { index, id, kind, text, x, y, w, h, detail: detail(obj), rich }
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
            id: slide.ids.slide,
            title: slide.title.clone(),
            notes: slide.notes.clone(),
            master_id: slide.master_id,
            layout: slide.layout_id.map(|id| {
                let name = masters_ref
                    .iter()
                    .find(|m| Some(m.id) == slide.master_id)
                    .and_then(|m| m.layouts.iter().find(|l| l.id == id))
                    .map(|l| l.name.clone())
                    .unwrap_or_default();
                (id, name)
            }),
            objects: slide
                .objects
                .iter()
                .enumerate()
                .map(|(i, obj)| object_snapshot(i, slide.ids.objects.get(i).copied().unwrap_or(0), obj))
                .collect(),
        })
        .collect();
    let masters = masters_ref.iter().map(|m| (m.id, m.name.clone(), m.shapes.len())).collect();
    let master_details = masters_ref.iter().map(|m| m.shapes.iter().map(detail).collect()).collect();
    drop(slides);
    drop(masters_ref);
    let text_session = controller.text_session(|t| {
        let w = t.working();
        let (runs, paras) = rich_json(&w.runs, &w.body.paras);
        let (start, end) = t.range();
        format!(
            "{{\"slide_id\":{},\"object_id\":{},\"text\":{},\"runs\":{runs},\"paras\":{paras},\"start\":{start},\"end\":{end},\"changed\":{},\"can_undo\":{}}}",
            t.slide_id,
            t.object_id,
            json_str(&w.text),
            t.changed(),
            t.can_undo(),
        )
    });
    DeckSnapshot { slide_count, masters, master_details, editing_master: controller.editing_master(), slides: snapshots, selection: controller.selection(), text_session }
}

/// What the session's source context holds, for GUI journeys to assert
/// provenance instead of scraping dialogs (DCK-002). `None` for new
/// documents and recovered snapshots: no context, no claims. Separate
/// from [`DeckSnapshot`] so existing consumers never change shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvenanceSnapshot {
    pub format: String,
    pub carried_names: Vec<String>,
    pub opaque_names: Vec<String>,
    pub opaque_complete: bool,
    pub edited_slides: Vec<u64>,
    pub structure_changed: bool,
    pub masters_touched: bool,
    pub history_stepped: bool,
    pub destructive_loss_ids: Vec<String>,
}

pub fn provenance_snapshot(controller: &DecksController) -> Option<ProvenanceSnapshot> {
    controller.provenance_facts().map(|f| ProvenanceSnapshot {
        format: f.format,
        carried_names: f.carried_names,
        opaque_names: f.opaque_names,
        opaque_complete: f.opaque_complete,
        edited_slides: f.edited_slides,
        structure_changed: f.structure_changed,
        masters_touched: f.masters_touched,
        history_stepped: f.history_stepped,
        destructive_loss_ids: f.destructive_loss_ids,
    })
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
                            "{{\"index\":{},\"id\":{},\"kind\":{},\"text\":{},\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"detail\":{},\"runs\":{},\"paras\":{}}}",
                            o.index,
                            o.id,
                            json_str(o.kind),
                            json_opt_str(&o.text),
                            o.x,
                            o.y,
                            o.w,
                            o.h,
                            json_str(&o.detail),
                            o.rich.as_ref().map_or("null", |r| r.0.as_str()),
                            o.rich.as_ref().map_or("null", |r| r.1.as_str()),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"index\":{},\"id\":{},\"title\":{},\"notes\":{},\"master_id\":{},\"layout\":{},\"objects\":[{}]}}",
                    s.index,
                    s.id,
                    json_str(&s.title),
                    json_str(&s.notes),
                    s.master_id.map_or("null".to_string(), |id| id.to_string()),
                    s.layout.as_ref().map_or("null".to_string(), |(id, n)| format!("{{\"id\":{id},\"name\":{}}}", json_str(n))),
                    objects,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let masters = self
            .masters
            .iter()
            .zip(self.master_details.iter().chain(std::iter::repeat(&Vec::new())))
            .map(|((id, name, shapes), details)| {
                let details = details.iter().map(|d| json_str(d)).collect::<Vec<_>>().join(",");
                format!("{{\"id\":{id},\"name\":{},\"shapes\":{shapes},\"details\":[{details}]}}", json_str(name))
            })
            .collect::<Vec<_>>()
            .join(",");
        let editing = self.editing_master.map_or("null".to_string(), |id| id.to_string());
        // The selection, by stable ids, plus the view indices derived from
        // it that existing journeys assert on (`current_slide`,
        // `selected`/`selected_object` name the primary object's
        // z-position, or null).
        let current_index = self.selection.slide_id.and_then(|id| self.slides.iter().find(|s| s.id == id)).map(|s| s.index);
        let primary_index = self.selection.primary_object_id.and_then(|id| {
            self.slides
                .iter()
                .find(|s| Some(s.id) == self.selection.slide_id)?
                .objects
                .iter()
                .find(|o| o.id == id)
                .map(|o| o.index)
        });
        let selected_ids = self.selection.object_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        let mode = match self.selection.mode {
            crate::controller::SelectionMode::Object => "object",
            crate::controller::SelectionMode::Text => "text",
            crate::controller::SelectionMode::TableCell => "table-cell",
            crate::controller::SelectionMode::GroupScope => "group-scope",
        };
        let text_range = self.selection.text_range.map_or("null".to_string(), |r| format!("{{\"object_id\":{},\"start\":{},\"end\":{}}}", r.object_id, r.start, r.end));
        let text_session = self.text_session.as_deref().unwrap_or("null");
        format!(
            "{{\"slide_count\":{},\"masters\":[{masters}],\"editing_master\":{editing},\"current_slide\":{},\"selected\":{},\"selected_object\":{},\"slide_id\":{},\"selected_ids\":[{selected_ids}],\"primary_object_id\":{},\"selection_mode\":\"{mode}\",\"text_range\":{text_range},\"text_session\":{text_session},\"slides\":[{}]}}",
            self.slide_count,
            current_index.map_or("null".to_string(), |i| i.to_string()),
            primary_index.map_or("null".to_string(), |i| i.to_string()),
            primary_index.map_or("null".to_string(), |i| i.to_string()),
            self.selection.slide_id.map_or("null".to_string(), |id| id.to_string()),
            self.selection.primary_object_id.map_or("null".to_string(), |id| id.to_string()),
            slides
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Slide;

    fn slide(title: &str) -> Slide {
        Slide { hidden: false, title: title.into(), background: "#fff".into(), background_picture: None, objects: vec![], notes: String::new(), master_id: Some(0), transition: Default::default(), builds: vec![], ids: Default::default(), layout_id: None }
    }

    #[test]
    fn snapshot_reports_slides_and_objects() {
        let c = DecksController::new(vec![slide("S1"), slide("S2")], vec![]);
        let (first, second) = (c.slide_id_at(0).unwrap(), c.slide_id_at(1).unwrap());
        c.add_object(first, SlideObject::Rect { x: 1.0, y: 2.0, w: 10.0, h: 10.0, rotation: 0.0 });
        c.add_object(second, SlideObject::TextBox {
            text: "hi".into(), x: 0.0, y: 0.0, w: 5.0, h: 5.0, runs: vec![],
            rotation: 0.0,
            body: Default::default(),
        });
        let snap = snapshot(&c);
        assert_eq!(snap.slide_count, 2);
        assert_eq!(snap.slides[0].id, first);
        assert_eq!(snap.slides[0].objects.len(), 1);
        assert_eq!(snap.slides[0].objects[0].kind, "Rect");
        assert_eq!(snap.slides[1].objects[0].text.as_deref(), Some("hi"));
        // The selection is controller-owned ids, not window positions.
        assert_eq!(snap.selection.slide_id, Some(first));
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
        let kinds = objs.iter().enumerate().map(|(i, o)| object_snapshot(i, 100 + i as u64, o)).collect::<Vec<_>>();
        assert_eq!(kinds[0].kind, "TextBox");
        assert_eq!(kinds[0].text.as_deref(), Some("t"));
        assert_eq!(kinds[0].id, 100);
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
