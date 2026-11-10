use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};
use ruma::UserId;
use tracing::error;

use super::DirectChatUser;
use crate::{prelude::*, session::Session, spawn, spawn_tokio, utils::LoadingState};

mod imp {
    use std::cell::{Cell, RefCell};

    use tokio::task::AbortHandle;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::DirectChatUserList)]
    pub struct DirectChatUserList {
        /// The current list of results.
        list: RefCell<Vec<DirectChatUser>>,
        /// The current session.
        #[property(get, construct_only)]
        session: glib::WeakRef<Session>,
        /// The state of the list.
        #[property(get, builder(LoadingState::default()))]
        loading_state: Cell<LoadingState>,
        /// The search term.
        #[property(get, set = Self::set_search_term, explicit_notify, nullable)]
        search_term: RefCell<Option<String>>,
        abort_handle: RefCell<Option<AbortHandle>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DirectChatUserList {
        const NAME: &'static str = "DirectChatUserList";
        type Type = super::DirectChatUserList;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for DirectChatUserList {}

    impl ListModelImpl for DirectChatUserList {
        fn item_type(&self) -> glib::Type {
            DirectChatUser::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.list
                .borrow()
                .get(position as usize)
                .cloned()
                .and_upcast()
        }
    }

    impl DirectChatUserList {
        /// Set the search term.
        fn set_search_term(&self, search_term: Option<String>) {
            let search_term = search_term.filter(|s| !s.is_empty());

            if search_term == *self.search_term.borrow() {
                return;
            }

            self.search_term.replace(search_term.clone());

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.search_users(search_term).await;
                }
            ));

            self.obj().notify_search_term();
        }

        /// Set the loading state of the list.
        fn set_loading_state(&self, state: LoadingState) {
            if self.loading_state.get() == state {
                return;
            }

            self.loading_state.set(state);
            self.obj().notify_loading_state();
        }

        /// Replace the list of results.
        fn set_list(&self, users: Vec<DirectChatUser>) {
            let removed = self.n_items();
            self.list.replace(users);
            let added = self.n_items();

            self.obj().items_changed(0, removed, added);
        }

        /// Clear the list of results.
        fn clear_list(&self) {
            let removed = self.n_items();
            self.list.borrow_mut().clear();

            self.obj().items_changed(0, removed, 0);
        }

        /// Update the list of users for the given search term.
        async fn search_users(&self, search_term: Option<String>) {
            let Some(search_term) = search_term else {
                self.set_loading_state(LoadingState::Initial);
                return;
            };

            let Some(session) = self.session.upgrade() else {
                return;
            };
            let client = session.client();

            self.set_loading_state(LoadingState::Loading);
            self.clear_list();

            let search_term_clone = search_term.clone();
            let handle =
                spawn_tokio!(async move { client.search_users(&search_term_clone, 20).await });

            if let Some(abort_handle) = self.abort_handle.replace(Some(handle.abort_handle())) {
                abort_handle.abort();
            }

            let Ok(result) = handle.await else {
                // The future was aborted, which means that there is a new search term, we have
                // nothing to do.
                return;
            };

            // Check that the search term is the current one, in case the future was not
            // aborted in time.
            if self
                .search_term
                .borrow()
                .as_ref()
                .is_none_or(|term| *term != search_term)
            {
                return;
            }

            self.abort_handle.take();

            let response = match result {
                Ok(response) => response,
                Err(error) => {
                    error!("Could not search users: {error}");
                    self.set_loading_state(LoadingState::Error);
                    return;
                }
            };

            let mut list = Vec::with_capacity(response.results.len());

            // If the search term looks like a UserId and is not already in the response,
            // insert it.
            if let Ok(user_id) = UserId::parse(&search_term)
                && !response.results.iter().any(|item| item.user_id == user_id)
            {
                let user = DirectChatUser::new(&session, user_id, None, None);

                // Fetch the avatar and display name.
                spawn!(clone!(
                    #[weak]
                    user,
                    async move {
                        let _ = user.load_profile().await;
                    }
                ));

                list.push(user);
            }

            list.extend(response.results.into_iter().map(|user| {
                DirectChatUser::new(
                    &session,
                    user.user_id,
                    user.display_name.as_deref(),
                    user.avatar_url,
                )
            }));

            self.set_list(list);
            self.set_loading_state(LoadingState::Ready);
        }
    }
}

glib::wrapper! {
    /// List of users in the server's user directory matching a search term.
    pub struct DirectChatUserList(ObjectSubclass<imp::DirectChatUserList>)
        @implements gio::ListModel;
}

impl DirectChatUserList {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }
}
