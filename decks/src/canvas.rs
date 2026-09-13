// canvas.rs — Slide canvas rendering and image loading.
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk4::cairo;
use std::cell::RefCell;
use std::collections::HashMap;
use decks_core::engine::{Slide, SlideObject, MasterSlide};

// ── Image loading with cache ─────────────────────────────────────────

thread_local! {
    static IMAGE_CACHE: RefCell<HashMap<String, cairo::ImageSurface>> =
        RefCell::new(HashMap::new());
}

pub fn load_image(path: &str) -> Option<cairo::ImageSurface> {
    let cached = IMAGE_CACHE.with(|cache| cache.borrow().get(path).cloned());
    if let Some(surf) = cached { return Some(surf); }
    if path.ends_with(".png") {
        if let Ok(mut file) = std::fs::File::open(path) {
            if let Ok(surf) = cairo::ImageSurface::create_from_png(&mut file) {
                IMAGE_CACHE.with(|c| { c.borrow_mut().insert(path.to_string(), surf.clone()); });
                return Some(surf);
            }
        }
    }
    let img = image::open(path).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32).ok()?;
    {
        let mut data = surface.data().ok()?;
        for (i, pixel) in rgba.chunks(4).enumerate() {
            let offset = i * 4;
            data[offset] = pixel[2];
            data[offset + 1] = pixel[1];
            data[offset + 2] = pixel[0];
            data[offset + 3] = pixel[3];
        }
    }
    surface.flush();
    IMAGE_CACHE.with(|c| { c.borrow_mut().insert(path.to_string(), surface.clone()); });
    Some(surface)
}

// ── Snap-to-grid ──────────────────────────────────────────────────────

pub const GRID_SPACING: f64 = 20.0;

pub fn snap_to_grid(value: f64, grid: f64) -> f64 {
    (value / grid).round() * grid
}

// ── Slide geometry: fit-to-viewport ──────────────────────────────────

/// Slide placement inside the canvas: scale 960x540 to fit the viewport
/// (whichever axis binds), leave an 8% margin, center. Returns
/// (origin_x, origin_y, slide_w, slide_h). Used identically by drawing,
/// hit-testing, and coordinate conversion so they can never disagree.
pub fn slide_geometry(canvas_w: f64, canvas_h: f64) -> (f64, f64, f64, f64) {
    let scale = (canvas_w / 960.0).min(canvas_h / 540.0).max(0.05) * 0.92;
    let slide_w = 960.0 * scale;
    let slide_h = 540.0 * scale;
    ((canvas_w - slide_w) / 2.0, (canvas_h - slide_h) / 2.0, slide_w, slide_h)
}

// ── Coordinate conversion ────────────────────────────────────────────

/// Convert canvas (x,y) to slide coordinates (960x540 at 16:9).
pub fn canvas_to_slide(x: f64, y: f64, canvas_w: f64, canvas_h: f64) -> (f64, f64) {
    let (ox, oy, slide_w, slide_h) = slide_geometry(canvas_w, canvas_h);
    let sx = (x - ox) / slide_w * 960.0;
    let sy = (y - oy) / slide_h * 540.0;
    (sx, sy)
}

/// Convert slide coordinates to canvas position.
pub fn slide_to_canvas(sx: f64, sy: f64, canvas_w: f64, canvas_h: f64) -> (f64, f64) {
    let (ox, oy, slide_w, slide_h) = slide_geometry(canvas_w, canvas_h);
    let x = ox + (sx / 960.0) * slide_w;
    let y = oy + (sy / 540.0) * slide_h;
    (x, y)
}

// ── Hit test ─────────────────────────────────────────────────────────

