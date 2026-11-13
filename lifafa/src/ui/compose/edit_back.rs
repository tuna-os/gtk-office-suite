//! Edit an attachment and put it back (INTEGRATION-PLAN A2).
//!
//! Opening a draft's attachment hands another application a staged copy
//! (`attachment_io::open_bytes_editable`). This watches that copy: when the
//! other application saves it, the composer asks whether to use the changed
//! file in the message, and if so replaces the attachment with a snapshot of
//! it as one undoable step. The person's own file, wherever the attachment
//! came from, is never written to, and nothing changes in the draft without
//! the answer.
//!
//! Fork-only: upstream Hylki has no such hook.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::gio;

use super::messages::ComposeInput;
use super::types::ComposeStep;
use super::Compose;
use super::ComposeWidgets;
use crate::i18n::{i18n, i18n_f, i18n_noop};
use crate::relm::{ComponentSender, Sender};

/// How long a save has to be quiet before it is read: an application that
/// writes in pieces, or writes a temporary file and renames it, has finished
/// by then.
const SETTLE: Duration = Duration::from_millis(600);

struct Watch {
    /// Kept alive here; dropping it stops the watch.
    _monitor: gio::FileMonitor,
    /// The attachment in the draft this copy stands for, which a put-back
    /// changes to the snapshot it attached.
    attached: PathBuf,
    /// A digest of the content last seen, so a save that changed nothing,
    /// or the same change seen twice, does not ask again.
    seen: u64,
    /// A question about this copy is showing.
    asking: bool,
}

thread_local! {
    static WATCHES: RefCell<HashMap<PathBuf, Watch>> = RefCell::new(HashMap::new());
}

fn digest(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut h);
    h.finish()
}

/// Whether a save of the staged copy is worth asking about: it has content
/// (an editor truncates before it writes) and that content is new.
pub(super) fn is_new_version(data: &[u8], seen: u64) -> bool {
    !data.is_empty() && digest(data) != seen
}

/// Open the attachment at `attached` for editing and watch the copy the
/// other application is given.
pub(super) fn open_and_watch(
    attached: PathBuf,
    name: &str,
    data: &[u8],
    parent: Option<&gtk::Window>,
    input: Sender<ComposeInput>,
) {
    let Some(staged) = crate::ui::attachment_io::open_bytes_editable(name, data, parent) else {
        return;
    };
    let monitor = match gio::File::for_path(&staged)
        .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
    {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("cannot watch the opened attachment for saves: {e}");
            return;
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
        let ticket = generation.get().wrapping_add(1);
        generation.set(ticket);
        let generation = generation.clone();
        let watched = watched.clone();
        let input = input.clone();
        gtk::glib::timeout_add_local_once(SETTLE, move || {
            if generation.get() == ticket {
                check(&watched, &input);
            }
        });
    });
    WATCHES.with(|w| {
        w.borrow_mut().insert(
            staged,
            Watch {
                _monitor: monitor,
                attached,
                seen: digest(data),
                asking: false,
            },
        )
    });
}

/// Read the staged copy and, if it changed, ask the composer.
fn check(staged: &Path, input: &Sender<ComposeInput>) {
    let Ok(data) = std::fs::read(staged) else {
        return;
    };
    let ask = WATCHES.with(|w| {
        let mut w = w.borrow_mut();
        let Some(watch) = w.get_mut(staged) else {
            return false;
        };
        if watch.asking || !is_new_version(&data, watch.seen) {
            return false;
        }
        watch.seen = digest(&data);
        watch.asking = true;
        true
    });
    if ask
        && input
            .send(ComposeInput::AttachmentEdited(staged.to_path_buf()))
            .is_err()
    {
        // The composer is gone: nothing left to put the file back into.
        WATCHES.with(|w| w.borrow_mut().remove(staged));
    }
}

fn answered(staged: &Path) {
    WATCHES.with(|w| {
        if let Some(watch) = w.borrow_mut().get_mut(staged) {
            watch.asking = false;
        }
    });
}

impl Compose {
    /// Handles [`ComposeInput::AttachmentEdited`]: ask whether to use the
    /// changed file in the message.
    pub(super) fn on_attachment_edited(
        &mut self,
        staged: PathBuf,
        sender: &ComponentSender<Self>,
        root: &adw::BreakpointBin,
    ) {
        let attached =
            WATCHES.with(|w| w.borrow().get(&staged).map(|watch| watch.attached.clone()));
        // Removed from the draft, or undone away, since it was opened.
        let Some(attached) = attached.filter(|a| self.attachments.contains(a)) else {
            answered(&staged);
            return;
        };
        let name = attached
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dialog = adw::AlertDialog::new(
            Some(&i18n_f("Use the changed “{name}”?", &[("name", &name)])),
            Some(&i18n(
                "It was saved in another app. The message still has the version attached before.",
            )),
        );
        dialog.add_response("keep", &i18n("Keep Attached Version"));
        dialog.add_response("use", &i18n("Use Changed Version"));
        dialog.set_response_appearance("use", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("use"));
        dialog.set_close_response("keep");
        let s = sender.input_sender().clone();
        dialog.connect_response(None, move |_, resp| {
            answered(&staged);
            if resp != "use" {
                return;
            }
            let (staged, attached, name, s) =
                (staged.clone(), attached.clone(), name.clone(), s.clone());
            gtk::glib::spawn_future_local(async move {
                // A snapshot, so later saves in the other application do not
                // change what the draft holds without asking again.
                let snapshot = gio::spawn_blocking(move || {
                    let data = std::fs::read(&staged).ok()?;
                    crate::ui::attachment_io::stage_bytes(&name, &data)
                })
                .await
                .ok()
                .flatten();
                match snapshot {
                    Some(snapshot) => {
                        let _ = s.send(ComposeInput::ReplaceAttachment {
                            from: attached,
                            to: snapshot,
                        });
                    }
                    None => tracing::warn!("could not take the changed attachment"),
                }
            });
        });
        dialog.present(Some(root));
    }

    /// Handles [`ComposeInput::ReplaceAttachment`]: swap the attachment for
    /// the changed version, as one step Undo takes back.
    pub(super) fn on_replace_attachment(
        &mut self,
        from: PathBuf,
        to: PathBuf,
        widgets: &ComposeWidgets,
        sender: &ComponentSender<Self>,
    ) {
        let Some(at) = self.attachments.iter().position(|p| *p == from) else {
            return;
        };
        self.fields_dirty = true;
        let what = super::attachments::attachment_label(i18n_noop("Update {name}"), &to, 1);
        self.attachments[at] = to.clone();
        // Later saves of the same copy now replace the snapshot just attached.
        WATCHES.with(|w| {
            for watch in w.borrow_mut().values_mut() {
                if watch.attached == from {
                    watch.attached = to.clone();
                }
            }
        });
        self.push_history(what, ComposeStep::Replace { at, path: from });
        self.rebuild_attachments(&widgets.attach_box, sender);
        self.report_history(sender);
    }
}

#[cfg(test)]
mod tests {
    use super::{digest, is_new_version};

    #[test]
    fn a_save_that_changed_the_file_is_offered() {
        assert!(is_new_version(b"edited", digest(b"original")));
    }

    #[test]
    fn a_save_that_changed_nothing_is_not() {
        assert!(!is_new_version(b"original", digest(b"original")));
    }

    #[test]
    fn a_file_caught_mid_save_is_not() {
        // Editors truncate before they write; an empty read is that moment.
        assert!(!is_new_version(b"", digest(b"original")));
    }
}
