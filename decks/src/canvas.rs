// canvas.rs — Slide canvas rendering and image loading.
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk4::cairo;
use std::cell::RefCell;
use std::collections::HashMap;
use decks_core::engine::{Slide, SlideObject, MasterSlide};
use decks_core::engine::Run;

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
    // Sniff the format from the bytes: images extracted from a pptx are
    // written to extensionless temp files on purpose (gh-268: the
    // extension came from the untrusted document), and `image::open`
    // guesses by extension only, so every imported picture failed to load
    // and was drawn as the `<image>` placeholder (render lab `decks/image`).
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32).ok()?;
    {
        // Cairo's ARGB32 is premultiplied, native-endian (BGRA in memory
        // on little-endian), with a row stride that can exceed w * 4.
        let stride = surface.stride() as usize;
        let mut data = surface.data().ok()?;
        for (y, row) in rgba.rows().enumerate() {
            for (x, p) in row.enumerate() {
                let a = p[3] as u16;
                let pm = |c: u8| ((c as u16 * a + 127) / 255) as u8;
                let o = y * stride + x * 4;
                data[o] = pm(p[2]);
                data[o + 1] = pm(p[1]);
                data[o + 2] = pm(p[0]);
                data[o + 3] = p[3];
            }
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
            // The true outline: a click in an ellipse's bounding-box
            // corner is not a click on the ellipse.
            SlideObject::Shape { kind, x, y, w, h, .. } => {
                if decks_core::engine::shape::contains(kind, *w, *h, sx - *x, sy - *y) {
                    return Some(oi);
                }
            }
            SlideObject::Table { x, y, w, h, .. } => {
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
///
/// A slide on a layout with a look of its own gets the master with that
/// layout's background and decorations over it (`layouts::effective_master`).
pub fn master_for<'a>(
    slides: &[Slide],
    current_slide: usize,
    masters: &'a [MasterSlide],
) -> Option<std::borrow::Cow<'a, MasterSlide>> {
    let slide = slides.get(current_slide)?;
    let m = slide.master_idx.and_then(|mi| masters.get(mi))?;
    let own_look = slide.layout.and_then(|l| m.layouts.get(l)).is_some_and(|l| l.background.is_some() || !l.shapes.is_empty());
    Some(if own_look {
        std::borrow::Cow::Owned(decks_core::layouts::effective_master(m, slide.layout))
    } else {
        std::borrow::Cow::Borrowed(m)
    })
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

/// Put a text box's content into `layout`, styled run by run.
///
/// Slides and master decorations share this so that a run style honoured on
/// one is honoured on the other: the master path drew `text` with cairo's
/// toy API and ignored `runs` entirely, which is how a master could carry
/// styling that nothing drew. The empty-`runs` case is not a special style
/// but the absence of one — the box is plain text and `text` is the whole
/// of it.
///
/// `scale` converts a run's point size to the canvas's current zoom; it is
/// the same factor the caller used for the layout's base font.
pub fn set_styled_text(
    layout: &pango::Layout,
    text: &str,
    runs: &[Run],
    scale: f64,
) {
    if runs.is_empty() {
        layout.set_text(text);
        layout.set_attributes(None);
        return;
    }
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
        // The run's own typeface (pptx a:latin, theme fonts resolved; odp
        // fo:font-family). Without it every run took the master's font.
        if let Some(family) = run.style.font_family.as_deref().filter(|f| !f.trim().is_empty()) {
            add(pango::AttrString::new_family(family).into());
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
        // The run's own colour. The readers have always stored it (pptx
        // a:rPr solidFill, odp fo:color) and the writers write it back,
        // but nothing drew it: every run took the canvas text colour
        // (render lab decks/text-styles: red and blue text drawn black).
        if let Some((r, g, b)) = run.style.color.as_deref().and_then(hex_rgb16) {
            add(pango::AttrColor::new_foreground(r, g, b).into());
        }
    }
    layout.set_text(&buf);
    layout.set_attributes(Some(&attrs));
}

/// `RRGGBB` (optionally `#`-prefixed) as Pango's 16-bit colour channels.
fn hex_rgb16(hex: &str) -> Option<(u16, u16, u16)> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|v| v as u16 * 257);
    Some((c(0)?, c(2)?, c(4)?))
}

