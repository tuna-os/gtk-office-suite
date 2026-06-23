use gtk4 as gtk;
use gtk::prelude::*;

pub struct TablesWindow { window: libadwaita::ApplicationWindow }

impl TablesWindow {
    pub fn new(app: &gtk::Application) -> Self {
        let win = libadwaita::ApplicationWindow::builder().application(app).build();
        win.set_title(Some("Tables"));
        win.set_default_size(900, 600);

        let toolbar = suite_common::make_toolbar();
        let formula = gtk::Entry::new(); formula.set_placeholder_text(Some("Formula…"));
        let grid = gtk::TextView::new();
        grid.set_monospace(true); grid.set_editable(true); grid.set_wrap_mode(gtk::WrapMode::None);
        let buf = grid.buffer();
        let mut text = String::new();
        for c in 0..10u8 { text.push((b'A' + c) as char); if c < 9 { text.push('\t'); } }
        text.push('\n');
        for r in 1..50 { for c in 0..10 { if c > 0 { text.push('\t'); } text.push_str(&format!("{},{}", r, c)); } text.push('\n'); }
        buf.set_text(&text);
        let scroll = gtk::ScrolledWindow::new(); scroll.set_child(Some(&grid)); scroll.set_vexpand(true);
        let main = gtk::Box::new(gtk::Orientation::Vertical, 2);
        main.append(&formula); main.append(&scroll);
        suite_common::build_chrome(&win, &toolbar);
        // Add content below toolbar
        if let Some(tv) = win.content().and_then(|c| c.downcast::<adw::ToolbarView>().ok()) {
            tv.set_content(Some(&main));
        }
        suite_common::apply_dark();
        Self { window: win }
    }
    pub fn present(&self) { self.window.present(); }
}
