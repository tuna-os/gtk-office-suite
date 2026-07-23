// SPDX-License-Identifier: GPL-3.0-or-later
//
// TablesWindow — Spreadsheet window with Cairo grid, formula bar, sheet tabs, and file I/O.
// GNOME GUI spec: AdwApplicationWindow + AdwToolbarView + AdwBreakpoint.

use gtk4::cairo::{self, Context};
use gtk4::{self as gtk, gio, glib, prelude::*};
use libadwaita as adw;
use adw::prelude::{AdwDialogExt, AlertDialogExt, AlertDialogExtManual};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use suite_common::events::{Hint, Listener};
use tables_core::controller::{WorkbookController, WorkbookState};
use tables_core::sheet::*;
use crate::grid_render::{draw_grid, auto_fit_column};
use suite_common::format::{NumberFormat, NumberFormatKind};

// ── Constants ──────────────────────────────────────────────────────────
// ── Shared state ─────────────────────────────────────────────────────
pub type AppState = WorkbookState;

struct HistoryActionListener {
    undo: gtk4::gio::SimpleAction,
    redo: gtk4::gio::SimpleAction,
}

impl Listener<Hint> for HistoryActionListener {
    fn on_event(&self, hint: &Hint) {
        if let Hint::UndoStateChanged { can_undo, can_redo } = hint {
            self.undo.set_enabled(*can_undo);
            self.redo.set_enabled(*can_redo);
        }
    }
}

// File I/O lives in tables_core::io; window code only adapts AppState.
use tables_core::io::{load_file_into_engine, load_xlsx_workbook};

fn save_engine_to_xlsx(path: &str, state: &AppState) -> Result<(), String> {
    let sheets: Vec<SheetModel> = state.sheets.iter().map(|s| s.borrow().clone()).collect();
    tables_core::io::save_sheets_to_xlsx_with_engine(path, &sheets, Some(&state.engine))
}

// ── Main window ────────────────────────────────────────────────────────

pub struct TablesWindow {
    pub window: adw::ApplicationWindow,
    drawing_area: gtk4::DrawingArea,
    h_adj: gtk4::Adjustment,
    v_adj: gtk4::Adjustment,
    fx_entry: gtk4::Entry,
    stack: gtk4::Stack,
    controller: Rc<RefCell<WorkbookController>>,
    state: Rc<RefCell<AppState>>,
    sheet_model: gtk4::StringList,
    sheet_switcher: gtk4::DropDown,
    current_path: Rc<RefCell<Option<std::path::PathBuf>>>,
}

impl TablesWindow {
    pub fn new(app: &adw::Application) -> Self {
        let controller = Rc::new(RefCell::new(
            WorkbookController::new(DEFAULT_ROWS, DEFAULT_COLS)
                .expect("Failed to create spreadsheet controller"),
        ));
        let state = controller.borrow().state.clone();
        let current_path: Rc<RefCell<Option<std::path::PathBuf>>> = Rc::new(RefCell::new(None));

        // ── Scrolling ──────────────────────────────────────────────────
        let h_adj = gtk4::Adjustment::new(0.0, 0.0, 5000.0, 10.0, 50.0, 500.0);
        let v_adj = gtk4::Adjustment::new(0.0, 0.0, 5000.0, 10.0, 50.0, 500.0);

        // ── Drawing area ────────────────────────────────────────────────
        // GridArea exposes each cell as a virtual AT-SPI child
        // (grid_area.rs, issue #87); it IS a DrawingArea otherwise.
        let grid_area = crate::grid_area::GridArea::default();
        let drawing_area = grid_area.clone().upcast::<gtk4::DrawingArea>();
        drawing_area.set_vexpand(true);
        drawing_area.set_hexpand(true);
        // A11y: name the grid and keep its description tracking the active
        // cell; the per-cell children carry the detail.
        drawing_area.set_accessible_role(gtk4::AccessibleRole::Table);
        drawing_area.update_property(&[gtk4::accessible::Property::Label("Spreadsheet grid")]);
        update_grid_a11y(&drawing_area, "A", 0, "");

        {
            let da_state = state.clone();
            let da_h = h_adj.clone();
            let da_v = v_adj.clone();
            drawing_area.set_draw_func(move |_da, cr, width, height| {
                draw_grid(cr, &da_state, width as f64, height as f64,
                          da_h.value(), da_v.value());
            });
        }

        h_adj.connect_value_changed({ let da = drawing_area.clone(); move |_| da.queue_draw() });
        v_adj.connect_value_changed({ let da = drawing_area.clone(); move |_| da.queue_draw() });

        // ── Formula bar: name box (cell ref) + fx entry, Calc-style ────
        let name_box = gtk4::Entry::new();
        name_box.set_width_chars(7);
        name_box.set_max_width_chars(7);
        name_box.set_text("A1");
        name_box.set_tooltip_text(Some("Cell reference — type one to jump"));
        name_box.update_property(&[gtk4::accessible::Property::Label("Cell reference")]);

        let fx_label = gtk4::Label::new(Some(" fx"));
        fx_label.add_css_class("dim-label");
        fx_label.set_width_chars(5);
        let fx_entry = gtk4::Entry::new();
        fx_entry.set_hexpand(true);
        fx_entry.set_placeholder_text(Some("Formula or value\u{2026}"));
        fx_entry.update_property(&[gtk4::accessible::Property::Label("Formula input")]);

        let fx_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        fx_bar.set_margin_start(6); fx_bar.set_margin_end(6);
        fx_bar.set_margin_top(6); fx_bar.set_margin_bottom(6);
        fx_bar.set_halign(gtk4::Align::Fill);
        fx_bar.append(&name_box);
        fx_bar.append(&fx_label);
        fx_bar.append(&fx_entry);

        // Live selection readout (status area, DESIGN-UI: "live, not
        // decorative"): cell ref in the name box; sum/avg/count of a range.
        let stats_label = gtk4::Label::new(None);
        stats_label.add_css_class("caption");
        stats_label.add_css_class("dim-label");
        stats_label.set_hexpand(true);
        stats_label.set_halign(gtk4::Align::End);
        stats_label.update_property(&[gtk4::accessible::Property::Label("Selection statistics")]);

        let refresh_sel: Rc<dyn Fn()> = {
            let s = state.clone();
            let nb = name_box.clone();
            let stats = stats_label.clone();
            let ga = grid_area.clone();
            Rc::new(move || {
                let st = s.borrow();
                let sh = st.sheet();
                ga.sync_cells(&sh.data, &sh.formats, &sh.col_widths, sh.selection_rect());
                nb.set_text(&format!(
                    "{}{}",
                    tables_core::sheet::col_label(sh.selected_col),
                    sh.selected_row + 1
                ));
                if sh.has_range_selection() {
                    let fmt = |v: f64| {
                        if v.fract() == 0.0 && v.abs() < 1e15 {
                            format!("{}", v as i64)
                        } else {
                            format!("{v:.2}")
                        }
                    };
                    let stats_v = sh.selection_stats();
                    let (r0, c0, r1, c1) = sh.selection_rect();
                    let range = format!(
                        "{}{}:{}{}",
                        tables_core::sheet::col_label(c0), r0 + 1,
                        tables_core::sheet::col_label(c1), r1 + 1
                    );
                    if stats_v.count > 0 {
                        stats.set_text(&format!(
                            "{}  ·  Sum {}  ·  Avg {}  ·  Count {}",
                            range, fmt(stats_v.sum), fmt(stats_v.avg), stats_v.count
                        ));
                    } else {
                        stats.set_text(&range);
                    }
                } else {
                    stats.set_text("");
                }
            })
        };

        // Format Cells sheet (DESIGN-UI §Tables): right-click or the
        // palette opens it; the chosen format applies to the selection.
        {
            let ctl = controller.clone();
            let da = drawing_area.clone();
            let refresh = refresh_sel.clone();
            let act = gtk4::gio::SimpleAction::new("format-cells", None);
            act.connect_activate(move |_, _| {
                let parent = da.root().and_downcast::<adw::ApplicationWindow>();
                show_format_cells_dialog(&ctl, &da, &refresh, parent);
            });
            app.add_action(&act);
            suite_common::actions::register_labels(&[("app.format-cells", "Format Cells…")]);
        }
        {
            let app2 = app.clone();
            let right = gtk4::GestureClick::new();
            right.set_button(3);
            right.connect_pressed(move |_, _, _, _| {
                app2.activate_action("format-cells", None);
            });
            drawing_area.add_controller(right);
        }

        // Ctrl+G focuses the name box for a keyboard-only jump.
        {
            let nb = name_box.clone();
            let act = gtk4::gio::SimpleAction::new("goto-cell", None);
            act.connect_activate(move |_, _| {
                nb.grab_focus();
                nb.select_region(0, -1);
            });
            app.add_action(&act);
            app.set_accels_for_action("app.goto-cell", &["<Primary>g"]);
        }

        // Name box: Enter jumps to the typed reference.
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let refresh = refresh_sel.clone();
            let fx = fx_entry.clone();
            name_box.connect_activate(move |nb| {
                if let Some((r, c)) = tables_core::sheet::parse_cell_ref(&nb.text()) {
                    {
                        let st = s.borrow();
                        let mut sh = st.sheet_mut();
                        let r = r.min(sh.rows.saturating_sub(1));
                        let c = c.min(sh.cols.saturating_sub(1));
                        sh.select_cell(r, c);
                    }
                    refresh();
                    da.queue_draw();
                    fx.grab_focus();
                }
            });
        }

