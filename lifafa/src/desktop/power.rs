//! System suspend/resume and network-return detection.
//!
//! When the machine sleeps, open IMAP sockets are silently killed by the OS or
//! dropped by NAT/keepalive timeouts. On wake our long-lived worker sessions
//! (including a parked IMAP IDLE) are dead but nothing notices, so the app stops
//! pulling new mail until the next auto-fetch tick (#322). Three signals cover
//! it, and the app reconnects on any of them:
//!
//! - logind's `PrepareForSleep(false)` on the **system** bus, the moment the
//!   system resumes. Only outside the Flatpak: the sandbox has no system bus.
//! - A gap between the boot clock and the monotonic clock, which works
//!   everywhere and notices a wake within [`CLOCK_TICK`].
//! - The network coming back, since at the moment of waking a laptop is usually
//!   still rejoining its Wi-Fi and a reconnect then would only fail.
//!
//! The network monitor's state is also kept here for the workers: nothing
//! dials out while it says the network is down, and a metered connection
//! holds back the work done ahead of the user (#442).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const LOGIN1_DEST: &str = "org.freedesktop.login1";
const LOGIN1_PATH: &str = "/org/freedesktop/login1";
const LOGIN1_MANAGER: &str = "org.freedesktop.login1.Manager";

/// How often the clock check runs, and so the most a wake can go unnoticed
/// where logind is out of reach.
const CLOCK_TICK: u32 = 10;
/// How far the boot clock may run ahead of the monotonic one across a tick
/// before it counts as a sleep. Both advance together while awake, so any
/// real suspend clears this easily.
const SLEEP_GAP: Duration = Duration::from_secs(5);

/// Watch systemd-logind for resume-from-sleep, invoking `on_wake` each time the
/// system wakes. Runs on a dedicated thread; silently no-ops if logind or the
/// system bus is unavailable.
pub(crate) fn watch_resume<F: Fn() + Send + 'static>(on_wake: F) {
    let _ = std::thread::Builder::new()
        .name("logind-watch".into())
        .spawn(move || {
            if let Err(e) = watch_loop(&on_wake) {
                tracing::debug!("logind sleep watch stopped: {e}");
            }
        });
}

fn watch_loop<F: Fn()>(on_wake: &F) -> Result<(), Box<dyn std::error::Error>> {
    let conn = zbus::blocking::Connection::system()?;
    let proxy = zbus::blocking::Proxy::new(&conn, LOGIN1_DEST, LOGIN1_PATH, LOGIN1_MANAGER)?;
    // Blocks until logind emits PrepareForSleep; ends if the bus closes.
    let mut signals = proxy.receive_signal("PrepareForSleep")?;
    for msg in signals.by_ref() {
        // start == true just before sleeping, false once resumed. Only the
        // resume edge needs a reconnect.
        if let Ok(false) = msg.body().deserialize::<bool>() {
            on_wake();
        }
    }
    Ok(())
}

/// Notice a wake from the clocks alone, for where logind can't be reached.
/// `Instant` is CLOCK_MONOTONIC, which stands still while the system sleeps;
/// /proc/uptime is the boot clock, which keeps counting. Must be called on the
/// GTK main thread.
pub(crate) fn watch_clock_gap<F: Fn() + 'static>(on_wake: F) {
    let Some(mut uptime) = boot_clock() else {
        return;
    };
    let mut mono = Instant::now();
    gtk::glib::timeout_add_seconds_local(CLOCK_TICK, move || {
        let Some(now_uptime) = boot_clock() else {
            return gtk::glib::ControlFlow::Break;
        };
        let now_mono = Instant::now();
        let slept = (now_uptime.saturating_sub(uptime)).saturating_sub(now_mono - mono);
        uptime = now_uptime;
        mono = now_mono;
        if slept >= SLEEP_GAP {
            tracing::info!("woke from about {}s of sleep", slept.as_secs());
            on_wake();
        }
        gtk::glib::ControlFlow::Continue
    });
}

fn boot_clock() -> Option<Duration> {
    let text = std::fs::read_to_string("/proc/uptime").ok()?;
    let secs: f64 = text.split_whitespace().next()?.parse().ok()?;
    Some(Duration::from_secs_f64(secs))
}

/// What the network monitor reported, for [`watch_network`]'s callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkChange {
    /// Reachable again after being down.
    Back,
    /// No longer reachable.
    Lost,
    /// The connection started or stopped being metered.
    Metered(bool),
}

/// The monitor's view, kept where the worker threads can read it: GIO
/// delivers its changes on the main thread only (#442).
static ONLINE: AtomicBool = AtomicBool::new(true);
static METERED: AtomicBool = AtomicBool::new(false);
/// Settings: whether a metered connection holds back the work done ahead of
/// the user (on by default).
static SAVE_DATA: AtomicBool = AtomicBool::new(true);
/// Until when a Refresh clicked while offline may try the servers anyway,
/// in case the monitor is wrong about the network (milliseconds since the
/// process started, 0 for no grace).
static TRY_UNTIL: AtomicU64 = AtomicU64::new(0);
/// How long one such Refresh lets connections through.
const TRY_ANYWAY: Duration = Duration::from_secs(60);

