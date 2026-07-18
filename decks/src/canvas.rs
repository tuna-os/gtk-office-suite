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

// ── Coordinate conversion ────────────────────────────────────────────

/// Convert canvas (x,y) to slide coordinates (960x540 at 16:9).
pub fn canvas_to_slide(x: f64, y: f64, canvas_w: f64, canvas_h: f64) -> (f64, f64) {
    let slide_w = canvas_w * 0.85;
    let slide_h = slide_w * 9.0 / 16.0;
    let ox = (canvas_w - slide_w) / 2.0;
    let oy = (canvas_h - slide_h) / 2.0;
    let sx = (x - ox) / slide_w * 960.0;
    let sy = (y - oy) / slide_h * 540.0;
    (sx, sy)
}

/// Convert slide coordinates to canvas position.
pub fn slide_to_canvas(sx: f64, sy: f64, canvas_w: f64, canvas_h: f64) -> (f64, f64) {
    let slide_w = canvas_w * 0.85;
    let slide_h = slide_w * 9.0 / 16.0;
    let ox = (canvas_w - slide_w) / 2.0;
    let oy = (canvas_h - slide_h) / 2.0;
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
            | SlideObject::Rect { x, y, w, h }
            | SlideObject::Image { x, y, w, h, .. } => {
                if sx >= *x && sx <= *x + *w && sy >= *y && sy <= *y + *h {
                    return Some(oi);
                }
            }
            SlideObject::Circle { x: cx, y: cy, r } => {
                let dx = sx - *cx;
                let dy = sy - *cy;
                if dx * dx + dy * dy <= *r * *r { return Some(oi); }
            }
        }
    }
    None
}

// ── Main slide rendering ─────────────────────────────────────────────