/// Hit-test slide objects. Returns index of topmost object under (x, y)
/// in slide coordinates, or None.
pub fn hit_test_object(objects: &[SlideObject], sx: f64, sy: f64) -> Option<usize> {
    for (oi, obj) in objects.iter().enumerate().rev() {
        match obj {
            SlideObject::TextBox { x, y, w, h, .. }
            | SlideObject::Rect { x, y, w, h, .. }
            | SlideObject::Image { x, y, w, h, .. } => {
                if sx >= *x && sx <= *x + *w && sy >= *y && sy <= *y + *h {
                    return Some(oi);
                }
            }
            SlideObject::Circle { x: cx, y: cy, r, .. } => {
                let dx = sx - *cx;
                let dy = sy - *cy;
                if dx * dx + dy * dy <= *r * *r { return Some(oi); }
            }
        }
    }
    None
}

// ── Selection Handle Definitions ──────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionHandle {
    TopLeft,
    TopCenter,
    TopRight,
    RightCenter,
    BottomRight,
    BottomCenter,
    BottomLeft,
    LeftCenter,
    Rotate,
}

pub const HANDLE_SIZE: f64 = 8.0;

/// Returns handle hit under canvas coords (cx, cy) if within threshold of object's handles.
pub fn hit_test_handles(
    obj: &SlideObject,
    cx: f64, cy: f64,
    canvas_w: f64, canvas_h: f64,
) -> Option<SelectionHandle> {
    let (ox, oy, slide_w, slide_h) = slide_geometry(canvas_w, canvas_h);
    let (x, y, w, h) = decks_core::undo::obj_bounds(obj);
    let sx = ox + (x / 960.0) * slide_w;
    let sy = oy + (y / 540.0) * slide_h;
    let sw = (w / 960.0) * slide_w;
    let sh = (h / 540.0) * slide_h;

    let rot_y = sy - 20.0;
    let rot_x = sx + sw / 2.0;
    let dist_sq = (cx - rot_x).powi(2) + (cy - rot_y).powi(2);
    if dist_sq <= 8.0 * 8.0 {
        return Some(SelectionHandle::Rotate);
    }

    let positions = [
        (SelectionHandle::TopLeft, sx, sy),
        (SelectionHandle::TopCenter, sx + sw / 2.0, sy),
        (SelectionHandle::TopRight, sx + sw, sy),
        (SelectionHandle::RightCenter, sx + sw, sy + sh / 2.0),
        (SelectionHandle::BottomRight, sx + sw, sy + sh),
        (SelectionHandle::BottomCenter, sx + sw / 2.0, sy + sh),
        (SelectionHandle::BottomLeft, sx, sy + sh),
        (SelectionHandle::LeftCenter, sx, sy + sh / 2.0),
    ];

    for (handle, hx, hy) in positions {
        if (cx - hx).abs() <= HANDLE_SIZE && (cy - hy).abs() <= HANDLE_SIZE {
            return Some(handle);
        }
    }
    None
}

pub fn draw_handles(cr: &cairo::Context, sx: f64, sy: f64, sw: f64, sh: f64, accent: (f64, f64, f64)) {
    let (ar, ag, ab) = accent;

    // Bounding stroke
    cr.set_source_rgb(ar, ag, ab);
    cr.set_line_width(1.5);
    cr.rectangle(sx - 1.0, sy - 1.0, sw + 2.0, sh + 2.0);
    cr.stroke().unwrap();

    // Rotation handle stem and circle
    let rot_x = sx + sw / 2.0;
    let rot_y = sy - 20.0;
    cr.move_to(rot_x, sy);
    cr.line_to(rot_x, rot_y);
    cr.stroke().unwrap();

    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.arc(rot_x, rot_y, 5.0, 0.0, 2.0 * std::f64::consts::PI);
    cr.fill().unwrap();
    cr.set_source_rgb(ar, ag, ab);
    cr.arc(rot_x, rot_y, 5.0, 0.0, 2.0 * std::f64::consts::PI);
    cr.stroke().unwrap();

    // 8 resize handle boxes
    let positions = [
        (sx, sy),
        (sx + sw / 2.0, sy),
        (sx + sw, sy),
        (sx + sw, sy + sh / 2.0),
        (sx + sw, sy + sh),
        (sx + sw / 2.0, sy + sh),
        (sx, sy + sh),
        (sx, sy + sh / 2.0),
    ];

    let hs = 6.0;
    for (hx, hy) in positions {
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.rectangle(hx - hs / 2.0, hy - hs / 2.0, hs, hs);
        cr.fill().unwrap();
        cr.set_source_rgb(ar, ag, ab);
        cr.set_line_width(1.5);
        cr.rectangle(hx - hs / 2.0, hy - hs / 2.0, hs, hs);
        cr.stroke().unwrap();
    }
}