fn since_start() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Read the monitor's current state into [`online`] and [`metered`]. Called
/// before the workers start, so their first connection already knows it.
/// Must be called on the GTK main thread.
pub(crate) fn read_network(save_data: bool) {
    use gtk::gio::prelude::*;
    let monitor = gtk::gio::NetworkMonitor::default();
    ONLINE.store(monitor.is_network_available(), Ordering::Relaxed);
    METERED.store(monitor.is_network_metered(), Ordering::Relaxed);
    SAVE_DATA.store(save_data, Ordering::Relaxed);
    since_start();
}

/// Invoke `on_change` when the network goes down, comes back, or turns
/// metered or not. GIO answers through the network-monitor portal inside the
/// Flatpak, so this needs no permission. Must be called on the GTK main thread.
pub(crate) fn watch_network<F: Fn(NetworkChange) + 'static>(on_change: F) {
    use gtk::gio::prelude::*;
    let monitor = gtk::gio::NetworkMonitor::default();
    let on_change = std::rc::Rc::new(on_change);
    // Fires on every route change too (VPNs, container bridges); only the
    // edges mean anything here.
    {
        let on_change = on_change.clone();
        monitor.connect_network_changed(move |_, up| {
            let was_up = ONLINE.swap(up, Ordering::Relaxed);
            if up && !was_up {
                tracing::info!("network is back");
                on_change(NetworkChange::Back);
            } else if !up && was_up {
                tracing::info!("network is gone");
                on_change(NetworkChange::Lost);
            }
        });
    }
    monitor.connect_network_metered_notify(move |m| {
        let now = m.is_network_metered();
        if METERED.swap(now, Ordering::Relaxed) != now {
            tracing::info!("network is {}metered", if now { "" } else { "not " });
            on_change(NetworkChange::Metered(now));
        }
    });
}

/// Whether the network looks reachable right now.
pub(crate) fn online() -> bool {
    ONLINE.load(Ordering::Relaxed)
}

/// Whether the connection is metered.
pub(crate) fn metered() -> bool {
    METERED.load(Ordering::Relaxed)
}

/// Settings: hold back the work done ahead of the user on a metered
/// connection.
pub(crate) fn set_save_data(on: bool) {
    SAVE_DATA.store(on, Ordering::Relaxed);
}

/// Whether the work done ahead of the user (prefetching bodies and
/// attachments, indexing old mail, watching extra folders, fetching logos)
/// waits: the connection is metered and Settings asks to save data there.
pub(crate) fn saving_data() -> bool {
    metered() && SAVE_DATA.load(Ordering::Relaxed)
}

/// Whether pictures nobody asked for yet (sender logos, Gravatars) may be
/// fetched: there is a network, and it is not one to save data on.
pub(crate) fn fetches_ahead() -> bool {
    reaches_internet() && !saving_data()
}

/// A Refresh clicked while offline: let connections through for a while,
/// since the monitor can be wrong (a network with no default route).
pub(crate) fn try_anyway() {
    TRY_UNTIL.store(
        since_start() + TRY_ANYWAY.as_millis() as u64,
        Ordering::Relaxed,
    );
}

/// Whether a connection to `host` is worth trying. A server on this machine
/// (Proton Mail Bridge) is reachable with no network at all.
pub(crate) fn reaches(host: &str) -> bool {
    online() || is_local(host) || since_start() < TRY_UNTIL.load(Ordering::Relaxed)
}

/// Whether background traffic to the internet should go out: not while
/// offline, unless a Refresh asked to try anyway.
pub(crate) fn reaches_internet() -> bool {
    reaches("")
}

/// [`reaches`] for a server given as a URL (JMAP).
pub(crate) fn reaches_url(url: &str) -> bool {
    reaches(url_host(url))
}

fn url_host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => host.split(':').next().unwrap_or(""),
    }
}

fn is_local(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_hosts() {
        assert!(is_local("localhost"));
        assert!(is_local("LOCALHOST"));
        assert!(is_local("127.0.0.1"));
        assert!(is_local("127.0.0.2"));
        assert!(is_local("::1"));
        assert!(is_local("[::1]"));
        assert!(!is_local("imap.example.com"));
        assert!(!is_local("192.168.1.10"));
        assert!(!is_local(""));
    }

    #[test]
    fn url_hosts() {
        assert_eq!(
            url_host("https://api.fastmail.com/jmap/session"),
            "api.fastmail.com"
        );
        assert_eq!(url_host("http://localhost:8080"), "localhost");
        assert_eq!(url_host("https://user@[::1]:443/jmap"), "::1");
        assert_eq!(url_host("mail.example.com"), "mail.example.com");
    }
}
