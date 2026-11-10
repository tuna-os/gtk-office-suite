use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use super::ToastableDialog;
use crate::{
    Window,
    components::{Avatar, LoadingButton},
    i18n::ngettext_f,
    prelude::*,
    session::{RemoteRoom, Session},
    toast,
    utils::{
        LoadingState,
        matrix::{MatrixIdUri, MatrixRoomIdUri},
    },
};

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/dialogs/room_preview.ui")]
    #[properties(wrapper_type = super::RoomPreviewDialog)]
    pub struct RoomPreviewDialog {
        #[template_child]
        go_back_btn: TemplateChild<gtk::Button>,
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        entry_page: TemplateChild<gtk::Box>,
        #[template_child]
        search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        look_up_btn: TemplateChild<LoadingButton>,
        #[template_child]
        room_avatar: TemplateChild<Avatar>,
        #[template_child]
        room_name: TemplateChild<gtk::Label>,
        #[template_child]
        room_alias: TemplateChild<gtk::Label>,
        #[template_child]
        room_topic: TemplateChild<gtk::Label>,
        #[template_child]
        room_members_box: TemplateChild<gtk::Box>,
        #[template_child]
        room_members_count: TemplateChild<gtk::Label>,
        #[template_child]
        view_or_join_btn: TemplateChild<LoadingButton>,
        /// The current session.
        #[property(get, set = Self::set_session, construct_only)]
        session: glib::WeakRef<Session>,
        /// The URI to preview.
        uri: RefCell<Option<MatrixRoomIdUri>>,
        /// The room that is previewed.
        #[property(get)]
        room: RefCell<Option<RemoteRoom>>,
        /// Whether the "Go back" button is disabled.
        disable_go_back: Cell<bool>,
        room_loading_handler: RefCell<Option<glib::SignalHandlerId>>,
        room_list_info_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RoomPreviewDialog {
        const NAME: &'static str = "RoomPreviewDialog";
        type Type = super::RoomPreviewDialog;
        type ParentType = ToastableDialog;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for RoomPreviewDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.room_topic.connect_activate_link(clone!(
                #[weak]
                obj,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, uri| {
                    let Ok(uri) = MatrixIdUri::parse(uri) else {
                        return glib::Propagation::Proceed;
                    };
                    let Some(parent_window) =
                        obj.ancestor(Window::static_type()).and_downcast::<Window>()
                    else {
                        return glib::Propagation::Proceed;
                    };

                    parent_window.session_view().show_matrix_uri(uri);
                    glib::Propagation::Stop
                }
            ));
        }

        fn dispose(&self) {
            self.disconnect_signals();
        }
    }

    impl WidgetImpl for RoomPreviewDialog {
        fn map(&self) {
            self.parent_map();

            // Override the default focus (the first selectable widget, in this case the
            // room topic), and instead focus on the View/Join button instead.
            if self
                .stack
                .visible_child_name()
                .is_some_and(|s| s == "details")
            {
                // Call `grab_focus()` outside of `debug_assert!`, which is not
                // evaluated at all in release builds.
                let focused = self.view_or_join_btn.grab_focus();
                debug_assert!(focused);
            }
        }
    }

    impl AdwDialogImpl for RoomPreviewDialog {}
    impl ToastableDialogImpl for RoomPreviewDialog {}

    #[gtk::template_callbacks]
    impl RoomPreviewDialog {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            self.session.set(session);

            self.obj().notify_session();
            self.update_entry_page();
        }

        /// Set the room URI to look up.
        pub(super) fn set_uri(&self, uri: MatrixRoomIdUri) {
            self.uri.replace(Some(uri.clone()));
            self.disable_go_back(true);
            self.set_visible_page("loading");

            self.look_up_room_inner(uri);
        }

        /// Set the room that is previewed.
        pub(super) fn set_room(&self, room: &RemoteRoom) {
            if self.room.borrow().as_ref().is_some_and(|r| r == room) {
                return;
            }

            self.disconnect_signals();

            let room_list_info = room.room_list_info();
            let is_joining_handler = room_list_info.connect_is_joining_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_view_or_join_button();
                }
            ));
            let local_room_handler = room_list_info.connect_local_room_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_view_or_join_button();
                }
            ));
            self.room_list_info_handlers
                .replace(vec![is_joining_handler, local_room_handler]);

            self.room.replace(Some(room.clone()));

            if matches!(
                room.loading_state(),
                LoadingState::Ready | LoadingState::Error
            ) {
                self.fill_details();
            } else {
                let room_loading_handler = room.connect_loading_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |room| {
                        if matches!(
                            room.loading_state(),
                            LoadingState::Ready | LoadingState::Error
                        ) {
                            if let Some(handler) = imp.room_loading_handler.take() {
                                room.disconnect(handler);
                            }

                            imp.fill_details();
                        }
                    }
                ));
                self.room_loading_handler
                    .replace(Some(room_loading_handler));
            }

            self.update_view_or_join_button();
            self.obj().notify_room();
        }

        /// Set whether to disable the "Go back" button.
        pub(super) fn disable_go_back(&self, disable: bool) {
            self.disable_go_back.set(disable);
        }

        /// Whether we can go back to the previous screen.
        fn can_go_back(&self) -> bool {
            !self.disable_go_back.get()
                && self.stack.visible_child_name().as_deref() == Some("details")
        }

        /// Set the currently visible page.
        fn set_visible_page(&self, page_name: &str) {
            self.stack.set_visible_child_name(page_name);
            self.go_back_btn.set_visible(self.can_go_back());
        }

        /// Update the state of the entry page.
        #[template_callback]
        fn update_entry_page(&self) {
            let Some(session) = self.session.upgrade() else {
                self.entry_page.set_sensitive(false);
                return;
            };
            self.entry_page.set_sensitive(true);

            let Some(uri) = MatrixRoomIdUri::parse(self.search_entry.text().trim()) else {
                self.look_up_btn.set_sensitive(false);
                self.uri.take();
                return;
            };
            self.look_up_btn.set_sensitive(true);

            let id = uri.id.clone();
            self.uri.replace(Some(uri));

            if session
                .room_list()
                .get_by_identifier(&id)
                .is_some_and(|room| room.is_joined())
            {
                // Translators: This is a verb, as in 'View Room'.
                self.look_up_btn.set_content_label(gettext("View"));
            } else {
                // Translators: This is a verb, as in 'Look up Room'.
                self.look_up_btn.set_content_label(gettext("Look Up"));
            }
        }

        /// Look up the room that was entered, if it is valid.
        ///
        /// If the room is known, this will open it instead.
        #[template_callback]
        fn look_up_room(&self) {
            let Some(uri) = self.uri.borrow().clone() else {
                return;
            };
            let obj = self.obj();

            let Some(window) = obj.root().and_downcast::<Window>() else {
                return;
            };

            self.look_up_btn.set_is_loading(true);
            self.entry_page.set_sensitive(false);

            // Join or view the room with the given identifier.
            if window.session_view().select_room_if_exists(&uri.id) {
                obj.close();
            } else {
                self.look_up_room_inner(uri);
            }
        }

        fn look_up_room_inner(&self, uri: MatrixRoomIdUri) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            // Reset state before switching to possible pages.
            self.go_back_btn.set_sensitive(true);

            let room = session.remote_cache().room(uri);
            self.set_room(&room);
        }

        /// Fill the details with the given result.
        fn fill_details(&self) {
            let Some(room) = self.room.borrow().clone() else {
                return;
            };

            self.room_name.set_label(&room.display_name());

            let alias = room.canonical_alias();
            if let Some(alias) = &alias {
                self.room_alias.set_label(alias.as_str());
            }
            self.room_alias
                .set_visible(room.name().is_some() && alias.is_some());

            self.room_avatar.set_data(Some(room.avatar_data()));

            if room.loading_state() == LoadingState::Error {
                self.room_topic.set_label(&gettext(
                "The room details cannot be previewed. It can be because the room is not known by the homeserver or because its details are private. You can still try to join it."
            ));
                self.room_topic.set_visible(true);
                self.room_members_box.set_visible(false);

                self.set_visible_page("details");
                return;
            }

            if let Some(topic) = room.topic_linkified() {
                self.room_topic.set_label(&topic);
                self.room_topic.set_visible(true);
            } else {
                self.room_topic.set_visible(false);
            }

            let members_count = room.joined_members_count();
            self.room_members_count
                .set_label(&members_count.to_string());

            let members_tooltip = ngettext_f(
                // Translators: Do NOT translate the content between '{' and '}',
                // this is a variable name.
                "1 member",
                "{n} members",
                members_count,
                &[("n", &members_count.to_string())],
            );
            self.room_members_box
                .set_tooltip_text(Some(&members_tooltip));
            self.room_members_box.set_visible(true);

            self.update_view_or_join_button();
            self.set_visible_page("details");
        }

        /// Update the button for viewing or joining the previewed room given
        /// the current state.
        fn update_view_or_join_button(&self) {
            let Some(room) = self.room.borrow().clone() else {
                return;
            };
            let room_list_info = room.room_list_info();

            let label = if room_list_info.local_room().is_some() {
                gettext("View")
            } else if room.can_knock() {
                gettext("Request an Invite")
            } else {
                gettext("Join")
            };
            self.view_or_join_btn.set_content_label(label);
            self.view_or_join_btn
                .set_is_loading(room_list_info.is_joining());
        }

        /// View or join the room that was previewed.
        #[template_callback]
        async fn view_or_join_room(&self) {
            let Some(room) = self.room.borrow().clone() else {
                return;
            };

            if let Some(local_room) = room.room_list_info().local_room() {
                let obj = self.obj();

                if let Some(window) = obj.root().and_downcast_ref::<Window>() {
                    window.session_view().select_room(local_room);
                    obj.close();
                }
            } else {
                self.knock_or_join_room(&room).await;
            }
        }

        /// Knock or join the room that was previewed, if it is valid.
        async fn knock_or_join_room(&self, room: &RemoteRoom) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            self.go_back_btn.set_sensitive(false);

            // Join the room with the given identifier.
            let room_list = session.room_list();
            let uri = room.uri().clone();

            let result = if room.can_knock() {
                room_list.knock(uri.id, uri.via).await
            } else {
                room_list.join_by_id_or_alias(uri.id, uri.via).await
            };

            match result {
                Ok(room_id) => {
                    let obj = self.obj();

                    if let Some(local_room) = room_list.get_wait(&room_id, None).await
                        && let Some(window) = obj.root().and_downcast_ref::<Window>()
                    {
                        window.session_view().select_room(local_room);
                    }

                    obj.close();
                }
                Err(error) => {
                    toast!(self.obj(), error);

                    self.go_back_btn.set_sensitive(true);
                }
            }
        }

        /// Go back to the previous screen.
        ///
        /// If we can't go back, closes the window.
        #[template_callback]
        fn go_back(&self) {
            if self.can_go_back() {
                // There is only one screen to go back to.
                self.look_up_btn.set_is_loading(false);
                self.entry_page.set_sensitive(true);
                self.set_visible_page("entry");
            } else {
                self.obj().close();
            }
        }

        /// Disconnect the signal handlers of this dialog.
        fn disconnect_signals(&self) {
            if let Some(room) = self.room.borrow().as_ref() {
                if let Some(handler) = self.room_loading_handler.take() {
                    room.disconnect(handler);
                }

                let room_list_info = room.room_list_info();
                for handler in self.room_list_info_handlers.take() {
                    room_list_info.disconnect(handler);
                }
            }
        }
    }
}

glib::wrapper! {
    /// Dialog to preview a room and eventually join it.
    pub struct RoomPreviewDialog(ObjectSubclass<imp::RoomPreviewDialog>)
        @extends gtk::Widget, adw::Dialog, ToastableDialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

#[gtk::template_callbacks]
impl RoomPreviewDialog {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }

    /// Set the room URI to look up.
    pub(crate) fn set_uri(&self, uri: MatrixRoomIdUri) {
        self.imp().set_uri(uri);
    }

    /// Set the room to preview.
    pub(crate) fn set_room(&self, room: &RemoteRoom) {
        let imp = self.imp();
        imp.disable_go_back(true);
        imp.set_room(room);
    }
}
