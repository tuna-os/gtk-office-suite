use gtk4::prelude::*;
mod engine;
mod window;
fn main() {
    let app = suite_common::make_app("org.tunaos.letters");
    suite_common::apply_dark_mode();
    app.connect_activate(|app| {
        let w = window::LettersWindow::new(app);
        w.present();
    });
    app.run();
}
