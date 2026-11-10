use gtk::{gio, glib, prelude::*, subclass::prelude::*};
use matrix_sdk_ui::timeline::ReactionsByKeyBySender;

use super::ReactionGroup;
use crate::session::User;

mod imp {
    use std::cell::{OnceCell, RefCell};

    use indexmap::IndexMap;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::ReactionList)]
    pub struct ReactionList {
        /// The user of the parent session.
        #[property(get, set)]
        user: OnceCell<User>,
        /// The list of reactions grouped by key.
        reactions: RefCell<IndexMap<String, ReactionGroup>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ReactionList {
        const NAME: &'static str = "ReactionList";
        type Type = super::ReactionList;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for ReactionList {}

    impl ListModelImpl for ReactionList {
        fn item_type(&self) -> glib::Type {
            ReactionGroup::static_type()
        }

        fn n_items(&self) -> u32 {
            self.reactions.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            let reactions = self.reactions.borrow();

            reactions
                .get_index(position as usize)
                .map(|(_key, reaction_group)| reaction_group.clone().upcast())
        }
    }

    impl ReactionList {
        /// Update the reaction list with the given reactions.
        pub(super) fn update(&self, new_reactions: Option<&ReactionsByKeyBySender>) {
            let mut pos = 0usize;

            let (removed, added) = {
                let mut reactions = self.reactions.borrow_mut();

                // Update the first groups with identical keys.
                for ((new_key, group_reactions), (old_key, group)) in new_reactions
                    .iter()
                    .flat_map(|new_reactions| new_reactions.iter())
                    .zip(reactions.iter())
                {
                    if new_key == old_key {
                        group.update(group_reactions);
                        pos += 1;
                    } else {
                        // Stop as soon as the keys do not match.
                        break;
                    }
                }

                // Remove all the groups after the mismatch, if any.
                let removed = reactions.len() - pos;
                if removed > 0 {
                    reactions.truncate(pos);
                }

                // Add new groups for the new keys, if any.
                let new_len = new_reactions
                    .map(|new_reactions| new_reactions.len())
                    .unwrap_or_default();
                let added = new_len - pos;
                if added > 0 {
                    let user = self.user.get().expect("user should be initialized");
                    reactions.extend(
                        new_reactions
                            .iter()
                            .flat_map(|new_reactions| new_reactions.iter())
                            .skip(pos)
                            .map(|(key, group_reactions)| {
                                let group = ReactionGroup::new(key, user);
                                group.update(group_reactions);
                                (key.clone(), group)
                            }),
                    );
                }

                (removed, added)
            };

            if removed != 0 || added != 0 {
                self.obj()
                    .items_changed(pos as u32, removed as u32, added as u32);
            }
        }

        /// Get a reaction group by its key.
        ///
        /// Returns `None` if no action group was found with this key.
        pub(super) fn reaction_group_by_key(&self, key: &str) -> Option<ReactionGroup> {
            self.reactions.borrow().get(key).cloned()
        }
    }
}

glib::wrapper! {
    /// List of all `ReactionGroup`s for an event.
    ///
    /// Implements `GListModel`. `ReactionGroup`s are sorted in "insertion order".
    pub struct ReactionList(ObjectSubclass<imp::ReactionList>)
        @implements gio::ListModel;
}

impl ReactionList {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Update the reaction list with the given reactions.
    pub(crate) fn update(&self, new_reactions: Option<&ReactionsByKeyBySender>) {
        self.imp().update(new_reactions);
    }

    /// Get a reaction group by its key.
    ///
    /// Returns `None` if no action group was found with this key.
    pub(crate) fn reaction_group_by_key(&self, key: &str) -> Option<ReactionGroup> {
        self.imp().reaction_group_by_key(key)
    }
}

impl Default for ReactionList {
    fn default() -> Self {
        Self::new()
    }
}
