use gtk4 as gtk;
use libadwaita::prelude::*;

pub struct TablesWindow {
    window: libadwaita::ApplicationWindow,
}

impl TablesWindow {
    pub fn new(app: &libadwaita::Application) -> Self {
        let win = libadwaita::ApplicationWindow::builder()
            .application(app)
            .build();
        win.set_title(Some("Tables"));
        win.set_default_size(900, 600);

        let header = suite_common::make_header_bar("Tables");
        let toolbar = suite_common::make_toolbar();
        let formula = gtk::Entry::new();
        formula.set_placeholder_text(Some("Formula…"));

        // Initialize spreadsheet engine with default sample data
        let sheet = std::rc::Rc::new(std::cell::RefCell::new(crate::engine::Spreadsheet::new(
            30, 8,
        )));
        {
            let mut s = sheet.borrow_mut();
            s.set(0, 0, "Item");
            s.set(0, 1, "Quantity");
            s.set(0, 2, "Price");
            s.set(0, 3, "Total");
            s.set(1, 0, "Widget A");
            s.set(1, 1, "10");
            s.set(1, 2, "5.5");
            s.set(1, 3, "55.0");
            s.set(2, 0, "Widget B");
            s.set(2, 1, "20");
            s.set(2, 2, "3.0");
            s.set(2, 3, "60.0");
            s.set(3, 0, "Widget C");
            s.set(3, 1, "30");
            s.set(3, 2, "2.0");
            s.set(3, 3, "60.0");
            s.set(4, 0, "Sum Total");
            s.set(4, 1, "=SUM(B2:B4)");
        }

        let selected_cell = std::rc::Rc::new(std::cell::Cell::new((1, 1))); // (row, col)

        // Custom Grid Drawing Area
        let canvas = gtk::DrawingArea::new();
        canvas.set_hexpand(true);
        canvas.set_vexpand(true);

        // Drawing handler
        let sheet_clone = sheet.clone();
        let selected_cell_clone = selected_cell.clone();
        canvas.set_draw_func(move |_area, cr, _w, _h| {
            // Draw background
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().unwrap();

            let col_w = 100.0;
            let row_h = 25.0;
            let header_w = 50.0;
            let header_h = 25.0;

            let s = sheet_clone.borrow();
            let (sel_r, sel_c) = selected_cell_clone.get();

            // Draw grid lines and cell texts
            for r in 0..30 {
                let y = header_h + r as f64 * row_h;

                // Draw row header
                cr.set_source_rgb(0.93, 0.93, 0.93);
                cr.rectangle(0.0, y, header_w, row_h);
                cr.fill().unwrap();

                cr.set_source_rgb(0.8, 0.8, 0.8);
                cr.rectangle(0.0, y, header_w, row_h);
                cr.stroke().unwrap();

                cr.set_source_rgb(0.2, 0.2, 0.2);
                cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
                cr.set_font_size(11.0);
                cr.move_to(15.0, y + 17.0);
                cr.show_text(&format!("{}", r + 1)).unwrap();

                for c in 0..8 {
                    let x = header_w + c as f64 * col_w;

                    // Draw grid cell border
                    cr.set_source_rgb(0.88, 0.88, 0.88);
                    cr.rectangle(x, y, col_w, row_h);
                    cr.stroke().unwrap();

                    // Evaluate value if formula
                    let raw_val = s.get(r, c);
                    let display_val = if raw_val.starts_with('=') {
                        crate::engine::eval_formula(raw_val, &s)
                            .unwrap_or_else(|e| format!("Error: {}", e))
                    } else {
                        raw_val.to_string()
                    };

                    if !display_val.is_empty() {
                        cr.set_source_rgb(0.1, 0.1, 0.1);
                        cr.move_to(x + 6.0, y + 17.0);
                        cr.show_text(&display_val).unwrap();
                    }
                }
            }

            // Draw column headers (A, B, C...)
            for c in 0..8 {
                let x = header_w + c as f64 * col_w;
                cr.set_source_rgb(0.93, 0.93, 0.93);
                cr.rectangle(x, 0.0, col_w, header_h);
                cr.fill().unwrap();

                cr.set_source_rgb(0.8, 0.8, 0.8);
                cr.rectangle(x, 0.0, col_w, header_h);
                cr.stroke().unwrap();

                cr.set_source_rgb(0.2, 0.2, 0.2);
                cr.set_font_size(11.0);
                cr.move_to(x + 45.0, 17.0);
                let col_letter = (b'A' + c as u8) as char;
                cr.show_text(&col_letter.to_string()).unwrap();
            }

            // Draw top-left header corner
            cr.set_source_rgb(0.9, 0.9, 0.9);
            cr.rectangle(0.0, 0.0, header_w, header_h);
            cr.fill().unwrap();
            cr.set_source_rgb(0.8, 0.8, 0.8);
            cr.rectangle(0.0, 0.0, header_w, header_h);
            cr.stroke().unwrap();

            // Draw selection border
            let sel_x = header_w + sel_c as f64 * col_w;
            let sel_y = header_h + sel_r as f64 * row_h;
            cr.set_source_rgb(0.1, 0.45, 0.85);
            cr.set_line_width(2.0);
            cr.rectangle(sel_x, sel_y, col_w, row_h);
            cr.stroke().unwrap();
        });

        let scroll = gtk::ScrolledWindow::new();
        let overlay = gtk::Overlay::new();

        scroll.set_child(Some(&canvas));
        scroll.set_vexpand(true);
        overlay.set_child(Some(&scroll));

        // Active overlay editor entry
        let entry_overlay = gtk::Entry::new();
        entry_overlay.set_visible(false);
        overlay.add_overlay(&entry_overlay);

        // Click selection and double-click editor trigger
        let gesture = gtk::GestureClick::new();
        let selected_cell_clone2 = selected_cell.clone();
        let sheet_clone2 = sheet.clone();
        let formula_clone = formula.clone();
        let canvas_clone = canvas.clone();
        let entry_overlay_clone = entry_overlay.clone();

        gesture.connect_pressed(move |_, n_press, x, y| {
            let col_w = 100.0;
            let row_h = 25.0;
            let header_w = 50.0;
            let header_h = 25.0;

            let c = ((x - header_w) / col_w).floor() as i32;
            let r = ((y - header_h) / row_h).floor() as i32;

            if (0..30).contains(&r) && (0..8).contains(&c) {
                selected_cell_clone2.set((r as usize, c as usize));

                let s = sheet_clone2.borrow();
                let val = s.get(r as usize, c as usize);
                formula_clone.set_text(val);
                canvas_clone.queue_draw();

                if n_press == 2 {
                    entry_overlay_clone.set_text(val);
                    let x_pos = header_w + c as f64 * col_w;
                    let y_pos = header_h + r as f64 * row_h;

                    entry_overlay_clone.set_margin_start(x_pos as i32);
                    entry_overlay_clone.set_margin_top(y_pos as i32);
                    entry_overlay_clone.set_size_request(100, 25);
                    entry_overlay_clone.set_visible(true);
                    entry_overlay_clone.grab_focus();
                }
            }
        });
        canvas.add_controller(gesture);

        // Overlay editor commit (Enter)
        let entry_overlay_clone2 = entry_overlay.clone();
        let sheet_clone3 = sheet.clone();
        let selected_cell_clone3 = selected_cell.clone();
        let formula_clone2 = formula.clone();
        let canvas_clone2 = canvas.clone();
        entry_overlay.connect_activate(move |_| {
            let val = entry_overlay_clone2.text().to_string();
            let (r, c) = selected_cell_clone3.get();
            sheet_clone3.borrow_mut().set(r, c, &val);
            formula_clone2.set_text(&val);
            entry_overlay_clone2.set_visible(false);
            canvas_clone2.queue_draw();
        });

        // Overlay editor commit (Focus Out)
        let focus_controller = gtk::EventControllerFocus::new();
        let entry_overlay_clone3 = entry_overlay.clone();
        let sheet_clone4 = sheet.clone();
        let selected_cell_clone4 = selected_cell.clone();
        let formula_clone3 = formula.clone();
        let canvas_clone3 = canvas.clone();
        focus_controller.connect_leave(move |_| {
            if WidgetExt::is_visible(&entry_overlay_clone3) {
                let val = entry_overlay_clone3.text().to_string();
                let (r, c) = selected_cell_clone4.get();
                sheet_clone4.borrow_mut().set(r, c, &val);
                formula_clone3.set_text(&val);
                entry_overlay_clone3.set_visible(false);
                canvas_clone3.queue_draw();
            }
        });
        entry_overlay.add_controller(focus_controller);

        // Top formula bar commit (Enter)
        let sheet_clone5 = sheet.clone();
        let selected_cell_clone5 = selected_cell.clone();
        let canvas_clone4 = canvas.clone();
        formula.connect_activate(move |f| {
            let val = f.text().to_string();
            let (r, c) = selected_cell_clone5.get();
            sheet_clone5.borrow_mut().set(r, c, &val);
            canvas_clone4.queue_draw();
        });

        // Connect Open/Save header buttons
        let mut child = header.first_child();
        while let Some(w) = child {
            if let Some(btn) = w.downcast_ref::<gtk::Button>() {
                let name = btn.widget_name();
                if name == "open" {
                    let sheet_clone6 = sheet.clone();
                    let canvas_clone5 = canvas.clone();
                    btn.connect_clicked(move |_| {
                        let path = std::path::Path::new("/home/james/dev/hanthor/spreadsheet.xlsx");
                        if path.exists() {
                            if let Ok(new_sheet) = crate::engine::read_spreadsheet(path) {
                                *sheet_clone6.borrow_mut() = new_sheet;
                                canvas_clone5.queue_draw();
                                println!("Spreadsheet loaded.");
                            }
                        }
                    });
                } else if name == "save" {
                    let sheet_clone7 = sheet.clone();
                    btn.connect_clicked(move |_| {
                        let path = std::path::Path::new("/home/james/dev/hanthor/spreadsheet.xlsx");
                        let s = sheet_clone7.borrow();
                        let _ = crate::engine::write_spreadsheet(path, &s);
                        println!("Spreadsheet saved.");
                    });
                }
            }
            child = w.next_sibling();
        }

        let main = gtk::Box::new(gtk::Orientation::Vertical, 2);
        main.append(&toolbar);
        main.append(&formula);
        main.append(&overlay);

        let toolbar_view = libadwaita::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(&main));
        win.set_content(Some(&toolbar_view));
        Self { window: win }
    }
    pub fn present(&self) {
        self.window.present();
    }
}
