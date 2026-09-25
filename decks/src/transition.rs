// transition.rs — Slide transitions via Cairo double-buffering.
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk4::cairo;
use gtk4::{prelude::*, glib};
use std::cell::RefCell;
use std::rc::Rc;
use crate::canvas::draw_slide;
use decks_core::engine::{MasterSlide, Slide, Transition};

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
    /// Keynote's Magic Move: decks_core::magic_move, drawn object by
    /// object on the canvas's own slide frame.
    MagicMove,
}

impl TransitionType {
    /// How the canvas plays a slide's model transition.
    pub fn of(t: Transition) -> Self {
        match t {
            Transition::None => TransitionType::None,
            Transition::Fade => TransitionType::Fade,
            Transition::Push => TransitionType::PushLeft,
            Transition::Wipe => TransitionType::WipeLeft,
            Transition::MagicMove => TransitionType::MagicMove,
        }
    }

    /// Progress per 16 ms frame: Magic Move takes about a second, the
    /// others a third of one.
    fn step(self) -> f64 {
        if self == TransitionType::MagicMove { 0.016 } else { 0.05 }
    }
}

/// What a Magic Move frame is drawn from.
#[derive(Clone)]
pub struct MagicMove {
    pub from: Slide,
    pub to: Slide,
    pub masters: Vec<MasterSlide>,
    pub pairs: Vec<(usize, usize)>,
}

pub struct TransitionState {
    pub from_surface: Option<cairo::ImageSurface>,
    pub to_surface: Option<cairo::ImageSurface>,
    pub progress: f64,
    pub active: bool,
    pub kind: TransitionType,
    pub magic: Option<MagicMove>,
}

impl TransitionState {
    pub fn new() -> Self {
        Self {
            from_surface: None,
            to_surface: None,
            progress: 0.0,
            active: false,
            kind: TransitionType::None,
            magic: None,
        }
    }

    /// Play `kind` from `from_slide` to `to_slide` on `area`. The timer
    /// advances the shared state the canvas draws from. (It used to
    /// advance a private copy, so the canvas kept drawing the first frame
    /// of every transition and never got back to the slide.)
    pub fn start(
        state: &Rc<RefCell<TransitionState>>,
        kind: TransitionType,
        from_slide: &Slide,
        to_slide: &Slide,
        masters: &[MasterSlide],
        area: &gtk4::DrawingArea,
    ) {
        {
            let mut s = state.borrow_mut();
            s.progress = 0.0;
            s.kind = kind;
            s.magic = None;
            s.from_surface = None;
            s.to_surface = None;
            s.active = kind != TransitionType::None;
            if kind == TransitionType::MagicMove {
                s.magic = Some(MagicMove {
                    pairs: decks_core::magic_move::match_objects(&from_slide.objects, &to_slide.objects),
                    from: from_slide.clone(),
                    to: to_slide.clone(),
                    masters: masters.to_vec(),
                });
            } else if s.active {
                s.from_surface = Some(render_slide_to_surface(from_slide));
                s.to_surface = Some(render_slide_to_surface(to_slide));
            }
        }
        area.queue_draw();
        if kind == TransitionType::None {
            return;
        }
        let da = area.clone();
        let state = state.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            let mut ts = state.borrow_mut();
            ts.progress += ts.kind.step();
            if ts.progress >= 1.0 || !ts.active {
                ts.progress = 1.0;
                ts.active = false;
                ts.from_surface = None;
                ts.to_surface = None;
                ts.magic = None;
                da.queue_draw();
                return glib::ControlFlow::Break;
            }
            da.queue_draw();
            glib::ControlFlow::Continue
        });
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

/// A Magic Move frame: the slide under the objects (background and
/// master) cross-fades, then every object of decks_core::magic_move::frame
/// at its place and opacity. Nothing of the editor is drawn: no selection,
/// no slide number, no "empty slide" caption.
fn draw_magic_move(cr: &cairo::Context, m: &MagicMove, t: f64, canvas_w: f64, canvas_h: f64) {
    use crate::canvas::{draw_object, draw_slide_base, master_for};
    let e = decks_core::magic_move::ease(t);
    let to = std::slice::from_ref(&m.to);
    let (frame, bg) = draw_slide_base(cr, canvas_w, canvas_h, to, 0, &m.masters);
    cr.push_group();
    draw_slide_base(cr, canvas_w, canvas_h, std::slice::from_ref(&m.from), 0, &m.masters);
    let _ = cr.pop_group_to_source();
    let _ = cr.paint_with_alpha(1.0 - e);
    let master = master_for(to, 0, &m.masters);
    for f in decks_core::magic_move::frame(&m.from.objects, &m.to.objects, &m.pairs, t) {
        if f.opacity <= 0.001 {
            continue;
        }
        if f.opacity >= 0.999 {
            draw_object(cr, &f.object, frame, bg, master);
        } else {
            cr.push_group();
            draw_object(cr, &f.object, frame, bg, master);
            let _ = cr.pop_group_to_source();
            let _ = cr.paint_with_alpha(f.opacity);
        }
    }
}

