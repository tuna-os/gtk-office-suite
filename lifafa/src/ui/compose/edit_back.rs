//! Edit an attachment and put it back (INTEGRATION-PLAN A2): the shell half.
//!
//! Opening a draft's attachment hands another application a staged copy and
//! watches it (`ui::edit_watch`, which decides when a save is a finished
//! edit). For a finished edit the composer asks whether to use the
//! changed file in the message; if so it attaches a snapshot of it in place
//! of the old one, as one undoable step. The person's own file, wherever the
//! attachment came from, is never written to, and nothing in the draft
//! changes without the answer.
//!
//! Fork-only: upstream Hylki has no such hook.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use adw::prelude::*;
use gtk::gio;

use super::messages::ComposeInput;
use super::types::ComposeStep;
use super::Compose;
use super::ComposeWidgets;
use crate::i18n::{i18n, i18n_f, i18n_noop};
use crate::relm::{ComponentSender, Sender};
use crate::ui::edit_watch;

thread_local! {
    /// Each watched copy (`edit_watch`) and the attachment in the draft it
    /// stands for, which a put-back changes to the snapshot it attached.
    static ATTACHED: RefCell<HashMap<PathBuf, PathBuf>> = RefCell::new(HashMap::new());
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
    let staged = edit_watch::open_and_watch(name, data, parent, move |staged| {
        if input.send(ComposeInput::AttachmentEdited(staged.clone())).is_err() {
            // The composer is gone: nothing left to put the file back into.
            edit_watch::stop(&staged);
            ATTACHED.with(|a| a.borrow_mut().remove(&staged));
        }
    });
    if let Some(staged) = staged {
        ATTACHED.with(|a| a.borrow_mut().insert(staged, attached));
    }
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
        let attached = ATTACHED.with(|a| a.borrow().get(&staged).cloned());
        // Removed from the draft, or undone away, since it was opened.
        let Some(attached) = attached.filter(|a| self.attachments.contains(a)) else {
            edit_watch::answer(&staged, false);
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
            let Some(data) = edit_watch::answer(&staged, resp == "use") else {
                return;
            };
            let (attached, name, s) = (attached.clone(), name.clone(), s.clone());
            gtk::glib::spawn_future_local(async move {
                // A snapshot, so later saves in the other application do not
                // change what the draft holds without asking again.
                let snapshot = gio::spawn_blocking(move || {
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
        ATTACHED.with(|a| {
            for attached in a.borrow_mut().values_mut() {
                if *attached == from {
                    *attached = to.clone();
                }
            }
        });
        self.push_history(what, ComposeStep::Replace { at, path: from });
        self.rebuild_attachments(&widgets.attach_box, sender);
        self.report_history(sender);
    }
}
