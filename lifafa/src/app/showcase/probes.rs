//! Probes that work on real profiles too (not demo-gated): memory reports,
//! zoom, deleting an attachment, opening, translating and deleting
//! messages, switching to an Inbox, list search and the new-mail
//! notification.

use relm4::prelude::*;

use crate::app::model::AppModel;
use crate::app::msg::AppMsg;
use crate::models::FolderKind;
use crate::ui::message_list::MessageListInput;
use crate::ui::message_view::MessageViewInput;

use super::capture::{input_after, showcase_at};

/// HYLKI_SHOWCASE_MEMORY=/path.txt writes the exported log (memory
/// section included) there after HYLKI_SHOWCASE_MEMORY_AT seconds
/// (default 20), on a real mailbox as much as the demo: a memory
/// reading is only worth having once the index has loaded. With
/// HYLKI_SHOWCASE_MEMORY_EVERY=N it is rewritten every N seconds
/// after that, for watching a session grow.
pub(super) fn probe_memory(sender: &ComponentSender<AppModel>) {
    if let Some(path) = std::env::var_os("HYLKI_SHOWCASE_MEMORY") {
        let secs = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u32>().ok());
        let at = secs("HYLKI_SHOWCASE_MEMORY_AT").unwrap_or(20);
        let every = secs("HYLKI_SHOWCASE_MEMORY_EVERY").filter(|n| *n > 0);
        let s = sender.clone();
        let path = std::path::PathBuf::from(path);
        gtk::glib::timeout_add_seconds_local_once(at, move || {
            s.input(AppMsg::ExportLogTo(path.clone()));
            if let Some(every) = every {
                gtk::glib::timeout_add_seconds_local(every, move || {
                    s.input(AppMsg::ExportLogTo(path.clone()));
                    gtk::glib::ControlFlow::Continue
                });
            }
        });
    }
}

/// HYLKI_SHOWCASE_ZOOM=N presses Ctrl+ N times (Ctrl- for a negative
/// N) at 8 s, so the live zoom path can be captured.
pub(super) fn probe_zoom(sender: &ComponentSender<AppModel>) {
    if let Some(n) = std::env::var("HYLKI_SHOWCASE_ZOOM")
        .ok()
        .and_then(|v| v.parse::<i8>().ok())
    {
        let s = sender.clone();
        gtk::glib::timeout_add_seconds_local_once(8, move || {
            for _ in 0..n.unsigned_abs() {
                s.input(AppMsg::ZoomMessage(n.signum()));
            }
        });
    }
}

/// HYLKI_SHOWCASE_DELETE_ATTACHMENT=<name>[:yes] chooses Delete from
/// Server for that file of the message on screen at 10 s
/// (HYLKI_SHOWCASE_DELETE_AT overrides the seconds); `:yes` answers
/// the question as well (#289).
/// `gallery:<name>` opens the attachments gallery 4 s before and
/// deletes the file from there.
pub(super) fn probe_delete_attachment(sender: &ComponentSender<AppModel>) {
    if let Ok(v) = std::env::var("HYLKI_SHOWCASE_DELETE_ATTACHMENT") {
        let (name, confirmed) = match v.strip_suffix(":yes") {
            Some(name) => (name.to_string(), true),
            None => (v, false),
        };
        let at: u32 = std::env::var("HYLKI_SHOWCASE_DELETE_AT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        if name.starts_with("gallery:") {
            input_after(sender, at.saturating_sub(4), AppMsg::ShowAttachments);
        }
        let s = sender.clone();
        gtk::glib::timeout_add_seconds_local_once(at, move || {
            s.input(AppMsg::ShowcaseDeleteAttachment { name, confirmed });
        });
    }
}