        // Escape in the fx entry or name box returns focus to the grid
        // (Calc behavior: cancel editing, back to navigation).
        for entry in [&fx_entry, &name_box] {
            let da = drawing_area.clone();
            let key = gtk4::EventControllerKey::new();
            key.connect_key_pressed(move |_, keyval, _code, _mods| {
                if keyval == gtk::gdk::Key::Escape {
                    da.grab_focus();
                    return gtk4::glib::Propagation::Stop;
                }
                gtk4::glib::Propagation::Proceed
            });
            entry.add_controller(key);
        }

        // Wire formula bar: Enter commits
        {
            let s = state.clone();
            let ctl = controller.clone();
            let da = drawing_area.clone();
            let fx = fx_entry.clone();
            let refresh = refresh_sel.clone();
            fx_entry.connect_activate(move |_| {
                let val = fx.text().to_string();
                let (r, c) = {
                    let st = s.borrow();
                    let sh = st.sheet();
                    let r = sh.selected_row;
                    let c = sh.selected_col;
                    if let Some(rule) = &sh.validations[r][c] {
                        if !rule.validate(&val) {
                            let toast = adw::Toast::new("Invalid input — value rejected");
                            toast.set_timeout(3);
                            return;
                        }
                    }
                    (r, c)
                };
                ctl.borrow_mut().edit_cell(r, c, val);
                let shown = s.borrow().engine.cell(r, c);
                update_grid_a11y(&da, &tables_core::sheet::col_label(c), r, &shown);
                // Commit returns focus to the grid so arrow keys navigate
                // (Calc behavior); the next keystroke edits via the grid.
                refresh();
                da.grab_focus();
                da.queue_draw();
            });
        }

        // ── Mouse: single-click select ──────────────────────────────────
        {
            let s = state.clone();
            let ctl = controller.clone();
            let da = drawing_area.clone();
            let fx = fx_entry.clone();
            let h = h_adj.clone();
            let v = v_adj.clone();
            let refresh = refresh_sel.clone();
            let click = gtk4::GestureClick::new();
            click.connect_pressed(move |g, _n, x, y| {
                let wx = x + h.value();
                let wy = y + v.value();
                let st = s.borrow();
                let sh = st.sheet();
                // Check if click is in column header zone
                if wy < COL_HEADER_HEIGHT && wx > ROW_HEADER_WIDTH {
                    // Find which column was clicked
                    let mut cx = ROW_HEADER_WIDTH;
                    let mut clicked_col = None;
                    for c in 0..sh.cols {
                        cx += sh.col_width(c);
                        if wx < cx { clicked_col = Some(c); break; }
                    }
                    if let Some(col) = clicked_col {
                        drop(sh); drop(st);
                        ctl.borrow_mut().toggle_sort(col);
                        da.queue_draw();
                        return;
                    }
                }
                if let Some((col, row)) = xy_to_cell(wx, wy, h.value(), &*sh) {
                    drop(sh); drop(st);
                    let shift = g
                        .current_event_state()
                        .contains(gtk4::gdk::ModifierType::SHIFT_MASK);
                    {
                        let mut st = s.borrow_mut();
                        {
                            let shown = st.engine.cell(row, col);
                            update_grid_a11y(&da, &tables_core::sheet::col_label(col), row, &shown);
                        }
                        let mut sh = st.sheet_mut();
                        if shift {
                            sh.extend_selection(row, col);
                        } else {
                            sh.select_cell(row, col);
                        }
                        let val = sh.data[row][col].clone();
                        fx.set_text(&val);
                    }
                    refresh();
                    da.grab_focus();
                    da.queue_draw();
                }
            });
            drawing_area.add_controller(click);
        }

