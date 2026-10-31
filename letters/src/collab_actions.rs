// SPDX-License-Identifier: GPL-3.0-or-later
//
// collab_actions.rs — what the Collaborate feature adds to a Letters window
// (RFC-0005 5.1): three items in the main menu, the actions behind them, the
// header bar's status button, the two-versions banner, and the window's
// record of its sessions.
//
// Nothing here runs until a person acts: no sidecar is started, no connection
// is opened and no permission is asked for. The actions are registered and the
// widgets exist, hidden, in a build that has the feature.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use gtk4::prelude::*;
use gtk4::{self as gtk, gio, glib};
use letters_core::Document;
use libadwaita as adw;

use crate::collab_session::Session;
use crate::collab_status::{self, Status};

/// Builds a tab for a joined document and selects it; given by the window.
pub(crate) type OpenTab = Box<dyn Fn(&Document, &str) -> Option<gtk::TextBuffer>>;

/// The actions whose sensitivity follows the selected tab's session.
struct Actions {
    share: gio::SimpleAction,
    join: gio::SimpleAction,
    leave: gio::SimpleAction,
    copy_code: gio::SimpleAction,
    resolve: gio::SimpleAction,
}

/// One window's collaboration: its sessions, and the widgets they drive.
pub(crate) struct Host {
    window: glib::WeakRef<adw::ApplicationWindow>,
    tab_view: adw::TabView,
    toasts: adw::ToastOverlay,
    status: Status,
    banner: adw::Banner,
    /// Open a document as a new tab and select it: what a join shows. Given
    /// by the window, which owns how a tab is built.
    open: OpenTab,
    sessions: RefCell<Vec<Rc<Session>>>,
    actions: Actions,
}

thread_local! {
    /// Every window's host, so the save path (which has only a buffer) can
    /// find the session of the document it just saved.
    static HOSTS: RefCell<Vec<Weak<Host>>> = const { RefCell::new(Vec::new()) };
}

impl Host {
    /// The session of the tab whose buffer is `buf`, in whichever window.
    pub(crate) fn session_of(buf: &gtk::TextBuffer) -> Option<Rc<Session>> {
        let hosts: Vec<Rc<Host>> = HOSTS.with(|h| h.borrow().iter().filter_map(Weak::upgrade).collect());
        hosts.iter().find_map(|h| h.sessions.borrow().iter().find(|s| s.buffer().as_ref() == Some(buf)).cloned())
    }

    pub(crate) fn toast(&self, text: &str) {
        let toast = adw::Toast::new(text);
        toast.set_timeout(5);
        self.toasts.add_toast(toast);
    }

    pub(crate) fn alert(&self, heading: &str, body: &str) {
        suite_common::show_error_dialog(self.window.upgrade().as_ref(), heading, body);
    }

    pub(crate) fn window(&self) -> Option<adw::ApplicationWindow> {
        self.window.upgrade()
    }

    /// The tab title of the document in `buf`.
    pub(crate) fn title_of(&self, buf: &gtk::TextBuffer) -> Option<String> {
        (0..self.tab_view.n_pages())
            .map(|i| self.tab_view.nth_page(i))
            .find(|p| crate::page_container::buffer_of(&p.child()).as_ref() == Some(buf))
            .map(|p| p.title().to_string())
    }

    /// Open a shared document in a new tab.
    pub(crate) fn open_shared(&self, doc: &Document, title: &str) -> Option<gtk::TextBuffer> {
        (self.open)(doc, title)
    }

    /// A new session, remembered until it has nothing left to do.
    pub(crate) fn new_session(self: &Rc<Self>) -> Rc<Session> {
        let session = Session::new(self);
        self.sessions.borrow_mut().push(session.clone());
        session
    }

    /// The selected tab, and the live session it is in, if any.
    pub(crate) fn selected(&self) -> Option<(gtk::TextBuffer, Option<Rc<Session>>)> {
        let buf = crate::dialogs::active_buffer(&self.tab_view)?;
        let session = self.sessions.borrow().iter().find(|s| s.buffer().as_ref() == Some(&buf) && s.has_document()).cloned();
        Some((buf, session))
    }

    pub(crate) fn selected_session(&self) -> Option<Rc<Session>> {
        self.selected().and_then(|(_, s)| s)
    }

