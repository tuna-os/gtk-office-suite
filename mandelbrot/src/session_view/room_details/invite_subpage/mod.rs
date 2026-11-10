use adw::{prelude::*, subclass::prelude::*};
use gettextrs::{gettext, ngettext};
use gtk::{gdk, glib, glib::clone};
use tracing::error;

mod item;
mod list;
mod row;

use self::{
    item::InviteItem,
    list::{InviteList, InviteListState},
    row::InviteRow,
};
use crate::{
    components::{LoadingButton, PillSearchEntry, PillSource},
    prelude::*,
    session::{Room, User},
    toast,
};

mod imp {
    use std::cell::OnceCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/invite_subpage/mod.ui"
    )]
    #[properties(wrapper_type = super::InviteSubpage)]
    pub struct InviteSubpage {
        #[template_child]
        search_entry: TemplateChild<PillSearchEntry>,
        #[template_child]
        list_view: TemplateChild<gtk::ListView>,
        #[template_child]
        invite_button: TemplateChild<LoadingButton>,
        #[template_child]
        cancel_button: TemplateChild<gtk::Button>,
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        matching_page: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        no_matching_page: TemplateChild<adw::StatusPage>,
        #[template_child]
        no_search_page: TemplateChild<adw::StatusPage>,
        #[template_child]
        error_page: TemplateChild<adw::StatusPage>,
        /// The room users will be invited to.
        #[property(get, set = Self::set_room, construct_only)]
        room: glib::WeakRef<Room>,
        /// The list managing the invited users.
        #[property(get)]
        invite_list: OnceCell<InviteList>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for InviteSubpage {
        const NAME: &'static str = "RoomDetailsInviteSubpage";
        type Type = super::InviteSubpage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            InviteRow::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.add_binding(gdk::Key::Escape, gdk::ModifierType::empty(), |obj| {
                obj.imp().close();
                glib::Propagation::Stop
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for InviteSubpage {}

    impl WidgetImpl for InviteSubpage {}

    impl NavigationPageImpl for InviteSubpage {
        fn shown(&self) {
            self.search_entry.grab_focus();
        }
    }

    #[gtk::template_callbacks]
    impl InviteSubpage {
        /// Set the room users will be invited to.
        fn set_room(&self, room: &Room) {
            let invite_list = self.invite_list.get_or_init(|| InviteList::new(room));
            invite_list.connect_invitee_added(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, invitee| {
                    imp.search_entry.add_pill(&invitee.user());
                }
            ));

            invite_list.connect_invitee_removed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, invitee| {
                    imp.search_entry.remove_pill(&invitee.user().identifier());
                }
            ));

            invite_list.connect_state_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_view();
                }
            ));

            self.search_entry
                .bind_property("text", invite_list, "search-term")
                .sync_create()
                .build();

            invite_list
                .bind_property("has-invitees", &*self.invite_button, "sensitive")
                .sync_create()
                .build();

            self.list_view
                .set_model(Some(&gtk::NoSelection::new(Some(invite_list.clone()))));

            self.room.set(Some(room));
            self.obj().notify_room();
        }

        /// The list managing the invited users.
        fn invite_list(&self) -> &InviteList {
            self.invite_list
                .get()
                .expect("invite list should be initialized")
        }

        /// Update the view for the current state of the list.
        fn update_view(&self) {
            let state = self.invite_list().state();

            let page = match state {
                InviteListState::Initial => "no-search",
                InviteListState::Loading => "loading",
                InviteListState::NoMatching => "no-results",
                InviteListState::Matching => "results",
                InviteListState::Error => "error",
            };

            self.stack.set_visible_child_name(page);
        }

        /// Close this subpage.
        #[template_callback]
        fn close(&self) {
            let obj = self.obj();
            let Some(window) = obj.root().and_downcast::<adw::PreferencesWindow>() else {
                return;
            };

            if obj.can_pop() {
                window.pop_subpage();
            } else {
                window.close();
            }
        }

        /// Toggle the invited state of the item at the given index.
        #[template_callback]
        fn toggle_item_is_invitee(&self, index: u32) {
            let Some(item) = self.invite_list().item(index).and_downcast::<InviteItem>() else {
                return;
            };

            item.set_is_invitee(!item.is_invitee());
        }

        /// Uninvite the user from the given pill source.
        #[template_callback]
        fn remove_pill_invitee(&self, source: PillSource) {
            if let Ok(user) = source.downcast::<User>() {
                self.invite_list().remove_invitee(user.user_id());
            }
        }

        /// Invite the selected users to the room.
        #[template_callback]
        async fn invite(&self) {
            let Some(room) = self.room.upgrade() else {
                return;
            };

            self.invite_button.set_is_loading(true);

            let invite_list = self.invite_list();
            let invitees = invite_list.invitees_ids();

            match room.invite(&invitees).await {
                Ok(()) => {
                    self.close();
                }
                Err(failed_users) => {
                    invite_list.retain_invitees(&failed_users);

                    let n_failed = failed_users.len();
                    let n = invite_list.n_invitees();
                    if n != n_failed {
                        // This should not be possible.
                        error!(
                            "The number of failed users does not match the number of remaining invitees: expected {n_failed}, got {n}"
                        );
                    }

                    // We don't use the count in the strings so we use separate gettext calls for
                    // singular and plural rather than using ngettext.
                    if n == 0 {
                        self.close();
                    } else if n == 1 {
                        let first_failed =
                            invite_list.first_invitee().map(|item| item.user()).unwrap();

                        toast!(
                            self.obj(),
                            gettext(
                                // Translators: Do NOT translate the content between '{' and '}', these
                                // are variable names.
                                "Could not invite {user} to {room}",
                            ),
                            @user = first_failed,
                            @room,
                            n,
                        );
                    } else {
                        toast!(
                            self.obj(),
                            ngettext(
                                // Translators: Do NOT translate the content between '{' and '}', these
                                // are variable names. The count is always greater than 1.
                                "Could not invite 1 user to {room}",
                                "Could not invite {n} users to {room}",
                                n as u32,
                            ),
                            @room,
                            n,
                        );
                    }
                }
            }

            self.invite_button.set_is_loading(false);
        }
    }
}

glib::wrapper! {
    /// Subpage to invite new members to a room.
    pub struct InviteSubpage(ObjectSubclass<imp::InviteSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl InviteSubpage {
    /// Construct a new `InviteSubpage` with the given room.
    pub fn new(room: &Room) -> Self {
        glib::Object::builder().property("room", room).build()
    }
}