impl AppModel {
    /// HYLKI_SHOWCASE_SELECT=<account>:<id> opens that message at 5 s
    /// (HYLKI_SHOWCASE_SELECT_AT overrides the seconds) through its row,
    /// or the head of its conversation, real accounts included: a way to
    /// open a conversation the reader assembles from the cache without a
    /// pointer, so its log can be read (#236).
    /// Several may be given, comma-separated, each with its own
    /// @<seconds>, to open one message and then another.
    pub(super) fn probe_select(&self) {
        if let Ok(v) = std::env::var("HYLKI_SHOWCASE_SELECT") {
            let default_at: u32 = std::env::var("HYLKI_SHOWCASE_SELECT_AT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5);
            for item in v.split(',') {
                let (item, at) = showcase_at(item, default_at);
                let Some((a, id)) = item
                    .split_once(':')
                    .and_then(|(a, id)| Some((a.parse::<u32>().ok()?, id.parse::<u32>().ok()?)))
                else {
                    continue;
                };
                let list = self.message_list.sender().clone();
                gtk::glib::timeout_add_seconds_local_once(at, move || {
                    let _ = list.send(MessageListInput::SelectAndLoad((a, id)));
                });
            }
        }
    }

    /// HYLKI_SHOWCASE_TRANSLATE=<account>:<id>[@<seconds>] translates
    /// that message (open by then) at 8 s, as its A文 button would, so a
    /// translation can be captured (#327).
    pub(super) fn probe_translate(&self) {
        if let Ok(v) = std::env::var("HYLKI_SHOWCASE_TRANSLATE") {
            let (item, at) = showcase_at(&v, 8);
            if let Some((a, id)) = item
                .split_once(':')
                .and_then(|(a, id)| Some((a.parse::<u32>().ok()?, id.parse::<u32>().ok()?)))
            {
                let view = self.message_view.sender().clone();
                gtk::glib::timeout_add_seconds_local_once(at, move || {
                    let _ = view.send(MessageViewInput::Translate { account_id: a, id });
                });
            }
        }
    }

    /// HYLKI_SHOWCASE_CARD_DELETE=<account>:<id>[@<seconds>] deletes that
    /// card of the open conversation at 8 s, as its Delete would. Several
    /// may be given, comma-separated.
    pub(super) fn probe_card_delete(&self) {
        if let Ok(v) = std::env::var("HYLKI_SHOWCASE_CARD_DELETE") {
            for item in v.split(',') {
                let (item, at) = showcase_at(item, 8);
                let Some((a, id)) = item
                    .split_once(':')
                    .and_then(|(a, id)| Some((a.parse::<u32>().ok()?, id.parse::<u32>().ok()?)))
                else {
                    continue;
                };
                let view = self.message_view.sender().clone();
                gtk::glib::timeout_add_seconds_local_once(at, move || {
                    let _ = view.send(MessageViewInput::CardAction {
                        action: crate::ui::message_list::RowAction::Delete,
                        account_id: a,
                        id,
                    });
                });
            }
        }
    }
}

/// HYLKI_SHOWCASE_INBOX=<account>[@<seconds>] switches to that
/// account's Inbox at 3 s or the time given (the first account's when
/// it is not a number), real accounts included, for the same probe
/// from a folder view.
pub(super) fn probe_inbox(sender: &ComponentSender<AppModel>) {
    if let Ok(v) = std::env::var("HYLKI_SHOWCASE_INBOX") {
        let (v, at) = showcase_at(&v, 3);
        let account = v.parse::<u32>().ok().filter(|a| *a > 0);
        let s = sender.clone();
        gtk::glib::timeout_add_seconds_local_once(at, move || {
            s.input(AppMsg::ShowcaseFolder {
                kind: FolderKind::Inbox,
                account,
            });
        });
    }
}

impl AppModel {
    /// HYLKI_SHOWCASE_LIST_SEARCH=<text>[:<scope>][@<seconds>] searches
    /// the list at 4 s or the time given, the scope by its place in the
    /// dropdown, real accounts included (#472).
    pub(super) fn probe_list_search(&self) {
        if let Ok(spec) = std::env::var("HYLKI_SHOWCASE_LIST_SEARCH") {
            let (spec, at) = showcase_at(&spec, 4);
            let (text, scope) = match spec.rsplit_once(':') {
                Some((t, s)) => (t.to_string(), s.parse().unwrap_or(0)),
                None => (spec.to_string(), 0),
            };
            let list = self.message_list.sender().clone();
            gtk::glib::timeout_add_seconds_local_once(at, move || {
                let _ = list.send(MessageListInput::ShowcaseSearch(text, scope));
            });
        }
    }
}

/// HYLKI_SHOWCASE_NOTIFY[=<seconds>[,<seconds>...]] posts the new-mail
/// notification at 5 s or at each time given (the same second twice
/// exercises the sound's hold-off), real accounts included.
pub(super) fn probe_notify(sender: &ComponentSender<AppModel>) {
    if let Ok(v) = std::env::var("HYLKI_SHOWCASE_NOTIFY") {
        let mut times: Vec<u32> = v.split(',').filter_map(|t| t.trim().parse().ok()).collect();
        if times.is_empty() {
            times.push(5);
        }
        for at in times {
            input_after(sender, at, AppMsg::ShowcaseNotify);
        }
    }
}