/// The layout one master decoration is drawn from.
///
/// Extracted so the master path's *use* of the runs is reachable from a
/// test: #732 left the equivalent call inside `draw_slide_multi` uncovered,
/// and a call site that silently stops passing its runs is exactly how a
/// carried style stops being drawn without anything failing.
///
/// Drawn through pango, and through the same run styling the slides use, so
/// a master's own emphasis shows. The cairo toy API this replaces could do
/// neither: it took one weight for the whole box — hardcoded bold, which
/// drew every unemphasised decoration as though the design asked for bold —
/// and it has no concept of a line break, so a multi-line decoration was
/// silently drawn as one line.
pub fn master_decoration_layout(
    cr: &cairo::Context,
    master: &MasterSlide,
    text: &str,
    runs: &[Run],
    scale: f64,
) -> pango::Layout {
    let layout = pangocairo::functions::create_layout(cr);
    layout.set_font_description(Some(&document_font_description(
        Some(master),
        11.0 * scale,
    )));
    set_styled_text(&layout, text, runs, scale);
    layout
}

#[allow(clippy::too_many_arguments)]
pub fn draw_slide(
    cr: &cairo::Context, width: f64, height: f64,
    slides: &[Slide], current_slide: usize, selected: Option<usize>,
    masters: &[MasterSlide], accent: (f64, f64, f64),
) {
    draw_slide_multi(cr, width, height, slides, current_slide, &selected.into_iter().collect::<std::collections::HashSet<_>>(), None, masters, accent);
}

/// What surrounds a slide: the editor's (grey, a shadow and a border, a
/// margin) or a show's (black, the slide as large as fits).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Chrome {
    #[default]
    Editor,
    Show,
    /// A slide shown inside other UI (the presenter display): as large as
    /// fits, a hairline border, nothing painted around it.
    Preview,
    /// A slide exported (export.rs): the slide alone, nothing around it
    /// and no border.
    Export,
}

/// The slide's frame on a `w`×`h` canvas for `chrome`.
pub fn slide_frame(width: f64, height: f64, chrome: Chrome) -> (f64, f64, f64, f64) {
    match chrome {
        Chrome::Editor => slide_geometry(width, height),
        Chrome::Show | Chrome::Preview | Chrome::Export => {
            let k = (width / 960.0).min(height / 540.0).max(0.01);
            let (w, h) = (960.0 * k, 540.0 * k);
            ((width - w) / 2.0, (height - h) / 2.0, w, h)
        }
    }
}

/// A whole slide as a show presents it: black around it, no editor
/// marks. Used by the audience and presenter windows.
pub fn draw_slide_show(cr: &cairo::Context, width: f64, height: f64, slides: &[Slide], index: usize, masters: &[MasterSlide]) {
    draw_slide_in(cr, width, height, slides, index, masters, Chrome::Show);
}

/// A whole slide with `chrome` around it, without editor marks.
pub fn draw_slide_in(cr: &cairo::Context, width: f64, height: f64, slides: &[Slide], index: usize, masters: &[MasterSlide], chrome: Chrome) {
    let objects: Vec<decks_core::magic_move::FrameObject> = slides
        .get(index)
        .map(|s| s.objects.iter().map(|o| decks_core::magic_move::FrameObject { object: o.clone(), opacity: 1.0 }).collect())
        .unwrap_or_default();
    draw_slide_objects(cr, width, height, slides, index, masters, chrome, &objects);
}

