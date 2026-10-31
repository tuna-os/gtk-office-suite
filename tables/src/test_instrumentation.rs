// SPDX-License-Identifier: GPL-3.0-or-later
//! Opt-in observation hooks for GUI journeys, render parity and headless export.
//! All action registration lives behind the test-mode gate in `attach`.

use gtk4::prelude::*;
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use tables_core::controller::WorkbookController;

/// Register observation actions only in explicitly requested test/export mode.
/// Called after the drawing area exists so snapshots include GTK geometry.
pub fn attach(
    app: &adw::Application,
    controller: &Rc<RefCell<WorkbookController>>,
    drawing_area: &gtk4::DrawingArea,
    h_adj: &gtk4::Adjustment,
    v_adj: &gtk4::Adjustment,
) {
    if std::env::var_os("GTK_OFFICE_TEST_MODE").is_some() {
        let ctl = controller.clone();
        let snap_area = drawing_area.clone();
        let (sh, sv) = (h_adj.clone(), v_adj.clone());
        let act = gtk4::gio::SimpleAction::new("test-snapshot", None);
        act.connect_activate(move |_, _| {
            let Ok(path) = std::env::var("GTK_OFFICE_SNAPSHOT_PATH") else { return };
            let mut snap = tables_core::snapshot::snapshot(&ctl.borrow(), 0..100, 0..26);
            let view = (f64::from(snap_area.width()), f64::from(snap_area.height()));
            snap.cell_rects = tables_core::snapshot::cell_rects(&ctl.borrow().state.borrow().sheet(), (sh.value(), sv.value()), view);
            // In the window's *surface* coordinates, which is what
            // input arrives in: the root widget starts inside the CSD
            // resize border, which a window that isn't maximized has
            // (the 400 and 800 px display-matrix runs, #520).
            let (tx, ty) = snap_area.native().map_or((0.0, 0.0), |n| n.surface_transform());
            snap.grid_origin = snap_area
                .root()
                .and_then(|root| {
                    snap_area.compute_point(&root, &gtk4::graphene::Point::new(0.0, 0.0))
                })
                .map(|pt| ((f64::from(pt.x()) + tx).round() as i32, (f64::from(pt.y()) + ty).round() as i32));
            let _ = std::fs::write(path, snap.to_json());
        });
        app.add_action(&act);

        // Render lab Tier A (docs/RENDER-PARITY-ROADMAP.md): the grid
        // as GTK painted it, headers plus the used range. LibreOffice's
        // reference prints exactly that (headings and gridlines on), so
        // both images show the same cells.
        let (area, ctl, h, v) = (drawing_area.clone(), controller.clone(), h_adj.clone(), v_adj.clone());
        let act = gtk4::gio::SimpleAction::new("test-render-dump", None);
        act.connect_activate(move |_, _| {
            let Some(dir) = suite_common::render_dump::dump_dir() else { return };
            let path = suite_common::render_dump::page_path(&dir, 0);
            let (aw, ah) = (area.width() as f64, area.height() as f64);
            let state = ctl.borrow().state.clone();
            crate::grid_render::restore_saved_view(&state, &h, &v);
            // To the used range's far edge as it is on screen: past a
            // frozen pane and the scroll the file's view asked for.
            let used = {
                let state = state.borrow();
                let sheet = state.sheet();
                sheet.used_extent().map(|(r, c)| {
                    (
                        tables_core::sheet::col_x(c, h.value(), &sheet) + sheet.col_width(c),
                        tables_core::sheet::row_y(r, v.value(), &sheet) + sheet.row_height(r),
                    )
                })
            };
            let (w, h) = used.unwrap_or((aw, ah));
            let rect = (0.0, 0.0, w.min(aw), h.min(ah));
            suite_common::render_dump::write_geometry(&area, &[rect]);
            if let Err(e) = suite_common::render_dump::widget_to_png(&area, Some(rect), &path) {
                eprintln!("render-dump: {e}");
            }
        });
        app.add_action(&act);
        // Headless `--export-pdf <out>` (docs/TABLES-EXPORT-PARITY.md):
        // the used-range rect `test-render-dump` snapshots — computed
        // after `restore_saved_view`, so the frozen fixture's saved
        // scroll is honoured — written as a PDF to GTK_OFFICE_EXPORT_PDF
        // through the grid path (`render_sheet_pdf` on a PDF surface).
        // main.rs schedules this after the window settles.
        let (area, ctl, h, v) = (drawing_area.clone(), controller.clone(), h_adj.clone(), v_adj.clone());
        let act = gtk4::gio::SimpleAction::new("test-export-pdf", None);
        act.connect_activate(move |_, _| {
            let Some(out) = std::env::var_os(suite_common::render_dump::EXPORT_PDF_ENV) else { return };
            let state = ctl.borrow().state.clone();
            crate::grid_render::restore_saved_view(&state, &h, &v);
            let (aw, ah) = (area.width() as f64, area.height() as f64);
            let accent = suite_common::accent_rgb(&area);
            if let Err(e) = crate::grid_render::render_sheet_pdf(
                &state, h.value(), v.value(), aw, ah,
                accent, std::path::Path::new(&out),
            ) {
                eprintln!("export-pdf: {e}");
            }
        });
        app.add_action(&act);
    }
}
