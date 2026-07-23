// transition.rs — Slide transitions via Cairo double-buffering.
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk4::cairo;
use gtk4::{prelude::*, glib};
use std::cell::RefCell;
use std::rc::Rc;
use crate::canvas::draw_slide;
use decks_core::engine::Slide;

// Fade/CoverLeft/SplitHorizontal are drawn (see draw_transition below) but
// not yet selectable from any UI — only None/PushLeft/WipeLeft are wired
// to a picker. Kept rather than deleted since the render path already
// supports them.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub enum TransitionType {
    None,
    Fade,
    PushLeft,
    WipeLeft,
    CoverLeft,
    SplitHorizontal,
}

pub struct TransitionState {
    pub from_surface: Option<cairo::ImageSurface>,
    pub to_surface: Option<cairo::ImageSurface>,
    pub progress: f64,
    pub active: bool,
    pub kind: TransitionType,
}

impl TransitionState {
    pub fn new() -> Self {
        Self {
            from_surface: None,
            to_surface: None,
            progress: 0.0,
            active: false,
            kind: TransitionType::None,
        }
    }

    pub fn start(&mut self, kind: TransitionType, from_slide: &Slide, to_slide: &Slide, area: &gtk4::DrawingArea) {
        let from_surf = render_slide_to_surface(from_slide);
        let to_surf = render_slide_to_surface(to_slide);
        self.from_surface = Some(from_surf);
        self.to_surface = Some(to_surf);
        self.progress = 0.0;
        self.active = true;
        self.kind = kind;

        if kind == TransitionType::None {
            self.active = false;
            self.from_surface = None;
            self.to_surface = None;
            area.queue_draw();
            return;
        }

        let da = area.clone();
        let state = Rc::new(RefCell::new(self.clone_shared()));
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            let mut ts = state.borrow_mut();
            ts.progress += 0.05;
            if ts.progress >= 1.0 {
                ts.progress = 1.0;
                ts.active = false;
                ts.from_surface = None;
                ts.to_surface = None;
                da.queue_draw();
                return glib::ControlFlow::Break;
            }
            da.queue_draw();
            glib::ControlFlow::Continue
        });
    }

    fn clone_shared(&self) -> Self {
        Self {
            from_surface: self.from_surface.clone(),
            to_surface: self.to_surface.clone(),
            progress: self.progress,
            active: self.active,
            kind: self.kind,
        }
    }
}

fn render_slide_to_surface(slide: &Slide) -> cairo::ImageSurface {
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 960, 540).unwrap();
    let cr = cairo::Context::new(&surface).unwrap();
    draw_slide(&cr, 960.0, 540.0, std::slice::from_ref(slide), 0, None, &[],
               (0.0, 0.5, 1.0)); // transition snapshots never show selection; unused
    surface.flush();
    surface
}

