use gtk4 as gtk;
use libadwaita::prelude::*;

pub struct DecksWindow {
    window: libadwaita::ApplicationWindow,
}

fn markdown_to_pango(text: &str) -> String {
    let mut res = text.to_string();
    let mut is_heading = false;
    let mut size = "";

    if res.starts_with("### ") {
        res = res[4..].to_string();
        is_heading = true;
        size = "xx-large";
    } else if res.starts_with("## ") {
        res = res[3..].to_string();
        is_heading = true;
        size = "xx-large";
    } else if res.starts_with("# ") {
        res = res[2..].to_string();
        is_heading = true;
        size = "32000";
    }

    // Inline bold markdown
    while let Some(start) = res.find("**") {
        if let Some(end) = res[start + 2..].find("**") {
            let before = &res[..start];
            let middle = &res[start + 2..start + 2 + end];
            let after = &res[start + 4 + end..];
            res = format!("{}<b>{}</b>{}", before, middle, after);
        } else {
            break;
        }
    }

    // Inline italic markdown
    while let Some(start) = res.find("*") {
        if let Some(end) = res[start + 1..].find("*") {
            let before = &res[..start];
            let middle = &res[start + 1..start + 1 + end];
            let after = &res[start + 2 + end..];
            res = format!("{}<i>{}</i>{}", before, middle, after);
        } else {
            break;
        }
    }

    if is_heading {
        res = format!("<span size=\"{}\" weight=\"bold\">{}</span>", size, res);
    }

    res
}