// ── Main slide rendering ─────────────────────────────────────────────

/// Selection-highlight color, in place of a hardcoded blue
/// (tuna-os/gtk-office-suite#78). `AdwStyleManager::accent_color_rgba()`
/// would be the direct way to ask for this, but it's gated behind
/// libadwaita's v1_6 feature and CI's Ubuntu 24.04 runner only ships
/// libadwaita 1.5 — so this reads the `@accent_bg_color` named CSS color
/// instead, which is a plain GTK4 mechanism available since libadwaita
/// 1.0. `lookup_color` only exists on the (deprecated-since-4.10)
/// StyleContext; there's no non-deprecated widget-level replacement for
/// looking up a *named* color yet.
#[allow(deprecated)]
pub fn accent_rgb(widget: &impl gtk4::prelude::WidgetExt) -> (f64, f64, f64) {
    use gtk4::prelude::StyleContextExt;
    widget
        .style_context()
        .lookup_color("accent_bg_color")
        .map(|c| (c.red() as f64, c.green() as f64, c.blue() as f64))
        .unwrap_or((0.0, 0.5, 1.0))
}

/// The master a slide is drawn against, if it has one that exists.
///
/// `master_idx` is an index into a list that a reader, an undo step or a
/// snapshot may have changed, so it can point past the end; every caller
/// has to handle that. Before this existed `draw_slide_multi` resolved the
/// master three separate times — twice as `masters.get(mi)` and once as a
/// hand-written `mi < masters.len()` bounds check — which is three chances
/// for the styles applied to one slide to disagree about which master it
/// even has.
pub fn master_for<'a>(
    slides: &[Slide],
    current_slide: usize,
    masters: &'a [MasterSlide],
) -> Option<&'a MasterSlide> {
    slides
        .get(current_slide)?
        .master_idx
        .and_then(|mi| masters.get(mi))
}

/// The font family a slide's **document** text is drawn in: the master's
/// `default_font`, or the renderer's own default when there is no master or
/// it names nothing.
///
/// The blank-and-absent policy itself lives on `MasterSlide::font_family`,
/// because the renderer and both format writers have to agree on it: a
/// font carried into a package that the canvas would not have drawn is a
/// round-trip of something nothing honours, which is the defect this whole
/// row keeps collecting.
///
/// Chrome keeps its own hardcoded face on purpose: the `<image>` placeholder
/// label and the "Slide N" empty-slide indicator are this application's
/// furniture, not the author's content, and a deck whose master asks for a
/// display face should not restyle them.
pub fn master_font_family(master: Option<&MasterSlide>) -> &str {
    master.map(|m| m.font_family()).unwrap_or(MasterSlide::DEFAULT_FONT)
}

/// The pango description slide text is drawn with, at `base_pt` points.
///
/// Built here rather than inline at the draw site so a test can assert the
/// family actually reaches pango. `master_font_family` alone would only
/// prove which string was chosen, not that the renderer put it anywhere —
/// and a resolver whose answer is dropped on the floor is the shape of
/// defect this whole row keeps turning up.
pub fn document_font_description(
    master: Option<&MasterSlide>,
    base_pt: f64,
) -> pango::FontDescription {
    let mut desc = pango::FontDescription::from_string(master_font_family(master));
    desc.set_absolute_size(base_pt * 96.0 / 72.0 * pango::SCALE as f64);
    desc
}

