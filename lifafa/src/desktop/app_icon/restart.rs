//! Restarting the app: a helper process (activated over D-Bus inside
//! Flatpak) waits for the running instance to let go of its name, then
//! becomes the new one.

use std::path::PathBuf;

/// Command-line flag for the restart helper: waits for the running instance
/// to let go of its D-Bus name, then becomes a fresh one.
pub(crate) const RESTART_FLAG: &str = "--restart-helper";

/// Well-known name of the restart helper's D-Bus service. Inside Flatpak the
/// helper can't just be spawned — the sandbox is torn down with the process
/// that started it — so it is *activated*: the bus starts the exported
/// service (a new sandbox instance) and the running app exits under it.
fn restart_service_name() -> String {
    format!("{}.Restart", crate::APP_ID)
}

/// Start the helper that will bring the app back, and report whether it is
/// in place. The caller quits only on `Ok`, so a failed attempt leaves the
/// app running rather than gone.
pub(crate) fn launch_restart_helper() -> Result<(), String> {
    if crate::desktop::platform::is_flatpak() {
        activate_restart_service()
    } else {
        spawn_restart_helper()
    }
}

/// What running Hylki again means for this install. From an AppImage it is
/// the bundle, never `current_exe()`: that path is inside the runtime's
/// temporary mount, which is unmounted as this process exits, so a helper
/// holding it would re-exec a path that had stopped resolving.
fn relaunch_path() -> Result<PathBuf, String> {
    match crate::desktop::platform::appimage() {
        Some(bundle) => Ok(bundle),
        None => std::env::current_exe().map_err(|e| e.to_string()),
    }
}

/// Outside Flatpak: run our own binary as a detached helper.
fn spawn_restart_helper() -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let exe = relaunch_path()?;
    std::process::Command::new(exe)
        .arg(RESTART_FLAG)
        .process_group(0)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Inside Flatpak: ask the session bus to start the helper service, which
/// the export turned into `flatpak run … --restart-helper`. Returns once
/// the helper owns its name (or the bus gave up).
fn activate_restart_service() -> Result<(), String> {
    use gtk::glib::prelude::ToVariant;
    let conn = gtk::gio::bus_get_sync(gtk::gio::BusType::Session, gtk::gio::Cancellable::NONE)
        .map_err(|e| e.to_string())?;
    let reply = conn
        .call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "StartServiceByName",
            Some(&(restart_service_name(), 0u32).to_variant()),
            None,
            gtk::gio::DBusCallFlags::NONE,
            15_000,
            gtk::gio::Cancellable::NONE,
        )
        .map_err(|e| e.to_string())?;
    match reply.get::<(u32,)>() {
        // 1 = started, 2 = was already running: either way a helper is up.
        Some((1,)) | Some((2,)) => Ok(()),
        other => Err(format!("unexpected StartServiceByName reply {other:?}")),
    }
}

/// The helper's whole life: own the service name (so a bus activation
/// completes), wait for the app to release its own name, then exec a normal
/// instance in this process — inside Flatpak that keeps the helper's
/// sandbox alive as the app's.
pub(crate) fn run_restart_helper() -> ! {
    use gtk::glib::prelude::ToVariant;
    use std::os::unix::process::CommandExt;
    let conn = gtk::gio::bus_get_sync(gtk::gio::BusType::Session, gtk::gio::Cancellable::NONE).ok();
    if let Some(conn) = &conn {
        let _ = conn.call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
            Some(&(restart_service_name(), 4u32).to_variant()), // DO_NOT_QUEUE
            None,
            gtk::gio::DBusCallFlags::NONE,
            3000,
            gtk::gio::Cancellable::NONE,
        );
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    // Give the app a moment to start quitting before the first look.
    std::thread::sleep(std::time::Duration::from_millis(300));
    while crate::primary_instance_running() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    drop(conn);
    let exe = relaunch_path().unwrap_or_else(|_| PathBuf::from("hylki"));
    // A one-off launch's review and capture switches must not carry over
    // into the instance that comes back.
    let err = std::process::Command::new(exe)
        .env_remove("HYLKI_WELCOME")
        .env_remove("HYLKI_SHOWCASE")
        .env_remove("HYLKI_SHOWCASE_PAGE")
        .env_remove("HYLKI_SHOWCASE_SETTINGS")
        .env_remove("HYLKI_SHOWCASE_SCROLL")
        .env_remove("HYLKI_SHOWCASE_DELAY")
        .exec();
    eprintln!("hylki: restart failed: {err}");
    std::process::exit(1);
}