/// A slide's background and master with `objects` (each at its opacity)
/// instead of the slide's own: one moment of a build or a Magic Move.
/// Objects are clipped to the slide, so a build moving in from beyond an
/// edge enters from the edge.
#[allow(clippy::too_many_arguments)]
pub fn draw_slide_objects(
    cr: &cairo::Context,
    width: f64,
    height: f64,
    slides: &[Slide],
    index: usize,
    masters: &[MasterSlide],
    chrome: Chrome,
    objects: &[decks_core::magic_move::FrameObject],
) {
    let (frame, bg) = draw_slide_base(cr, width, height, slides, index, masters, chrome);
    let master = master_for(slides, index, masters);
    let master = master.as_deref();
    let _ = cr.save();
    cr.rectangle(frame.0, frame.1, frame.2, frame.3);
    cr.clip();
    for f in objects {
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
    let _ = cr.restore();
}

/// A slide's frame on the canvas `(x, y, w, h)` and the background colour
/// (Cairo channels) text contrasts with.
pub type SlideFrame = ((f64, f64, f64, f64), (f64, f64, f64));

/// Everything under a slide's objects: the canvas around it, its shadow,
/// its background (the master's when the slide has none) and the master's
/// decorations. Returns the slide's frame on the canvas and the background
/// colour text contrasts with. Shared by the editor and Magic Move frames.
pub fn draw_slide_base(
    cr: &cairo::Context, width: f64, height: f64,
    slides: &[Slide], current_slide: usize, masters: &[MasterSlide], chrome: Chrome,
) -> SlideFrame {
    suite_common::use_ui_font_rendering(cr);
    let (ox, oy, slide_w, slide_h) = slide_frame(width, height, chrome);
    if chrome == Chrome::Show {
        cr.set_source_rgb(0.0, 0.0, 0.0);
        cr.paint().unwrap();
    } else if chrome == Chrome::Editor {
        cr.set_source_rgb(0.86, 0.86, 0.86);
        cr.paint().unwrap();
        // Shadow
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.15);
        cr.rectangle(ox + 3.0, oy + 3.0, slide_w, slide_h);
        cr.fill().unwrap();
    }

    // Slide background — an unset (white) slide inherits its master's.
    // Captured for the text-color contrast check below
    // (tuna-os/gtk-office-suite#78).
    let mut slide_bg_rgb: (f64, f64, f64) = (1.0, 1.0, 1.0);
    if current_slide < slides.len() {
        let slide_bg = &slides[current_slide].background;
        let master = master_for(slides, current_slide, masters);
        let master_bg = master
            .as_deref()
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

    if chrome == Chrome::Editor || chrome == Chrome::Preview {
        // Border
        cr.set_source_rgb(0.7, 0.7, 0.7);
        cr.set_line_width(1.0);
        cr.rectangle(ox, oy, slide_w, slide_h);
        cr.stroke().unwrap();
    }

    // Draw master slide shapes (background pattern, logos, headers)
    if let Some(master) = master_for(slides, current_slide, masters) {
        let master: &MasterSlide = &master;
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
                SlideObject::TextBox { text, x, y, runs, .. } => {
                    let sx = ox + (x / 960.0) * slide_w;
                    let sy = oy + (y / 540.0) * slide_h;
                    cr.set_source_rgba(0.3, 0.3, 0.3, 0.4);
                    let layout =
                        master_decoration_layout(cr, master, text, runs, slide_w / 960.0);
                    cr.move_to(sx + 4.0, sy + 4.0);
                    pangocairo::functions::show_layout(cr, &layout);
                }
                // A styled shape (an odp master's, a theme's decoration)
                // carries its own paint: drawn as on a slide.
                SlideObject::Shape { .. } => {
                    draw_object(cr, obj, (ox, oy, slide_w, slide_h), slide_bg_rgb, Some(master));
                }
                _ => {}
            }
            cr.restore().unwrap();
        }
    }

    ((ox, oy, slide_w, slide_h), slide_bg_rgb)
}

/// An empty layout placeholder, while editing: a dashed outline and its
/// prompt ("Click to add title"), dimmed. Never drawn in a show, a
/// thumbnail or an export, which is where an empty box is simply empty.
fn draw_placeholder_prompt(cr: &cairo::Context, obj: &SlideObject, rect: (f64, f64, f64, f64), scale: f64, bg: (f64, f64, f64)) {
    let SlideObject::TextBox { text, body, .. } = obj else { return };
    let Some(role) = body.placeholder.filter(|_| text.is_empty()) else { return };
    let dark = 0.299 * bg.0 + 0.587 * bg.1 + 0.114 * bg.2 < 0.5;
    let ink = if dark { 1.0 } else { 0.0 };
    let (x, y, w, h) = rect;
    let _ = cr.save();
    cr.set_source_rgba(ink, ink, ink, 0.35);
    cr.set_line_width(1.0);
    cr.set_dash(&[4.0, 3.0], 0.0);
    cr.rectangle(x, y, w, h);
    let _ = cr.stroke();
    let layout = pangocairo::functions::create_layout(cr);
    let size = if role == decks_core::layouts::Placeholder::Title { 28.0 } else { 18.0 };
    // The editor's furniture, like the "Slide N" caption: not the author's font.
    let mut desc = pango::FontDescription::from_string("Sans");
    desc.set_absolute_size(size * scale * pango::SCALE as f64);
    layout.set_font_description(Some(&desc));
    layout.set_text(role.prompt());
    let (tw, th) = layout.pixel_size();
    cr.set_source_rgba(ink, ink, ink, 0.45);
    cr.move_to(x + (w - tw as f64) / 2.0, y + (h - th as f64) / 2.0);
    pangocairo::functions::show_layout(cr, &layout);
    let _ = cr.restore();
}