#[allow(clippy::too_many_arguments)]
pub fn draw_slide(
    cr: &cairo::Context, width: f64, height: f64,
    slides: &[Slide], current_slide: usize, selected: Option<usize>,
    masters: &[MasterSlide], accent: (f64, f64, f64),
) {
    draw_slide_multi(cr, width, height, slides, current_slide, &selected.into_iter().collect::<std::collections::HashSet<_>>(), None, masters, accent);
}

#[allow(clippy::too_many_arguments)]
pub fn draw_slide_multi(
    cr: &cairo::Context, width: f64, height: f64,
    slides: &[Slide], current_slide: usize, selected_indices: &std::collections::HashSet<usize>,
    marquee: Option<(f64, f64, f64, f64)>,
    masters: &[MasterSlide], accent: (f64, f64, f64),
) {
    cr.set_source_rgb(0.86, 0.86, 0.86);
    cr.paint().unwrap();

    let (ar, ag, ab) = accent;

    let (ox, oy, slide_w, slide_h) = slide_geometry(width, height);

    // Shadow
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.15);
    cr.rectangle(ox + 3.0, oy + 3.0, slide_w, slide_h);
    cr.fill().unwrap();

    // Slide background — an unset (white) slide inherits its master's.
    // Captured for the text-color contrast check below
    // (tuna-os/gtk-office-suite#78).
    let mut slide_bg_rgb: (f64, f64, f64) = (1.0, 1.0, 1.0);
    if current_slide < slides.len() {
        let slide_bg = &slides[current_slide].background;
        let master_bg = master_for(slides, current_slide, masters)
            .map(|m| m.background.as_str())
            .filter(|b| !b.is_empty() && *b != "#ffffff");
        let bg: &str = if slide_bg == "#ffffff" || slide_bg.is_empty() {
            master_bg.unwrap_or(slide_bg)
        } else {
            slide_bg
        };
        let resolved_bg = if bg == "#ffffff" || bg.is_empty() {
            (1.0, 1.0, 1.0)
        } else if bg.starts_with('#') && bg.len() >= 7 {
            let r = u8::from_str_radix(&bg[1..3], 16).unwrap_or(255) as f64 / 255.0;
            let g = u8::from_str_radix(&bg[3..5], 16).unwrap_or(255) as f64 / 255.0;
            let b = u8::from_str_radix(&bg[5..7], 16).unwrap_or(255) as f64 / 255.0;
            (r, g, b)
        } else { (1.0, 1.0, 1.0) };
        cr.set_source_rgb(resolved_bg.0, resolved_bg.1, resolved_bg.2);
        slide_bg_rgb = resolved_bg;
    } else { cr.set_source_rgb(1.0, 1.0, 1.0); }
    cr.rectangle(ox, oy, slide_w, slide_h);
    cr.fill().unwrap();

    // Border
    cr.set_source_rgb(0.7, 0.7, 0.7);
    cr.set_line_width(1.0);
    cr.rectangle(ox, oy, slide_w, slide_h);
    cr.stroke().unwrap();

    // Draw master slide shapes (background pattern, logos, headers)
    if let Some(master) = master_for(slides, current_slide, masters) {
        for obj in &master.shapes {
            cr.save().unwrap();
            // Render master shapes with reduced opacity
            match obj {
                SlideObject::Rect { x, y, w, h, .. } => {
                    let sx = ox + (x / 960.0) * slide_w;
                    let sy = oy + (y / 540.0) * slide_h;
                    let sw = (w / 960.0) * slide_w;
                    let sh = (h / 540.0) * slide_h;
                    cr.set_source_rgba(0.8, 0.8, 0.8, 0.3);
                    cr.rectangle(sx, sy, sw, sh);
                    cr.fill().unwrap();
                }
                SlideObject::TextBox { text, x, y, .. } => {
                    let sx = ox + (x / 960.0) * slide_w;
                    let sy = oy + (y / 540.0) * slide_h;
                    cr.set_source_rgba(0.3, 0.3, 0.3, 0.4);
                    cr.select_font_face(
                        master_font_family(Some(master)),
                        cairo::FontSlant::Normal,
                        cairo::FontWeight::Bold,
                    );
                    cr.set_font_size(11.0);
                    cr.move_to(sx + 4.0, sy + 14.0);
                    cr.show_text(text).unwrap();
                }
                _ => {}
            }
            cr.restore().unwrap();
        }
    }

    // Draw objects
    if current_slide < slides.len() {
        for (oi, obj) in slides[current_slide].objects.iter().enumerate() {
            let is_selected = selected_indices.contains(&oi);
            let rot = obj.rotation();

            cr.save().unwrap();
            let (x, y, w, h) = decks_core::undo::obj_bounds(obj);
            let sx = ox + (x / 960.0) * slide_w;
            let sy = oy + (y / 540.0) * slide_h;
            let sw = (w / 960.0) * slide_w;
            let sh = (h / 540.0) * slide_h;

            if rot != 0.0 {
                let cx = sx + sw / 2.0;
                let cy = sy + sh / 2.0;
                cr.translate(cx, cy);
                cr.rotate(rot.to_radians());
                cr.translate(-cx, -cy);
            }

            match obj {
                SlideObject::TextBox { text, runs, .. } => {
                    let luminance = 0.299 * slide_bg_rgb.0 + 0.587 * slide_bg_rgb.1 + 0.114 * slide_bg_rgb.2;
                    if luminance < 0.5 {
                        cr.set_source_rgb(0.95, 0.95, 0.95);
                    } else {
                        cr.set_source_rgb(0.1, 0.1, 0.1);
                    }
                    let layout = pangocairo::functions::create_layout(cr);
                    layout.set_width(((sw - 8.0).max(8.0) as i32) * pango::SCALE);
                    layout.set_wrap(pango::WrapMode::WordChar);
                    let scale = slide_w / 960.0;
                    let base_pt = 18.0 * scale;
                    let desc = document_font_description(
                        master_for(slides, current_slide, masters),
                        base_pt,
                    );
                    layout.set_font_description(Some(&desc));
                    if runs.is_empty() {
                        layout.set_text(text);
                    } else {
                        let attrs = pango::AttrList::new();
                        let mut buf = String::new();
                        for run in runs {
                            let start = buf.len() as u32;
                            buf.push_str(&run.text);
                            let end = buf.len() as u32;
                            let add = |mut a: pango::Attribute| {
                                a.set_start_index(start);
                                a.set_end_index(end);
                                attrs.insert(a);
                            };
                            if run.style.bold {
                                add(pango::AttrInt::new_weight(pango::Weight::Bold).into());
                            }
                            if run.style.italic {
                                add(pango::AttrInt::new_style(pango::Style::Italic).into());
                            }
                            if run.style.underline {
                                add(pango::AttrInt::new_underline(pango::Underline::Single).into());
                            }
                            if run.style.strikethrough {
                                add(pango::AttrInt::new_strikethrough(true).into());
                            }
                            if let Some(hp) = run.style.font_size_hp {
                                let pt = hp as f64 / 2.0 * scale;
                                add(pango::AttrSize::new_size_absolute(
                                    (pt * 96.0 / 72.0 * pango::SCALE as f64) as i32,
                                )
                                .into());
                            }
                        }
                        layout.set_text(&buf);
                        layout.set_attributes(Some(&attrs));
                    }
                    cr.move_to(sx + 4.0, sy + 4.0);
                    pangocairo::functions::show_layout(cr, &layout);
                }
                SlideObject::Rect { .. } => {
                    cr.set_source_rgb(0.3, 0.5, 0.9);
                    cr.rectangle(sx, sy, sw, sh);
                    cr.fill().unwrap();
                }
                SlideObject::Circle { x: cx_slide, y: cy_slide, r: r_slide, .. } => {
                    let cx = ox + (cx_slide / 960.0) * slide_w;
                    let cy = oy + (cy_slide / 540.0) * slide_h;
                    let radius = (r_slide / 540.0) * slide_h;
                    cr.set_source_rgb(0.9, 0.3, 0.2);
                    cr.arc(cx, cy, radius, 0.0, 2.0 * std::f64::consts::PI);
                    cr.fill().unwrap();
                }
                SlideObject::Image { path, .. } => {
                    if let Some(img_surf) = load_image(path) {
                        let iw = img_surf.width() as f64;
                        let ih = img_surf.height() as f64;
                        let scale_x = sw / iw;
                        let scale_y = sh / ih;
                        let scale = scale_x.min(scale_y);
                        let dx = sx + (sw - iw * scale) / 2.0;
                        let dy = sy + (sh - ih * scale) / 2.0;
                        cr.save().unwrap();
                        cr.translate(dx, dy);
                        cr.scale(scale, scale);
                        cr.set_source_surface(&img_surf, 0.0, 0.0).unwrap();
                        cr.paint().unwrap();
                        cr.restore().unwrap();
                    } else {
                        cr.set_source_rgb(0.92, 0.92, 0.92);
                        cr.rectangle(sx, sy, sw, sh);
                        cr.fill().unwrap();
                        cr.set_source_rgb(0.6, 0.6, 0.6);
                        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
                        cr.set_font_size(11.0);
                        let txt = "<image>";
                        let ext = cr.text_extents(txt).unwrap();
                        cr.move_to(sx + (sw - ext.width()) / 2.0, sy + (sh + ext.height()) / 2.0);
                        cr.show_text(txt).unwrap();
                    }
                }
            }

            cr.restore().unwrap();

            // Draw selection handles / outline
            if is_selected {
                if selected_indices.len() == 1 {
                    draw_handles(cr, sx, sy, sw, sh, (ar, ag, ab));
                } else {
                    cr.set_source_rgb(ar, ag, ab);
                    cr.set_line_width(1.5);
                    cr.rectangle(sx - 1.0, sy - 1.0, sw + 2.0, sh + 2.0);
                    cr.stroke().unwrap();
                }
            }
        }
    }

    // Marquee drag rectangle
    if let Some((mx, my, mw, mh)) = marquee {
        let msx = ox + (mx / 960.0) * slide_w;
        let msy = oy + (my / 540.0) * slide_h;
        let msw = (mw / 960.0) * slide_w;
        let msh = (mh / 540.0) * slide_h;

        cr.set_source_rgba(ar, ag, ab, 0.15);
        cr.rectangle(msx, msy, msw, msh);
        cr.fill().unwrap();
        cr.set_source_rgba(ar, ag, ab, 0.8);
        cr.set_line_width(1.0);
        cr.set_dash(&[4.0, 4.0], 0.0);
        cr.rectangle(msx, msy, msw, msh);
        cr.stroke().unwrap();
        cr.set_dash(&[], 0.0);
    }

    // Empty slide indicator
    if current_slide < slides.len() && slides[current_slide].objects.is_empty() {
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.5);
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(14.0);
        let text = format!("Slide {}", current_slide + 1);
        let extents = cr.text_extents(&text).unwrap();
        cr.move_to(ox + (slide_w - extents.width()) / 2.0, oy + slide_h - 20.0);
        cr.show_text(&text).unwrap();
    }

    // Slide number badge
    if current_slide < slides.len() {
        let badge = format!("{}", current_slide + 1);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.4);
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(11.0);
        cr.move_to(ox + slide_w - 30.0, oy + 20.0);
        cr.show_text(&badge).unwrap();
    }
}