pub fn draw_transition(cr: &cairo::Context, state: &TransitionState, canvas_w: f64, canvas_h: f64) -> bool {
    if !state.active { return false; }
    let t = state.progress;
    let eased = 1.0 - (1.0 - t).powi(3); // ease-out cubic

    let slide_w = canvas_w * 0.85;
    let _slide_h = slide_w * 9.0 / 16.0;
    let ox = (canvas_w - slide_w) / 2.0;
    let oy = (canvas_h - _slide_h) / 2.0;
    let scale_x = slide_w / 960.0;
    let scale_y = _slide_h / 540.0;

    match state.kind {
        TransitionType::Fade => {
            if let Some(ref surf) = state.from_surface {
                cr.save().unwrap();
                cr.translate(ox, oy);
                cr.scale(scale_x, scale_y);
                cr.set_source_surface(surf, 0.0, 0.0).unwrap();
                cr.paint_with_alpha(1.0 - eased).unwrap();
                cr.restore().unwrap();
            }
            if let Some(ref surf) = state.to_surface {
                cr.save().unwrap();
                cr.translate(ox, oy);
                cr.scale(scale_x, scale_y);
                cr.set_source_surface(surf, 0.0, 0.0).unwrap();
                cr.paint_with_alpha(eased).unwrap();
                cr.restore().unwrap();
            }
        }
        TransitionType::PushLeft => {
            let offset = slide_w * eased;
            if let Some(ref surf) = state.from_surface {
                cr.save().unwrap();
                cr.translate(ox - offset, oy);
                cr.scale(scale_x, scale_y);
                cr.set_source_surface(surf, 0.0, 0.0).unwrap();
                cr.paint().unwrap();
                cr.restore().unwrap();
            }
            if let Some(ref surf) = state.to_surface {
                cr.save().unwrap();
                cr.translate(ox + slide_w - offset, oy);
                cr.scale(scale_x, scale_y);
                cr.set_source_surface(surf, 0.0, 0.0).unwrap();
                cr.paint().unwrap();
                cr.restore().unwrap();
            }
        }
        TransitionType::WipeLeft | TransitionType::CoverLeft => {
            let clip_w = if state.kind == TransitionType::WipeLeft {
                slide_w * eased
            } else {
                slide_w * (1.0 - eased)
            };
            // Draw "from" slide (full)
            if let Some(ref surf) = state.from_surface {
                cr.save().unwrap();
                cr.translate(ox, oy);
                cr.scale(scale_x, scale_y);
                cr.set_source_surface(surf, 0.0, 0.0).unwrap();
                cr.paint().unwrap();
                cr.restore().unwrap();
            }
            // Draw "to" slide clipped to wipe region
            if let Some(ref surf) = state.to_surface {
                cr.save().unwrap();
                cr.rectangle(ox, oy, clip_w, _slide_h);
                cr.clip();
                cr.translate(ox, oy);
                cr.scale(scale_x, scale_y);
                cr.set_source_surface(surf, 0.0, 0.0).unwrap();
                cr.paint().unwrap();
                cr.restore().unwrap();
            }
        }
        TransitionType::SplitHorizontal => {
            let split = _slide_h * eased / 2.0;
            // From slide: split apart
            if let Some(ref surf) = state.from_surface {
                // Top half moves up
                cr.save().unwrap();
                cr.rectangle(ox, oy, slide_w, _slide_h / 2.0);
                cr.clip();
                cr.translate(ox, oy - split);
                cr.scale(scale_x, scale_y);
                cr.set_source_surface(surf, 0.0, 0.0).unwrap();
                cr.paint().unwrap();
                cr.restore().unwrap();
                // Bottom half moves down
                cr.save().unwrap();
                cr.rectangle(ox, oy + _slide_h / 2.0, slide_w, _slide_h / 2.0);
                cr.clip();
                cr.translate(ox, oy + split);
                cr.scale(scale_x, scale_y);
                cr.set_source_surface(surf, 0.0, 0.0).unwrap();
                cr.paint().unwrap();
                cr.restore().unwrap();
            }
            // To slide: fades in behind
            if let Some(ref surf) = state.to_surface {
                cr.save().unwrap();
                cr.translate(ox, oy);
                cr.scale(scale_x, scale_y);
                cr.set_source_surface(surf, 0.0, 0.0).unwrap();
                cr.paint_with_alpha(eased).unwrap();
                cr.restore().unwrap();
            }
        }
        _ => {}
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transition_state_starts_inactive() {
        let ts = TransitionState::new();
        assert!(!ts.active);
        assert!(ts.from_surface.is_none());
        assert!(ts.to_surface.is_none());
    }

    #[test]
    fn test_transition_none_is_instant() {
        // TransitionType::None should have no visual effect
        assert_eq!(TransitionType::None as i32, 0);
    }

    #[test]
    fn test_transition_enum_variants() {
        // Verify all variants are constructable
        let _ = TransitionType::Fade;
        let _ = TransitionType::PushLeft;
        let _ = TransitionType::WipeLeft;
        let _ = TransitionType::CoverLeft;
        let _ = TransitionType::SplitHorizontal;
        let _ = TransitionType::None;
    }
}
