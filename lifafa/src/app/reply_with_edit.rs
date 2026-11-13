//! Reply with an attachment edited in another app (INTEGRATION-PLAN A2, the
//! reader's half; the composer's is `ui::compose::edit_back`).
//!
//! Opening a received message's attachment in the reader hands the other app
//! an editable copy (`ui::edit_watch`). When that app finishes saving a new
//! version, Lifafa asks whether to reply with it; if so, a snapshot of the
//! edited file goes into a reply to that message, through the same path a
//! file handed in from another app takes ("Reply to a message" in the
//! hand-off: `AppMsg::HandOffReply`, with its size check). The received
//! message is never changed.
//!
//! Fork-only: upstream Hylki has no such hook.

use std::path::PathBuf;

use crate::relm::prelude::*;
use adw::prelude::*;

use crate::i18n::{i18n, i18n_f};
use crate::models::Attachment;

use super::hand_off::FileHandOff;
use super::model::AppModel;
use super::msg::AppMsg;

impl AppModel {
    /// Open a received attachment for editing, watching it so an edit can
    /// become a reply. Falls back to opening it as before when it cannot be
    /// watched.
    pub(super) fn open_for_reply(
        &self,
        account_id: u32,
        id: u32,
        att: &Attachment,
        sender: &ComponentSender<Self>,
    ) {
        let s = sender.clone();
        let watched = crate::ui::edit_watch::open_and_watch(
            &att.name,
            &att.data,
            Some(self.window.upcast_ref::<gtk::Window>()),
            move |staged| {
                s.input(AppMsg::AttachmentEditedForReply {
                    staged,
                    account_id,
                    id,
                });
            },
        );
        if watched.is_none() {
            crate::ui::attachment_io::open_bytes(
                &att.name,
                &att.data,
                Some(self.window.upcast_ref::<gtk::Window>()),
            );
        }
    }

    /// Handles [`AppMsg::AttachmentEditedForReply`]: ask whether to reply
    /// with the changed file.
    pub(super) fn on_attachment_edited_for_reply(
        &mut self,
        msg: AppMsg,
        sender: ComponentSender<Self>,
    ) {
        let AppMsg::AttachmentEditedForReply {
            staged,
            account_id,
            id,
        } = msg
        else {
            unreachable!("not routed to on_attachment_edited_for_reply");
        };
        // The message may have been deleted or moved since it was opened.
        let Some(message) = self.shown_copy(account_id, id) else {
            crate::ui::edit_watch::answer(&staged, false);
            return;
        };
        let name = staged
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dialog = adw::AlertDialog::new(
            Some(&i18n_f("Reply with the changed “{name}”?", &[("name", &name)])),
            Some(&i18n(
                "It was saved in another app. A reply to the message it came with opens with the changed file attached; the message itself is not changed.",
            )),
        );
        dialog.add_response("cancel", &i18n("Not Now"));
        dialog.add_response("reply", &i18n("Reply With Changed File"));
        dialog.set_response_appearance("reply", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("reply"));
        dialog.set_close_response("cancel");
        dialog.connect_response(None, move |_, resp| {
            let Some(data) = crate::ui::edit_watch::answer(&staged, resp == "reply") else {
                return;
            };
            let (name, message, s) = (name.clone(), message.clone(), sender.clone());
            gtk::glib::spawn_future_local(async move {
                // A snapshot, so the reply holds this version even if the
                // other app saves again.
                let snapshot: Option<PathBuf> = gtk::gio::spawn_blocking(move || {
                    crate::ui::attachment_io::stage_bytes(&name, &data)
                })
                .await
                .ok()
                .flatten();
                match snapshot {
                    Some(path) => s.input(AppMsg::HandOffReply {
                        hand_off: FileHandOff {
                            files: vec![path],
                            ..Default::default()
                        },
                        message: Some(message),
                    }),
                    None => tracing::warn!("could not take the changed attachment"),
                }
            });
        });
        dialog.present(Some(&self.window));
    }
}
