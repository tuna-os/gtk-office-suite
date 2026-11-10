use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};

mod user;
mod user_list;

use self::{user::DirectChatUser, user_list::DirectChatUserList};
use crate::{
    Window,
    components::{PillSource, PillSourceRow},
    gettext,
    session::{Session, User},
};

/// A page of the [`CreateDirectChatDialog`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreateDirectChatDialogPage {
    /// The page when there is no search term.
    NoSearchTerm,
    /// The loading page.
    Loading,
    /// The page displaying the results.
    Results,
    /// The page when there are no results.
    Empty,
    /// The error page.
    Error,
}

impl CreateDirectChatDialogPage {
    /// Get the name of this page.
    const fn name(self) -> &'static str {
        match self {
            Self::NoSearchTerm => "no-search-term",
            Self::Loading => "loading",
            Self::Results => "results",
            Self::Empty => "empty",
            Self::Error => "error",
        }
    }
}

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;
    use crate::utils::LoadingState;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/create_direct_chat_dialog/mod.ui"
    )]
    #[properties(wrapper_type = super::CreateDirectChatDialog)]
    pub struct CreateDirectChatDialog {
        #[template_child]
        list_box: TemplateChild<gtk::ListBox>,
        #[template_child]
        search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        error_page: TemplateChild<adw::StatusPage>,
        /// The current session.
        #[property(get, set = Self::set_session, explicit_notify, nullable)]
        session: glib::WeakRef<Session>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CreateDirectChatDialog {
        const NAME: &'static str = "CreateDirectChatDialog";
        type Type = super::CreateDirectChatDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            PillSourceRow::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for CreateDirectChatDialog {}

    impl WidgetImpl for CreateDirectChatDialog {}
    impl AdwDialogImpl for CreateDirectChatDialog {}

    #[gtk::template_callbacks]
    impl CreateDirectChatDialog {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            if self.session.upgrade().as_ref() == session {
                return;
            }

            if let Some(session) = session {
                let user_list = DirectChatUserList::new(session);

                // We don't need to disconnect this signal since the `DirectChatUserList` will
                // be disposed once unbound from the `gtk::ListBox`
                user_list.connect_loading_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |model| {
                        imp.update_view(model);
                    }
                ));
                user_list.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |model, _, _, _| {
                        imp.update_view(model);
                    }
                ));

                self.search_entry
                    .bind_property("text", &user_list, "search-term")
                    .sync_create()
                    .build();

                self.list_box.bind_model(Some(&user_list), |user| {
                    let source = user
                        .downcast_ref::<PillSource>()
                        .expect("DirectChatUserList should only contain DirectChatUsers");
                    let row = PillSourceRow::new();
                    row.set_source(Some(source.clone()));

                    row.upcast()
                });

                self.update_view(&user_list);
            } else {
                self.list_box.unbind_model();
            }

            self.session.set(session);
            self.obj().notify_session();
        }

        /// Set the visible page of the dialog.
        fn set_visible_page(&self, page: CreateDirectChatDialogPage) {
            self.stack.set_visible_child_name(page.name());
        }

        /// Update the view for the current state of the user list.
        fn update_view(&self, user_list: &DirectChatUserList) {
            let page = match user_list.loading_state() {
                LoadingState::Initial => CreateDirectChatDialogPage::NoSearchTerm,
                LoadingState::Loading => CreateDirectChatDialogPage::Loading,
                LoadingState::Ready => {
                    if user_list.n_items() > 0 {
                        CreateDirectChatDialogPage::Results
                    } else {
                        CreateDirectChatDialogPage::Empty
                    }
                }
                LoadingState::Error => {
                    self.show_error(&gettext("An error occurred while searching for users"));
                    return;
                }
            };

            self.set_visible_page(page);
        }

        /// Show the given error message.
        fn show_error(&self, message: &str) {
            self.error_page.set_description(Some(message));
            self.set_visible_page(CreateDirectChatDialogPage::Error);
        }

        /// Create a direct chat with the user from the given row.
        #[template_callback]
        async fn create_direct_chat(&self, row: &gtk::ListBoxRow) {
            let Some(user) = row
                .downcast_ref::<PillSourceRow>()
                .and_then(PillSourceRow::source)
                .and_downcast::<User>()
            else {
                return;
            };

            self.set_visible_page(CreateDirectChatDialogPage::Loading);
            self.search_entry.set_sensitive(false);

            if let Ok(room) = user.get_or_create_direct_chat().await {
                let obj = self.obj();

                let Some(window) = obj
                    .parent()
                    .and_then(|widget| widget.root())
                    .and_downcast::<Window>()
                else {
                    return;
                };

                window.session_view().select_room(room);
                obj.close();
            } else {
                self.show_error(&gettext("Could not create a new Direct Chat"));
                self.search_entry.set_sensitive(true);
            }
        }
    }
}

glib::wrapper! {
    /// Dialog to create a new direct chat.
    pub struct CreateDirectChatDialog(ObjectSubclass<imp::CreateDirectChatDialog>)
        @extends gtk::Widget, adw::Dialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl CreateDirectChatDialog {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }
}
