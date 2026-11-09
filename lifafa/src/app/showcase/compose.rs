//! Showcase hooks for composing and the app's look: Focus Mode, special
//! folders, replies, new messages and forwards, the composer's menus and
//! formats, handed-in files, the light/dark flip and themes.

use relm4::prelude::*;

use crate::app::{AppModel, AppMsg};
use crate::config;
use crate::models::FolderKind;
use crate::ui::preferences::PrefOutput;

use super::{input_after, showcase_at, showcase_log_focus};

/// HYLKI_SHOWCASE_FOCUS=<seconds> switches Focus Mode on at
/// that moment (its parts as saved in focus.toml), so a
/// capture a little later catches the slide, and one later
/// still the settled layout. HYLKI_SHOWCASE_FOCUS_OFF=<seconds>
/// switches it off again.
pub(super) fn stage_focus(sender: &ComponentSender<AppModel>) {
    for (var, on) in [
        ("HYLKI_SHOWCASE_FOCUS", true),
        ("HYLKI_SHOWCASE_FOCUS_OFF", false),
    ] {
        if let Some(at) = std::env::var(var).ok().and_then(|v| v.parse::<f64>().ok()) {
            let s = sender.clone();
            gtk::glib::timeout_add_local_once(
                std::time::Duration::from_millis((at * 1000.0) as u64),
                move || {
                    let mut focus = config::load_focus_mode();
                    focus.enabled = on;
                    s.input(AppMsg::Pref(PrefOutput::SetFocusMode(focus)));
                },
            );
        }
    }
}

/// HYLKI_SHOWCASE_FOLDER=drafts|templates|sent|archive|junk|trash|outbox[@<s>]
/// switches to that folder at 2 s, before the staging's 3 s
/// selection moves onto its first row, or at the time given.
pub(super) fn stage_folder(sender: &ComponentSender<AppModel>) {
    if let Ok(v) = std::env::var("HYLKI_SHOWCASE_FOLDER") {
        let (kind, at) = showcase_at(&v, 2);
        if kind == "outbox" {
            input_after(sender, at, AppMsg::ShowOutbox);
        }
        let kind = match kind {
            "drafts" => Some(FolderKind::Drafts),
            "templates" => Some(FolderKind::Templates),
            "sent" => Some(FolderKind::Sent),
            "archive" => Some(FolderKind::Archive),
            "junk" => Some(FolderKind::Junk),
            "trash" => Some(FolderKind::Trash),
            _ => None,
        };
        if let Some(kind) = kind {
            let s = sender.clone();
            gtk::glib::timeout_add_seconds_local_once(at, move || {
                s.input(AppMsg::ShowcaseFolder {
                    kind,
                    account: None,
                });
            });
        }
    }
}

/// HYLKI_SHOWCASE_SEND=<seconds>[:<text>|:@<file>] writes in the
/// reply's editor and sends it at that moment, for the send
/// morph.
pub(super) fn stage_send(sender: &ComponentSender<AppModel>) {
    if let Ok(v) = std::env::var("HYLKI_SHOWCASE_SEND") {
        let (at, text) = v
            .split_once(':')
            .unwrap_or((v.as_str(), "Sounds good, ship it.\nJason"));
        let at = at.parse::<f64>().unwrap_or(9.0);
        // `@<path>` reads the text from a file.
        let text = match text.strip_prefix('@') {
            Some(path) => std::fs::read_to_string(path).unwrap_or_default(),
            None => text.replace("\\n", "\n"),
        };
        let s = sender.clone();
        gtk::glib::timeout_add_local_once(
            std::time::Duration::from_millis((at * 1000.0) as u64),
            move || {
                s.input(AppMsg::ShowcaseSend(text));
            },
        );
    }
}

/// HYLKI_SHOWCASE_UNDO=<seconds> presses Undo on the first
/// message sent, to check it comes back where it was (#434).
pub(super) fn stage_undo(sender: &ComponentSender<AppModel>) {
    if let Some(at) = std::env::var("HYLKI_SHOWCASE_UNDO")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
    {
        let s = sender.clone();
        gtk::glib::timeout_add_local_once(
            std::time::Duration::from_millis((at * 1000.0) as u64),
            move || {
                s.input(AppMsg::UndoSend(1));
            },
        );
    }
}

/// HYLKI_SHOWCASE_COPY=<seconds> has the sent reply's copy
/// read back then, so its stand-in card can be seen to give
/// way to it.
pub(super) fn stage_copy(sender: &ComponentSender<AppModel>) {
    if let Some(at) = std::env::var("HYLKI_SHOWCASE_COPY")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
    {
        let s = sender.clone();
        gtk::glib::timeout_add_local_once(
            std::time::Duration::from_millis((at * 1000.0) as u64),
            move || {
                s.input(AppMsg::ShowcaseCopy);
            },
        );
    }
}

