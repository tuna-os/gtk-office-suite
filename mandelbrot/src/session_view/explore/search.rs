use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};
use ruma::{
    OwnedServerName,
    api::client::directory::get_public_rooms_filtered,
    assign,
    directory::{Filter, RoomNetwork, RoomTypeFilter},
};
use tokio::task::AbortHandle;
use tracing::error;

use super::ExploreServer;
use crate::{
    session::{RemoteRoom, Session},
    spawn, spawn_tokio,
    utils::{LoadingState, matrix::MatrixRoomIdUri},
};

/// The maximum size of a batch of public rooms.
const PUBLIC_ROOMS_BATCH_SIZE: u32 = 20;

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::ExploreSearch)]
    pub struct ExploreSearch {
        /// The list of public rooms for the current search.
        #[property(get = Self::list_owned)]
        list: OnceCell<gio::ListStore>,
        /// The current search.
        search: RefCell<ExploreSearchData>,
        /// The next batch to continue the search, if any.
        next_batch: RefCell<Option<String>>,
        /// The loading state of the list.
        #[property(get, builder(LoadingState::default()))]
        loading_state: Cell<LoadingState>,
        /// The abort handle for the current request.
        abort_handle: RefCell<Option<AbortHandle>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ExploreSearch {
        const NAME: &'static str = "ExploreSearch";
        type Type = super::ExploreSearch;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ExploreSearch {}

    impl ExploreSearch {
        /// The list of public rooms for the current search.
        fn list(&self) -> &gio::ListStore {
            self.list.get_or_init(gio::ListStore::new::<RemoteRoom>)
        }

        /// The owned list of public rooms for the current search.
        fn list_owned(&self) -> gio::ListStore {
            self.list().clone()
        }

        /// Set the current search.
        pub(super) fn set_search(&self, search: ExploreSearchData) {
            if *self.search.borrow() == search {
                return;
            }

            self.search.replace(search);

            // Trigger a new search.
            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.load(true).await;
                }
            ));
        }

        /// Set the loading state.
        fn set_loading_state(&self, state: LoadingState) {
            if self.loading_state.get() == state {
                return;
            }

            self.loading_state.set(state);
            self.obj().notify_loading_state();
        }

        /// Whether the list is empty.
        pub(super) fn is_empty(&self) -> bool {
            self.list().n_items() == 0
        }

        /// Whether we can load more rooms with the current search.
        pub(super) fn can_load_more(&self) -> bool {
            self.loading_state.get() != LoadingState::Loading && self.next_batch.borrow().is_some()
        }

        /// Load rooms.
        ///
        /// If `clear` is `true`, we start a new search and replace the list of
        /// rooms, otherwise we use the `next_batch` and add more rooms.
        pub(super) async fn load(&self, clear: bool) {
            // Only make a request if we can load more items or we want to replace the
            // current list.
            if !clear && !self.can_load_more() {
                return;
            }

            if clear {
                // Clear the list.
                self.list().remove_all();
                self.next_batch.take();

                // Abort any ongoing request.
                if let Some(handle) = self.abort_handle.take() {
                    handle.abort();
                }
            }

            let search = self.search.borrow().clone();

            let Some(session) = search.session.upgrade() else {
                return;
            };

            self.set_loading_state(LoadingState::Loading);

            let next_batch = self.next_batch.borrow().clone();
            let request = search.as_request(next_batch);

            let client = session.client();
            let handle = spawn_tokio!(async move { client.public_rooms_filtered(request).await });

            self.abort_handle.replace(Some(handle.abort_handle()));

            let Ok(result) = handle.await else {
                // The request was aborted.
                self.abort_handle.take();
                return;
            };

            self.abort_handle.take();

            if *self.search.borrow() != search {
                // This is not the current search anymore, ignore the response.
                return;
            }

            match result {
                Ok(response) => self.add_rooms(&session, &search, response),
                Err(error) => {
                    self.set_loading_state(LoadingState::Error);
                    error!("Could not search public rooms: {error}");
                }
            }
        }

        /// Add the rooms from the given response to this list.
        fn add_rooms(
            &self,
            session: &Session,
            search: &ExploreSearchData,
            response: get_public_rooms_filtered::v3::Response,
        ) {
            self.next_batch.replace(response.next_batch);

            let new_rooms = response
                .chunk
                .into_iter()
                .map(|data| {
                    let id = data
                        .canonical_alias
                        .clone()
                        .map_or_else(|| data.room_id.clone().into(), Into::into);
                    let uri = MatrixRoomIdUri {
                        id,
                        via: search.server.clone().into_iter().collect(),
                    };

                    RemoteRoom::with_data(session, uri, data)
                })
                .collect::<Vec<_>>();

            self.list().extend_from_slice(&new_rooms);

            self.set_loading_state(LoadingState::Ready);
        }
    }
}

glib::wrapper! {
    /// The search API of the view to explore rooms in the public directory of homeservers.
    pub struct ExploreSearch(ObjectSubclass<imp::ExploreSearch>);
}

impl ExploreSearch {
    /// Construct a new empty `ExploreSearch`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Whether the list is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.imp().is_empty()
    }

    /// Search the given term on the given server.
    pub(crate) fn search(
        &self,
        session: &Session,
        search_term: Option<String>,
        server: &ExploreServer,
    ) {
        let session_weak = glib::WeakRef::new();
        session_weak.set(Some(session));

        let search = ExploreSearchData {
            session: session_weak,
            search_term,
            server: server.server().cloned(),
            third_party_network: server.third_party_network(),
        };
        self.imp().set_search(search);
    }

    /// Load more rooms.
    pub(crate) fn load_more(&self) {
        let imp = self.imp();

        if imp.can_load_more() {
            spawn!(clone!(
                #[weak]
                imp,
                async move { imp.load(false).await }
            ));
        }
    }
}

impl Default for ExploreSearch {
    fn default() -> Self {
        Self::new()
    }
}

/// Data about a search in the public rooms directory.
#[derive(Debug, Clone, Default, PartialEq)]
struct ExploreSearchData {
    /// The session to use for performing the search.
    session: glib::WeakRef<Session>,
    /// The term to search.
    search_term: Option<String>,
    /// The server to search.
    server: Option<OwnedServerName>,
    /// The network to search.
    third_party_network: Option<String>,
}

impl ExploreSearchData {
    /// Convert this `ExploreSearchData` to a request.
    fn as_request(&self, next_batch: Option<String>) -> get_public_rooms_filtered::v3::Request {
        let room_network = if let Some(third_party_network) = &self.third_party_network {
            RoomNetwork::ThirdParty(third_party_network.clone())
        } else {
            RoomNetwork::Matrix
        };

        assign!( get_public_rooms_filtered::v3::Request::new(), {
            limit: Some(PUBLIC_ROOMS_BATCH_SIZE.into()),
            since: next_batch,
            room_network,
            server: self.server.clone(),
            filter: assign!(
                Filter::new(),
                { generic_search_term: self.search_term.clone(), room_types: vec![RoomTypeFilter::Default] }
            ),
        })
    }
}
