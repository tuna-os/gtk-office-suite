use gettextrs::gettext;
use gtk::{
    gio, glib,
    glib::{clone, closure_local},
    prelude::*,
    subclass::prelude::*,
};
use matrix_sdk::ruma::{
    OwnedUserId, UserId, api::client::user_directory::search_users::v3::User as SearchUser,
};
use tracing::error;

use super::InviteItem;
use crate::{
    prelude::*,
    session::{Member, Membership, Room, User},
    spawn, spawn_tokio,
};

#[derive(Debug, Default, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[enum_type(name = "RoomDetailsInviteListState")]
pub enum InviteListState {
    #[default]
    Initial,
    Loading,
    NoMatching,
    Matching,
    Error,
}

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        collections::HashMap,
        marker::PhantomData,
        sync::LazyLock,
    };

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::InviteList)]
    pub struct InviteList {
        list: RefCell<Vec<InviteItem>>,
        /// The room this invitee list refers to.
        #[property(get, construct_only)]
        room: OnceCell<Room>,
        /// The state of the list.
        #[property(get, builder(InviteListState::default()))]
        state: Cell<InviteListState>,
        /// The search term.
        #[property(get, set = Self::set_search_term, explicit_notify)]
        search_term: RefCell<Option<String>>,
        pub(super) invitee_list: RefCell<HashMap<OwnedUserId, InviteItem>>,
        abort_handle: RefCell<Option<tokio::task::AbortHandle>>,
        /// Whether some users are invited.
        #[property(get = Self::has_invitees)]
        has_invitees: PhantomData<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for InviteList {
        const NAME: &'static str = "RoomDetailsInviteList";
        type Type = super::InviteList;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for InviteList {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    Signal::builder("invitee-added")
                        .param_types([InviteItem::static_type()])
                        .build(),
                    Signal::builder("invitee-removed")
                        .param_types([InviteItem::static_type()])
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }
    }

    impl ListModelImpl for InviteList {
        fn item_type(&self) -> glib::Type {
            InviteItem::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.list
                .borrow()
                .get(position as usize)
                .map(glib::object::Cast::upcast_ref::<glib::Object>)
                .cloned()
        }
    }

    impl InviteList {
        /// The room this invitee list refers to.
        fn room(&self) -> &Room {
            self.room.get().expect("room should be initialized")
        }

        /// Set the search term.
        fn set_search_term(&self, search_term: Option<String>) {
            let search_term = search_term.filter(|s| !s.is_empty());

            if search_term == *self.search_term.borrow() {
                return;
            }

            self.search_term.replace(search_term);

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.search_users().await;
                }
            ));

            self.obj().notify_search_term();
        }

        /// Whether some users are invited.
        fn has_invitees(&self) -> bool {
            !self.invitee_list.borrow().is_empty()
        }

        /// Set the state of the list.
        pub fn set_state(&self, state: InviteListState) {
            if state == self.state.get() {
                return;
            }

            self.state.set(state);
            self.obj().notify_state();
        }

        /// Replace this list with the given items.
        fn replace_list(&self, items: Vec<InviteItem>) {
            let added = items.len();

            let prev_items = self.list.replace(items);

            self.obj()
                .items_changed(0, prev_items.len() as u32, added as u32);
        }

        /// Clear this list.
        fn clear_list(&self) {
            self.replace_list(Vec::new());
        }

        /// Search for the current search term in the user directory.
        async fn search_users(&self) {
            let Some(session) = self.room().session() else {
                return;
            };

            let Some(search_term) = self.search_term.borrow().clone() else {
                // Do nothing for no search term, but reset state when currently loading.
                if self.state.get() == InviteListState::Loading {
                    self.set_state(InviteListState::Initial);
                }
                if let Some(abort_handle) = self.abort_handle.take() {
                    abort_handle.abort();
                }

                return;
            };

            self.set_state(InviteListState::Loading);
            self.clear_list();

            let client = session.client();
            let search_term_clone = search_term.clone();
            let handle =
                spawn_tokio!(async move { client.search_users(&search_term_clone, 10).await });

            let abort_handle = handle.abort_handle();

            // Keep the abort handle so we can abort the request if the user changes the
            // search term.
            if let Some(prev_abort_handle) = self.abort_handle.replace(Some(abort_handle)) {
                // Abort the previous request.
                prev_abort_handle.abort();
            }

            match handle.await {
                Ok(Ok(response)) => {
                    // The request succeeded.
                    if self
                        .search_term
                        .borrow()
                        .as_ref()
                        .is_some_and(|s| *s == search_term)
                    {
                        self.update_from_search_results(response.results);
                    }
                }
                Ok(Err(error)) => {
                    // The request failed.
                    error!("Could not search user directory: {error}");
                    self.set_state(InviteListState::Error);
                    self.clear_list();
                }
                Err(_) => {
                    // The request was aborted.
                }
            }

            self.abort_handle.take();
        }

        /// Update this list from the given search results.
        fn update_from_search_results(&self, results: Vec<SearchUser>) {
            let Some(session) = self.room().session() else {
                return;
            };
            let Some(search_term) = self.search_term.borrow().clone() else {
                return;
            };

            // We should have a strong reference to the list in the main page so we can use
            // `get_or_create_members()`.
            let member_list = self.room().get_or_create_members();

            // If the search term looks like a user ID and it is not already in the
            // response, we will insert it in the list.
            let search_term_user_id = UserId::parse(search_term)
                .ok()
                .filter(|user_id| !results.iter().any(|item| item.user_id == *user_id));
            let search_term_user = search_term_user_id.clone().map(SearchUser::new);

            let new_len = results
                .len()
                .saturating_add(search_term_user.is_some().into());
            if new_len == 0 {
                self.set_state(InviteListState::NoMatching);
                self.clear_list();
                return;
            }

            let mut list = Vec::with_capacity(new_len);
            let results = search_term_user.into_iter().chain(results);

            for result in results {
                let member = member_list.get(&result.user_id);

                // 'Disable' users that can't be invited.
                let invite_exception = member.as_ref().and_then(|m| match m.membership() {
                    Membership::Join => Some(gettext("Member")),
                    Membership::Ban => Some(gettext("Banned")),
                    Membership::Invite => Some(gettext("Invited")),
                    _ => None,
                });

                // If it's an invitee, reuse the item.
                let invitee = self.invitee_list.borrow().get(&result.user_id).cloned();
                if let Some(item) = invitee {
                    let user = item.user();

                    // The profile data may have changed in the meantime, but don't overwrite a
                    // joined member's data.
                    if !user
                        .downcast_ref::<Member>()
                        .is_some_and(|m| m.membership() == Membership::Join)
                    {
                        user.set_avatar_url(result.avatar_url);
                        user.set_name(result.display_name);
                    }

                    // The membership state may have changed in the meantime.
                    item.set_invite_exception(invite_exception);

                    list.push(item);
                    continue;
                }

                // If it's a joined room member, reuse the user.
                if let Some(member) = member.filter(|m| m.membership() == Membership::Join) {
                    let item = self.create_item(&member, invite_exception);
                    list.push(item);

                    continue;
                }

                // If it's the dummy result for the search term user ID, use the remote cache to
                // fetch its profile.
                if search_term_user_id
                    .as_ref()
                    .is_some_and(|user_id| *user_id == result.user_id)
                {
                    let user = session.remote_cache().user(result.user_id);
                    let item = self.create_item(&user, invite_exception);
                    list.push(item);

                    continue;
                }

                // As a last resort, we just use the data of the result.
                let user = User::new(&session, result.user_id);
                user.set_avatar_url(result.avatar_url);
                user.set_name(result.display_name);

                let item = self.create_item(&user, invite_exception);
                list.push(item);
            }

            self.replace_list(list);
            self.set_state(InviteListState::Matching);
        }

        /// Create an item for the given user and invite exception.
        fn create_item(
            &self,
            user: &impl IsA<User>,
            invite_exception: Option<String>,
        ) -> InviteItem {
            let item = InviteItem::new(user);
            item.set_invite_exception(invite_exception);

            item.connect_is_invitee_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |item| {
                    imp.update_invitees_for_item(item);
                }
            ));
            item.connect_can_invite_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |item| {
                    imp.update_invitees_for_item(item);
                }
            ));

            item
        }

        /// Update the list of invitees for the current state of the item.
        fn update_invitees_for_item(&self, item: &InviteItem) {
            if item.is_invitee() && item.can_invite() {
                self.add_invitee(item);
            } else {
                self.remove_invitee(item.user().user_id());
            }
        }

        /// Add the given item as an invitee.
        fn add_invitee(&self, item: &InviteItem) {
            let had_invitees = self.has_invitees();

            item.set_is_invitee(true);
            self.invitee_list
                .borrow_mut()
                .insert(item.user().user_id().clone(), item.clone());

            let obj = self.obj();
            obj.emit_by_name::<()>("invitee-added", &[&item]);

            if !had_invitees {
                obj.notify_has_invitees();
            }
        }

        /// Update the list of invitees so only the invitees with the given user
        /// IDs remain.
        pub(super) fn retain_invitees(&self, invitees_ids: &[&UserId]) {
            if !self.has_invitees() {
                // Nothing to do.
                return;
            }

            let invitee_list = self.invitee_list.take();

            let (invitee_list, removed_invitees) = invitee_list
                .into_iter()
                .partition(|(key, _)| invitees_ids.contains(&key.as_ref()));
            self.invitee_list.replace(invitee_list);

            for item in removed_invitees.values() {
                self.handle_removed_invitee(item);
            }

            if !self.has_invitees() {
                self.obj().notify_has_invitees();
            }
        }

        /// Remove the invitee with the given user ID from the list.
        pub(super) fn remove_invitee(&self, user_id: &UserId) {
            let Some(item) = self.invitee_list.borrow_mut().remove(user_id) else {
                return;
            };

            self.handle_removed_invitee(&item);

            if !self.has_invitees() {
                self.obj().notify_has_invitees();
            }
        }

        /// Handle when the given item was removed from the list of invitees.
        fn handle_removed_invitee(&self, item: &InviteItem) {
            item.set_is_invitee(false);
            self.obj().emit_by_name::<()>("invitee-removed", &[&item]);
        }
    }
}

