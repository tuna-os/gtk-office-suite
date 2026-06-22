// main.rs — Tables spreadsheet, pure Rust + gtk4-rs + suite-common.
use gtk4::prelude::*;
mod charts;
mod engine;
mod window;

fn main() {
    let app = suite_common::make_app("org.tunaos.tables");
    app.connect_activate(|app| {
        init_dark_mode(app);
        let win = window::TablesWindow::new(app);
        win.present();
    });
    app.run();
}

// Dark mode support
fn init_dark_mode(_app: &libadwaita::Application) {
    let style = libadwaita::StyleManager::default();
    println!("Dark mode: {}", style.is_dark());
}