/// HYLKI_SHOWCASE_REPLY opens the inline reply composer on the
/// selected message, to check the composer's grounds (#148).
/// Two seconds on, it logs where focus landed (#266).
pub(super) fn stage_reply(root: &adw::ApplicationWindow, sender: &ComponentSender<AppModel>) {
    if let Ok(v) = std::env::var("HYLKI_SHOWCASE_REPLY") {
        // A number is the moment, in seconds (4 otherwise).
        let at = v.parse::<u32>().unwrap_or(4);
        let s = sender.clone();
        let window = root.clone();
        gtk::glib::timeout_add_seconds_local_once(at, move || {
            s.input(AppMsg::Reply);
            // After the split's slide, which focus waits for.
            gtk::glib::timeout_add_seconds_local_once(2, move || showcase_log_focus(&window));
        });
    }
}

/// HYLKI_SHOWCASE_NEW=<seconds>[:<address>] opens a new message
/// at that moment (4 s unless it parses), addressed when an
/// address follows, and, a second later, logs
/// where keyboard focus landed: the field row's title, or the
/// focused widget's type (#266).
pub(super) fn stage_new(root: &adw::ApplicationWindow, sender: &ComponentSender<AppModel>) {
    if let Ok(v) = std::env::var("HYLKI_SHOWCASE_NEW") {
        let (at, to) = v.split_once(':').unwrap_or((v.as_str(), ""));
        let at = at.parse::<u32>().unwrap_or(4);
        let to = to.to_string();
        let s = sender.clone();
        let window = root.clone();
        gtk::glib::timeout_add_seconds_local_once(at, move || {
            s.input(if to.is_empty() {
                AppMsg::Compose
            } else {
                AppMsg::ComposeTo(to)
            });
            gtk::glib::timeout_add_seconds_local_once(1, move || showcase_log_focus(&window));
        });
    }
}

/// HYLKI_SHOWCASE_FORWARD=<seconds> does the same with
/// Forward at that moment (4 s unless it parses), to check
/// the original's attachments come along (#240). As
/// <seconds>:<account>:<id> it forwards that message of the
/// open conversation, the way its card's button would, so a
/// message that is not the conversation's newest can be
/// picked.
pub(super) fn stage_forward(sender: &ComponentSender<AppModel>) {
    if let Ok(v) = std::env::var("HYLKI_SHOWCASE_FORWARD") {
        let mut parts = v.split(':');
        let at = parts
            .next()
            .and_then(|p| p.parse::<u32>().ok())
            .unwrap_or(4);
        let target = parts
            .next()
            .zip(parts.next())
            .and_then(|(a, id)| Some((a.parse::<u32>().ok()?, id.parse::<u32>().ok()?)));
        let s = sender.clone();
        gtk::glib::timeout_add_seconds_local_once(at, move || match target {
            Some((account_id, id)) => s.input(AppMsg::ShowcaseForward { account_id, id }),
            None => s.input(AppMsg::Forward),
        });
    }
}

/// HYLKI_SHOWCASE_COMPOSE_MENU=format opens the inline
/// composer's format chooser; any other value opens its
/// overflow menu. Pair either with HYLKI_SHOWCASE_MENU=main.
pub(super) fn stage_compose_menu(sender: &ComponentSender<AppModel>) {
    if let Ok(which) = std::env::var("HYLKI_SHOWCASE_COMPOSE_MENU") {
        let s = sender.clone();
        let format = which == "format";
        gtk::glib::timeout_add_seconds_local_once(6, move || {
            s.input(AppMsg::ShowcaseComposeMenu { format });
        });
    }
}

/// HYLKI_SHOWCASE_COMPOSE_PREVIEW=1 turns the inline
/// composer's preview on a beat after it opens, so a
/// capture can show the rendered message rather than the
/// source it was written in.
pub(super) fn stage_compose_preview(sender: &ComponentSender<AppModel>) {
    if std::env::var("HYLKI_SHOWCASE_COMPOSE_PREVIEW").is_ok() {
        input_after(sender, 6, AppMsg::ShowcaseComposePreview);
    }
}

/// HYLKI_SHOWCASE_COMPOSE_FROM=N picks the inline composer's
/// From entry N at 7 s, to check where the new account's
/// signature lands (#237).
pub(super) fn stage_compose_from(sender: &ComponentSender<AppModel>) {
    if let Some(Ok(n)) = std::env::var("HYLKI_SHOWCASE_COMPOSE_FROM")
        .ok()
        .map(|v| v.parse::<u32>())
    {
        input_after(sender, 7, AppMsg::ShowcaseComposeFrom(n));
    }
}

