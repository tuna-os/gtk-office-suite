#![doc(
    html_logo_url = "https://gitlab.gnome.org/World/fractal/-/raw/main/data/icons/org.tunaos.mandelbrot.svg?inline=false",
    html_favicon_url = "https://gitlab.gnome.org/World/fractal/-/raw/main/data/icons/org.tunaos.mandelbrot-symbolic.svg?inline=false"
)]
// Matrix SDK error enums are intentionally propagated by value. Boxing them
// only to satisfy this size heuristic would make these error paths less clear.
#![allow(clippy::result_large_err)]
// matrix-sdk 0.19's HTTP client future nests deeply enough that proving
// `Send` for futures handed to `spawn_tokio!` exceeds the default limit.
#![recursion_limit = "256"]

mod account_chooser_dialog;
mod account_switcher;
mod application;
mod components;
#[rustfmt::skip]
mod config;
mod account_settings;
mod contrib;
mod error_page;
mod i18n;
mod identity_verification_view;
mod intent;
mod login;
mod prelude;
mod secret;
mod session;
mod session_list;
mod session_view;
mod system_settings;
mod user_facing_error;
mod utils;
mod window;

use std::sync::LazyLock;

use gettextrs::*;
use gtk::{IconTheme, gdk::Display, gio};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use self::{application::*, config::*, i18n::*, utils::OneshotNotifier, window::Window};

/// The default tokio runtime to be used for async tasks
static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Runtime::new().expect("creating tokio runtime should succeed")
});

/// The notifier to make sure that only one `GtkMediaFile` is played at a single
/// time.
static MEDIA_FILE_NOTIFIER: LazyLock<OneshotNotifier> =
    LazyLock::new(|| OneshotNotifier::new("MEDIA_FILE_NOTIFIER"));

fn main() {
    // Initialize logger, debug is carried out via debug!, info!, warn! and error!.
    // Default to the INFO level for this crate and WARN for everything else.
    // It can be overridden with the RUST_LOG environment variable.
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("fractal=info,warn"));

    tracing_subscriber::registry()
        .with(fmt::layer().with_filter(env_filter))
        .init();

    // Prepare i18n
    // SAFETY: setlocale is called once during single-threaded startup; gettext-rs
    // 0.8 marks it unsafe.
    unsafe {
        setlocale(LocaleCategory::LcAll, "");
    }
    bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR).expect("Invalid argument passed to bindtextdomain");
    textdomain(GETTEXT_PACKAGE).expect("Invalid string passed to textdomain");

    gtk::glib::set_application_name("Mandelbrot");

    gtk::init().expect("Could not start GTK4");
    gst::init().expect("Could not initialize gst");

    #[cfg(target_os = "linux")]
    aperture::init(APP_ID);

    let res = gio::Resource::load(RESOURCES_FILE).expect("Could not load gresource file");
    gio::resources_register(&res);
    let ui_res = gio::Resource::load(UI_RESOURCES_FILE).expect("Could not load UI gresource file");
    gio::resources_register(&ui_res);

    IconTheme::for_display(&Display::default().unwrap())
        .add_resource_path("/org/tunaos/mandelbrot/icons");

    let app = Application::new();
    app.run();
}
