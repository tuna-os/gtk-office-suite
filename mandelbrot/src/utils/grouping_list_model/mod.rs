use std::{cmp::Ordering, fmt, ops::RangeInclusive};

use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};

mod group;
#[cfg(test)]
mod tests;

pub(crate) use self::group::GroupingListGroup;
use crate::utils::BoundObject;

/// A function to determine if an item should be grouped with another contiguous
/// item.
///
/// This function MUST always return `true` when used with any two items in the
/// same group.
pub(crate) type GroupFn = dyn Fn(&glib::Object, &glib::Object) -> bool;

mod imp {
    use std::{
        cell::{OnceCell, RefCell},
        collections::{HashSet, VecDeque},
    };

    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::GroupingListModel)]
    pub struct GroupingListModel {
        /// The underlying model.
        #[property(get, set = Self::set_model, explicit_notify, nullable)]
        model: BoundObject<gio::ListModel>,
        /// The function to determine if adjacent items should be grouped.
        pub(super) group_fn: OnceCell<Box<GroupFn>>,
        /// The groups created by this list model.
        items: RefCell<VecDeque<GroupingListItem>>,
    }

    impl fmt::Debug for GroupingListModel {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("GroupingListModel")
                .field("model", &self.model)
                .field("items", &self.items)
                .finish_non_exhaustive()
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GroupingListModel {
        const NAME: &'static str = "GroupingListModel";
        type Type = super::GroupingListModel;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for GroupingListModel {}

    impl ListModelImpl for GroupingListModel {
        fn item_type(&self) -> glib::Type {
            glib::Object::static_type()
        }

        fn n_items(&self) -> u32 {
            self.items.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            let model = self.model.obj()?;
            self.items
                .borrow()
                .get(position as usize)
                .and_then(|item| match item {
                    GroupingListItem::Singleton(position) => model.item(*position),
                    GroupingListItem::Group(obj) => Some(obj.clone().upcast()),
                })
        }
    }

    impl GroupingListModel {
        /// The function to determine if adjacent items should be grouped.
        fn group_fn(&self) -> &GroupFn {
            self.group_fn.get().expect("group Fn should be initialized")
        }

        /// Set the underlying model.
        fn set_model(&self, model: Option<gio::ListModel>) {
            let prev_model = self.model.obj();

            if prev_model == model {
                return;
            }

            self.model.disconnect_signals();

            if let Some(model) = model {
                let items_changed_handler = model.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |model, position, removed, added| {
                        imp.items_changed(model, position, removed, added);
                    }
                ));

                self.model.set(model.clone(), vec![items_changed_handler]);

                let removed = prev_model.map(|model| model.n_items()).unwrap_or_default();
                self.items_changed(&model, 0, removed, model.n_items());
            } else {
                let removed = self.n_items();
                self.items.borrow_mut().clear();
                self.obj().items_changed(0, removed, 0);
            }

            self.obj().notify_model();
        }

        /// Find the index of the list item containing the given position in the
        /// underlying model.
        fn model_position_to_index(&self, position: u32) -> Option<usize> {
            for (index, item) in self.items.borrow().iter().enumerate() {
                if item.contains(position) {
                    return Some(index);
                }

                // Because items are sorted, we can return early when the position is in a gap
                // between items. This should only happen during `items_changed`.
                if item.end() > position {
                    return None;
                }
            }

            None
        }

        /// Handle when items have changed in the underlying model.
        #[allow(clippy::too_many_lines)]
        fn items_changed(&self, model: &gio::ListModel, position: u32, removed: u32, added: u32) {
            if removed == 0 && added == 0 {
                // Nothing to do.
                return;
            }

            // Index of the list item that contains the item right before the changes in the
            // model.
            let index_before_changes = position
                .checked_sub(1)
                .and_then(|position| self.model_position_to_index(position));

            let mut replaced_list_items = HashSet::new();

            let mut list_items_removed =
                self.items_removed(position, removed, index_before_changes);
            let mut list_items_added = self.items_added(
                model,
                position,
                added,
                &mut replaced_list_items,
                index_before_changes,
            );

            let position_after_changes = position + added;
            let mut index_after_changes = self.model_position_to_index(position_after_changes);

            // Check if the list item after the changes can be merged with the previous list
            // item.
            if let Some(index_after_changes) =
                index_after_changes.as_mut().filter(|index| **index > 0)
            {
                let mut items = self.items.borrow_mut();

                let previous_item_in_other_list_item = !items
                    .get(*index_after_changes)
                    .expect("list item index should be valid")
                    .contains(position_after_changes - 1);

                if previous_item_in_other_list_item {
                    let item_after_changes = model
                        .item(position_after_changes)
                        .expect("item position should be valid");
                    let previous_item = model
                        .item(position_after_changes - 1)
                        .expect("item position should be valid");

                    if self.group_fn()(&item_after_changes, &previous_item) {
                        // We can merge the items.
                        *index_after_changes -= 1;

                        let (removed_list_item, list_item_to_merge_into) = if index_before_changes
                            .is_some_and(|index| index == *index_after_changes)
                        {
                            // Merge into the list item before changes.
                            let list_item_after_changes = items
                                .remove(*index_after_changes + 1)
                                .expect("list item index should be valid");
                            let list_item_before_changes = items
                                .get_mut(*index_after_changes)
                                .expect("list item index should be valid");
                            (list_item_after_changes, list_item_before_changes)
                        } else {
                            // Merge into the list item after changes.
                            let previous_list_item = items
                                .remove(*index_after_changes)
                                .expect("list item index should be valid");
                            let list_item_after_changes = items
                                .get_mut(*index_after_changes)
                                .expect("list item index should be valid");
                            (previous_list_item, list_item_after_changes)
                        };

                        let list_item_replacement = list_item_to_merge_into.add(
                            removed_list_item.start(),
                            removed_list_item.len(),
                            model,
                        );

                        if let Some(replacement) = list_item_replacement {
                            *list_item_to_merge_into = replacement;
                            replaced_list_items.insert(*index_after_changes);
                        }

                        if let Some(added) = list_items_added.checked_sub(1) {
                            list_items_added = added;
                        } else {
                            list_items_removed += 1;
                        }
                    }
                }
            }

            let obj = self.obj();

            if list_items_removed > 0 || list_items_added > 0 {
                let index_at_changes = index_before_changes
                    .map(|index| index + 1)
                    .unwrap_or_default();

                // Drop the batches of the new groups, we do not want to send signals about
                // changed items later for them.
                self.items
                    .borrow()
                    .range(index_at_changes..index_at_changes + list_items_added)
                    .for_each(|list_item| match list_item {
                        GroupingListItem::Singleton(_) => {}
                        GroupingListItem::Group(group) => group.drop_batch(),
                    });

                obj.items_changed(
                    index_at_changes as u32,
                    list_items_removed as u32,
                    list_items_added as u32,
                );
            }

            // Change groups with a single item to singletons.
            for index in index_before_changes.into_iter().chain(index_after_changes) {
                let mut items = self.items.borrow_mut();
                let item = items
                    .get_mut(index)
                    .expect("list item index should be valid");

                if matches!(item, GroupingListItem::Group(_)) && item.len() == 1 {
                    *item = GroupingListItem::Singleton(item.start());
                    replaced_list_items.insert(index);
                }
            }

            for index in replaced_list_items {
                obj.items_changed(index as u32, 1, 1);
            }

            // Generate a list of groups before processing the batches, to avoid holding a
            // ref while we send signals about changed items.
            let groups = {
                let items = self.items.borrow();

                let first_possible_group_with_changes_index =
                    index_before_changes.unwrap_or_default();
                let after_last_possible_group_with_changes_index =
                    (first_possible_group_with_changes_index + list_items_added + 2)
                        .min(items.len());

                items
                    .range(
                        first_possible_group_with_changes_index
                            ..after_last_possible_group_with_changes_index,
                    )
                    .filter_map(|list_item| match list_item {
                        GroupingListItem::Singleton(_) => None,
                        GroupingListItem::Group(group) => group.has_batch().then(|| group.clone()),
                    })
                    .collect::<Vec<_>>()
            };

            for group in groups {
                group.process_batch();
            }
        }

        /// Handle when items were removed in the underlying model.
        ///
        /// Returns a `(position, removed)` tuple if items were removed in this
        /// list.
        fn items_removed(
            &self,
            position: u32,
            removed: u32,
            index_before_changes: Option<usize>,
        ) -> usize {
            if removed == 0 {
                // Nothing to do.
                return 0;
            }

            // Index of the list item that contains the item right after the changes in the
            // model.
            let index_after_changes = position
                .checked_add(removed)
                .and_then(|position| self.model_position_to_index(position));

            let mut items = self.items.borrow_mut();

            // Update the range of the list item before changes, if it's not the same as the
            // list item after the changes and if it contains removed items.
            if let Some(index_before_changes) = index_before_changes.filter(|index| {
                index_after_changes.is_none_or(|index_after_changes| index_after_changes > *index)
            }) {
                items
                    .get_mut(index_before_changes)
                    .expect("list item index should be valid")
                    .handle_removal(position, removed);
            }

            // Update the range of the list items after the changes.
            if let Some(index_after_changes) = index_after_changes {
                items
                    .range_mut(index_after_changes..)
                    .for_each(|list_item| list_item.handle_removal(position, removed));
            }

            // If items were removed, we should have at least one list item.
            let max_index = items.len() - 1;

            let removal_start = if let Some(index_before_changes) = index_before_changes {
                // The list items removal starts at the list item after the one before the
                // changes.
                index_before_changes
                    .checked_add(1)
                    .filter(|index| *index <= max_index)
            } else {
                // There is no list item before, we are at the start of the list items.
                Some(0)
            };

            let removal_end = if let Some(index_after_changes) = index_after_changes {
                // The list items removal starts at the list item before the one after the
                // changes.
                index_after_changes.checked_sub(1)
            } else {
                // There is no list item after, we are at the end of the list items.
                Some(max_index)
            };

            // Remove list items if needed.
            let Some((removal_start, removal_end)) = removal_start
                .zip(removal_end)
                .filter(|(removal_start, removal_end)| removal_start <= removal_end)
            else {
                return 0;
            };

            let is_at_items_start = removal_start == 0;
            let is_at_items_end = removal_end == items.len().saturating_sub(1);

            // Try to optimize the removal by using the most appropriate `VecDeque` method.
            if is_at_items_start && is_at_items_end {
                // Remove all items.
                items.clear();
            } else if is_at_items_end {
                // Remove the end of the items.
                items.truncate(removal_start);
            } else {
                // We can only remove each item separately.
                for i in (removal_start..=removal_end).rev() {
                    items.remove(i);
                }
            }

            removal_end - removal_start + 1
        }

        /// Handle when items were added to the underlying model.
        ///
        /// Returns the number of items that were added, if any.
        fn items_added(
            &self,
            model: &gio::ListModel,
            position: u32,
            added: u32,
            replaced_list_items: &mut HashSet<usize>,
            index_before_changes: Option<usize>,
        ) -> usize {
            if added == 0 {
                // Nothing to do.
                return 0;
            }

            let mut list_items_added = 0;

            let position_before = position.checked_sub(1);
            // The previous item in the underlying model and the index of the list item that
            // contains it.
            let mut previous_item_and_index =
                position_before.and_then(|position| model.item(position).zip(index_before_changes));

            let group_fn = self.group_fn();
            let mut items = self.items.borrow_mut();

            for current_position in position..position + added {
                let item = model
                    .item(current_position)
                    .expect("item position should be valid");

                if let Some((previous_item, previous_index)) = &mut previous_item_and_index {
                    let previous_list_item = items
                        .get_mut(*previous_index)
                        .expect("list item index should be valid");

                    if group_fn(&item, previous_item) {
                        // Add the position to the list item.
                        let list_item_replacement =
                            previous_list_item.add(current_position, 1, model);

                        if let Some(replacement) = list_item_replacement {
                            *previous_list_item = replacement;

                            if current_position == position {
                                // We will need to send a signal because we replaced a list item
                                // that already existed.
                                replaced_list_items.insert(*previous_index);
                            }
                        }

                        // The previous item changed but the list item that contains it is the same.
                        *previous_item = item;

                        continue;
                    } else if previous_list_item.contains(current_position) {
                        // We need to split the group.
                        let end_list_item = previous_list_item.split(current_position);

                        items.insert(*previous_index + 1, end_list_item);
                        list_items_added += 1;
                    }
                }

                // The item is a singleton.
                let index = previous_item_and_index
                    .take()
                    .map(|(_, index)| index + 1)
                    .unwrap_or_default();
                items.insert(index, GroupingListItem::Singleton(current_position));
                list_items_added += 1;

                previous_item_and_index = Some((item, index));
            }

            let (_, last_index_with_changes) =
                previous_item_and_index.expect("there should have been at least one addition");
            let index_after_changes = last_index_with_changes + 1;

            // Update the ranges of the list items after the changes.
            if index_after_changes < items.len() {
                items
                    .range_mut(index_after_changes..)
                    .for_each(|list_item| list_item.handle_addition(position, added));
            }

            list_items_added
        }
    }
}