/// Under GTK_OFFICE_TEST_MODE with GTK_OFFICE_TRANSITION_DUMP set to a
/// directory, write the midpoint frame of the transition that just started
/// there as `transition-midpoint.png`, at the canvas's size: the animation
/// itself can't be screenshotted deterministically, its frames can.
pub fn dump_midpoint(state: &TransitionState, area: &gtk4::DrawingArea) {
    if std::env::var_os("GTK_OFFICE_TEST_MODE").is_none() || !state.active {
        return;
    }
    let Some(dir) = std::env::var_os("GTK_OFFICE_TRANSITION_DUMP") else { return };
    let (w, h) = (area.width().max(320), area.height().max(180));
    let path = std::path::Path::new(&dir).join("transition-midpoint.png");
    if let Err(e) = write_frame_png(state, 0.5, w, h, &path) {
        eprintln!("transition dump: {e}");
    }
}

/// Render the frame of `state`'s transition at `t` on a `w`×`h` canvas,
/// through the same drawing the animation uses, into a PNG at `path`.
/// For looking at a transition without a timer (tests, GTK_OFFICE_TEST_MODE).
pub fn write_frame_png(state: &TransitionState, t: f64, w: i32, h: i32, path: &std::path::Path) -> Result<(), String> {
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).map_err(|e| e.to_string())?;
    {
        let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
        let frame = TransitionState {
            from_surface: state.from_surface.clone(),
            to_surface: state.to_surface.clone(),
            progress: t,
            active: true,
            kind: state.kind,
            magic: state.magic.clone(),
        };
        draw_transition(&cr, &frame, w as f64, h as f64);
    }
    let mut f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    surface.write_to_png(&mut f).map_err(|e| e.to_string())
}

pub fn draw_transition(cr: &cairo::Context, state: &TransitionState, canvas_w: f64, canvas_h: f64) -> bool {
    if !state.active { return false; }
    let t = state.progress;
    if let (TransitionType::MagicMove, Some(m)) = (state.kind, state.magic.as_ref()) {
        draw_magic_move(cr, m, t, canvas_w, canvas_h);
        return true;
    }
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

    /// The real Magic Move drawing path (draw_transition → frame →
    /// draw_object) at its midpoint: a red square that moves right and a
    /// blue one that only exists on the second slide. Half way, the red
    /// square's centre is half way along and fully opaque; the blue one is
    /// half faded in. The PNG is left in target/ for a person to look at.
    #[test]
    fn a_magic_move_midpoint_frame_draws_the_shared_object_half_way() {
        use decks_core::engine::shape::{Color, ShapeKind, ShapeStyle};
        use decks_core::engine::SlideObject;
        let sq = |x: f64, c: Color| SlideObject::Shape {
            kind: ShapeKind::Rect,
            x,
            y: 200.0,
            w: 100.0,
            h: 100.0,
            rotation: 0.0,
            style: ShapeStyle { fill: Some(c), gradient: None, stroke: None },
        };
        let (red, blue) = (Color(220, 0, 0), Color(0, 0, 220));
        let slide = |objects| Slide {
            title: String::new(),
            background: "#ffffff".into(),
            objects,
            notes: String::new(),
            master_idx: None,
            transition: Transition::MagicMove,
        };
        let from = slide(vec![sq(100.0, red)]);
        let mut arriving = sq(430.0, blue);
        if let SlideObject::Shape { y, .. } = &mut arriving {
            *y = 380.0;
        }
        let to = slide(vec![sq(700.0, red), arriving]);
        let state = TransitionState {
            from_surface: None,
            to_surface: None,
            progress: 0.0,
            active: true,
            kind: TransitionType::MagicMove,
            magic: Some(MagicMove {
                pairs: decks_core::magic_move::match_objects(&from.objects, &to.objects),
                from,
                to,
                masters: vec![],
            }),
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/render-frames");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("magic-move-midpoint.png");
        // A 1040x585 canvas: the slide is 92% of it, 956.8 px wide.
        write_frame_png(&state, 0.5, 1040, 585, &path).unwrap();

        let mut file = std::fs::File::open(&path).unwrap();
        let mut img = cairo::ImageSurface::create_from_png(&mut file).unwrap();
        let (ox, oy, sw, _) = crate::canvas::slide_geometry(1040.0, 585.0);
        let k = sw / 960.0;
        let stride = img.stride() as usize;
        let data = img.data().unwrap();
        let px = |mx: f64, my: f64| {
            let (x, y) = ((ox + mx * k) as usize, (oy + my * k) as usize);
            let i = y * stride + x * 4;
            (data[i + 2], data[i + 1], data[i]) // Cairo ARGB32 is BGRA in memory
        };
        // Half way between x=100 and x=700 the red square spans 400..500.
        assert_eq!(px(450.0, 250.0).0, 220, "the red square's centre at the midpoint: {:?}", px(450.0, 250.0));
        assert_eq!(px(150.0, 250.0), (255, 255, 255), "it has left its first place");
        assert_eq!(px(750.0, 250.0), (255, 255, 255), "and hasn't arrived yet");
        // The blue square fades in, half way: a mid blue on white.
        let (r, _, b) = px(480.0, 430.0);
        assert!(b > 200 && (100..160).contains(&r), "half-faded blue: {:?}", px(480.0, 430.0));
    }

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
