use gtk4::prelude::*;
mod window;
mod engine;
mod export;
mod undo;
mod canvas;
mod sidebar;
mod toolbar;

fn main() {
    let suite = suite_common::SuiteApp::new("org.tunaos.decks-rust");
    suite.app.connect_activate(|app| {
        let w = window::DecksWindow::new(app);
        w.present();
    });
    suite.run();
}
