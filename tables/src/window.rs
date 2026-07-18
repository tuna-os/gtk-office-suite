// SPDX-License-Identifier: GPL-3.0-or-later
//
// TablesWindow — Spreadsheet window with Cairo grid, formula bar, sheet tabs, and file I/O.
// GNOME GUI spec: AdwApplicationWindow + AdwToolbarView + AdwBreakpoint.

use gtk4::cairo::{self, Context};
use gtk4::{self as gtk, gio, glib, prelude::*};
use libadwaita as adw;
use adw::prelude::{AdwDialogExt, AlertDialogExt};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use tables_core::engine::TablesEngine;
use tables_core::sheet::*;
use crate::grid_render::{draw_grid, auto_fit_column};
use suite_common::format::{NumberFormat, NumberFormatKind};
use suite_common::undo::UndoManager;
use tables_core::undo::SheetState;

// ── Constants ──────────────────────────────────────────────────────────
// ── Shared state ─────────────────────────────────────────────────────
pub struct AppState {
    pub sheets: Vec<Rc<RefCell<SheetModel>>>,
    pub active_sheet: usize,
    pub engine: TablesEngine,
}

impl AppState {
    fn sheet(&self) -> std::cell::Ref<SheetModel> {
        self.sheets[self.active_sheet].borrow()
    }

    fn sheet_mut(&self) -> std::cell::RefMut<SheetModel> {
        self.sheets[self.active_sheet].borrow_mut()
    }

    fn grid_data(&self) -> Vec<Vec<String>> {
        let mut grid = Vec::new();
        let s = self.sheet();
        for r in 0..s.rows {
            let mut row = Vec::new();
            for c in 0..s.cols {
                row.push(s.data[r][c].clone());
            }
            grid.push(row);
        }
        grid
    }
}

// File I/O lives in tables_core::io; window code only adapts AppState.
use tables_core::io::load_file_into_engine;

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
    undo: Rc<RefCell<UndoManager<SheetState>>>,
}

impl TablesWindow {
    pub fn new(app: &adw::Application) -> Self {
        let engine = TablesEngine::new(DEFAULT_ROWS, DEFAULT_COLS)
            .expect("Failed to create spreadsheet engine");

        let sheet = SheetModel::new("Sheet1", DEFAULT_ROWS, DEFAULT_COLS, 0);
        let sheet_clone = sheet.clone();
        let state = Rc::new(RefCell::new(AppState {
            sheets: vec![Rc::new(RefCell::new(sheet))],
            active_sheet: 0,
            engine,
        }));

        // ── Undo manager ─────────────────────────────────────────────
        let undo_state = Rc::new(RefCell::new(SheetState {
            sheets: vec![sheet_clone],
            active_sheet: 0,
        }));
        let undo_mgr: Rc<RefCell<UndoManager<SheetState>>> = Rc::new(RefCell::new(UndoManager::new(undo_state)));

        // ── Scrolling ──────────────────────────────────────────────────
        let h_adj = gtk4::Adjustment::new(0.0, 0.0, 5000.0, 10.0, 50.0, 500.0);
        let v_adj = gtk4::Adjustment::new(0.0, 0.0, 5000.0, 10.0, 50.0, 500.0);

        // ── Drawing area ────────────────────────────────────────────────
        let drawing_area = gtk4::DrawingArea::new();
        drawing_area.set_vexpand(true);
        drawing_area.set_hexpand(true);
        // A11y: name the grid and keep its description tracking the active
        // cell, so screen readers announce state and AT-SPI tests can
        // assert it (the DrawingArea is otherwise opaque — issue #87).
        drawing_area.set_accessible_role(gtk4::AccessibleRole::Img);
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

        // ── Formula bar ─────────────────────────────────────────────────
        let fx_label = gtk4::Label::new(Some(" fx"));
        fx_label.add_css_class("dim-label");
        fx_label.set_width_chars(5);
        let fx_entry = gtk4::Entry::new();
        fx_entry.set_hexpand(true);
        fx_entry.set_placeholder_text(Some("Formula or value\u{2026}"));
        fx_entry.update_property(&[gtk4::accessible::Property::Label("Formula input")]);

        let fx_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        fx_bar.set_margin_start(6); fx_bar.set_margin_end(6);
        fx_bar.set_margin_top(2); fx_bar.set_margin_bottom(2);
        fx_bar.set_halign(gtk4::Align::Fill);
        fx_bar.append(&fx_label);
        fx_bar.append(&fx_entry);

        // Wire formula bar: Enter commits
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let fx = fx_entry.clone();
            fx_entry.connect_activate(move |_| {
                let val = fx.text().to_string();
                let mut st = s.borrow_mut();
                let r = st.sheet().selected_row;
                let c = st.sheet().selected_col;
                // Validate
                let sh = st.sheet();
                if let Some(rule) = &sh.validations[r][c] {
                    if !rule.validate(&val) {
                        let _toast = adw::Toast::new("Invalid input — value rejected");
                        _toast.set_timeout(3);
                        return;
                    }
                }
                drop(sh);
                st.engine.set_cell_text(r, c, &val);
                {
                    let mut sh = st.sheets[st.active_sheet].borrow_mut();
                    sh.data[r][c] = val.clone();
                    sh.formulas[r][c] = val.starts_with('=');
                }
                st.sheet_mut().sync_from_engine(&st.engine);
                let shown = st.engine.cell(r, c);
                update_grid_a11y(&da, &tables_core::sheet::col_label(c), r, &shown);
                da.queue_draw();
            });
        }