pub fn draw_slide(
    cr: &cairo::Context, width: f64, height: f64,
    slides: &[Slide], current_slide: usize, selected: Option<usize>,
    masters: &[MasterSlide],
) {
    cr.set_source_rgb(0.86, 0.86, 0.86);
    cr.paint().unwrap();

    let slide_w = width * 0.85;
    let slide_h = slide_w * 9.0 / 16.0;
    let ox = (width - slide_w) / 2.0;
    let oy = (height - slide_h) / 2.0;

    // Shadow
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.15);
    cr.rectangle(ox + 3.0, oy + 3.0, slide_w, slide_h);
    cr.fill().unwrap();

    // Slide background
    if current_slide < slides.len() {
        let bg = &slides[current_slide].background;
        if bg == "#ffffff" || bg.is_empty() {
            cr.set_source_rgb(1.0, 1.0, 1.0);
        } else if bg.starts_with('#') && bg.len() >= 7 {
            let r = u8::from_str_radix(&bg[1..3], 16).unwrap_or(255) as f64 / 255.0;
            let g = u8::from_str_radix(&bg[3..5], 16).unwrap_or(255) as f64 / 255.0;
            let b = u8::from_str_radix(&bg[5..7], 16).unwrap_or(255) as f64 / 255.0;
            cr.set_source_rgb(r, g, b);
        } else { cr.set_source_rgb(1.0, 1.0, 1.0); }
    } else { cr.set_source_rgb(1.0, 1.0, 1.0); }
    cr.rectangle(ox, oy, slide_w, slide_h);
    cr.fill().unwrap();

    // Border
    cr.set_source_rgb(0.7, 0.7, 0.7);
    cr.set_line_width(1.0);
    cr.rectangle(ox, oy, slide_w, slide_h);
    cr.stroke().unwrap();

    // Draw master slide shapes (background pattern, logos, headers)
    if current_slide < slides.len() {
        if let Some(mi) = slides[current_slide].master_idx {
            if mi < masters.len() {
                let master = &masters[mi];
                for obj in &master.shapes {
                    cr.save().unwrap();
                    // Render master shapes with reduced opacity
                    match obj {
                        SlideObject::Rect { x, y, w, h } => {
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
                            cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                            cr.set_font_size(11.0);
                            cr.move_to(sx + 4.0, sy + 14.0);
                            cr.show_text(text).unwrap();
                        }
                        _ => {}
                    }
                    cr.restore().unwrap();
                }
            }
        }
    }

    // Draw objects
    if current_slide < slides.len() {
        for (oi, obj) in slides[current_slide].objects.iter().enumerate() {
            let is_selected = selected == Some(oi);
            match obj {
                SlideObject::TextBox { text, x, y, w, h, runs } => {
                    let sx = ox + (x / 960.0) * slide_w;
                    let sy = oy + (y / 540.0) * slide_h;
                    let sw = (w / 960.0) * slide_w;
                    let sh = (h / 540.0) * slide_h;
                    if is_selected {
                        cr.set_source_rgb(0.0, 0.5, 1.0);
                        cr.set_line_width(2.0);
                        cr.rectangle(sx - 2.0, sy - 2.0, sw + 4.0, sh + 4.0);
                        cr.stroke().unwrap();
                    }
                    // Pango layout with per-run attributes (shared WYSIWYG
                    // primitives with Letters) — replaces the old toy-text
                    // path that truncated at 20 chars and ignored styling.
                    cr.set_source_rgb(0.1, 0.1, 0.1);
                    let layout = pangocairo::functions::create_layout(cr);
                    layout.set_width(((sw - 8.0).max(8.0) as i32) * pango::SCALE);
                    layout.set_wrap(pango::WrapMode::WordChar);
                    layout.set_font_description(Some(&pango::FontDescription::from_string("Sans 16")));
                    if runs.is_empty() {
                        layout.set_text(text);
                    } else {
                        let attrs = pango::AttrList::new();
                        let mut buf = String::new();
                        for run in runs {
                            let start = buf.len() as u32;
                            buf.push_str(&run.text);
                            let end = buf.len() as u32;
                            let mut add = |mut a: pango::Attribute| {
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
                        }
                        layout.set_text(&buf);
                        layout.set_attributes(Some(&attrs));
                    }
                    cr.move_to(sx + 4.0, sy + 4.0);
                    pangocairo::functions::show_layout(cr, &layout);
                }
                SlideObject::Rect { x, y, w, h } => {
                    let sx = ox + (x / 960.0) * slide_w;
                    let sy = oy + (y / 540.0) * slide_h;
                    let sw = (w / 960.0) * slide_w;
                    let sh = (h / 540.0) * slide_h;
                    if is_selected {
                        cr.set_source_rgb(0.0, 0.5, 1.0);
                        cr.set_line_width(2.0);
                        cr.rectangle(sx - 2.0, sy - 2.0, sw + 4.0, sh + 4.0);
                        cr.stroke().unwrap();
                    }
                    cr.set_source_rgb(0.3, 0.5, 0.9);
                    cr.rectangle(sx, sy, sw, sh);
                    cr.fill().unwrap();
                }
                SlideObject::Circle { x, y, r } => {
                    let cx = ox + (x / 960.0) * slide_w;
                    let cy = oy + (y / 540.0) * slide_h;
                    let radius = (r / 540.0) * slide_h;
                    if is_selected {
                        cr.set_source_rgb(0.0, 0.5, 1.0);
                        cr.set_line_width(2.0);
                        cr.arc(cx, cy, radius + 2.0, 0.0, 2.0 * std::f64::consts::PI);
                        cr.stroke().unwrap();
                    }
                    cr.set_source_rgb(0.9, 0.3, 0.2);
                    cr.arc(cx, cy, radius, 0.0, 2.0 * std::f64::consts::PI);
                    cr.fill().unwrap();
                }
                SlideObject::Image { path, x, y, w, h } => {
                    let sx = ox + (x / 960.0) * slide_w;
                    let sy = oy + (y / 540.0) * slide_h;
                    let sw = (w / 960.0) * slide_w;
                    let sh = (h / 540.0) * slide_h;
                    if is_selected {
                        cr.set_source_rgb(0.0, 0.5, 1.0);
                        cr.set_line_width(2.0);
                        cr.rectangle(sx - 2.0, sy - 2.0, sw + 4.0, sh + 4.0);
                        cr.stroke().unwrap();
                    }
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
        }
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