        // ── Column resize: drag divider in header ───────────────────────
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let h = h_adj.clone();
            let drag_col = Rc::new(Cell::new(None::<(usize, f64)>));
            let resize_before = Rc::new(RefCell::new(None::<SheetModel>));
            let drag = gtk4::GestureDrag::new();
            drag.set_button(1);
            let dc2 = drag_col.clone();
            let dc3 = drag_col.clone();
            let before_begin = resize_before.clone();
            let before_end = resize_before.clone();
            let s2 = s.clone();
            let resize_controller = controller.clone();
            let h2 = h.clone();
            drag.connect_drag_begin(move |_g, x, y| {
                let st = s.borrow();
                let sh = st.sheet();
                if let Some(col) = hit_col_divider(x as f64, y as f64, h.value(), &*sh) {
                    *before_begin.borrow_mut() = Some(sh.clone());
                    dc2.set(Some((col, sh.col_width(col))));
                }
            });
            drag.connect_drag_update(move |_g, dx, _dy| {
                if let Some((col, start_w)) = drag_col.get() {
                    let new_w = (start_w + dx as f64).clamp(30.0, 500.0);
                    let mut st = s2.borrow_mut();
                    let mut sh = st.sheet_mut();
                    sh.set_col_width(col, new_w);
                    drop(sh); drop(st);
                    da.queue_draw();
                }
            });
            drag.connect_drag_end(move |_g, _dx, _dy| {
                if let (Some((col, old_width)), Some(before)) =
                    (dc3.get(), before_end.borrow_mut().take())
                {
                    let new_width = resize_controller
                        .borrow()
                        .state
                        .borrow()
                        .sheet()
                        .col_width(col);
                    if (new_width - old_width).abs() > f64::EPSILON {
                        resize_controller
                            .borrow_mut()
                            .record_sheet_mutation("Resize Column", before);
                    }
                }
                dc3.set(None);
            });
            drawing_area.add_controller(drag);
        }

        // ── Drag range selection on the cell area ───────────────────────
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let h = h_adj.clone();
            let v = v_adj.clone();
            let refresh = refresh_sel.clone();
            // Anchor cell of an in-progress selection drag, set on begin
            // only when the press is in the cell area (not header/divider).
            let anchor = Rc::new(Cell::new(None::<(usize, usize)>));
            let drag = gtk4::GestureDrag::new();
            drag.set_button(1);
            {
                let s = s.clone();
                let h = h.clone();
                let v = v.clone();
                let anchor = anchor.clone();
                drag.connect_drag_begin(move |_g, x, y| {
                    let st = s.borrow();
                    let sh = st.sheet();
                    let wx = x + h.value();
                    let wy = y + v.value();
                    if hit_col_divider(x, y, h.value(), &sh).is_none() && wy >= COL_HEADER_HEIGHT {
                        if let Some((col, row)) = xy_to_cell(wx, wy, h.value(), &sh) {
                            anchor.set(Some((row, col)));
                        }
                    }
                });
            }
            {
                let s = s.clone();
                let anchor = anchor.clone();
                drag.connect_drag_update(move |g, dx, dy| {
                    let Some((ar, ac)) = anchor.get() else { return };
                    // Ignore sub-threshold jitters so plain clicks stay clicks.
                    if dx.abs() < 4.0 && dy.abs() < 4.0 { return; }
                    let Some((sx, sy)) = g.start_point() else { return };
                    {
                        let st = s.borrow();
                        let sh = st.sheet();
                        let wx = sx + dx + h.value();
                        let wy = (sy + dy + v.value()).max(COL_HEADER_HEIGHT);
                        let Some((col, row)) = xy_to_cell(wx, wy, h.value(), &sh) else { return };
                        drop(sh);
                        let mut sh = st.sheet_mut();
                        sh.select_cell(ar, ac);
                        sh.extend_selection(row, col);
                    }
                    refresh();
                    da.queue_draw();
                });
            }
            {
                let anchor = anchor.clone();
                drag.connect_drag_end(move |_g, _dx, _dy| anchor.set(None));
            }
            drawing_area.add_controller(drag);
        }

        // ── Cursor feedback: col-resize over divider ────────────────────
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let h = h_adj.clone();
            let motion = gtk4::EventControllerMotion::new();
            motion.connect_motion(move |_m, x, y| {
                let st = s.borrow();
                let sh = st.sheet();
                let over_div = hit_col_divider(x as f64, y as f64, h.value(), &*sh).is_some();
                let over_head = (y as f64) < COL_HEADER_HEIGHT && (x as f64) > ROW_HEADER_WIDTH;
                if over_div {
                    da.set_cursor_from_name(Some("col-resize"));
                } else if over_head {
                    da.set_cursor_from_name(Some("pointer"));
                } else {
                    da.set_cursor_from_name(Some("default"));
                }
            });
            drawing_area.add_controller(motion);
        }

        // ── Double-click: inline edit or auto-fit column ────────────────
        {
            let s = state.clone();
            let ctl = controller.clone();
            let da = drawing_area.clone();
            let h = h_adj.clone();
            let v = v_adj.clone();
            let dbl = gtk4::GestureClick::new();
            dbl.set_button(1);
            dbl.set_touch_only(false);
            dbl.connect_pressed(move |_g, n, x, y| {
                if n < 2 { return; }
                let wx = x + h.value();
                let wy = y + v.value();
                // Check for divider double-click first (auto-fit)
                {
                    let st = s.borrow();
                    let sh = st.sheet();
                    if let Some(col) = hit_col_divider(wx, wy, h.value(), &*sh) {
                        drop(sh); drop(st);
                        // Auto-fit by temporarily setting draw func to measure
                        let s2 = s.clone();
                        let h2 = h.clone();
                        let v2 = v.clone();
                        let da2 = da.clone();
                        da.set_draw_func(move |_area, cr, width, height| {
                            let mut st = s2.borrow_mut();
                            let mut sh = st.sheet_mut();
                            auto_fit_column(cr, &mut *sh, col, h2.value());
                            drop(sh);
                            draw_grid(cr, &s2, width as f64, height as f64, h2.value(), v2.value());
                            // Restore normal draw func
                            let s3 = s2.clone();
                            let h3 = h2.clone();
                            let v3 = v2.clone();
                            da2.set_draw_func(move |_, cr, w, h| {
                                draw_grid(cr, &s3, w as f64, h as f64, h3.value(), v3.value());
                            });
                        });
                        da.queue_draw();
                        return;
                    }
                }
                let st = s.borrow();
                let sh = st.sheet();
                if let Some((col, row)) = xy_to_cell(wx, wy, h.value(), &*sh) {
                    let mut st = s.borrow_mut();
                    let val = st.sheet().data[row][col].clone();
                    // Compute cell x-offset using per-column widths
                    let cell_x = ROW_HEADER_WIDTH + (0..col).map(|cc| st.sheet().col_width(cc)).sum::<f64>();
                    let cell_w = st.sheet().col_width(col);
                    drop(st);
                    // Position entry overlay at cell
                    let sx = cell_x - h.value();
                    let sy = COL_HEADER_HEIGHT + row as f64 * ROW_HEIGHT - v.value();
                    let entry = gtk4::Entry::new();
                    entry.set_text(&val);
                    entry.set_size_request(cell_w as i32 - 4, ROW_HEIGHT as i32 - 2);
                    let overlay = gtk4::Fixed::new();
                    overlay.put(&entry, sx, sy);
                    entry.grab_focus();
                    entry.select_region(0, -1);

                    let ctl2 = ctl.clone();
                    let da2 = da.clone();
                    entry.connect_activate(move |e| {
                        let new_val = e.text().to_string();
                        ctl2.borrow_mut().edit_cell(row, col, new_val);
                        e.parent().map(|p| { p.unparent(); });
                        da2.queue_draw();
                    });
                    let focus_ctrl = gtk4::EventControllerFocus::new();
                    let e2 = entry.clone();
                    focus_ctrl.connect_leave(move |_| {
                        e2.parent().map(|p| { p.unparent(); });
                    });
                    entry.add_controller(focus_ctrl);
                }
            });
            drawing_area.add_controller(dbl);
        }

        // ── Sheet switcher (dynamic) ────────────────────────────────────
        let sheet_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        sheet_bar.set_margin_start(6); sheet_bar.set_margin_end(6);
        sheet_bar.set_margin_top(2); sheet_bar.set_margin_bottom(2);

        let sheet_model = gtk4::StringList::new(&["Sheet1"]);
        let sheet_switcher = gtk4::DropDown::builder()
            .model(&sheet_model)
            .tooltip_text("Switch sheet")
            .build();
        sheet_bar.append(&sheet_switcher);
        sheet_switcher.update_property(&[gtk4::accessible::Property::Label("Sheet switcher")]);

        let add_btn = gtk4::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add sheet")
            .build();
        add_btn.set_css_classes(&["flat", "circular"]);
        add_btn.update_property(&[gtk4::accessible::Property::Label("Add sheet")]);
        sheet_bar.append(&add_btn);

        let rename_sheet_btn = gtk4::Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text("Rename sheet")
            .build();
        rename_sheet_btn.set_css_classes(&["flat", "circular"]);
        rename_sheet_btn.update_property(&[gtk4::accessible::Property::Label("Rename sheet")]);
        sheet_bar.append(&rename_sheet_btn);

        let move_sheet_left_btn = gtk4::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Move sheet left")
            .build();
        move_sheet_left_btn.set_css_classes(&["flat", "circular"]);
        move_sheet_left_btn
            .update_property(&[gtk4::accessible::Property::Label("Move sheet left")]);
        sheet_bar.append(&move_sheet_left_btn);

        let move_sheet_right_btn = gtk4::Button::builder()
            .icon_name("go-next-symbolic")
            .tooltip_text("Move sheet right")
            .build();
        move_sheet_right_btn.set_css_classes(&["flat", "circular"]);
        move_sheet_right_btn
            .update_property(&[gtk4::accessible::Property::Label("Move sheet right")]);
        sheet_bar.append(&move_sheet_right_btn);

        let delete_sheet_btn = gtk4::Button::builder()
            .icon_name("edit-delete-symbolic")
            .tooltip_text("Delete sheet")
            .build();
        delete_sheet_btn.set_css_classes(&["flat", "circular"]);
        delete_sheet_btn.update_property(&[gtk4::accessible::Property::Label("Delete sheet")]);
        sheet_bar.append(&delete_sheet_btn);
        // Selection statistics live at the right end of the sheet bar,
        // Calc-style (one bottom bar, tabs left / stats right).
        sheet_bar.append(&stats_label);

        // Rebuild the sheet-name dropdown from the live sheet list. Used
        // after any structural change (delete/rename/reorder) so the
        // switcher never drifts from `WorkbookState::sheets`.
        fn refresh_sheet_model(sm: &gtk4::StringList, state: &Rc<RefCell<WorkbookState>>) {
            let names: Vec<String> = state
                .borrow()
                .sheets
                .iter()
                .map(|sheet| sheet.borrow().name.clone())
                .collect();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            sm.splice(0, sm.n_items(), &refs);
        }

        // Rename sheet
        {
            let ctl = controller.clone();
            let sm = sheet_model.clone();
            let sd = sheet_switcher.clone();
            let btn = rename_sheet_btn.clone();
            rename_sheet_btn.connect_clicked(move |_| {
                let w = btn.root().and_downcast::<adw::ApplicationWindow>();
                let idx = sd.selected() as usize;
                let controller = ctl.borrow();
                let state = controller.state.clone();
                let current_name = state.borrow().sheets[idx].borrow().name.clone();
                drop(controller);

                let entry = gtk4::Entry::builder()
                    .text(&current_name)
                    .activates_default(true)
                    .build();
                let dlg = adw::AlertDialog::builder()
                    .heading(&suite_common::i18n("Rename Sheet"))
                    .build();
                dlg.set_extra_child(Some(&entry));
                dlg.add_response("cancel", &suite_common::i18n("Cancel"));
                dlg.add_response("rename", &suite_common::i18n("Rename"));
                dlg.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
                dlg.set_default_response(Some("rename"));
                let sm = sm.clone();
                dlg.connect_response(None, move |_, resp| {
                    if resp != "rename" {
                        return;
                    }
                    let name = entry.text().to_string();
                    if name.is_empty() || name == current_name {
                        return;
                    }
                    if state.borrow_mut().rename_sheet(idx, &name).is_ok() {
                        refresh_sheet_model(&sm, &state);
                    }
                });
                dlg.present(w.as_ref());
            });
        }

        // Move sheet left / right
        {
            let ctl = controller.clone();
            let sm = sheet_model.clone();
            let sd = sheet_switcher.clone();
            let da = drawing_area.clone();
            move_sheet_left_btn.connect_clicked(move |_| {
                let controller = ctl.borrow();
                let state = controller.state.clone();
                drop(controller);
                let idx = sd.selected() as usize;
                if idx == 0 {
                    return;
                }
                let count = state.borrow().sheets.len();
                let mut order: Vec<usize> = (0..count).collect();
                order.swap(idx, idx - 1);
                if state.borrow_mut().reorder_sheets(&order).is_ok() {
                    refresh_sheet_model(&sm, &state);
                    // `ctl`'s own borrow is dropped above: set_selected fires
                    // selected-notify synchronously, which also borrows `ctl`.
                    sd.set_selected((idx - 1) as u32);
                    da.queue_draw();
                }
            });
        }
        {
            let ctl = controller.clone();
            let sm = sheet_model.clone();
            let sd = sheet_switcher.clone();
            let da = drawing_area.clone();
            move_sheet_right_btn.connect_clicked(move |_| {
                let controller = ctl.borrow();
                let state = controller.state.clone();
                drop(controller);
                let idx = sd.selected() as usize;
                let count = state.borrow().sheets.len();
                if idx + 1 >= count {
                    return;
                }
                let mut order: Vec<usize> = (0..count).collect();
                order.swap(idx, idx + 1);
                if state.borrow_mut().reorder_sheets(&order).is_ok() {
                    refresh_sheet_model(&sm, &state);
                    // `ctl`'s own borrow is dropped above: set_selected fires
                    // selected-notify synchronously, which also borrows `ctl`.
                    sd.set_selected((idx + 1) as u32);
                    da.queue_draw();
                }
            });
        }

        // Delete sheet (with confirmation; refuses to delete the only sheet)
        {
            let ctl = controller.clone();
            let sm = sheet_model.clone();
            let sd = sheet_switcher.clone();
            let da = drawing_area.clone();
            let btn = delete_sheet_btn.clone();
            delete_sheet_btn.connect_clicked(move |_| {
                let w = btn.root().and_downcast::<adw::ApplicationWindow>();
                let controller = ctl.borrow();
                let state = controller.state.clone();
                let idx = sd.selected() as usize;
                drop(controller);
                if state.borrow().sheets.len() <= 1 {
                    let err = adw::AlertDialog::builder()
                        .heading(&suite_common::i18n("Can't Delete Sheet"))
                        .body(&suite_common::i18n("A workbook needs at least one sheet."))
                        .build();
                    err.add_response("ok", &suite_common::i18n("OK"));
                    err.present(w.as_ref());
                    return;
                }
                let name = state.borrow().sheets[idx].borrow().name.clone();
                let dlg = adw::AlertDialog::builder()
                    .heading(&suite_common::i18n("Delete Sheet?"))
                    .body(&format!("“{name}” will be permanently deleted."))
                    .build();
                dlg.add_response("cancel", &suite_common::i18n("Cancel"));
                dlg.add_response("delete", &suite_common::i18n("Delete"));
                dlg.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
                dlg.set_default_response(Some("cancel"));
                let sm = sm.clone();
                let sd = sd.clone();
                let da = da.clone();
                dlg.connect_response(None, move |_, resp| {
                    if resp != "delete" {
                        return;
                    }
                    if state.borrow_mut().delete_sheet(idx).is_ok() {
                        refresh_sheet_model(&sm, &state);
                        sd.set_selected(state.borrow().active_sheet as u32);
                        da.queue_draw();
                    }
                });
                dlg.present(w.as_ref());
            });
        }

        // Add sheet action
        {
            let ctl = controller.clone();
            let sm = sheet_model.clone();
            let sd = sheet_switcher.clone();
            let da = drawing_area.clone();
            add_btn.connect_clicked(move |_| {
                let idx = ctl.borrow().state.borrow().sheets.len();
                let name = format!("Sheet{}", idx + 1);
                {
                    let controller = ctl.borrow_mut();
                    controller
                        .state
                        .borrow_mut()
                        .add_sheet(name.clone(), DEFAULT_ROWS, DEFAULT_COLS)
                        .expect("add worksheet");
                    controller.state.borrow_mut().switch_sheet(idx).expect("switch worksheet");
                }
                sm.append(&name);
                // Dropped the controller borrow above: GtkDropDown fires
                // selected-notify synchronously, and that handler also
                // borrows `ctl` — holding our own borrow across this call
                // panics with "RefCell already borrowed" (issue found via
                // TablesMultiSheetSmoke GUI test).
                sd.set_selected(idx as u32);
                da.queue_draw();
            });
        }

        // Switch sheet. This also fires synchronously whenever add/rename/
        // delete/reorder call `sheet_switcher.set_selected()`, so it is the
        // one place that needs to refresh the grid's a11y description after
        // any structural sheet change.
        {
            let ctl = controller.clone();
            let s = state.clone();
            let da = drawing_area.clone();
            let fx = fx_entry.clone();
            sheet_switcher.connect_selected_notify(move |dd| {
                let idx = dd.selected() as usize;
                if ctl.borrow_mut().state.borrow_mut().switch_sheet(idx).is_ok() {
                    fx.set_text("");
                    refresh_grid_a11y(&da, &s);
                    da.queue_draw();
                }
            });
        }

        // ── Content stack ───────────────────────────────────────────────
        // Overlay scrollbars (GNOME idiom): thin indicators floating over
        // the grid edges instead of reserved gutters with a dead corner.
        let scroll_grid = gtk4::Overlay::new();
        scroll_grid.set_vexpand(true);
        scroll_grid.set_hexpand(true);
        let h_scroll = gtk4::Scrollbar::new(gtk4::Orientation::Horizontal, Some(&h_adj));
        let v_scroll = gtk4::Scrollbar::new(gtk4::Orientation::Vertical, Some(&v_adj));
        h_scroll.set_valign(gtk4::Align::End);
        h_scroll.set_margin_end(14); // keep clear of the vertical bar
        v_scroll.set_halign(gtk4::Align::End);
        v_scroll.set_margin_bottom(14);
        for sb in [&h_scroll, &v_scroll] {
            sb.add_css_class("overlay-indicator");
        }
        scroll_grid.set_child(Some(&drawing_area));
        scroll_grid.add_overlay(&v_scroll);
        scroll_grid.add_overlay(&h_scroll);

        let stack = gtk4::Stack::new();
        stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
        stack.set_transition_duration(200);
        let empty_page = suite_common::make_empty_state(
            "Tables",
            "Create a new spreadsheet or open an existing one",
            "x-office-spreadsheet-symbolic",
            "Open File\u{2026}",
        );
        stack.add_titled(&empty_page, Some("empty"), "Empty");
        stack.add_titled(&scroll_grid, Some("editor"), "Editor");
        stack.set_visible_child_name("empty");

        // ── Chart dialog ──────────────────────────────────────────────
        let win_ref = Rc::new(RefCell::new(None::<adw::ApplicationWindow>));
        let show_chart_dialog = {
            let wr = win_ref.clone();
            let s = state.clone();
            let ctl = controller.clone();
            Box::new(move || {
                let st = s.borrow();
                let active = st.active_sheet;
                let sheet = st.sheets[active].borrow();
                let col = sheet.selected_col;
                let mut data = Vec::new();
                for r in 0..sheet.rows {
                    let label = sheet.data[r][0].clone();
                    let val_str = &sheet.data[r][col];
                    if let Ok(val) = val_str.parse::<f64>() {
                        let lbl = if label.is_empty() { format!("Row {}", r + 1) } else { label };
                        data.push((lbl, val));
                    }
                }
                if data.is_empty() { return; }

                let dialog = adw::Dialog::builder()
                    .title(&suite_common::i18n("Chart"))
                    .content_width(600)
                    .content_height(480)
                    .build();

                let chart_type = Rc::new(Cell::new(crate::charts::ChartType::Bar));
                let data_rc = Rc::new(data);

                let preview = gtk::DrawingArea::new();
                preview.set_vexpand(true);
                preview.set_hexpand(true);
                let ct = chart_type.clone();
                let d = data_rc.clone();
                preview.set_draw_func(move |_, cr, w, h| {
                    let surface = crate::charts::render_chart(&d, ct.get(), w, h);
                    cr.set_source_surface(&surface, 0.0, 0.0).unwrap();
                    cr.paint().unwrap();
                });

                let type_combo = gtk::DropDown::from_strings(&["Bar", "Line", "Pie"]);
                let ct2 = chart_type.clone();
                let pv = preview.clone();
                type_combo.connect_selected_notify(move |dd| {
                    ct2.set(match dd.selected() {
                        0 => crate::charts::ChartType::Bar,
                        1 => crate::charts::ChartType::Line,
                        _ => crate::charts::ChartType::Pie,
                    });
                    pv.queue_draw();
                });

                let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                header.set_margin_start(12); header.set_margin_end(12); header.set_margin_top(6);
                header.append(&gtk::Label::new(Some("Type:")));
                header.append(&type_combo);

                // Insert persists the chart on the sheet (saved into xlsx).
                let insert_btn = gtk::Button::with_label(&suite_common::i18n("Insert into Sheet"));
                insert_btn.add_css_class("suggested-action");
                insert_btn.set_halign(gtk::Align::End);
                insert_btn.set_margin_end(12);
                insert_btn.set_margin_bottom(12);
                {
                    let ctl = ctl.clone();
                    let ct = chart_type.clone();
                    let dlg = dialog.clone();
                    insert_btn.connect_clicked(move |_| {
                        use tables_core::sheet::{ChartKind, ChartSpec};
                        let (col, first, last) = {
                            let state = ctl.borrow().state.clone();
                            let st = state.borrow();
                            let sheet = st.sheet();
                            let col = sheet.selected_col;
                            let mut first = None;
                            let mut last = 0;
                            for r in 0..sheet.rows {
                                if sheet.data[r][col].parse::<f64>().is_ok() {
                                    first.get_or_insert(r);
                                    last = r;
                                }
                            }
                            (col, first, last)
                        };
                        let Some(first) = first else { return };
                        let kind = match ct.get() {
                            crate::charts::ChartType::Bar => ChartKind::Bar,
                            crate::charts::ChartType::Line => ChartKind::Line,
                            crate::charts::ChartType::Pie => ChartKind::Pie,
                        };
                        let chart = ChartSpec {
                            kind,
                            title: String::new(),
                            cat: (first, 0, last),
                            val: (first, col, last),
                            anchor: (last + 2, col),
                        };
                        ctl.borrow_mut().mutate_sheet("Insert Chart", move |sheet| {
                            sheet.charts.push(chart);
                        });
                        dlg.close();
                    });
                }

                let box_content = gtk::Box::new(gtk::Orientation::Vertical, 6);
                box_content.append(&header);
                box_content.append(&preview);
                box_content.append(&insert_btn);
                dialog.set_child(Some(&box_content));
                let pw = wr.borrow().clone();
                dialog.present(pw.as_ref());
            })
        };

        let s_pdf = state.clone();
        let win_ref = Rc::new(RefCell::new(None));
        let export_pdf = Box::new({
            let s = s_pdf.clone();
            let wr = win_ref.clone();
            move || {
                let parent_win = wr.borrow().clone();
                let s2 = s.clone();
                let dlg = gtk4::FileDialog::new();
                let f = gtk4::FileFilter::new();
                f.add_pattern("*.pdf");
                f.set_name(Some("PDF Documents"));
                let fl = gio::ListStore::new::<gtk4::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));
                dlg.set_initial_name(Some("Spreadsheet.pdf"));
                
                let wr2 = wr.clone();
                dlg.save(parent_win.as_ref(), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                let mut st = s2.borrow_mut();
                                // Sync sheet data to engine first
                                let active = st.active_sheet;
                                let sheet_model = st.sheets[active].borrow().clone();
                                for r in 0..sheet_model.rows {
                                    for c in 0..sheet_model.cols {
                                        st.engine.set_cell_text(r, c, &sheet_model.data[r][c]);
                                    }
                                }
                                st.engine.evaluate();
                                let parent_win = wr2.borrow().clone();
                                if let Err(err_msg) = tables_core::export::to_pdf(&st.engine, &path_str) {
                                    let alert = adw::AlertDialog::builder()
                                        .heading("Export Failed")
                                        .body(&err_msg)
                                        .build();
                                    alert.add_response("ok", "OK");
                                    alert.present(parent_win.as_ref());
                                } else {
                                    println!("PDF exported successfully to {}", path_str);
                                }
                            }
                        }
                    },
                );
            }
        });

        let toggle_format = {
            let ctl = controller.clone();
            let da = drawing_area.clone();
            Box::new(move || {
                ctl.borrow_mut().mutate_sheet("Change Number Format", |sh| {
                    let r = sh.selected_row;
                    let c = sh.selected_col;
                    let current = &sh.formats[r][c].kind;
                    let next = match current {
                        NumberFormatKind::General => NumberFormatKind::Number(2),
                        NumberFormatKind::Number(_) => NumberFormatKind::Currency("$".into(), 2),
                        NumberFormatKind::Currency(_, _) => NumberFormatKind::Percent(1),
                        NumberFormatKind::Percent(_) => NumberFormatKind::Date("%Y-%m-%d".into()),
                        NumberFormatKind::Date(_) => NumberFormatKind::Scientific(2),
                        NumberFormatKind::Scientific(_) => NumberFormatKind::General,
                        _ => NumberFormatKind::General,
                    };
                    sh.formats[r][c] = NumberFormat::new(next);
                });
                da.queue_draw();
            })
        };

        let toggle_border = {
            let ctl = controller.clone();
            let da = drawing_area.clone();
            Box::new(move || {
                ctl.borrow_mut().mutate_sheet("Change Cell Border", |sh| {
                    let r = sh.selected_row;
                    let c = sh.selected_col;
                    let current = &sh.borders[r][c].top;
                    let next = match current {
                        BorderStyle::None => BorderStyle::Solid,
                        BorderStyle::Solid => BorderStyle::Dashed,
                        BorderStyle::Dashed => BorderStyle::Dotted,
                        BorderStyle::Dotted => BorderStyle::Double,
                        BorderStyle::Double => BorderStyle::None,
                    };
                    sh.borders[r][c] = CellBorder::outline(next, (0.0, 0.0, 0.0));
                });
                da.queue_draw();
            })
        };

        let toggle_merge = {
            let ctl = controller.clone();
            let da = drawing_area.clone();
            Box::new(move || {
                ctl.borrow_mut()
                    .mutate_sheet("Merge Cells", SheetModel::toggle_merge);
                da.queue_draw();
            })
        };

        // Toolbar operations are named GioActions so they are keyboard-
        // reachable and appear in the shortcuts dialog / command palette.
        {
            let mk = |name: &str, f: Box<dyn Fn()>| {
                let a = gtk4::gio::SimpleAction::new(name, None);
                a.connect_activate(move |_, _| f());
                app.add_action(&a);
            };
            mk("cycle-number-format", toggle_format);
            mk("cycle-cell-border", toggle_border);
            mk("merge-cells", toggle_merge);
            mk("insert-chart", show_chart_dialog);
            {
                let ctl = controller.clone();
                let da = drawing_area.clone();
                let wr = win_ref.clone();
                let act = gtk4::gio::SimpleAction::new("conditional-format", None);
                act.connect_activate(move |_, _| {
                    show_conditional_format_dialog(&ctl, &da, wr.borrow().as_ref());
                });
                app.add_action(&act);
            }
            mk("export-pdf", export_pdf);
        }

        suite_common::actions::register_labels(&[
            ("app.cycle-number-format", "Cycle Number Format"),
            ("app.cycle-cell-border", "Cycle Cell Border"),
            ("app.merge-cells", "Merge Cells"),
            ("app.insert-chart", "Insert Chart…"),
            ("app.conditional-format", "Conditional Formatting…"),
            ("app.export-pdf", "Export as PDF…"),
            ("app.open-file", "Open Spreadsheet…"),
            ("app.save-file", "Save"),
            ("app.save-file-as", "Save as Excel Workbook…"),
            ("app.new-document", "New Spreadsheet"),
            ("app.undo", "Undo"),
            ("app.redo", "Redo"),
            ("app.goto-cell", "Go to Cell…"),
        ]);

        let extended_toolbar: Vec<suite_common::ToolbarItem> = vec![
            ("preferences-other-symbolic", "Toggle number format", "app.cycle-number-format"),
            ("format-text-strikethrough-symbolic", "Toggle cell border", "app.cycle-cell-border"),
            ("object-group-symbolic", "Merge cells", "app.merge-cells"),
            ("insert-object-symbolic", "Chart", "app.insert-chart"),
            ("document-send-symbolic", "Export PDF", "app.export-pdf"),
        ];

        let suite_win = suite_common::SuiteWindow::new(app, "Tables", vec![], extended_toolbar);
        *win_ref.borrow_mut() = Some(suite_win.window.clone());

        suite_win.add_top_bar(&fx_bar);
        suite_win.set_content(&stack);
        suite_win.add_bottom_bar(&sheet_bar);

        // ── Window close-request: Save / Discard / Cancel guard ──────────
        // Mirrors Letters' force_close re-entrancy pattern: close-request
        // can't await a dialog response, so the handler stops the first
        // close, and the dialog's own response re-invokes .close() with the
        // guard set so this handler lets it through the second time.
        {
            let ctl = controller.clone();
            let s = state.clone();
            let path_state = current_path.clone();
            let force_close = Rc::new(Cell::new(false));
            suite_win.window.connect_close_request(move |win| {
                if force_close.get() {
                    return gtk4::glib::Propagation::Proceed;
                }
                if !ctl.borrow().is_dirty() {
                    return gtk4::glib::Propagation::Proceed;
                }
                let dialog = adw::AlertDialog::builder()
                    .heading("Save changes?")
                    .body("This workbook has unsaved changes. If you close without saving, they will be lost.")
                    .build();
                dialog.add_responses(&[
                    ("cancel", "_Cancel"),
                    ("discard", "_Discard"),
                    ("save", "_Save"),
                ]);
                dialog.set_close_response("cancel");
                dialog.set_default_response(Some("save"));
                dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
                dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);

                let win_weak = win.downgrade();
                let force_close = force_close.clone();
                let ctl = ctl.clone();
                let s = s.clone();
                let path_state = path_state.clone();
                dialog.choose(Some(win), None::<&gio::Cancellable>, move |response: glib::GString| {
                    let Some(win) = win_weak.upgrade() else { return };
                    if response == "discard" {
                        force_close.set(true);
                        win.close();
                        return;
                    }
                    if response != "save" {
                        return;
                    }
                    let existing_path = path_state.borrow().clone();
                    if let Some(path) = existing_path {
                        match save_engine_to_xlsx(&path.to_string_lossy(), &s.borrow()) {
                            Ok(()) => {
                                ctl.borrow_mut().mark_clean();
                                force_close.set(true);
                                win.close();
                            }
                            Err(e) => {
                                let err = adw::AlertDialog::builder()
                                    .heading("Error saving file")
                                    .body(&e)
                                    .build();
                                err.add_response("ok", "OK");
                                err.present(Some(&win));
                            }
                        }
                        return;
                    }
                    // Never saved: prompt for a destination, then close only
                    // once that save actually succeeds.
                    let dlg = gtk4::FileDialog::new();
                    let f = gtk4::FileFilter::new();
                    f.add_suffix("xlsx");
                    f.set_name(Some("Excel Workbook (.xlsx)"));
                    let fl = gio::ListStore::new::<gtk4::FileFilter>();
                    fl.append(&f);
                    dlg.set_filters(Some(&fl));
                    dlg.set_initial_name(Some("Untitled.xlsx"));
                    let win2 = win.clone();
                    dlg.save(Some(&win), None::<&gio::Cancellable>, move |result| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                match save_engine_to_xlsx(&path_str, &s.borrow()) {
                                    Ok(()) => {
                                        *path_state.borrow_mut() = Some(path);
                                        ctl.borrow_mut().mark_clean();
                                        force_close.set(true);
                                        win2.close();
                                    }
                                    Err(e) => {
                                        let err = adw::AlertDialog::builder()
                                            .heading("Error saving file")
                                            .body(&e)
                                            .build();
                                        err.add_response("ok", "OK");
                                        err.present(Some(&win2));
                                    }
                                }
                            }
                        }
                    });
                });
                gtk4::glib::Propagation::Stop
            });
        }

        // ── App actions ─────────────────────────────────────────────────
        let st = stack.clone();
        let fx = fx_entry.clone();
        let act = gtk4::gio::SimpleAction::new("new-document", None);
        act.connect_activate(move |_, _| {
            st.set_visible_child_name("editor");
            // Focus the formula entry so typing works immediately (and so
            // AT-SPI tests can drive the grid deterministically).
            fx.grab_focus();
        });
        app.add_action(&act);

        // File Open action
        {
            let s = state.clone();
            let st = stack.clone();
            let w = suite_win.window.clone();
            let fx = fx_entry.clone();
            let da = drawing_area.clone();
            let sm = sheet_model.clone();
            let sd = sheet_switcher.clone();
            let path_state = current_path.clone();
            let act = gtk4::gio::SimpleAction::new("open-file", None);
            act.connect_activate(move |_, _| {
                let dlg = gtk4::FileDialog::new();
                let f = gtk4::FileFilter::new();
                f.add_pattern("*.xlsx"); f.add_pattern("*.xls");
                f.add_pattern("*.ods"); f.add_pattern("*.csv");
                f.set_name(Some("Spreadsheets"));
                let fl = gio::ListStore::new::<gtk4::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));
                let s = s.clone(); let st = st.clone();
                let w2 = w.clone(); let fx = fx.clone();
                let da = da.clone(); let sm = sm.clone(); let sd = sd.clone();
                let path_state = path_state.clone();
                dlg.open(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                match load_file_into_engine(&path_str, &mut s.borrow_mut().engine) {
                                    Ok((rows, cols)) => {
                                        // Scoped so the RefMut guard drops before
                                        // set_selected() below: GtkDropDown fires
                                        // selected-notify synchronously, and that
                                        // handler also borrows this same state.
                                        {
                                            let mut ss = s.borrow_mut();
                                            // Replace with loaded data
                                            let sheet_id = ss.engine.sheet_id_at(0).unwrap_or(0);
                                            let mut sheet = SheetModel::new(
                                                "Sheet1", rows.max(DEFAULT_ROWS),
                                                cols.max(DEFAULT_COLS), sheet_id);
                                            sheet.sync_from_engine(&ss.engine);
                                            sheet.charts =
                                                tables_core::io::read_charts_from_xlsx(&path_str);
                                            sheet.cond_rules =
                                                tables_core::io::read_cond_rules_from_xlsx(&path_str);
                                            ss.sheets.clear();
                                            ss.sheets.push(Rc::new(RefCell::new(sheet)));
                                            ss.active_sheet = 0;
                                        }
                                        // Update sheet switcher
                                        sm.splice(0, sm.n_items(), &[]);
                                        sm.append("Sheet1");
                                        sd.set_selected(0);
                                        fx.set_text("");
                                        st.set_visible_child_name("editor");
                                        let name = std::path::Path::new(&path_str)
                                            .file_name()
                                            .map(|n| n.to_string_lossy().to_string())
                                            .unwrap_or_default();
                                        w2.set_title(Some(&format!("{name} — Tables")));
                                        *path_state.borrow_mut() = Some(path.clone());
                                        da.queue_draw();
                                    }
                                    Err(e) => {
                                        let err = adw::AlertDialog::builder()
                                            .heading(&suite_common::i18n("Error opening file"))
                                            .body(&e)
                                            .build();
                                        err.add_response("ok", "OK");
                                        err.set_default_response(Some("ok"));
                                        err.present(Some(&w2));
                                    }
                                }
                            }
                        }
                    },
                );
            });
            app.add_action(&act);
        }

        // File Save action
        {
            let s = state.clone();
            let ctl = controller.clone();
            let w = suite_win.window.clone();
            let path_state = current_path.clone();
            // no toast for now
            let act = gtk4::gio::SimpleAction::new("save-file-as", None);
            act.connect_activate(move |_, _| {
                // no toast
                let dlg = gtk4::FileDialog::new();
                let f = gtk4::FileFilter::new();
                f.add_suffix("xlsx");
                f.set_name(Some("Excel Workbook (.xlsx)"));
                let fl = gio::ListStore::new::<gtk4::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));
                dlg.set_initial_name(Some("Untitled.xlsx"));
                let s = s.clone(); let ctl = ctl.clone();
                let w2 = w.clone(); let path_state = path_state.clone();
                dlg.save(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                let ss = s.borrow();
                                match save_engine_to_xlsx(&path_str, &ss) {
                                    Ok(()) => {
                                        drop(ss);
                                        *path_state.borrow_mut() = Some(path);
                                        ctl.borrow_mut().mark_clean();
                                    }
                                    Err(e) => {
                                        let err = adw::AlertDialog::builder()
                                            .heading("Error saving file")
                                            .body(&e)
                                            .build();
                                        err.add_response("ok", "OK");
                                        err.set_default_response(Some("ok"));
                                        err.present(Some(&w2));
                                    }
                                }
                            }
                        }
                    },
                );
            });
            app.add_action(&act);
        }
        {
            let s = state.clone();
            let ctl = controller.clone();
            let w = suite_win.window.clone();
            let path_state = current_path.clone();
            let app_for_save = app.clone();
            let act = gtk4::gio::SimpleAction::new("save-file", None);
            act.connect_activate(move |_, _| {
                let Some(path) = path_state.borrow().clone() else {
                    app_for_save.activate_action("save-file-as", None);
                    return;
                };
                match save_engine_to_xlsx(&path.to_string_lossy(), &s.borrow()) {
                    Ok(()) => ctl.borrow_mut().mark_clean(),
                    Err(e) => {
                        let err = adw::AlertDialog::builder()
                            .heading("Error saving file")
                            .body(&e)
                            .build();
                        err.add_response("ok", "OK");
                        err.set_default_response(Some("ok"));
                        err.present(Some(&w));
                    }
                }
            });
            app.add_action(&act);
        }

        // ── Undo/redo as named actions (window-wide accels) ────────────
        {
            let ctl = controller.clone();
            let da = drawing_area.clone();
            let undo_action = gtk4::gio::SimpleAction::new("undo", None);
            undo_action.set_enabled(false);
            undo_action.connect_activate(move |_, _| {
                if ctl.borrow_mut().undo() {
                    da.queue_draw();
                }
            });
            app.add_action(&undo_action);
            let ctl = controller.clone();
            let da = drawing_area.clone();
            let redo_action = gtk4::gio::SimpleAction::new("redo", None);
            redo_action.set_enabled(false);
            redo_action.connect_activate(move |_, _| {
                if ctl.borrow_mut().redo() {
                    da.queue_draw();
                }
            });
            app.add_action(&redo_action);
            controller.borrow().listen_history(Rc::new(HistoryActionListener {
                undo: undo_action,
                redo: redo_action,
            }));
            app.set_accels_for_action("app.undo", &["<Primary>z"]);
            app.set_accels_for_action("app.redo", &["<Primary>y", "<Primary><Shift>z"]);
        }

        // ── Grid keyboard: arrows move, Shift+arrows extend, Delete clears ─
        drawing_area.set_focusable(true);
        {
            let s = state.clone();
            let ctl = controller.clone();
            let da = drawing_area.clone();
            let fx = fx_entry.clone();
            let refresh = refresh_sel.clone();
            let key = gtk4::EventControllerKey::new();
            key.connect_key_pressed(move |_, keyval, _code, mods| {
                use gtk4::gdk::Key;
                let shift = mods.contains(gtk4::gdk::ModifierType::SHIFT_MASK);
                let ctrl = mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
                if ctrl && keyval == Key::c {
                    let st = s.borrow();
                    let sh = st.sheet();
                    let (r0, c0, r1, c1) = sh.selection_rect();
                    let frag =
                        tables_core::fragment::copy_range(&sh, &st.engine, r0, c0, r1, c1);
                    let provider = suite_common::clipboard::provider(
                        tables_core::fragment::MIME,
                        &frag.to_json(),
                        &frag.to_html(),
                        &frag.to_plain(),
                    );
                    let _ = da.clipboard().set_content(Some(&provider));
                    return gtk4::glib::Propagation::Stop;
                }
                if ctrl && keyval == Key::v {
                    let clipboard = da.clipboard();
                    let ctl2 = ctl.clone();
                    let da2 = da.clone();
                    let refresh2 = refresh.clone();
                    let apply = move |frag: tables_core::fragment::Fragment| {
                        let (row, col) = {
                            let controller = ctl2.borrow();
                            let st = controller.state.borrow();
                            let sh = st.sheet();
                            (sh.selected_row, sh.selected_col)
                        };
                        ctl2.borrow_mut().paste_fragment(row, col, &frag);
                        refresh2();
                        da2.queue_draw();
                    };
                    if suite_common::clipboard::offers(&clipboard, tables_core::fragment::MIME) {
                        suite_common::clipboard::read_string(
                            &clipboard,
                            tables_core::fragment::MIME,
                            move |json| {
                                if let Some(frag) =
                                    json.as_deref().and_then(tables_core::fragment::Fragment::from_json)
                                {
                                    apply(frag);
                                }
                            },
                        );
                    } else {
                        clipboard.read_text_async(gtk4::gio::Cancellable::NONE, move |res| {
                            if let Ok(Some(text)) = res {
                                // Plain text pastes as a TSV grid.
                                let rows: Vec<Vec<tables_core::fragment::GridCell>> = text
                                    .lines()
                                    .map(|l| {
                                        l.split('\t')
                                            .map(|v| tables_core::fragment::GridCell {
                                                value: v.to_string(),
                                                ..Default::default()
                                            })
                                            .collect()
                                    })
                                    .collect();
                                if !rows.is_empty() {
                                    apply(tables_core::fragment::Fragment::Grid(rows));
                                }
                            }
                        });
                    }
                    return gtk4::glib::Propagation::Stop;
                }
                let delta = match keyval {
                    Key::Up => Some((-1i64, 0i64)),
                    Key::Down => Some((1, 0)),
                    Key::Left => Some((0, -1)),
                    Key::Right => Some((0, 1)),
                    _ => None,
                };
                if let Some((dr, dc)) = delta {
                    {
                        let st = s.borrow();
                        let mut sh = st.sheet_mut();
                        // Shift extends from the range's moving end; a plain
                        // arrow moves the anchor cell.
                        let (br, bc) = if shift {
                            (sh.sel_end_row, sh.sel_end_col)
                        } else {
                            (sh.selected_row, sh.selected_col)
                        };
                        let nr = (br as i64 + dr).clamp(0, sh.rows as i64 - 1) as usize;
                        let nc = (bc as i64 + dc).clamp(0, sh.cols as i64 - 1) as usize;
                        if shift {
                            sh.extend_selection(nr, nc);
                        } else {
                            sh.select_cell(nr, nc);
                            let val = sh.data[nr][nc].clone();
                            fx.set_text(&val);
                        }
                    }
                    {
                        let st = s.borrow();
                        let sh = st.sheet();
                        let (r, c) = (sh.selected_row, sh.selected_col);
                        let shown = sh.cell(r, c).to_string();
                        drop(sh);
                        update_grid_a11y(&da, &tables_core::sheet::col_label(c), r, &shown);
                    }
                    refresh();
                    da.queue_draw();
                    return gtk4::glib::Propagation::Stop;
                }
                if keyval == Key::Delete || keyval == Key::BackSpace {
                    let (r, c) = {
                        let st = s.borrow();
                        let sh = st.sheet();
                        (sh.selected_row, sh.selected_col)
                    };
                    ctl.borrow_mut().edit_cell(r, c, "");
                    da.queue_draw();
                    return gtk4::glib::Propagation::Stop;
                }
                // Type-to-edit: any other printable keystroke on the focused
                // grid starts editing the selected cell, spreadsheet-style
                // (Excel/Calc/Sheets), instead of requiring a click into the
                // formula bar first.
                if !ctrl && !mods.contains(gtk4::gdk::ModifierType::ALT_MASK) {
                    if let Some(ch) = keyval.to_unicode() {
                        if !ch.is_control() {
                            fx.set_text(&ch.to_string());
                            fx.grab_focus();
                            fx.set_position(-1);
                            return gtk4::glib::Propagation::Stop;
                        }
                    }
                }
                if keyval == Key::F2 || keyval == Key::Return || keyval == Key::KP_Enter {
                    let current = {
                        let st = s.borrow();
                        let (r, c) = (st.sheet().selected_row, st.sheet().selected_col);
                        st.cell_input(r, c)
                    };
                    fx.set_text(&current);
                    fx.grab_focus();
                    fx.set_position(-1);
                    return gtk4::glib::Propagation::Stop;
                }
                gtk4::glib::Propagation::Proceed
            });
            drawing_area.add_controller(key);
        }

        Self {
            window: suite_win.window,
            drawing_area,
            h_adj,
            v_adj,
            fx_entry,
            stack,
            controller,
            state,
            sheet_model,
            sheet_switcher,
            current_path,
        }
    }

    pub fn present(&self) { self.window.present(); }

    /// Open a spreadsheet file directly (CLI / file-manager open).
    /// Mirrors the open-file-dialog success path.
    pub fn open_path(&self, path: &str) -> Result<(), String> {
        if std::path::Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsx"))
        {
            let (engine, sheets) = load_xlsx_workbook(path)?;
            let names = sheets
                .iter()
                .map(|sheet| sheet.name.clone())
                .collect::<Vec<_>>();
            {
                let mut state = self.state.borrow_mut();
                state.engine = engine;
                state.sheets = sheets
                    .into_iter()
                    .map(|sheet| Rc::new(RefCell::new(sheet)))
                    .collect();
                state.active_sheet = 0;
            }
            self.sheet_model.splice(0, self.sheet_model.n_items(), &[]);
            for name in names {
                self.sheet_model.append(&name);
            }
            self.sheet_switcher.set_selected(0);
            self.fx_entry.set_text("");
            self.stack.set_visible_child_name("editor");
            let name = std::path::Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            self.window.set_title(Some(&format!("{name} — Tables")));
            *self.current_path.borrow_mut() = Some(std::path::PathBuf::from(path));
            self.drawing_area.queue_draw();
            return Ok(());
        }
        let (rows, cols) = load_file_into_engine(path, &mut self.state.borrow_mut().engine)?;
        {
            let mut ss = self.state.borrow_mut();
            let sheet_id = ss.engine.sheet_id_at(0).unwrap_or(0);
            let mut sheet =
                SheetModel::new("Sheet1", rows.max(DEFAULT_ROWS), cols.max(DEFAULT_COLS), sheet_id);
            sheet.sync_from_engine(&ss.engine);
            sheet.charts = tables_core::io::read_charts_from_xlsx(path);
            sheet.cond_rules = tables_core::io::read_cond_rules_from_xlsx(path);
            ss.sheets.clear();
            ss.sheets.push(Rc::new(RefCell::new(sheet)));
            ss.active_sheet = 0;
        }
        self.sheet_model.splice(0, self.sheet_model.n_items(), &[]);
        self.sheet_model.append("Sheet1");
        self.sheet_switcher.set_selected(0);
        self.fx_entry.set_text("");
        self.stack.set_visible_child_name("editor");
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        self.window.set_title(Some(&format!("{name} — Tables")));
        *self.current_path.borrow_mut() = Some(std::path::PathBuf::from(path));
        self.drawing_area.queue_draw();
        Ok(())
    }
}

