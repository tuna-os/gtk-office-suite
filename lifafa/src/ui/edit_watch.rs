//! Open an attachment for editing in another app and notice when it is saved
//! (INTEGRATION-PLAN A2).
//!
//! The copy handed to the other app is staged privately
//! (`attachment_io::open_bytes_editable`); this watches it and feeds every save
//! to the suite's GTK-free model,
//! `suite_common_core::attachment_edit::EditSession`, which decides when a
//! change is a finished edit (the same new bytes seen twice, since editors
//! save in steps). For a finished edit `on_edited` is called with the staged
//! path; the caller asks the person what to do and reports the answer with
//! [`answer`], which hands back the edited bytes when they said yes.
//!
//! The reader's "reply with the changed file" and the composer's put-back
//! (`ui::compose::edit_back`) both use it.
//!
//! Fork-only: upstream Hylki has no such hook.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::gio;
use suite_common_core::attachment_edit::{EditSession, Observation};

/// How long the file has to be quiet before it is read, and how long until
/// it is read again while the model says the save is still settling.
const SETTLE: Duration = Duration::from_millis(600);

type OnEdited = Rc<dyn Fn(PathBuf)>;

struct Watch {
    /// Kept alive here; dropping it stops the watch.
    _monitor: gio::FileMonitor,
    session: EditSession,
    /// A question about this copy is showing.
    asking: bool,
    on_edited: OnEdited,
}

thread_local! {
    static WATCHES: RefCell<HashMap<PathBuf, Watch>> = RefCell::new(HashMap::new());
}

/// Open `data`, named `name`, in the default application for editing, and
/// call `on_edited` with the staged copy's path each time the other app
/// finishes saving a new version. Returns the staged path, or `None` when it
/// could not be staged or watched (it is then opened, or not, as before).
pub(crate) fn open_and_watch(
    name: &str,
    data: &[u8],
    parent: Option<&gtk::Window>,
    on_edited: impl Fn(PathBuf) + 'static,
) -> Option<PathBuf> {
    let session = EditSession::start(name, data.to_vec());
    let staged = crate::ui::attachment_io::open_bytes_editable(session.file_name(), data, parent)?;
    let monitor = match gio::File::for_path(&staged)
        .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
    {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("cannot watch the opened attachment for saves: {e}");
            return None;
        }
    };
    // Each event restarts the wait; only the last one reads the file.
    let generation = Rc::new(Cell::new(0u64));
    let watched = staged.clone();
    monitor.connect_changed(move |_, _, _, event| {
        if matches!(
            event,
            gio::FileMonitorEvent::Deleted
                | gio::FileMonitorEvent::MovedOut
                | gio::FileMonitorEvent::PreUnmount
                | gio::FileMonitorEvent::Unmounted
        ) {
            return;
        }
        schedule(&generation, &watched);
    });
    WATCHES.with(|w| {
        w.borrow_mut().insert(
            staged.clone(),
            Watch {
                _monitor: monitor,
                session,
                asking: false,
                on_edited: Rc::new(on_edited),
            },
        )
    });
    Some(staged)
}

/// Read the copy once it has been quiet for [`SETTLE`]; a later call before
/// then replaces this one.
fn schedule(generation: &Rc<Cell<u64>>, staged: &Path) {
    let ticket = generation.get().wrapping_add(1);
    generation.set(ticket);
    let (generation, staged) = (generation.clone(), staged.to_path_buf());
    gtk::glib::timeout_add_local_once(SETTLE, move || {
        if generation.get() != ticket {
            return;
        }
        // Read again while settling: the model counts a change once it has
        // seen the same new bytes twice.
        if check(&staged) == Some(Observation::Settling) {
            schedule(&generation, &staged);
        }
    });
}

/// Read the staged copy, show it to the model, and report a finished edit.
fn check(staged: &Path) -> Option<Observation> {
    let data = std::fs::read(staged).ok()?;
    let (seen, on_edited) = WATCHES.with(|w| {
        let mut w = w.borrow_mut();
        let watch = w.get_mut(staged)?;
        if watch.asking {
            return None;
        }
        let seen = watch.session.observe(&data);
        let edited = seen == Observation::Edited;
        watch.asking = edited;
        Some((seen, edited.then(|| watch.on_edited.clone())))
    })?;
    // Called with WATCHES released: the callback may answer at once.
    if let Some(on_edited) = on_edited {
        on_edited(staged.to_path_buf());
    }
    Some(seen)
}

/// Stop watching `staged`: whoever wanted its edits is gone.
pub(crate) fn stop(staged: &Path) {
    WATCHES.with(|w| w.borrow_mut().remove(staged));
}

/// The person answered about the edit reported for `staged`: the edit becomes
/// the baseline either way (it is asked about again only if the file changes
/// once more), and its bytes come back when they said yes.
pub(crate) fn answer(staged: &Path, use_it: bool) -> Option<Vec<u8>> {
    WATCHES.with(|w| {
        let mut w = w.borrow_mut();
        let watch = w.get_mut(staged)?;
        watch.asking = false;
        let bytes = watch.session.replacement().map(<[u8]>::to_vec);
        if use_it {
            watch.session.accept();
        } else {
            watch.session.decline();
        }
        bytes.filter(|_| use_it)
    })
}
