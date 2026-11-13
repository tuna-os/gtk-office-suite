//! Edit an attachment and put it back (INTEGRATION-PLAN A2): the shell half.
//!
//! Opening a draft's attachment hands another application a staged copy
//! (`attachment_io::open_bytes_editable`). This watches that copy and feeds
//! each save to the suite's GTK-free model,
//! `suite_common_core::attachment_edit::EditSession`, which decides when a
//! change is a finished edit (the same new bytes seen twice, since editors
//! save in steps). For a finished edit the composer asks whether to use the
//! changed file in the message; if so it attaches a snapshot of it in place
//! of the old one, as one undoable step. The person's own file, wherever the
//! attachment came from, is never written to, and nothing in the draft
//! changes without the answer.
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

use super::messages::ComposeInput;
use super::types::ComposeStep;
use super::Compose;
use super::ComposeWidgets;
use crate::i18n::{i18n, i18n_f, i18n_noop};
use crate::relm::{ComponentSender, Sender};

/// How long the file has to be quiet before it is read, and how long until
/// it is read again while the model says the save is still settling.
const SETTLE: Duration = Duration::from_millis(600);

struct Watch {
    /// Kept alive here; dropping it stops the watch.
    _monitor: gio::FileMonitor,
    /// The attachment in the draft this copy stands for, which a put-back
    /// changes to the snapshot it attached.
    attached: PathBuf,
    session: EditSession,
    /// A question about this copy is showing.
    asking: bool,
}

thread_local! {
    static WATCHES: RefCell<HashMap<PathBuf, Watch>> = RefCell::new(HashMap::new());
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
    let session = EditSession::start(name, data.to_vec());
    let Some(staged) =
        crate::ui::attachment_io::open_bytes_editable(session.file_name(), data, parent)
    else {
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
        schedule(&generation, &watched, &input);
    });
    WATCHES.with(|w| {
        w.borrow_mut().insert(
            staged,
            Watch {
                _monitor: monitor,
                attached,
                session,
                asking: false,
            },
        )
    });
}

/// Read the copy once it has been quiet for [`SETTLE`]; a later call before
/// then replaces this one.
fn schedule(generation: &Rc<Cell<u64>>, staged: &Path, input: &Sender<ComposeInput>) {
    let ticket = generation.get().wrapping_add(1);
    generation.set(ticket);
    let (generation, staged, input) = (generation.clone(), staged.to_path_buf(), input.clone());
    gtk::glib::timeout_add_local_once(SETTLE, move || {
        if generation.get() != ticket {
            return;
        }
        // Read again while settling: the model counts a change once it has
        // seen the same new bytes twice.
        if check(&staged, &input) == Some(Observation::Settling) {
            schedule(&generation, &staged, &input);
        }
    });
}

/// Read the staged copy, show it to the model, and ask the composer about a
/// finished edit. Returns what the model made of it.
fn check(staged: &Path, input: &Sender<ComposeInput>) -> Option<Observation> {
    let data = std::fs::read(staged).ok()?;
    let (seen, ask) = WATCHES.with(|w| {
        let mut w = w.borrow_mut();
        let watch = w.get_mut(staged)?;
        if watch.asking {
            return None;
        }
        let seen = watch.session.observe(&data);
        let ask = seen == Observation::Edited;
        watch.asking = ask;
        Some((seen, ask))
    })?;
    if ask
        && input
            .send(ComposeInput::AttachmentEdited(staged.to_path_buf()))
            .is_err()
    {
        // The composer is gone: nothing left to put the file back into.
        WATCHES.with(|w| w.borrow_mut().remove(staged));
    }
    Some(seen)
}

/// The person answered: take the edit as the new baseline either way (the
/// model asks again only if the file changes once more), and hand back the
/// edited bytes when they said yes.
fn answered(staged: &Path, use_it: bool) -> Option<Vec<u8>> {
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
            answered(&staged, false);
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
            let Some(data) = answered(&staged, resp == "use") else {
                return;
            };
            let (attached, name, s) = (attached.clone(), name.clone(), s.clone());
            gtk::glib::spawn_future_local(async move {
                // A snapshot, so later saves in the other application do not
                // change what the draft holds without asking again.
                let snapshot = gio::spawn_blocking(move || {
                    crate::ui::attachment_io::stage(&name, &data)
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
