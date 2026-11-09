//! The unread count on the app's launcher icon (#271).
//!
//! There is no portal for this. The de facto interface is Unity's
//! `com.canonical.Unity.LauncherEntry`: a broadcast `Update` signal naming
//! the desktop file, which KDE Plasma's task manager, Dash to Dock and Dash
//! to Panel read. A broadcast needs no bus permission in the Flatpak sandbox.
//! Plain GNOME Shell ignores it.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use zbus::zvariant::Value;

const IFACE: &str = "com.canonical.Unity.LauncherEntry";
const PATH: &str = "/co/hyprlab/Hylki/LauncherEntry";

/// One connection for the life of the process: Plasma and Dash to Dock
/// drop a badge when the connection that set it goes away, so a connection
/// per update would clear the count as soon as it was shown.
fn connection() -> Option<&'static zbus::blocking::Connection> {
    static CONN: OnceLock<Option<zbus::blocking::Connection>> = OnceLock::new();
    CONN.get_or_init(|| match zbus::blocking::Connection::session() {
        Ok(conn) => Some(conn),
        Err(e) => {
            tracing::debug!("launcher badge: no session bus: {e}");
            None
        }
    })
    .as_ref()
}

/// The value last sent, so repeats can be skipped.
static LAST: Mutex<Option<u32>> = Mutex::new(None);

/// Forget what was last sent, so the next [`set_count`] goes out even if the
/// count has not changed. The dock can lose a badge without our connection
/// going away: GNOME Shell disables extensions like Dash to Dock while the
/// screen is locked and enables them afresh on unlock, and a restarted
/// Plasma shell starts empty. Neither asks for the count again.
pub(crate) fn resend() {
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Invoke `on_back` each time a dock starts listening for badges. Dash to
/// Dock and Ubuntu Dock own `com.canonical.Unity` for exactly as long as
/// they keep badges, and start empty each time they take it: after an
/// unlock (GNOME Shell disables them on the lock screen), after Do Not
/// Disturb, after the shell restarts. Must be called on the GTK main thread.
pub(crate) fn watch_dock<F: Fn() + Send + Sync + 'static>(on_back: F) {
    let id = gtk::gio::bus_watch_name(
        gtk::gio::BusType::Session,
        "com.canonical.Unity",
        gtk::gio::BusNameWatcherFlags::NONE,
        move |_, _, _| on_back(),
        |_, _| {},
    );
    // Watched for the life of the process: the id is never handed to
    // bus_unwatch_name, and dropping it does not end the watch.
    let _ = id;
}

/// Show `count` on the launcher icon; 0 hides the badge. Repeats of the
/// value last sent are skipped, since this runs on every unread change.
pub(crate) fn set_count(count: u32) {
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if *last == Some(count) {
            return;
        }
        *last = Some(count);
    }
    let Some(conn) = connection() else { return };
    let uri = format!("application://{}.desktop", crate::APP_ID);
    let mut props: HashMap<&str, Value> = HashMap::new();
    props.insert("count", Value::I64(i64::from(count)));
    props.insert("count-visible", Value::Bool(count > 0));
    if let Err(e) = conn.emit_signal(None::<&str>, PATH, IFACE, "Update", &(uri, props)) {
        tracing::debug!("launcher badge not set: {e}");
    }
}

/// Inbox mail that arrived while the main window was not in front (#431),
/// for the badge that counts only that. Arrival is decided as for the
/// new-mail notification: unread, and absent from the inbox's previous
/// listing. Whether each is still unread is asked at count time, of the
/// lists the app holds, so a message read or deleted in the meantime (from
/// a notification's buttons, or on another device) stops counting. Kept in
/// memory only: a restart starts from none.
#[derive(Debug, Default)]
pub(crate) struct Arrivals {
    /// (account, folder, UID).
    mail: HashSet<(u32, u32, u32)>,
}

impl Arrivals {
    /// Take in a fresh listing of an inbox. `old` is the previous listing's
    /// UIDs, `None` on the folder's first load, which arrives with everything
    /// the inbox already holds and so counts nothing. `listing` is the new
    /// one, as (UID, unread).
    pub(crate) fn note(
        &mut self,
        account_id: u32,
        folder_id: u32,
        old: Option<&HashSet<u32>>,
        listing: impl IntoIterator<Item = (u32, bool)>,
    ) {
        let Some(old) = old else { return };
        self.mail.extend(
            listing
                .into_iter()
                .filter(|&(uid, unread)| unread && !old.contains(&uid))
                .map(|(uid, _)| (account_id, folder_id, uid)),
        );
    }

    /// The window came forward: everything has been seen.
    pub(crate) fn clear(&mut self) {
        self.mail.clear();
    }

    /// How many of the arrivals are still unread, where `unread_uids` gives
    /// a folder's unread UIDs as the app now lists them.
    pub(crate) fn count<I: IntoIterator<Item = u32>>(
        &self,
        mut unread_uids: impl FnMut(u32, u32) -> I,
    ) -> u32 {
        let mut by_folder: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
        for &(a, f, uid) in &self.mail {
            by_folder.entry((a, f)).or_default().push(uid);
        }
        let mut n = 0u32;
        for ((a, f), uids) in by_folder {
            let unread: HashSet<u32> = unread_uids(a, f).into_iter().collect();
            n += uids.iter().filter(|u| unread.contains(u)).count() as u32;
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::Arrivals;
    use std::collections::HashSet;

    fn uids(v: &[u32]) -> HashSet<u32> {
        v.iter().copied().collect()
    }

    #[test]
    fn a_first_load_counts_nothing() {
        let mut a = Arrivals::default();
        a.note(1, 10, None, [(1, true), (2, true)]);
        assert_eq!(a.count(|_, _| vec![1, 2]), 0);
    }

    #[test]
    fn only_new_unread_mail_counts() {
        let mut a = Arrivals::default();
        // 1 and 2 were there before; 3 is new and unread, 4 new but read.
        a.note(
            1,
            10,
            Some(&uids(&[1, 2])),
            [(1, true), (2, true), (3, true), (4, false)],
        );
        assert_eq!(a.count(|_, _| vec![1, 2, 3]), 1);
        // A later sync repeats 3 and brings 5.
        a.note(1, 10, Some(&uids(&[1, 2, 3, 4])), [(3, true), (5, true)]);
        assert_eq!(a.count(|_, _| vec![1, 2, 3, 5]), 2);
    }

    #[test]
    fn reading_or_deleting_one_lowers_the_count() {
        let mut a = Arrivals::default();
        a.note(1, 10, Some(&uids(&[])), [(7, true), (8, true)]);
        a.note(2, 20, Some(&uids(&[])), [(7, true)]);
        assert_eq!(a.count(|_, _| vec![7, 8]), 3);
        // 8 read in account 1; account 2's 7 deleted.
        assert_eq!(
            a.count(|acct, _| if acct == 1 { vec![7] } else { vec![] }),
            1
        );
    }

    #[test]
    fn coming_forward_resets() {
        let mut a = Arrivals::default();
        a.note(1, 10, Some(&uids(&[])), [(7, true)]);
        a.clear();
        assert_eq!(a.count(|_, _| vec![7]), 0);
    }
}
