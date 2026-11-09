//! Capturing a window to a PNG, for the showcase runs that make the site's
//! and the store's screenshots (HYLKI_SHOWCASE_*). The app's showcase hooks
//! and the widgets that open their own dialogs both call it.

use adw::prelude::*;

/// Render the window's widget tree to a PNG at 2x (crisp text for marketing
/// shots). Content only — the compositor's shadow/frame is not part of the
/// tree, which is exactly what the site and store screenshots want.
pub(crate) fn showcase_capture(win: &gtk::Widget, path: &str) {
    let (w, h) = (win.width(), win.height());
    if w == 0 || h == 0 {
        tracing::error!("showcase: window not realized");
        return;
    }
    let paintable = gtk::WidgetPaintable::new(Some(win));
    let snapshot = gtk::Snapshot::new();
    snapshot.scale(2.0, 2.0);
    gtk::prelude::PaintableExt::snapshot(&paintable, &snapshot, w as f64, h as f64);
    let Some(node) = snapshot.to_node() else {
        // A fully occluded window is suspended by the compositor and stops
        // producing frames, so the snapshot comes back empty. Raise it and
        // try again shortly.
        tracing::warn!("showcase: nothing to render (window suspended?) — presenting + retrying");
        if let Some(w) = win.root().and_then(|r| r.downcast::<gtk::Window>().ok()) {
            w.present();
        }
        let win = win.clone();
        let path = path.to_string();
        gtk::glib::timeout_add_seconds_local_once(2, move || {
            showcase_capture(&win, &path);
        });
        return;
    };
    let Some(renderer) = win.native().and_then(|n| n.renderer()) else {
        tracing::error!("showcase: no renderer");
        return;
    };
    let rect = gtk::graphene::Rect::new(0.0, 0.0, (w * 2) as f32, (h * 2) as f32);
    let texture = renderer.render_texture(&node, Some(&rect));
    match texture.save_to_png(path) {
        Ok(()) => tracing::info!("showcase: saved {path}"),
        Err(e) => tracing::error!("showcase: could not save {path}: {e}"),
    }
}