#[cfg(test)]
mod font_tests {
    use super::*;

    fn master(font: &str) -> MasterSlide {
        MasterSlide {
            name: "M".into(),
            background: "#ffffff".into(),
            default_font: font.into(),
            shapes: vec![],
        }
    }

    fn slide(master_idx: Option<usize>) -> Slide {
        Slide {
            title: "S".into(),
            background: "#ffffff".into(),
            objects: vec![],
            notes: String::new(),
            master_idx,
        }
    }

    #[test]
    fn a_slide_resolves_the_master_it_points_at() {
        let masters = vec![master("A"), master("B")];
        let slides = vec![slide(Some(1))];
        assert_eq!(
            master_for(&slides, 0, &masters).map(|m| m.default_font.as_str()),
            Some("B")
        );
    }

    // A master_idx can outlive the list it indexes — a reader, an undo step
    // or a recovered snapshot can all leave one pointing past the end. The
    // three hand-rolled resolutions this replaced each had to get that
    // right separately; now there is one place to be wrong.
    #[test]
    fn a_master_idx_past_the_end_resolves_to_no_master() {
        let masters = vec![master("A")];
        let slides = vec![slide(Some(7))];
        assert!(master_for(&slides, 0, &masters).is_none());
        // ...and the font falls back rather than panicking on the index.
        assert_eq!(master_font_family(master_for(&slides, 0, &masters)), "Sans");
    }