// ── Coordinate conversion ─────────────────────────────────────────────


/// Format Cells dialog: number-format kind + decimals + currency
/// symbol, applied to the whole selection.
fn show_format_cells_dialog(
    controller: &Rc<RefCell<WorkbookController>>,
    da: &gtk4::DrawingArea,
    refresh: &Rc<dyn Fn()>,
    parent: Option<adw::ApplicationWindow>,
) {
    let kinds = ["General", "Number", "Currency", "Percent", "Date", "Scientific"];
    let dropdown = gtk4::DropDown::from_strings(&kinds);
    dropdown.update_property(&[gtk4::accessible::Property::Label("Format kind")]);
    // Preselect from the active cell's current format.
    {
        let state = controller.borrow().state.clone();
        let st = state.borrow();
        let sh = st.sheet();
        let idx = match sh.formats[sh.selected_row][sh.selected_col].kind {
            NumberFormatKind::General | NumberFormatKind::Text => 0,
            NumberFormatKind::Number(_) => 1,
            NumberFormatKind::Currency(_, _) => 2,
            NumberFormatKind::Percent(_) => 3,
            NumberFormatKind::Date(_) | NumberFormatKind::DateTime(_) => 4,
            NumberFormatKind::Scientific(_) => 5,
        };
        dropdown.set_selected(idx);
    }

    let decimals = gtk4::SpinButton::with_range(0.0, 6.0, 1.0);
    decimals.set_value(2.0);
    decimals.update_property(&[gtk4::accessible::Property::Label("Decimal places")]);
    let symbol = gtk4::Entry::new();
    symbol.set_text("$");
    symbol.set_max_width_chars(4);
    symbol.update_property(&[gtk4::accessible::Property::Label("Currency symbol")]);

    let grid = gtk4::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(12);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    let mut row = 0;
    for (label, widget) in [
        ("Format", dropdown.clone().upcast::<gtk4::Widget>()),
        ("Decimals", decimals.clone().upcast()),
        ("Symbol", symbol.clone().upcast()),
    ] {
        let l = gtk4::Label::new(Some(label));
        l.add_css_class("dim-label");
        l.set_halign(gtk4::Align::Start);
        grid.attach(&l, 0, row, 1, 1);
        grid.attach(&widget, 1, row, 1, 1);
        row += 1;
    }
    let apply = gtk4::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    grid.attach(&apply, 1, row, 1, 1);

    let dialog = adw::Dialog::builder()
        .title("Format Cells")
        .content_width(320)
        .build();
    dialog.set_child(Some(&grid));

    {
        let ctl = controller.clone();
        let da = da.clone();
        let refresh = refresh.clone();
        let dialog = dialog.clone();
        let dropdown = dropdown.clone();
        apply.connect_clicked(move |_| {
            let dp = decimals.value() as u8;
            let sym = symbol.text().to_string();
            let kind = match dropdown.selected() {
                1 => NumberFormatKind::Number(dp),
                2 => NumberFormatKind::Currency(sym, dp),
                3 => NumberFormatKind::Percent(dp),
                4 => NumberFormatKind::Date("%Y-%m-%d".into()),
                5 => NumberFormatKind::Scientific(dp),
                _ => NumberFormatKind::General,
            };
            ctl.borrow_mut().mutate_sheet("Format Cells", move |sh| {
                let (r0, c0, r1, c1) = sh.selection_rect();
                for r in r0..=r1 {
                    for c in c0..=c1 {
                        sh.formats[r][c] = NumberFormat::new(kind.clone());
                    }
                }
            });
            refresh();
            da.queue_draw();
            dialog.close();
        });
    }
    dialog.present(parent.as_ref());
}

