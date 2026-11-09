//! Collection of macros.

/// Spawn a local future on the default `GMainContext`.
///
/// A custom [`glib::Priority`] can be set as the first argument.
///
/// [`glib::Priority`]: gtk::glib::Priority
#[macro_export]
macro_rules! spawn {
    ($future:expr) => {
        gtk::glib::MainContext::default().spawn_local($future)
    };
    ($priority:expr, $future:expr) => {
        gtk::glib::MainContext::default().spawn_local_with_priority($priority, $future)
    };
}

/// Spawn a future on the tokio runtime.
#[macro_export]
macro_rules! spawn_tokio {
    ($future:expr) => {
        $crate::RUNTIME.spawn($future)
    };
}
