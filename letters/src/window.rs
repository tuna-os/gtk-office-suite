use gtk4 as gtk;
use gtk::prelude::*;

pub struct LettersWindow { window: gtk::ApplicationWindow }

impl LettersWindow {
    pub fn new(app: &gtk::Application) -> Self {
        let win = gtk::ApplicationWindow::builder().application(app).build();
        win.set_title(Some("Letters"));
        win.set_default_size(900, 600);
        let header = suite_common::make_header_bar();
        let toolbar = suite_common::make_toolbar();
        let content = gtk::Label::new(Some("Letters — Rust native"));
        content.set_vexpand(true);
        let main = gtk::Box::new(gtk::Orientation::Vertical, 2);
        main.append(&toolbar); main.append(&content);
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.append(&header); container.append(&main);
        win.set_child(Some(&container));
        Self { window: win }
    }
    pub fn present(&self) { self.window.present(); }
}
