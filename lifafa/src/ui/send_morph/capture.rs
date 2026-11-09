//! Pixels: stills of a widget, the colour behind a card, and the development
//! recorders (`HYLKI_MORPH_SLOW`, `HYLKI_MORPH_FRAMES`).

use super::flow::capture_next_frame;
use super::SendMorph;
use adw::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk};

/// HYLKI_MORPH_SLOW=<factor> plays the morph that many times slower, to
/// look at it (development only).
pub(super) fn slow_motion() -> f64 {
    thread_local! {
        static SLOW: f64 = std::env::var("HYLKI_MORPH_SLOW").ok().and_then(|v| v.parse().ok()).filter(|v: &f64| *v >= 1.0).unwrap_or(1.0);
    }
    SLOW.with(|s| *s)
}

/// HYLKI_MORPH_FRAMES=<dir> saves every frame of the morph there as a PNG
/// named by its time and progress (development only).
pub(super) fn record_frame(obj: &SendMorph) {
    let Some(dir) = std::env::var_os("HYLKI_MORPH_FRAMES") else {
        return;
    };
    if obj.width() <= 0 || obj.height() <= 0 {
        return;
    }
    let imp = obj.imp();
    let t = obj.motion_progress();
    let since = obj.elapsed_ms(imp.chrome_start.get());
    // The frame about to be drawn, drawn here first.
    let snap = gtk::Snapshot::new();
    let scale = imp.scale.get();
    snap.scale(scale, scale);
    obj.draw(&snap);
    let (Some(node), Some(renderer)) = (snap.to_node(), obj.native().and_then(|n| n.renderer()))
    else {
        return;
    };
    let rect = graphene::Rect::new(
        0.0,
        0.0,
        obj.width() as f32 * scale,
        obj.height() as f32 * scale,
    );
    let tex = renderer.render_texture(&node, Some(&rect));
    let landed = if imp.scene.borrow().is_some() {
        "m"
    } else {
        "w"
    };
    let path = std::path::Path::new(&dir).join(format!("{since:07.1}-{landed}-{t:.3}.png"));
    let _ = tex.save_to_png(path);
}

/// With HYLKI_MORPH_FRAMES set, the pane itself once the stage is gone, to
/// hold the last frame against: the whole window, with the pane's box
/// beside it to crop by, since the pane alone leaves out the window's
/// ground (development only).
pub(super) fn record_final(obj: &SendMorph) {
    let (Some(dir), Some(pane)) = (std::env::var_os("HYLKI_MORPH_FRAMES"), obj.parent()) else {
        return;
    };
    for (ms, name) in [(30, "zz-final-a.png"), (400, "zz-final-b.png")] {
        let (dir, pane) = (dir.clone(), pane.clone());
        glib::timeout_add_local_once(std::time::Duration::from_millis(ms), move || {
            let Some(window) = pane.root().map(|r| r.upcast::<gtk::Widget>()) else {
                return;
            };
            let Some(at) = pane.compute_bounds(&window) else {
                return;
            };
            capture_next_frame(&window, move |still| {
                let Some((tex, scale)) = still else { return };
                let dir = std::path::Path::new(&dir);
                let _ = tex.save_to_png(dir.join(name));
                let crop = format!(
                    "{}x{}+{}+{}",
                    (at.width() * scale) as i32,
                    (at.height() * scale) as i32,
                    (at.x() * scale) as i32,
                    (at.y() * scale) as i32
                );
                let _ = std::fs::write(dir.join(format!("{name}.crop")), crop);
            });
        });
    }
}

/// A still of `widget` as it is drawn now, at the display's scale, and that
/// scale. Nothing when part of it waits to be redrawn: see
/// [`capture_next_frame`].
pub(super) fn capture(widget: &gtk::Widget) -> Option<(gdk::Texture, f32)> {
    let (w, h) = (widget.width(), widget.height());
    if w <= 0 || h <= 0 {
        return None;
    }
    let native = widget.native()?;
    let renderer = native.renderer()?;
    let scale = native.surface().map(|s| s.scale()).unwrap_or(1.0) as f32;
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let snap = gtk::Snapshot::new();
    snap.scale(scale, scale);
    paintable.snapshot(&snap, w as f64, h as f64);
    let node = snap.to_node()?;
    let viewport = graphene::Rect::new(
        0.0,
        0.0,
        (w as f32 * scale).round(),
        (h as f32 * scale).round(),
    );
    Some((renderer.render_texture(&node, Some(&viewport)), scale))
}

/// The color of one pixel of a still, at logical point (x, y).
pub(super) fn sample(
    renderer: &gsk::Renderer,
    tex: &gdk::Texture,
    scale: f32,
    x: f32,
    y: f32,
) -> Option<gdk::RGBA> {
    let snap = gtk::Snapshot::new();
    snap.append_texture(
        tex,
        &graphene::Rect::new(0.0, 0.0, tex.width() as f32, tex.height() as f32),
    );
    let node = snap.to_node()?;
    let (px, py) = ((x * scale).floor(), (y * scale).floor());
    if px < 0.0 || py < 0.0 || px >= tex.width() as f32 || py >= tex.height() as f32 {
        return None;
    }
    pixel(renderer, &node, px, py)
}

/// The window's own color behind the pane: at the first of `points` where
/// the pane's still is see-through, what the whole window draws there.
pub(super) fn ground_behind(
    root: &gtk::Widget,
    still: &gdk::Texture,
    scale: f32,
    points: &[(f32, f32)],
) -> Option<gdk::RGBA> {
    let renderer = root.native()?.renderer()?;
    let window: gtk::Widget = root.root()?.upcast();
    let &(x, y) = points
        .iter()
        .find(|(x, y)| sample(&renderer, still, scale, *x, *y).is_some_and(|c| c.alpha() < 0.02))?;
    let at = root.compute_point(&window, &graphene::Point::new(x, y))?;
    let snap = gtk::Snapshot::new();
    snap.scale(scale, scale);
    gtk::WidgetPaintable::new(Some(&window)).snapshot(
        &snap,
        window.width() as f64,
        window.height() as f64,
    );
    let node = snap.to_node()?;
    pixel(
        &renderer,
        &node,
        (at.x() * scale).floor(),
        (at.y() * scale).floor(),
    )
}

/// One device pixel of a render node, at (px, py).
fn pixel(renderer: &gsk::Renderer, node: &gsk::RenderNode, px: f32, py: f32) -> Option<gdk::RGBA> {
    let one = renderer.render_texture(node, Some(&graphene::Rect::new(px, py, 1.0, 1.0)));
    let mut d = gdk::TextureDownloader::new(&one);
    d.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, _) = d.download_bytes();
    let b = bytes.get(..4)?;
    Some(gdk::RGBA::new(
        b[0] as f32 / 255.0,
        b[1] as f32 / 255.0,
        b[2] as f32 / 255.0,
        b[3] as f32 / 255.0,
    ))
}
