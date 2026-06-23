use gtk4 as gtk;
use gtk::prelude::*;

pub struct DecksWindow { window: gtk::ApplicationWindow }

impl DecksWindow {
    pub fn new(app: &gtk::Application) -> Self {
        let win = gtk::ApplicationWindow::builder().application(app).build();
        win.set_title(Some("Decks"));
        win.set_default_size(960, 600);

        let header = suite_common::build_chrome();
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        toolbar.set_halign(gtk::Align::Center);
        toolbar.add_css_class("toolbar");
        for label in &["Text", "Rect", "Image"] {
            toolbar.append(&gtk::Button::with_label(label));
        }

        // Slide sidebar
        let sidebar = gtk::ListBox::new();
        sidebar.set_width_request(160);
        for i in 1..=3 {
            let row = gtk::Label::new(Some(&format!("Slide {}", i)));
            sidebar.append(&row);
        }

        // Canvas area
        let canvas = gtk::DrawingArea::new();
        canvas.set_draw_func(|_area, cr, w, h| {
            cr.set_source_rgb(1.0, 1.0, 1.0); cr.paint().unwrap();
            cr.set_source_rgb(0.2, 0.2, 0.2);
            cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            cr.set_font_size(28.0_f64);
            let txt = "🎬 Slide canvas";
            let ext = cr.text_extents(txt).unwrap();
            cr.move_to(((w as f64 - ext.width())) / 2.0, (h as f64) / 2.0);
            cr.show_text(txt).unwrap();
        });
        canvas.set_hexpand(true); canvas.set_vexpand(true);

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        content.append(&sidebar); content.append(&canvas);

        let m = gtk::Box::new(gtk::Orientation::Vertical, 2);
        m.append(&toolbar); m.append(&content);
        let c = gtk::Box::new(gtk::Orientation::Vertical, 0);
        c.append(&header); c.append(&m);
        win.set_child(Some(&c));
        Self { window: win }
    }
    pub fn present(&self) { self.window.present(); }
}