glib::wrapper! {
    /// A list model that groups some items according to a function.
    pub struct GroupingListModel(ObjectSubclass<imp::GroupingListModel>)
        @implements gio::ListModel;
}

impl GroupingListModel {
    /// Construct a new `GroupingListModel` with the given function to determine
    /// if adjacent items should be grouped.
    pub fn new<GroupFn>(group_fn: GroupFn) -> Self
    where
        GroupFn: Fn(&glib::Object, &glib::Object) -> bool + 'static,
    {
        let obj = glib::Object::new::<Self>();
        // Ignore the error because we cannot `.expect()` when the value is a function.
        let _ = obj.imp().group_fn.set(Box::new(group_fn));
        obj
    }
}

/// An item in the [`GroupingListModel`].
#[derive(Debug, Clone)]
enum GroupingListItem {
    /// An item that is not in a group.
    Singleton(u32),

    /// A group of items.
    Group(GroupingListGroup),
}

impl GroupingListItem {
    /// Construct a list item with the given range for the given model.
    fn with_range(range: RangeInclusive<u32>, model: &gio::ListModel) -> Self {
        if range.start() == range.end() {
            Self::Singleton(*range.start())
        } else {
            Self::Group(GroupingListGroup::new(model, range))
        }
    }

    /// Whether this list item contains the given position.
    fn contains(&self, position: u32) -> bool {
        match self {
            Self::Singleton(pos) => *pos == position,
            Self::Group(group) => group.contains(position),
        }
    }

