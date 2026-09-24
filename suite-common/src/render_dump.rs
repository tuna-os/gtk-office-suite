// render_dump.rs — Test-only "what did we draw?" capture for the render lab.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// When GTK_OFFICE_RENDER_DUMP=<dir> is set, the app opens its document
// normally, waits until the main loop has been idle long enough for layout
// and first paint, activates its `app.test-render-dump` action (which each
// app registers to write <dir>/A-<n>.png per page or slide), and quits.
//
// This is Tier A of docs/RENDER-PARITY-ROADMAP.md. It is never active
// unless the environment variable is set, so a shipped app pays one
// getenv at startup.

use gtk4::{gio, glib, prelude::*};

pub const ENV: &str = "GTK_OFFICE_RENDER_DUMP";

/// Whether this process is a render-lab capture. Cached: widgets ask on
/// every frame to leave out editing chrome (caret, selection) that is not
/// document content and that LibreOffice's reference never shows.
pub fn active() -> bool {
    static ACTIVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ACTIVE.get_or_init(|| dump_dir().is_some())
}

/// The dump directory, if the render lab asked for one.
pub fn dump_dir() -> Option<std::path::PathBuf> {
    std::env::var_os(ENV).map(std::path::PathBuf::from)
}

/// Path for page/slide `index` (0-based) inside the dump directory.
pub fn page_path(dir: &std::path::Path, index: usize) -> std::path::PathBuf {
    dir.join(format!("A-{}.png", index + 1))
}

/// Path for the on-screen template of page/slide `index`: the region of
/// the window that geom.json names, exactly as GTK painted it on screen.
/// Tier B locates the page in the browser screenshot with it. Apps whose
/// A-<n>.png is already that region (Letters, Tables) don't write one.
pub fn screen_path(dir: &std::path::Path, index: usize) -> std::path::PathBuf {
    dir.join(format!("S-{}.png", index + 1))
}

/// Call once after the document has been opened. No-op unless the render
/// lab asked for a dump.
pub fn schedule(app: &impl IsA<gio::Application>) {
    if dump_dir().is_none() {
        return;
    }
    // The text caret blinks, so whether it is in a capture is timing, and
    // OCR reads a caret after a word as part of it ("Third|"). Hide it for
    // the capture; it is not document content.
    if let Some(display) = gtk4::gdk::Display::default() {
        let css = gtk4::CssProvider::new();
        css.load_from_string("* { caret-color: transparent; }");
        gtk4::style_context_add_provider_for_display(&display, &css, gtk4::STYLE_PROVIDER_PRIORITY_USER);
    }
    let app = app.as_ref().clone();
    // 1.5 s covers debounced relayout (Letters paginates on a 500 ms
    // debounce) and image decoding; the lab would rather be slow than
    // capture a half-laid-out page.
    glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
        if let Some(dir) = dump_dir() {
            let _ = std::fs::create_dir_all(&dir);
        }
        if app.lookup_action("test-render-dump").is_some() {
            app.activate_action("test-render-dump", None);
        } else {
            eprintln!("render-dump: app has no test-render-dump action");
        }
        // Tier B keeps the app on screen so a browser can capture it; the
        // lab kills the process when it is done.
        if std::env::var_os("GTK_OFFICE_RENDER_HOLD").is_none() {
            app.quit();
        }
    });
}

/// Record where the captured pages sit inside the toplevel's GDK surface,
/// as `<dir>/geom.json` = `[[x, y, w, h], ...]` (one per page, `rects` in
/// `widget` coordinates). Tier B (Broadway) crops the browser screenshot
/// with these, so both tiers compare the same pixels.
pub fn write_geometry(widget: &impl IsA<gtk4::Widget>, rects: &[(f64, f64, f64, f64)]) {
    let Some(dir) = dump_dir() else { return };
    let widget = widget.as_ref();
    let (Some(root), Some(native)) = (widget.root(), widget.native()) else { return };
    let Some(origin) = widget.compute_point(&root, &gtk4::graphene::Point::new(0.0, 0.0)) else { return };
    // The surface includes client-side decoration shadows around the root.
    let (sx, sy) = native.surface_transform();
    let json: Vec<String> = rects
        .iter()
        .map(|(x, y, w, h)| {
            format!("[{:.1},{:.1},{:.1},{:.1}]", x + origin.x() as f64 + sx, y + origin.y() as f64 + sy, w, h)
        })
        .collect();
    let _ = std::fs::write(dir.join("geom.json"), format!("[{}]", json.join(",")));
}

/// Render `widget` through GTK's own snapshot → GSK pipeline (the same
/// render nodes the compositor gets on screen) and save the region `clip`
/// (widget coordinates; `None` = the whole allocation) as a PNG.
///
/// This is what makes Tier A honest for widgets that don't draw through a
/// reusable Cairo function (Letters' TextView-on-pages): nothing is
/// re-implemented for the capture.
pub fn widget_to_png(
    widget: &impl IsA<gtk4::Widget>,
    clip: Option<(f64, f64, f64, f64)>,
    path: &std::path::Path,
) -> Result<(), String> {
    use gtk4::{gdk, graphene};
    let widget = widget.as_ref();
    let (w, h) = (widget.width() as f64, widget.height() as f64);
    if w <= 0.0 || h <= 0.0 {
        return Err("widget has no allocation (not mapped?)".into());
    }
    let paintable = gtk4::WidgetPaintable::new(Some(widget));
    let snapshot = gtk4::Snapshot::new();
    paintable.snapshot(&snapshot, w, h);
    let node = snapshot.to_node().ok_or("widget produced no render node (drew nothing)")?;
    let renderer = widget
        .native()
        .and_then(|n| n.renderer())
        .ok_or("widget is not in a realized window")?;
    let (x, y, cw, ch) = clip.unwrap_or((0.0, 0.0, w, h));
    let viewport = graphene::Rect::new(x as f32, y as f32, cw as f32, ch as f32);
    let texture: gdk::Texture = renderer.render_texture(&node, Some(&viewport));
    texture.save_to_png(path).map_err(|e| e.to_string())
}