impl DecksWindow {
    pub fn new(app: &libadwaita::Application) -> Self {
        let win = libadwaita::ApplicationWindow::builder()
            .application(app)
            .build();
        win.set_title(Some("Decks"));
        win.set_default_size(1024, 700);

        let header = suite_common::make_header_bar("Decks");

        // Toolbar
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        toolbar.set_halign(gtk::Align::Center);
        toolbar.add_css_class("toolbar");
        toolbar.add_css_class("linked");

        let text_btn = gtk::Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text("Insert Text Box")
            .css_classes(vec!["flat".to_string()])
            .build();
        let rect_btn = gtk::Button::builder()
            .icon_name("shapes-symbolic")
            .tooltip_text("Insert Rectangle")
            .css_classes(vec!["flat".to_string()])
            .build();
        let add_slide_btn = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add Slide")
            .css_classes(vec!["flat".to_string()])
            .build();

        toolbar.append(&text_btn);
        toolbar.append(&rect_btn);
        toolbar.append(&add_slide_btn);

        // Slide state
        let deck = std::rc::Rc::new(std::cell::RefCell::new(crate::engine::Deck::new()));
        let current_slide_idx = std::rc::Rc::new(std::cell::Cell::new(0));

        // Slide sidebar list
        let sidebar = gtk::ListBox::new();
        sidebar.set_width_request(180);

        let populate_sidebar = {
            let sidebar_clone = sidebar.clone();
            let deck_clone = deck.clone();
            move || {
                // Clear previous rows
                while let Some(row) = sidebar_clone.row_at_index(0) {
                    sidebar_clone.remove(&row);
                }
                let d = deck_clone.borrow();
                for (i, slide) in d.slides.iter().enumerate() {
                    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                    let label = gtk::Label::new(Some(&format!("{}: {}", i + 1, slide.title)));
                    row_box.append(&label);
                    sidebar_clone.append(&row_box);
                }
            }
        };
        populate_sidebar();

        // Canvas area
        let canvas = gtk::DrawingArea::new();
        canvas.set_hexpand(true);
        canvas.set_vexpand(true);

        // Sidebar row selection handler
        let current_slide_idx_clone = current_slide_idx.clone();
        let canvas_clone = canvas.clone();
        sidebar.connect_row_selected(move |_, row| {
            if let Some(r) = row {
                current_slide_idx_clone.set(r.index() as usize);
                canvas_clone.queue_draw();
            }
        });

        // Set drawing function
        let deck_clone2 = deck.clone();
        let current_slide_idx_clone2 = current_slide_idx.clone();
        canvas.set_draw_func(move |area, cr, _w, _h| {
            cr.set_source_rgb(0.9, 0.9, 0.9);
            cr.paint().unwrap();

            // Slide frame
            let slide_w = 800.0;
            let slide_h = 480.0;
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.rectangle(20.0, 20.0, slide_w, slide_h);
            cr.fill().unwrap();

            cr.set_source_rgb(0.7, 0.7, 0.7);
            cr.set_line_width(1.0);
            cr.rectangle(20.0, 20.0, slide_w, slide_h);
            cr.stroke().unwrap();

            let idx = current_slide_idx_clone2.get();
            let d = deck_clone2.borrow();
            if let Some(slide) = d.slides.get(idx) {
                for obj in &slide.objects {
                    match obj {
                        crate::engine::SlideObject::Rect { x, y, w, h } => {
                            cr.set_source_rgb(0.2, 0.5, 0.8);
                            cr.rectangle(20.0 + x, 20.0 + y, *w, *h);
                            cr.fill().unwrap();
                            cr.set_source_rgb(0.1, 0.3, 0.6);
                            cr.rectangle(20.0 + x, 20.0 + y, *w, *h);
                            cr.stroke().unwrap();
                        }
                        crate::engine::SlideObject::TextBox {
                            text,
                            x,
                            y,
                            w,
                            h: _,
                        } => {
                            let pango_ctx = area.pango_context();
                            let layout = gtk::pango::Layout::new(&pango_ctx);
                            layout.set_markup(&markdown_to_pango(text));
                            layout.set_width((*w * gtk::pango::SCALE as f64) as i32);

                            cr.save().unwrap();
                            cr.translate(20.0 + x, 20.0 + y);
                            cr.set_source_rgb(0.15, 0.15, 0.15);
                            pangocairo::functions::show_layout(cr, &layout);
                            cr.restore().unwrap();
                        }
                    }
                }
            }
        });

        // Add Slide button clicked handler
        let deck_clone3 = deck.clone();
        let populate_sidebar_clone = populate_sidebar.clone();
        let sidebar_clone2 = sidebar.clone();
        add_slide_btn.connect_clicked(move |_| {
            deck_clone3.borrow_mut().add_slide();
            populate_sidebar_clone();
            // Auto select new slide
            let idx = deck_clone3.borrow().slides.len() - 1;
            if let Some(row) = sidebar_clone2.row_at_index(idx as i32) {
                sidebar_clone2.select_row(Some(&row));
            }
        });

        // Add Text button clicked handler
        let deck_clone4 = deck.clone();
        let current_slide_idx_clone3 = current_slide_idx.clone();
        let canvas_clone2 = canvas.clone();
        text_btn.connect_clicked(move |_| {
            let idx = current_slide_idx_clone3.get();
            let mut d = deck_clone4.borrow_mut();
            if let Some(slide) = d.slides.get_mut(idx) {
                slide.objects.push(crate::engine::SlideObject::TextBox {
                    text: "Double click to edit **Rich Text**".into(),
                    x: 100.0,
                    y: 150.0,
                    w: 400.0,
                    h: 40.0,
                });
            }
            canvas_clone2.queue_draw();
        });

        // Add Rect button clicked handler
        let deck_clone5 = deck.clone();
        let current_slide_idx_clone4 = current_slide_idx.clone();
        let canvas_clone3 = canvas.clone();
        rect_btn.connect_clicked(move |_| {
            let idx = current_slide_idx_clone4.get();
            let mut d = deck_clone5.borrow_mut();
            if let Some(slide) = d.slides.get_mut(idx) {
                slide.objects.push(crate::engine::SlideObject::Rect {
                    x: 200.0,
                    y: 250.0,
                    w: 200.0,
                    h: 80.0,
                });
            }
            canvas_clone3.queue_draw();
        });

        let split_view = libadwaita::OverlaySplitView::new();
        split_view.set_sidebar(Some(&sidebar));
        split_view.set_content(Some(&canvas));
        split_view.set_min_sidebar_width(200.0);
        split_view.set_max_sidebar_width(280.0);
        split_view.set_pin_sidebar(true);

        let m = gtk::Box::new(gtk::Orientation::Vertical, 2);
        m.append(&toolbar);
        m.append(&split_view);

        let toolbar_view = libadwaita::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(&m));
        win.set_content(Some(&toolbar_view));
        Self { window: win }
    }
    pub fn present(&self) {
        self.window.present();
    }
}