/// HYLKI_SHOWCASE_COMPOSE_CLOSE=N cancels the inline composer
/// N seconds in (pair with HYLKI_SHOWCASE_REPLY), to check
/// that a closed composer's web view, and so its web process,
/// actually goes away (#221).
pub(super) fn stage_compose_close(sender: &ComponentSender<AppModel>) {
    if let Some(at) = std::env::var("HYLKI_SHOWCASE_COMPOSE_CLOSE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    {
        input_after(sender, at, AppMsg::ShowcaseComposeClose);
    }
}

/// HYLKI_SHOWCASE_COMPOSE_FORMAT=plain@6,rich@10 switches the
/// inline composer's format at those moments (#424). Pair
/// with HYLKI_SHOWCASE_REPLY.
pub(super) fn stage_compose_format(sender: &ComponentSender<AppModel>) {
    if let Ok(steps) = std::env::var("HYLKI_SHOWCASE_COMPOSE_FORMAT") {
        for step in steps.split(',') {
            let (name, at) = showcase_at(step, 6);
            let format = match name {
                "rich" => config::ComposeFormat::Rich,
                "markdown" => config::ComposeFormat::Markdown,
                "html" => config::ComposeFormat::Html,
                "plain" => config::ComposeFormat::Plain,
                _ => continue,
            };
            input_after(sender, at, AppMsg::ShowcaseComposeFormat(format));
        }
    }
}

/// HYLKI_SHOWCASE_COMPOSE_UNDO=1 runs the composer's history
/// (#200) through a scripted round of edits and undos on the
/// `hylki::compose::undo` log target: the one way to exercise
/// it without a keyboard, since the keys cannot be injected
/// on this desktop. Pair with HYLKI_SHOWCASE_REPLY.
/// Set it to markdown, html or plain to walk a source-mode
/// body (a textarea) instead of the rich document, to
/// `backspace` to trim text rather than type it, or to
/// `pause` to leave a long gap between two runs of typing.
pub(super) fn stage_compose_undo(sender: &ComponentSender<AppModel>) {
    if let Ok(which) = std::env::var("HYLKI_SHOWCASE_COMPOSE_UNDO") {
        let format = match which.as_str() {
            "markdown" => Some(config::ComposeFormat::Markdown),
            "html" => Some(config::ComposeFormat::Html),
            "plain" => Some(config::ComposeFormat::Plain),
            _ => None,
        };
        if let Some(format) = format {
            input_after(sender, 5, AppMsg::ShowcaseComposeFormat(format));
        }
        input_after(sender, 7, AppMsg::ShowcaseComposeUndo);
        // …and drop the burger menu open over it near the end,
        // which is the state the entries are actually read in.
        input_after(sender, 16, AppMsg::ShowcaseBurger);
    }
}

/// HYLKI_SHOWCASE_FILES=/a:/b hands those files in at 4 s,
/// as GNOME Files' "Send with Hylki" would; with
/// HYLKI_SHOWCASE_TOP=1 the capture takes the newest window
/// (the dialog asking about them) instead of the main one.
pub(super) fn stage_files(sender: &ComponentSender<AppModel>) {
    if let Ok(list) = std::env::var("HYLKI_SHOWCASE_FILES") {
        let s = sender.clone();
        let paths: Vec<std::path::PathBuf> = list
            .split(':')
            .filter(|p| !p.is_empty())
            .map(Into::into)
            .collect();
        gtk::glib::timeout_add_seconds_local_once(4, move || {
            s.input(AppMsg::OpenWithFiles(paths));
        });
    }
}

/// HYLKI_SHOWCASE_FLIP=dark|light switches the app theme
/// at 6 s, to check a live flip re-resolves those grounds.
pub(super) fn stage_flip(sender: &ComponentSender<AppModel>) {
    if let Ok(flip) = std::env::var("HYLKI_SHOWCASE_FLIP") {
        input_after(
            sender,
            6,
            AppMsg::Pref(PrefOutput::SetAppTheme(if flip == "dark" {
                config::AppTheme::Dark
            } else {
                config::AppTheme::Light
            })),
        );
    }
}

/// HYLKI_SHOWCASE_THEME=<id> picks that appearance theme at
/// 6 s, exactly as the Settings gallery does, so the live
/// repaint (chrome, reader and composer) can be captured.
pub(super) fn stage_theme(sender: &ComponentSender<AppModel>) {
    if let Ok(id) = std::env::var("HYLKI_SHOWCASE_THEME") {
        input_after(sender, 6, AppMsg::Pref(PrefOutput::SetTheme(id)));
    }
}