#[allow(clippy::too_many_arguments)]
pub fn draw_slide_multi(
    cr: &cairo::Context, width: f64, height: f64,
    slides: &[Slide], current_slide: usize, selected_indices: &std::collections::HashSet<usize>,
    marquee: Option<(f64, f64, f64, f64)>,
    masters: &[MasterSlide], accent: (f64, f64, f64),
) {
    suite_common::use_ui_font_rendering(cr);
    let (ar, ag, ab) = accent;
    let ((ox, oy, slide_w, slide_h), slide_bg_rgb) = draw_slide_base(cr, width, height, slides, current_slide, masters, Chrome::Editor);

    // Draw objects
    if current_slide < slides.len() {
        let master = master_for(slides, current_slide, masters);
        let master = master.as_deref();
        let frame = (ox, oy, slide_w, slide_h);
        for (oi, obj) in slides[current_slide].objects.iter().enumerate() {
            let rect = draw_object(cr, obj, frame, slide_bg_rgb, master);
            draw_placeholder_prompt(cr, obj, rect, slide_w / 960.0, slide_bg_rgb);
            if selected_indices.contains(&oi) {
                draw_selection(cr, selected_indices.len(), rect, (ar, ag, ab));
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

/// Draw one slide object in the slide frame `(ox, oy, slide_w, slide_h)`
/// (canvas pixels), text that names no colour contrasting with
/// `slide_bg_rgb`. Returns the object's box on the canvas, for selection
/// handles. Shared by the editor canvas and Magic Move's frames.
pub fn draw_object(
    cr: &cairo::Context,
    obj: &SlideObject,
    frame: (f64, f64, f64, f64),
    slide_bg_rgb: (f64, f64, f64),
    master: Option<&MasterSlide>,
) -> (f64, f64, f64, f64) {
    let (ox, oy, slide_w, slide_h) = frame;
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
        SlideObject::TextBox { text, runs, body, .. } => {
            // The colour for runs that name none; runs read from a
            // file carry the colour their styles resolve to.
            let luminance = 0.299 * slide_bg_rgb.0 + 0.587 * slide_bg_rgb.1 + 0.114 * slide_bg_rgb.2;
            if luminance < 0.5 {
                cr.set_source_rgb(0.95, 0.95, 0.95);
            } else {
                cr.set_source_rgb(0.1, 0.1, 0.1);
            }
            let scale = slide_w / 960.0;
            let desc = document_font_description(master, 18.0 * scale);
            if !body.is_plain() {
                crate::text_render::draw_text_body(cr, text, runs, body, (sx, sy, sw, sh), scale, &desc);
            } else {
                let layout = pangocairo::functions::create_layout(cr);
                layout.set_width(((sw - 8.0).max(8.0) as i32) * pango::SCALE);
                layout.set_wrap(pango::WrapMode::WordChar);
                layout.set_font_description(Some(&desc));
                set_styled_text(&layout, text, runs, scale);
                cr.move_to(sx + 4.0, sy + 4.0);
                pangocairo::functions::show_layout(cr, &layout);
            }
        }
        SlideObject::Rect { .. } => {
            cr.set_source_rgb(0.3, 0.5, 0.9);
            cr.rectangle(sx, sy, sw, sh);
            cr.fill().unwrap();
        }
        SlideObject::Shape { kind, style, .. } => {
            draw_shape(cr, kind, style, (sx, sy, sw, sh), slide_w / 960.0);
        }
        SlideObject::Table { table, .. } => {
            let desc = document_font_description(master, 18.0 * slide_w / 960.0);
            draw_table(cr, table, (sx, sy, sw, sh), slide_w / 960.0, &desc);
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
                let scale = (sw / iw).min(sh / ih);
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
    (sx, sy, sw, sh)
}

/// Smart guides (decks_core::guides) in the accent colour: solid lines
/// for alignment, and for equal spacing and matched sizes a line with end
/// ticks across each measured gap or side.
pub fn draw_guides(cr: &cairo::Context, width: f64, height: f64, guides: &[decks_core::guides::Guide], accent: (f64, f64, f64)) {
    use decks_core::guides::GuideKind;
    if guides.is_empty() {
        return;
    }
    let (ox, oy, sw, sh) = slide_geometry(width, height);
    let (kx, ky) = (sw / 960.0, sh / 540.0);
    let _ = cr.save();
    cr.set_source_rgb(accent.0, accent.1, accent.2);
    cr.set_line_width(1.0);
    for g in guides {
        // Canvas coordinates, on the half pixel so a 1 px line is crisp.
        let (x0, y0, x1, y1) = if g.vertical {
            let x = (ox + g.at * kx).round() + 0.5;
            (x, oy + g.from * ky, x, oy + g.to * ky)
        } else {
            let y = (oy + g.at * ky).round() + 0.5;
            (ox + g.from * kx, y, ox + g.to * kx, y)
        };
        cr.move_to(x0, y0);
        cr.line_to(x1, y1);
        if matches!(g.kind, GuideKind::Spacing | GuideKind::Size) {
            let t = 4.0;
            for (x, y) in [(x0, y0), (x1, y1)] {
                if g.vertical {
                    cr.move_to(x - t, y);
                    cr.line_to(x + t, y);
                } else {
                    cr.move_to(x, y - t);
                    cr.line_to(x, y + t);
                }
            }
        }
    }
    let _ = cr.stroke();
    let _ = cr.restore();
}

/// Handles for a single selection, an outline for one of several.
fn draw_selection(cr: &cairo::Context, count: usize, rect: (f64, f64, f64, f64), accent: (f64, f64, f64)) {
    let (sx, sy, sw, sh) = rect;
    if count == 1 {
        draw_handles(cr, sx, sy, sw, sh, accent);
    } else {
        cr.set_source_rgb(accent.0, accent.1, accent.2);
        cr.set_line_width(1.5);
        cr.rectangle(sx - 1.0, sy - 1.0, sw + 2.0, sh + 2.0);
        cr.stroke().unwrap();
    }
}

/// Draw a preset shape's outline in the box `(x, y, w, h)` (canvas pixels),
/// then fill and stroke it in its own style. `scale` is canvas pixels per
/// model unit, for the stroke width.
pub fn draw_shape(
    cr: &cairo::Context,
    kind: &decks_core::engine::shape::ShapeKind,
    style: &decks_core::engine::shape::ShapeStyle,
    (x, y, w, h): (f64, f64, f64, f64),
    scale: f64,
) {
    use decks_core::engine::shape::{polygon, ShapeKind};
    cr.new_path();
    match kind {
        ShapeKind::Ellipse => {
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            cr.save().unwrap();
            cr.translate(x + w / 2.0, y + h / 2.0);
            cr.scale(w / 2.0, h / 2.0);
            cr.arc(0.0, 0.0, 1.0, 0.0, 2.0 * std::f64::consts::PI);
            cr.restore().unwrap();
        }
        ShapeKind::RoundRect { radius } => {
            let r = radius.clamp(0.0, 0.5) * w.min(h);
            let pi = std::f64::consts::PI;
            cr.new_sub_path();
            cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
            cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
            cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
            cr.arc(x + r, y + r, r, pi, 1.5 * pi);
            cr.close_path();
        }
        _ => {
            for (i, (px, py)) in polygon(kind, w, h).unwrap_or_default().into_iter().enumerate() {
                if i == 0 {
                    cr.move_to(x + px, y + py);
                } else {
                    cr.line_to(x + px, y + py);
                }
            }
            cr.close_path();
        }
    }
    if let Some(grad) = &style.gradient {
        // Colours run along `angle` (clockwise from left-to-right) across
        // the box: the line through its centre, from the box's projection
        // at one end to the other.
        let a = grad.angle.to_radians();
        let (dx, dy) = (a.cos(), a.sin());
        let half = (w * dx.abs() + h * dy.abs()) / 2.0;
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let pattern = cairo::LinearGradient::new(cx - dx * half, cy - dy * half, cx + dx * half, cy + dy * half);
        for stop in &grad.stops {
            let (r, g, b) = stop.color.to_f64();
            pattern.add_color_stop_rgb(stop.pos, r, g, b);
        }
        cr.set_source(&pattern).unwrap();
        cr.fill_preserve().unwrap();
    } else if let Some(fill) = style.fill {
        let (r, g, b) = fill.to_f64();
        cr.set_source_rgb(r, g, b);
        cr.fill_preserve().unwrap();
    }
    if let Some(stroke) = style.stroke {
        let (r, g, b) = stroke.color.to_f64();
        cr.set_source_rgb(r, g, b);
        cr.set_line_width((stroke.width * scale).max(0.5));
        cr.stroke_preserve().unwrap();
    }
    cr.new_path();
}

/// Draw a table in the box `(x, y, w, h)` (canvas pixels): each cell's
/// fill from the table style, its text at DrawingML's default cell margins
/// (0.1 in across, 0.05 in down), then the style's white 1 pt rules.
/// `scale` is canvas pixels per model unit.
pub fn draw_table(
    cr: &cairo::Context,
    table: &decks_core::engine::table::TableData,
    (x, y, w, h): (f64, f64, f64, f64),
    scale: f64,
    font: &pango::FontDescription,
) {
    let (cols, rows) = table.fitted(w, h);
    let (mx, my) = table.margins();
    let (pad_x, pad_y) = (mx * scale, my * scale);
    let mut cy = y;
    for (r, rh) in rows.iter().enumerate() {
        let mut cx = x;
        for (c, cw) in cols.iter().enumerate() {
            let paint = table.cell_paint(r, c);
            if let Some(fill) = paint.fill {
                let (fr, fg, fb) = fill.to_f64();
                cr.set_source_rgb(fr, fg, fb);
                cr.rectangle(cx, cy, *cw, *rh);
                cr.fill().unwrap();
            }
            if let Some(cell) = table.rows.get(r).and_then(|row| row.get(c)) {
                let layout = pangocairo::functions::create_layout(cr);
                layout.set_font_description(Some(font));
                layout.set_width(((cw - 2.0 * pad_x).max(1.0) * pango::SCALE as f64) as i32);
                layout.set_wrap(pango::WrapMode::WordChar);
                set_styled_text(&layout, &cell.text(), &cell.runs, scale);
                if paint.bold {
                    let attrs = layout.attributes().unwrap_or_default();
                    attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
                    layout.set_attributes(Some(&attrs));
                }
                let (tr, tg, tb) = paint.text.to_f64();
                cr.set_source_rgb(tr, tg, tb);
                cr.save().unwrap();
                cr.rectangle(cx, cy, *cw, *rh);
                cr.clip();
                cr.move_to(cx + pad_x, cy + pad_y);
                pangocairo::functions::show_layout(cr, &layout);
                cr.restore().unwrap();
            }
            cx += cw;
        }
        cy += rh;
    }
    // "Medium Style 2" separates cells with white 1 pt rules.
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.set_line_width((1.0 * 960.0 / 720.0 * scale).max(1.0));
    let mut cx = x;
    for cw in &cols[..cols.len().saturating_sub(1)] {
        cx += cw;
        cr.move_to(cx, y);
        cr.line_to(cx, y + h);
    }
    let mut cy = y;
    for rh in &rows[..rows.len().saturating_sub(1)] {
        cy += rh;
        cr.move_to(x, cy);
        cr.line_to(x + w, cy);
    }
    cr.stroke().unwrap();
}

/// Render slide `index` exactly as the editor canvas draws it, cropped to
/// the slide, at `width` pixels wide (height follows 16:9). Used by the
/// render lab's Tier A capture (`test-render-dump`), so it deliberately goes
/// through `draw_slide` rather than a separate export path: what we compare
/// against LibreOffice must be what the user sees.
pub fn render_slide_png(
    slides: &[Slide], masters: &[MasterSlide], index: usize, width: i32, path: &std::path::Path,
) -> Result<(), String> {
    let height = width * 540 / 960;
    // draw_slide insets the slide to 92% of the canvas; size the canvas so
    // the slide lands at exactly width x height, then crop to it.
    let canvas_w = (width as f64 / 0.92).ceil();
    let canvas_h = (height as f64 / 0.92).ceil();
    let full = cairo::ImageSurface::create(cairo::Format::ARgb32, canvas_w as i32, canvas_h as i32)
        .map_err(|e| e.to_string())?;
    {
        let cr = cairo::Context::new(&full).map_err(|e| e.to_string())?;
        draw_slide(&cr, canvas_w, canvas_h, slides, index, None, masters, (0.0, 0.5, 1.0));
    }
    let (ox, oy, _, _) = slide_geometry(canvas_w, canvas_h);
    let out = cairo::ImageSurface::create(cairo::Format::ARgb32, width, height).map_err(|e| e.to_string())?;
    {
        let cr = cairo::Context::new(&out).map_err(|e| e.to_string())?;
        cr.set_source_surface(&full, -ox.round(), -oy.round()).map_err(|e| e.to_string())?;
        cr.paint().map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    out.write_to_png(&mut file).map_err(|e| e.to_string())
}

#[cfg(test)]
mod font_tests {
    use super::*;
    use decks_core::engine::RunStyle;

    /// A build half way, through the show's own drawing (builds::frame →
    /// draw_slide_objects): a square moving in from the left edge is half
    /// way from beyond the edge to its place, clipped to the slide, and a
    /// dissolving one is half opaque. The PNG is left in target/ to look at.
    #[test]
    fn a_build_midpoint_is_drawn_part_way_and_clipped_to_the_slide() {
        use decks_core::builds::{Build, BuildEffect, Edge};
        use decks_core::engine::shape::{Color, ShapeKind, ShapeStyle};
        let sq = |x: f64, y: f64, c: Color| SlideObject::Shape {
            kind: ShapeKind::Rect,
            x,
            y,
            w: 200.0,
            h: 100.0,
            rotation: 0.0,
            style: ShapeStyle { fill: Some(c), gradient: None, stroke: None },
        };
        let slide = Slide {
            title: String::new(),
            background: "#ffffff".into(),
            objects: vec![sq(400.0, 100.0, Color(220, 0, 0)), sq(400.0, 300.0, Color(0, 0, 220))],
            notes: String::new(),
            master_idx: None,
            transition: Default::default(),
            builds: vec![
                Build { object: 0, effect: BuildEffect::Move(Edge::Left), out: false },
                Build { object: 1, effect: BuildEffect::Dissolve, out: false },
            ],
            ids: Default::default(),
            layout: None,
        };
        let slides = [slide];
        let (w, h) = (960, 540);
        let draw = |step, t| {
            let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).unwrap();
            {
                let cr = cairo::Context::new(&surface).unwrap();
                let objects = decks_core::builds::frame(&slides[0], step, t);
                draw_slide_objects(&cr, w as f64, h as f64, &slides, 0, &[], Chrome::Show, &objects);
            }
            surface
        };
        let px = |surface: &mut cairo::ImageSurface, x: usize, y: usize| {
            let stride = surface.stride() as usize;
            let d = surface.data().unwrap();
            let i = y * stride + x * 4;
            (d[i + 2], d[i + 1], d[i])
        };
        // Move in from the left, half way: from x = -200 to 400, now at 100.
        let mut first = draw(0, 0.5);
        assert_eq!(px(&mut first, 150, 150), (220, 0, 0), "the red square half way in");
        assert_eq!(px(&mut first, 450, 150), (255, 255, 255), "not yet at its place");
        assert_eq!(px(&mut first, 450, 350), (255, 255, 255), "the blue one waits for its click");
        // After the first build, the second dissolves in.
        let mut second = draw(1, 0.5);
        assert_eq!(px(&mut second, 450, 150), (220, 0, 0));
        let (r, _, b) = px(&mut second, 450, 350);
        assert!(b > 200 && (100..160).contains(&r), "half-faded blue: {:?}", (r, b));
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/render-frames");
        std::fs::create_dir_all(&dir).unwrap();
        first.write_to_png(&mut std::fs::File::create(dir.join("build-move-in-midpoint.png")).unwrap()).unwrap();
    }

    fn master(font: &str) -> MasterSlide {
        MasterSlide {
            name: "M".into(),
            background: "#ffffff".into(),
            default_font: font.into(),
            shapes: vec![],
            page_emu: None,
            layouts: Vec::new(),
        }
    }

    fn slide(master_idx: Option<usize>) -> Slide {
        Slide {
            title: "S".into(),
            background: "#ffffff".into(),
            objects: vec![],
            notes: String::new(),
            master_idx,
            transition: Default::default(),
            builds: Vec::new(),
            ids: Default::default(),
            layout: None,
        }
    }

    /// Pictures imported from a pptx live in extensionless temp files
    /// (gh-268), so the loader must sniff the format from the bytes.
    /// Half-transparent red must come out premultiplied, as Cairo expects.
    #[test]
    fn an_image_without_an_extension_still_loads() {
        let dir = std::env::temp_dir().join(format!("decks-load-image-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("picture"); // no extension
        let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([255, 0, 0, 128]));
        img.save_with_format(&path, image::ImageFormat::Png).unwrap();

        let surf = load_image(path.to_str().unwrap()).expect("extensionless png should load");
        assert_eq!((surf.width(), surf.height()), (3, 2));
        // BGRA, premultiplied: red 255 at alpha 128 is stored as 128.
        surf.with_data(|data| assert_eq!(&data[0..4], &[0, 0, 128, 128])).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A run's colour reaches the layout as a foreground attribute; before,
    /// every run drew in the canvas text colour.
    #[test]
    fn a_coloured_run_is_drawn_in_its_colour() {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 10, 10).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        let layout = pangocairo::functions::create_layout(&cr);
        let runs = vec![
            Run { text: "plain ".into(), style: Default::default() },
            Run { text: "red".into(), style: RunStyle { color: Some("C80000".into()), ..Default::default() } },
        ];
        set_styled_text(&layout, "plain red", &runs, 1.0);
        let attrs = layout.attributes().expect("attributes set");
        let fg: Vec<_> = attrs
            .attributes()
            .into_iter()
            .filter(|a| a.type_() == pango::AttrType::Foreground)
            .collect();
        assert_eq!(fg.len(), 1, "one coloured run, one foreground attribute");
        let c = fg[0].downcast_ref::<pango::AttrColor>().unwrap().color();
        assert_eq!((c.red(), c.green(), c.blue()), (0xC8 * 257, 0, 0));
        assert_eq!((fg[0].start_index(), fg[0].end_index()), (6, 9), "only the red run");
        assert_eq!(hex_rgb16("#0000c8"), Some((0, 0, 0xC8 * 257)));
        assert_eq!(hex_rgb16("bad"), None);
    }

    /// A run's typeface reaches the layout as a family attribute over
    /// exactly that run.
    #[test]
    fn a_runs_font_family_is_drawn() {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 10, 10).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        let layout = pangocairo::functions::create_layout(&cr);
        let runs = vec![
            Run { text: "plain ".into(), style: Default::default() },
            Run { text: "serif".into(), style: RunStyle { font_family: Some("Liberation Serif".into()), ..Default::default() } },
        ];
        set_styled_text(&layout, "plain serif", &runs, 1.0);
        let attrs = layout.attributes().expect("attributes set");
        let fam: Vec<_> = attrs.attributes().into_iter().filter(|a| a.type_() == pango::AttrType::Family).collect();
        assert_eq!(fam.len(), 1);
        assert_eq!(fam[0].downcast_ref::<pango::AttrString>().unwrap().value().as_str(), "Liberation Serif");
        assert_eq!((fam[0].start_index(), fam[0].end_index()), (6, 11));
    }

    #[test]
    fn a_slide_resolves_the_master_it_points_at() {
        let masters = vec![master("A"), master("B")];
        let slides = vec![slide(Some(1))];
        assert_eq!(
            master_for(&slides, 0, &masters).map(|m| m.default_font.clone()).as_deref(),
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
        assert_eq!(master_font_family(master_for(&slides, 0, &masters).as_deref()), "Sans");
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

    fn layout() -> pango::Layout {
        use pangocairo::prelude::FontMapExt;
        pango::Layout::new(&pangocairo::FontMap::default().create_context())
    }

    fn run(text: &str, style: RunStyle) -> Run {
        Run { text: text.to_string(), style }
    }

    /// A box with no runs is plain text, not a styled box that happens to
    /// have no styles: whatever a previous layout left behind must not
    /// survive into it.
    #[test]
    fn a_box_without_runs_is_drawn_as_plain_text() {
        let l = layout();
        l.set_attributes(Some(&pango::AttrList::new()));
        set_styled_text(&l, "just text", &[], 1.0);
        assert_eq!(l.text(), "just text");
        assert!(l.attributes().is_none(), "a stale attribute list survived");
    }

    /// The drawn string is the runs concatenated — the same string the model
    /// keeps in `text` — with no separator invented between them.
    #[test]
    fn runs_are_drawn_as_one_string_with_no_separator() {
        let l = layout();
        set_styled_text(
            &l,
            "ignored when runs are present",
            &[
                run("Plain ", RunStyle::default()),
                run("Bold", RunStyle { bold: true, ..RunStyle::default() }),
            ],
            1.0,
        );
        assert_eq!(l.text(), "Plain Bold");
    }

    /// Emphasis lands on the run that asked for it and nothing else. The
    /// offsets are byte offsets into the concatenated string, so an
    /// off-by-one here bolds the wrong characters rather than failing
    /// loudly.
    #[test]
    fn a_runs_emphasis_covers_exactly_that_run() {
        let l = layout();
        set_styled_text(
            &l,
            "",
            &[
                run("Plain ", RunStyle::default()),
                run("Bold", RunStyle { bold: true, ..RunStyle::default() }),
            ],
            1.0,
        );
        let attrs = l.attributes().expect("styled runs produce an attribute list");
        let bold: Vec<_> = attrs
            .attributes()
            .into_iter()
            .filter(|a| a.type_() == pango::AttrType::Weight)
            .map(|a| (a.start_index(), a.end_index()))
            .collect();
        assert_eq!(
            bold,
            vec![("Plain ".len() as u32, "Plain Bold".len() as u32)],
            "bold did not cover exactly the emphasised run"
        );
    }

    fn ctx() -> cairo::Context {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 64, 64)
            .expect("an image surface needs no display");
        cairo::Context::new(&surface).expect("context")
    }

    /// The master path actually passes its decoration's runs through.
    ///
    /// The gap #732 recorded and could not close: a call site that stops
    /// passing its runs draws unstyled text and nothing fails, because
    /// nothing asserts over what was drawn. Going through the layout the
    /// master path builds — rather than the pixels it produces — makes that
    /// reachable without depending on which fonts are installed.
    #[test]
    fn a_master_decorations_emphasis_reaches_the_layout_it_is_drawn_from() {
        let m = master("Cantarell");
        let l = master_decoration_layout(
            &ctx(),
            &m,
            "ACME Confidential",
            &[
                run("ACME ", RunStyle::default()),
                run(
                    "Confidential",
                    RunStyle { bold: true, ..RunStyle::default() },
                ),
            ],
            1.0,
        );
        assert_eq!(l.text(), "ACME Confidential");
        let attrs = l
            .attributes()
            .expect("the decoration's runs never reached the layout");
        assert!(
            attrs
                .attributes()
                .into_iter()
                .any(|a| a.type_() == pango::AttrType::Weight),
            "the decoration was drawn without its emphasis"
        );
    }

    /// And the master's font still reaches it, which the same call site is
    /// also responsible for.
    #[test]
    fn a_master_decoration_is_drawn_in_the_masters_font() {
        let m = master("Liberation Serif");
        let l = master_decoration_layout(&ctx(), &m, "plain", &[], 1.0);
        assert_eq!(
            l.font_description()
                .and_then(|d| d.family())
                .map(|f| f.to_string())
                .as_deref(),
            Some("Liberation Serif")
        );
    }
}