    /// The position of the first item in this list item.
    fn start(&self) -> u32 {
        match self {
            Self::Singleton(position) => *position,
            Self::Group(group) => group.start(),
        }
    }

    /// The position of the last item in this list item.
    fn end(&self) -> u32 {
        match self {
            Self::Singleton(position) => *position,
            Self::Group(group) => group.end(),
        }
    }

    /// The length of the range of this list item.
    fn len(&self) -> u32 {
        match self {
            Self::Singleton(_) => 1,
            Self::Group(group) => {
                let (start, end) = group.bounds();
                end - start + 1
            }
        }
    }

    /// Handle the given removal of items that might affect this group.
    ///
    /// This function panics if there would be no items left in this group.
    fn handle_removal(&mut self, position: u32, removed: u32) {
        match self {
            Self::Singleton(pos) => match position.cmp(pos) {
                Ordering::Less => *pos -= removed,
                Ordering::Equal => panic!("should not remove list item"),
                Ordering::Greater => {}
            },
            Self::Group(group) => group.handle_removal(position, removed),
        }
    }

    /// Handle the given addition of items that might affect this group.
    fn handle_addition(&mut self, position: u32, added: u32) {
        match self {
            Self::Singleton(pos) => {
                if position <= *pos {
                    *pos += added;
                }
            }
            Self::Group(group) => group.handle_addition(position, added),
        }
    }

    /// Add items to this list item.
    ///
    /// Returns the newly created group if this item was a singleton.
    ///
    /// Panics if the added items are not contiguous to the current ones.
    fn add(&self, position: u32, added: u32, model: &gio::ListModel) -> Option<Self> {
        debug_assert!(
            (position + added) >= self.start() || position <= self.end().saturating_add(1),
            "items to add should be contiguous"
        );

        match self {
            Self::Singleton(pos) => {
                let start = position.min(*pos);
                let end = start + added;
                Some(Self::with_range(start..=end, model))
            }
            Self::Group(group) => {
                group.add(position, added);
                None
            }
        }
    }

    /// Split this group at the given position.
    ///
    /// `position` must be greater than the start and lower or equal to the end
    /// of this group.
    ///
    /// Returns the new list item containing the second part of the group,
    /// starting at `at`.
    fn split(&self, position: u32) -> Self {
        match self {
            Self::Singleton(_) => panic!("singleton cannot be split"),
            Self::Group(group) => {
                let model = group.model().expect("model should be initialized");
                let end = group.end();

                group.handle_removal(position, end - position + 1);

                Self::with_range(position..=end, &model)
            }
        }
    }
}