/// Keep the grid's accessible description in sync with the active cell so
/// assistive tech and AT-SPI tests can read grid state.
fn update_grid_a11y(da: &gtk4::DrawingArea, col: &str, row: usize, value: &str) {
    let desc = if value.is_empty() {
        format!("cell {}{}, empty", col, row + 1)
    } else {
        format!("cell {}{}: {}", col, row + 1, value)
    };
    da.update_property(&[gtk4::accessible::Property::Description(&desc)]);
}

/// Recompute the grid's a11y description from the currently active sheet's
/// selection. Sheet switches change which cell "A1" refers to without
/// necessarily moving the on-screen selection, so anything that changes the
/// active sheet (add, switch, delete, reorder) must call this — otherwise
/// screen readers keep announcing the previous sheet's stale content.
fn refresh_grid_a11y(da: &gtk4::DrawingArea, state: &Rc<RefCell<WorkbookState>>) {
    let st = state.borrow();
    let sh = st.sheet();
    let (r, c) = (sh.selected_row, sh.selected_col);
    let shown = sh.cell(r, c).to_string();
    drop(sh);
    update_grid_a11y(da, &tables_core::sheet::col_label(c), r, &shown);
}


/// Conditional Formatting dialog: operator + threshold(s) + fill color,
/// applied to the current selection (ADR 0003 §4 — cell-value rules).
fn show_conditional_format_dialog(
    controller: &Rc<RefCell<WorkbookController>>,
    da: &gtk4::DrawingArea,
    parent: Option<&adw::ApplicationWindow>,
) {
    use tables_core::sheet::{CondOp, CondRule};
    let dialog = adw::Dialog::builder()
        .title(&suite_common::i18n("Conditional Formatting"))
        .content_width(360)
        .build();

    let op_combo = gtk::DropDown::from_strings(&["Greater than", "Less than", "Equal to", "Between"]);
    let value_entry = gtk::Entry::builder().placeholder_text("Value").build();
    let value2_entry = gtk::Entry::builder().placeholder_text("Upper bound").build();
    value2_entry.set_sensitive(false);
    {
        let v2 = value2_entry.clone();
        op_combo.connect_selected_notify(move |dd| v2.set_sensitive(dd.selected() == 3));
    }
    let color_btn = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    color_btn.set_rgba(&gtk4::gdk::RGBA::new(1.0, 0.75, 0.75, 1.0));

    let grid = gtk4::Grid::new();
    grid.set_row_spacing(8);
    grid.set_column_spacing(12);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    let lbl = |t: &str| {
        let l = gtk::Label::new(Some(t));
        l.set_halign(gtk::Align::Start);
        l
    };
    grid.attach(&lbl("Condition"), 0, 0, 1, 1);
    grid.attach(&op_combo, 1, 0, 1, 1);
    grid.attach(&lbl("Value"), 0, 1, 1, 1);
    grid.attach(&value_entry, 1, 1, 1, 1);
    grid.attach(&lbl("And"), 0, 2, 1, 1);
    grid.attach(&value2_entry, 1, 2, 1, 1);
    grid.attach(&lbl("Fill"), 0, 3, 1, 1);
    grid.attach(&color_btn, 1, 3, 1, 1);

    let apply = gtk::Button::with_label(&suite_common::i18n("Apply to Selection"));
    apply.add_css_class("suggested-action");
    grid.attach(&apply, 1, 4, 1, 1);

    {
        let ctl = controller.clone();
        let da = da.clone();
        let dlg = dialog.clone();
        let op_combo = op_combo.clone();
        let value_entry = value_entry.clone();
        let value2_entry = value2_entry.clone();
        let color_btn = color_btn.clone();
        apply.connect_clicked(move |_| {
            let Ok(value) = value_entry.text().trim().parse::<f64>() else { return };
            let value2 = value2_entry.text().trim().parse::<f64>().unwrap_or(value);
            let op = match op_combo.selected() {
                0 => CondOp::Greater,
                1 => CondOp::Less,
                2 => CondOp::Equal,
                _ => CondOp::Between,
            };
            let rgba = color_btn.rgba();
            let fill = format!(
                "{:02X}{:02X}{:02X}",
                (rgba.red() * 255.0) as u8,
                (rgba.green() * 255.0) as u8,
                (rgba.blue() * 255.0) as u8
            );
            ctl.borrow_mut().mutate_sheet("Conditional Formatting", move |sheet| {
                let (r0, c0, r1, c1) = sheet.selection_rect();
                sheet.cond_rules.push(CondRule {
                    range: (r0, c0, r1, c1),
                    op,
                    value,
                    value2,
                    fill,
                });
            });
            da.queue_draw();
            dlg.close();
        });
    }

    dialog.set_child(Some(&grid));
    dialog.present(parent);
}