    /// Bring the window's widgets in line with the selected tab's session.
    pub(crate) fn refresh(&self) {
        self.sessions.borrow_mut().retain(|s| !s.is_idle());
        let (has_tab, session) = match self.selected() {
            Some((_, s)) => (true, s),
            None => (false, None),
        };
        let status = session.as_ref().and_then(|s| s.status());
        self.status.update(status.as_ref());
        let conflict = session.as_ref().is_some_and(|s| s.conflict());
        self.banner.set_revealed(conflict);
        let shared = session.is_some();
        self.actions.share.set_enabled(has_tab && !shared);
        self.actions.join.set_enabled(true);
        self.actions.leave.set_enabled(shared);
        self.actions.copy_code.set_enabled(session.as_ref().is_some_and(|s| s.invite().is_some()));
        self.actions.resolve.set_enabled(conflict);
    }

    /// End the sessions of the tab whose buffer is `buf` (it closed).
    fn tab_closed(&self, buf: &gtk::TextBuffer) {
        let closing: Vec<Rc<Session>> = self.sessions.borrow().iter().filter(|s| s.buffer().as_ref() == Some(buf)).cloned().collect();
        for session in closing {
            session.shutdown();
        }
    }

    fn shutdown_all(&self) {
        let all: Vec<Rc<Session>> = self.sessions.borrow().clone();
        for session in all {
            session.shutdown();
        }
    }
}

/// Add the Collaborate section to the main menu, the actions, the status
/// button and the banner to `window`. `add_banner` puts the banner above the
/// document; `open` builds a tab for a joined document.
pub(crate) fn install(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    tab_view: &adw::TabView,
    toasts: &adw::ToastOverlay,
    header: &adw::HeaderBar,
    add_banner: impl FnOnce(&adw::Banner),
    open: OpenTab,
) {
    use suite_common::{i18n, menus::section};
    if let Some(menu) = suite_common::menus::primary_menu(header.upcast_ref()) {
        // After Save and Print, before Preferences: unlabelled, like the others.
        menu.insert_section(
            1,
            None,
            &section(&[
                (&i18n("Share This Document…"), "app.collab-share"),
                (&i18n("Join a Shared Document…"), "app.collab-join"),
                (&i18n("Stop Sharing"), "app.collab-leave"),
            ]),
        );
    }
    suite_common::actions::register_labels(&[
        ("app.collab-share", &i18n("Share This Document…")),
        ("app.collab-join", &i18n("Join a Shared Document…")),
        ("app.collab-leave", &i18n("Stop Sharing")),
        ("app.collab-copy-code", &i18n("Copy Join Code")),
        ("app.collab-resolve", &i18n("Resolve Two Versions…")),
    ]);

    let status = Status::new();
    header.pack_end(&status.button);
    let banner = collab_status::banner();
    add_banner(&banner);

    let action = |name: &str, enabled: bool| {
        let a = gio::SimpleAction::new(name, None);
        a.set_enabled(enabled);
        app.add_action(&a);
        a
    };
    let actions = Actions {
        share: action("collab-share", false),
        join: action("collab-join", true),
        leave: action("collab-leave", false),
        copy_code: action("collab-copy-code", false),
        resolve: action("collab-resolve", false),
    };
    let host = Rc::new(Host {
        window: window.downgrade(),
        tab_view: tab_view.clone(),
        toasts: toasts.clone(),
        status,
        banner,
        open,
        sessions: RefCell::new(Vec::new()),
        actions,
    });
    HOSTS.with(|h| {
        let mut h = h.borrow_mut();
        h.retain(|w| w.upgrade().is_some());
        h.push(Rc::downgrade(&host));
    });

    {
        let h = host.clone();
        host.actions.share.connect_activate(move |_, _| crate::collab_dialogs::share(&h));
        let h = host.clone();
        host.actions.join.connect_activate(move |_, _| crate::collab_dialogs::join(&h));
        let h = host.clone();
        host.actions.leave.connect_activate(move |_, _| {
            if let Some(session) = h.selected_session() {
                session.leave();
            }
        });
        let h = host.clone();
        host.actions.copy_code.connect_activate(move |_, _| {
            let code = h.selected_session().and_then(|s| s.invite());
            if let (Some(code), Some(window)) = (code, h.window()) {
                window.clipboard().set_text(&code);
                h.toast(&suite_common::i18n("Join code copied"));
            }
        });
        let h = host.clone();
        host.actions.resolve.connect_activate(move |_, _| crate::collab_dialogs::resolve(&h));
    }
    {
        let h = host.clone();
        tab_view.connect_selected_page_notify(move |_| h.refresh());
        let h = host.clone();
        tab_view.connect_page_detached(move |_, page, _| {
            if let Some(buf) = crate::page_container::buffer_of(&page.child()) {
                h.tab_closed(&buf);
            }
        });
        let h = host.clone();
        window.connect_destroy(move |_| h.shutdown_all());
    }
    host.refresh();
}