        // ── Mouse: single-click select ──────────────────────────────────
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let fx = fx_entry.clone();
            let h = h_adj.clone();
            let v = v_adj.clone();
            let click = gtk4::GestureClick::new();
            click.connect_pressed(move |_g, _n, x, y| {
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
                        let mut st = s.borrow_mut();
                        st.sheet_mut().toggle_sort(col);
                        da.queue_draw();
                        return;
                    }
                }
                if let Some((col, row)) = xy_to_cell(wx, wy, h.value(), &*sh) {
                    drop(sh); drop(st);
                    let mut st = s.borrow_mut();
                    {
                        let shown = st.engine.cell(row, col);
                        update_grid_a11y(&da, &tables_core::sheet::col_label(col), row, &shown);
                    }
                    let mut sh = st.sheet_mut();
                    sh.selected_row = row;
                    sh.selected_col = col;
                    let val = sh.data[row][col].clone();
                    if sh.is_formula(row, col) {
                        fx.set_text(&val);
                    } else {
                        fx.set_text(&val);
                    }
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
            let drag = gtk4::GestureDrag::new();
            drag.set_button(1);
            let dc2 = drag_col.clone();
            let dc3 = drag_col.clone();
            let s2 = s.clone();
            let h2 = h.clone();
            drag.connect_drag_begin(move |_g, x, y| {
                let st = s.borrow();
                let sh = st.sheet();
                if let Some(col) = hit_col_divider(x as f64, y as f64, h.value(), &*sh) {
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
                dc3.set(None);
            });
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

                    let s2 = s.clone();
                    let da2 = da.clone();
                    entry.connect_activate(move |e| {
                        let new_val = e.text().to_string();
                        let mut st = s2.borrow_mut();
                        let r = row; let c = col;
                        st.engine.set_cell_text(r, c, &new_val);
                        {
                            let mut sh = st.sheet_mut();
                            sh.data[r][c] = new_val.clone();
                            sh.formulas[r][c] = new_val.starts_with('=');
                        }
                        st.sheet_mut().sync_from_engine(&st.engine);
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

        let add_btn = gtk4::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add sheet")
            .build();
        add_btn.set_css_classes(&["flat", "circular"]);
        sheet_bar.append(&add_btn);

        // Add sheet action
        {
            let s = state.clone();
            let sm = sheet_model.clone();
            let sd = sheet_switcher.clone();
            let da = drawing_area.clone();
            add_btn.connect_clicked(move |_| {
                let mut st = s.borrow_mut();
                let idx = st.sheets.len();
                let name = format!("Sheet{}", idx + 1);
                let sheet = SheetModel::new(&name, DEFAULT_ROWS, DEFAULT_COLS, idx);
                let rc = Rc::new(RefCell::new(sheet));
                st.sheets.push(rc);
                st.active_sheet = idx;
                sm.append(&name);
                sd.set_selected(idx as u32);
                da.queue_draw();
            });
        }

        // Switch sheet
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let fx = fx_entry.clone();
            sheet_switcher.connect_selected_notify(move |dd| {
                let idx = dd.selected() as usize;
                let mut st = s.borrow_mut();
                if idx < st.sheets.len() {
                    st.active_sheet = idx;
                    // Also sync engine state
                    st.sheet_mut().sync_from_engine(&st.engine);
                    fx.set_text("");
                    da.queue_draw();
                }
            });
        }

        // ── Content stack ───────────────────────────────────────────────
        let scroll_grid = gtk4::Grid::new();
        scroll_grid.set_vexpand(true);
        scroll_grid.set_hexpand(true);
        let h_scroll = gtk4::Scrollbar::new(gtk4::Orientation::Horizontal, Some(&h_adj));
        let v_scroll = gtk4::Scrollbar::new(gtk4::Orientation::Vertical, Some(&v_adj));
        scroll_grid.attach(&drawing_area, 0, 0, 1, 1);
        scroll_grid.attach(&v_scroll, 1, 0, 1, 1);
        scroll_grid.attach(&h_scroll, 0, 1, 1, 1);

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
                    .title("Chart")
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

                let box_content = gtk::Box::new(gtk::Orientation::Vertical, 6);
                box_content.append(&header);
                box_content.append(&preview);
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
            let s = state.clone();
            let da = drawing_area.clone();
            Box::new(move || {
                let mut st = s.borrow_mut();
                let mut sh = st.sheet_mut();
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
                da.queue_draw();
            })
        };

        let toggle_border = {
            let s = state.clone();
            let da = drawing_area.clone();
            Box::new(move || {
                let mut st = s.borrow_mut();
                let mut sh = st.sheet_mut();
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
                da.queue_draw();
            })
        };

        let toggle_merge = {
            let s = state.clone();
            let da = drawing_area.clone();
            Box::new(move || {
                let mut st = s.borrow_mut();
                st.sheet_mut().toggle_merge();
                da.queue_draw();
            })
        };

        let extended_toolbar: Vec<(&'static str, &'static str, Box<dyn Fn() + 'static>)> = vec![
            ("preferences-other-symbolic", "Toggle number format", toggle_format),
            ("format-text-strikethrough-symbolic", "Toggle cell border", toggle_border),
            ("object-group-symbolic", "Merge cells", toggle_merge),
            ("insert-object-symbolic", "Chart", show_chart_dialog),
            ("document-send-symbolic", "Export PDF", export_pdf),
        ];

        let suite_win = suite_common::SuiteWindow::new(app, "Tables", vec![], extended_toolbar);
        *win_ref.borrow_mut() = Some(suite_win.window.clone());

        suite_win.add_top_bar(&fx_bar);
        suite_win.set_content(&stack);
        suite_win.add_bottom_bar(&sheet_bar);

        // ── App actions ─────────────────────────────────────────────────
        let st = stack.clone();
        let act = gtk4::gio::SimpleAction::new("open-file", None);
        act.connect_activate(move |_, _| st.set_visible_child_name("editor"));
        app.add_action(&act);

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
            let act = gtk4::gio::SimpleAction::new("open-file-dialog", None);
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
                dlg.open(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                match load_file_into_engine(&path_str, &mut s.borrow_mut().engine) {
                                    Ok((rows, cols)) => {
                                        let mut ss = s.borrow_mut();
                                        // Replace with loaded data
                                        let mut sheet = SheetModel::new(
                                            "Sheet1", rows.max(DEFAULT_ROWS),
                                            cols.max(DEFAULT_COLS), 0);
                                        sheet.sync_from_engine(&ss.engine);
                                        ss.sheets.clear();
                                        ss.sheets.push(Rc::new(RefCell::new(sheet)));
                                        ss.active_sheet = 0;
                                        // Update sheet switcher
                                        sm.splice(0, sm.n_items(), &[]);
                                        sm.append("Sheet1");
                                        sd.set_selected(0);
                                        fx.set_text("");
                                        st.set_visible_child_name("editor");
                                        da.queue_draw();
                                    }
                                    Err(e) => {
                                        let err = adw::AlertDialog::builder()
                                            .heading("Error opening file")
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
            let w = suite_win.window.clone();
            // no toast for now
            let act = gtk4::gio::SimpleAction::new("save-file-dialog", None);
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
                let s = s.clone(); let w2 = w.clone();
                dlg.save(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                let ss = s.borrow();
                                match save_engine_to_xlsx(&path_str, &ss) {
                                    Ok(()) => {}
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

        // ── Keyboard shortcuts ──────────────────────────────────────────
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let u = undo_mgr.clone();
            let key = gtk4::EventControllerKey::new();
            key.connect_key_pressed(move |_, keyval, _code, mods| {
                if mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK) && keyval == gtk4::gdk::Key::z {
                    u.borrow_mut().undo();
                    da.queue_draw();
                    return gtk4::glib::Propagation::Stop;
                }
                if mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK | gtk4::gdk::ModifierType::SHIFT_MASK) && keyval == gtk4::gdk::Key::z {
                    u.borrow_mut().redo();
                    da.queue_draw();
                    return gtk4::glib::Propagation::Stop;
                }
                if keyval == gtk::gdk::Key::Delete || keyval == gtk::gdk::Key::BackSpace {
                    let mut st = s.borrow_mut();
                    let r = st.sheet().selected_row;
                    let c = st.sheet().selected_col;
                    st.engine.set_cell_text(r, c, "");
                    st.sheet_mut().data[r][c] = String::new();
                    st.sheet_mut().formulas[r][c] = false;
                    da.queue_draw();
                }
                gtk4::glib::Propagation::Proceed
            });
            drawing_area.add_controller(key);
        }

        Self { window: suite_win.window, drawing_area, h_adj, v_adj, fx_entry, stack, undo: undo_mgr }
    }

    pub fn present(&self) { self.window.present(); }
}

// ── Coordinate conversion ─────────────────────────────────────────────


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