glib::wrapper! {
    /// List of users after a search in the user directory.
    ///
    /// This also manages invitees.
    pub struct InviteList(ObjectSubclass<imp::InviteList>)
        @implements gio::ListModel;
}

impl InviteList {
    pub fn new(room: &Room) -> Self {
        glib::Object::builder().property("room", room).build()
    }

    /// Return the first invitee in the list, if any.
    pub(crate) fn first_invitee(&self) -> Option<InviteItem> {
        self.imp().invitee_list.borrow().values().next().cloned()
    }

    /// Get the number of invitees.
    pub(crate) fn n_invitees(&self) -> usize {
        self.imp().invitee_list.borrow().len()
    }

    /// Get the list of user IDs of the invitees.
    pub(crate) fn invitees_ids(&self) -> Vec<OwnedUserId> {
        self.imp().invitee_list.borrow().keys().cloned().collect()
    }

    /// Update the list of invitees so only the invitees with the given user IDs
    /// remain.
    pub(crate) fn retain_invitees(&self, invitees_ids: &[&UserId]) {
        self.imp().retain_invitees(invitees_ids);
    }

    /// Remove the invitee with the given user ID from the list.
    pub(crate) fn remove_invitee(&self, user_id: &UserId) {
        self.imp().remove_invitee(user_id);
    }

    /// Connect to the signal emitted when an invitee is added.
    pub fn connect_invitee_added<F: Fn(&Self, &InviteItem) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "invitee-added",
            true,
            closure_local!(move |obj: Self, invitee: InviteItem| {
                f(&obj, &invitee);
            }),
        )
    }

    /// Connect to the signal emitted when an invitee is removed.
    pub fn connect_invitee_removed<F: Fn(&Self, &InviteItem) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "invitee-removed",
            true,
            closure_local!(move |obj: Self, invitee: InviteItem| {
                f(&obj, &invitee);
            }),
        )
    }
}