    #[test]
    fn a_slide_index_past_the_end_resolves_to_no_master() {
        let masters = vec![master("A")];
        let slides = vec![slide(Some(0))];
        assert!(master_for(&slides, 4, &masters).is_none());
        assert!(master_for(&[], 0, &masters).is_none());
    }

    #[test]
    fn a_slide_with_no_master_idx_resolves_to_no_master() {
        let masters = vec![master("A")];
        let slides = vec![slide(None)];
        assert!(master_for(&slides, 0, &masters).is_none());
    }

    #[test]
    fn a_master_that_names_a_font_gets_it() {
        let m = master("Liberation Serif");
        assert_eq!(master_font_family(Some(&m)), "Liberation Serif");
    }

    #[test]
    fn a_slide_with_no_master_falls_back_to_the_renderer_default() {
        assert_eq!(master_font_family(None), "Sans");
    }

    // A master read from a package that records no font leaves the field
    // empty, and `FontDescription::from_string("")` asks pango for a family
    // with no name — which resolves to whatever it likes rather than to the
    // default we intend. Blank-but-present is the case a naive
    // `unwrap_or` misses, so it gets its own test.
    #[test]
    fn a_master_whose_font_is_blank_falls_back_rather_than_asking_for_nothing() {
        for blank in ["", "   ", "\t"] {
            let m = master(blank);
            assert_eq!(
                master_font_family(Some(&m)),
                "Sans",
                "a master whose font is {blank:?} should fall back"
            );
        }
    }

    // The point of the row: the family has to reach pango, not merely be
    // chosen. This asserts against the description the renderer itself
    // builds, and needs no display and no font installed — pango reports
    // back the family it was asked for whether or not it can resolve it.
    #[test]
    fn the_masters_font_reaches_the_description_the_renderer_draws_with() {
        let m = master("Liberation Serif");
        let desc = document_font_description(Some(&m), 18.0);
        assert_eq!(desc.family().map(|f| f.to_string()).as_deref(), Some("Liberation Serif"));
    }

    #[test]
    fn the_description_carries_the_size_it_was_asked_for() {
        let desc = document_font_description(None, 18.0);
        assert_eq!(desc.family().map(|f| f.to_string()).as_deref(), Some("Sans"));
        assert_eq!(desc.size(), (18.0 * 96.0 / 72.0 * pango::SCALE as f64) as i32);
        assert!(desc.is_size_absolute());
    }
}
