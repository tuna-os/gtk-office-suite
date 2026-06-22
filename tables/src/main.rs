// main.rs — Tables spreadsheet, pure Rust + gtk4-rs + suite-common.
use gtk4::prelude::*;
mod engine;
mod window;

fn main() {
    let app = suite_common::make_app("org.tunaos.tables");
    app.connect_activate(|app| {
        let win = window::TablesWindow::new(app);
        win.present();
    });
    app.run();
}
