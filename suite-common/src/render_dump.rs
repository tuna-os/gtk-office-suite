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

/// Environment variable naming the PDF the headless `--export-pdf <out>` hook
/// writes (docs/EXPORT-PARITY-SPEC.md item 1). Each app's main() sets it from
/// the flag and schedules `schedule_export`; the app's `test-export-pdf`
/// action writes exactly this path.
pub const EXPORT_PDF_ENV: &str = "GTK_OFFICE_EXPORT_PDF";

/// The `--export-pdf` command-line flag each app registers with
/// `add_main_option` (`OptionArg::Filename`, so GLib hands it over as a
/// byte string, whatever the locale).
pub const EXPORT_PDF_FLAG: &str = "export-pdf";

/// Read the `--export-pdf <out>` value out of already-parsed local options:
/// `Ok(Some(path))` when the flag was given, `Ok(None)` when it was absent,
/// `Err` when it is present but unusable. Display-free (pure GLib), so this
/// is unit-tested without a display.
pub fn export_pdf_path(options: &glib::VariantDict) -> Result<Option<std::path::PathBuf>, String> {
    if !options.contains(EXPORT_PDF_FLAG) {
        return Ok(None);
    }
    match options.lookup::<Vec<u8>>(EXPORT_PDF_FLAG) {
        Ok(Some(mut bytes)) => {
            // GLib hands a Filename over as a NUL-terminated byte string.
            if bytes.last() == Some(&0) {
                bytes.pop();
            }
            String::from_utf8(bytes)
                .map(std::path::PathBuf::from)
                .map(Some)
                .map_err(|_| "--export-pdf output path is not UTF-8".to_string())
        }
        Ok(None) => Err("--export-pdf needs an output path".to_string()),
        Err(_) => Err("--export-pdf option has an unexpected type".to_string()),
    }
}

/// Headless `--export-pdf` driver: like `schedule`, but activates the app's
/// `test-export-pdf` action — which writes the PDF named by `EXPORT_PDF_ENV`
/// and does nothing else — instead of `test-render-dump`, then quits.
/// No-op unless the variable is set. Kept separate from `schedule` (which it
/// mirrors) so Tier A capture is untouched.
pub fn schedule_export(app: &impl IsA<gio::Application>) {
    let Some(out) = std::env::var_os(EXPORT_PDF_ENV) else {
        return;
    };
    if let Some(parent) = std::path::Path::new(&out).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    // Same settle wait as `schedule`: Letters relayouts on a 500 ms debounce
    // and images decode, so exporting on first paint would write a stale page.
    let app = app.as_ref().clone();
    let hold = std::env::var_os("GTK_OFFICE_RENDER_HOLD").is_some();
    let start = std::time::Instant::now();
    let last = std::cell::Cell::new((0, 0));
    let stable = std::cell::Cell::new(0u32);
    glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        let window = app.downcast_ref::<gtk4::Application>().and_then(|a| a.active_window());
        let size = window.as_ref().map_or((0, 0), |w| (w.width(), w.height()));
        if size == last.get() && size != (0, 0) {
            stable.set(stable.get() + 1);
        } else {
            stable.set(0);
            last.set(size);
        }
        let maximized = !hold || window.as_ref().is_some_and(|w| w.is_maximized());
        let elapsed = start.elapsed();
        let ready = elapsed >= std::time::Duration::from_millis(1500) && stable.get() >= 3 && maximized;
        if !ready && elapsed < std::time::Duration::from_secs(15) {
            return glib::ControlFlow::Continue;
        }
        if !ready {
            eprintln!("export-pdf: window never settled (size {size:?}, maximized {maximized}); exporting anyway");
        }
        if app.lookup_action("test-export-pdf").is_some() {
            app.activate_action("test-export-pdf", None);
        } else {
            eprintln!("export-pdf: app has no test-export-pdf action");
        }
        // Tier B keeps the app on screen so a browser can capture it; the
        // lab kills the process when it is done.
        if !hold {
            app.quit();
        }
        glib::ControlFlow::Break
    });
}

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
    // Wait for the window to settle before capturing: at least 1.5 s (Letters
    // relayouts on a 500 ms debounce; images decode), then until its size has
    // been stable for three polls. Under Broadway (HOLD), also until it is
    // maximized: GDK Broadway grows the window to the browser's size only
    // when the screen size arrives, and on a loaded machine that came after
    // a fixed 1.5 s, so the dump described a 1024x768 window the browser was
    // no longer showing (decks/shapes Tier B went missing).
    let hold = std::env::var_os("GTK_OFFICE_RENDER_HOLD").is_some();
    let start = std::time::Instant::now();
    let last = std::cell::Cell::new((0, 0));
    let stable = std::cell::Cell::new(0u32);
    glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        let window = app.downcast_ref::<gtk4::Application>().and_then(|a| a.active_window());
        let size = window.as_ref().map_or((0, 0), |w| (w.width(), w.height()));
        if size == last.get() && size != (0, 0) {
            stable.set(stable.get() + 1);
        } else {
            stable.set(0);
            last.set(size);
        }
        let maximized = !hold || window.as_ref().is_some_and(|w| w.is_maximized());
        let elapsed = start.elapsed();
        let ready = elapsed >= std::time::Duration::from_millis(1500) && stable.get() >= 3 && maximized;
        // Give up waiting after 15 s and capture what there is; the lab's
        // own checks (template match, A-vs-B agreement) will say so.
        if !ready && elapsed < std::time::Duration::from_secs(15) {
            return glib::ControlFlow::Continue;
        }
        if !ready {
            eprintln!("render-dump: window never settled (size {size:?}, maximized {maximized}); capturing anyway");
        }
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
        if !hold {
            app.quit();
        }
        glib::ControlFlow::Break
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

#[cfg(test)]
mod export_pdf_tests {
    use super::*;

    fn dict_with(path: &[u8]) -> glib::VariantDict {
        let dict = glib::VariantDict::new(None);
        dict.insert_value(EXPORT_PDF_FLAG, &path.to_vec().to_variant());
        dict
    }

    #[test]
    fn an_absent_flag_is_no_export() {
        let dict = glib::VariantDict::new(None);
        assert_eq!(export_pdf_path(&dict).unwrap(), None);
    }

    #[test]
    fn the_flag_value_is_the_output_path() {
        let dict = dict_with(b"out.pdf");
        assert_eq!(
            export_pdf_path(&dict).unwrap(),
            Some(std::path::PathBuf::from("out.pdf"))
        );
    }

    #[test]
    fn non_utf8_is_an_error_not_a_silent_skip() {
        let dict = dict_with(&[0xff, 0xfe]);
        assert!(export_pdf_path(&dict).is_err());
    }

    #[test]
    fn the_filename_nul_terminator_is_stripped() {
        let dict = dict_with(b"out.pdf\0");
        assert_eq!(
            export_pdf_path(&dict).unwrap(),
            Some(std::path::PathBuf::from("out.pdf"))
        );
    }
}
