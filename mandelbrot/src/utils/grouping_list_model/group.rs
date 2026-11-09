use std::{cmp::Ordering, ops::RangeInclusive};

use gtk::{gio, glib, prelude::*, subclass::prelude::*};

mod imp {
    use std::{cell::RefCell, marker::PhantomData};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::GroupingListGroup)]
    pub struct GroupingListGroup {
        /// The underlying model.
        #[property(get, set = Self::set_model, construct_only)]
        model: glib::WeakRef<gio::ListModel>,
        /// The range of items in this group.
        pub(super) range: RefCell<Option<RangeInclusive<u32>>>,
        /// The position of the first item in this group.
        #[property(get = Self::start)]
        start: PhantomData<u32>,
        /// The position of the last item in this group.
        #[property(get = Self::end)]
        end: PhantomData<u32>,
        /// The batch of changes that have not been signalled yet.
        pub(super) batch: RefCell<Vec<ChangesBatch>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GroupingListGroup {
        const NAME: &'static str = "GroupingListGroup";
        type Type = super::GroupingListGroup;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for GroupingListGroup {}

    impl ListModelImpl for GroupingListGroup {
        fn item_type(&self) -> glib::Type {
            self.model
                .upgrade()
                .map_or_else(glib::Object::static_type, |model| model.item_type())
        }

        fn n_items(&self) -> u32 {
            self.range
                .borrow()
                .as_ref()
                .map(|range| range.end() - range.start() + 1)
                .unwrap_or_default()
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            if position >= self.n_items() {
                return None;
            }

            self.model
                .upgrade()
                .and_then(|m| m.item(self.start() + position))
        }
    }

    impl GroupingListGroup {
        /// Set the underlying model.
        fn set_model(&self, model: &gio::ListModel) {
            if self.model.upgrade().is_some_and(|prev| prev == *model) {
                return;
            }

            self.model.set(Some(model));

            self.obj().notify_model();
        }

        /// Set the range of items in this group.
        pub(super) fn set_range(&self, range: RangeInclusive<u32>) {
            if self
                .range
                .borrow()
                .as_ref()
                .is_some_and(|prev| *prev == range)
            {
                return;
            }

            let items_changes = if let Some(prev) = self.range.take() {
                let prev_start = *prev.start();
                let new_start = *range.start();
                let start_change = match new_start.cmp(&prev_start) {
                    Ordering::Less => Some((0, 0, prev_start - new_start)),
                    Ordering::Equal => None,
                    Ordering::Greater => Some((0, new_start - prev_start, 0)),
                };

                let prev_end = *prev.end();
                let new_end = *range.end();
                let end_change = match new_end.cmp(&prev_end) {
                    Ordering::Less => Some((new_end, prev_end - new_end, 0)),
                    Ordering::Equal => None,
                    Ordering::Greater => Some((prev_end, 0, new_end - prev_end)),
                };

                [start_change, end_change]
            } else {
                [Some((0, 0, range.end() - range.start())), None]
            };

            self.range.replace(Some(range));

            let obj = self.obj();
            for change in items_changes {
                let Some((pos, removed, added)) = change else {
                    continue;
                };

                obj.items_changed(pos, removed, added);
            }
        }

        /// The position of the first item in this group.
        fn start(&self) -> u32 {
            *self
                .range
                .borrow()
                .as_ref()
                .expect("range should be initialized")
                .start()
        }

        /// The position of the last item in this group.
        fn end(&self) -> u32 {
            *self
                .range
                .borrow()
                .as_ref()
                .expect("range should be initialized")
                .end()
        }

        /// The bounds of this group.
        pub(super) fn bounds(&self) -> (u32, u32) {
            self.range
                .borrow()
                .as_ref()
                .map(|range| (*range.start(), *range.end()))
                .expect("range should be initialized")
        }

        /// Add a change to the batch.
        pub(super) fn push_change(&self, change: ChangesBatch) {
            let mut batch = self.batch.borrow_mut();

            match &change {
                ChangesBatch::Remove(_) => batch.push(change),
                ChangesBatch::Add(add) => {
                    // Merge the change with the previous one if they are contiguous.
                    if let Some(prev_change) =
                        batch.last_mut().and_then(|previous| match previous {
                            ChangesBatch::Remove(_) => None,
                            ChangesBatch::Add(prev_change) => {
                                ((prev_change.position + prev_change.added) == add.position)
                                    .then_some(prev_change)
                            }
                        })
                    {
                        prev_change.added += add.added;
                    } else {
                        batch.push(change);
                    }
                }
            }
        }
    }
}

glib::wrapper! {
    /// A group of items in a [`GroupingListModel`].
    ///
    /// [`GroupingListModel`]: super::GroupingListModel
    pub struct GroupingListGroup(ObjectSubclass<imp::GroupingListGroup>)
        @implements gio::ListModel;
}

impl GroupingListGroup {
    /// Construct a new `GroupingListGroup` for the given model and range.
    pub fn new(model: &gio::ListModel, range: RangeInclusive<u32>) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("model", model)
            .build();
        obj.imp().set_range(range);
        obj
    }

    /// The bounds of this group.
    pub(super) fn bounds(&self) -> (u32, u32) {
        self.imp().bounds()
    }

    /// Whether this group contains the given position.
    pub(super) fn contains(&self, position: u32) -> bool {
        self.imp()
            .range
            .borrow()
            .as_ref()
            .is_some_and(|range| range.contains(&position))
    }

    /// Handle the given removal of items that might affect this group.
    ///
    /// This function panics if there would be no items left in this group.
    pub(super) fn handle_removal(&self, position: u32, removed: u32) {
        if removed == 0 {
            // Nothing to do.
            return;
        }

        let imp = self.imp();
        let (start, end) = imp.bounds();

        if position > end {
            // This group is not affected.
            return;
        }

        let removal_end = position + removed - 1;

        assert!(
            position > start || removal_end < end,
            "should not remove whole group",
        );

        let (remove, new_range) = if removal_end < start {
            let new_range = (start - removed)..=(end - removed);
            (None, new_range)
        } else if position <= start {
            let remove = RemoveBatch {
                position: 0,
                removed: removal_end - start + 1,
            };
            let new_range = position..=(end - removed);
            (Some(remove), new_range)
        } else if position > start && removal_end <= end {
            let remove = RemoveBatch {
                position: position - start,
                removed,
            };
            let new_range = start..=(end - removed);
            (Some(remove), new_range)
        } else {
            // position > start && removal_end > end
            let remove = RemoveBatch {
                position: position - start,
                removed: end - position + 1,
            };
            #[allow(clippy::range_minus_one)] // We need an inclusive range.
            let new_range = start..=(position - 1);
            (Some(remove), new_range)
        };

        if let Some(remove) = remove {
            imp.push_change(remove.into());
        }

        *imp.range.borrow_mut() = Some(new_range);
    }

    /// Handle the given addition of items that might affect this group.
    pub(super) fn handle_addition(&self, position: u32, added: u32) {
        if added == 0 {
            // Nothing to do.
            return;
        }

        let imp = self.imp();
        let (start, end) = imp.bounds();

        if position > end {
            // This group is not affected.
            return;
        }

        let (add, new_range) = if position <= start {
            let new_range = (start + added)..=(end + added);
            (None, new_range)
        } else {
            // start < position <= end
            let add = AddBatch {
                position: position - start,
                added,
            };
            let new_range = start..=(end + added);
            (Some(add), new_range)
        };

        if let Some(add) = add {
            imp.push_change(add.into());
        }

        *imp.range.borrow_mut() = Some(new_range);
    }

    /// Add items to this list item.
    ///
    /// This function assumes that the new items are contiguous to the current
    /// ones.
    pub(super) fn add(&self, position: u32, added: u32) {
        if added == 0 {
            // Nothing to do.
            return;
        }

        let imp = self.imp();
        let (start, end) = imp.bounds();

        let (add, new_range) = if position <= start {
            let new_range = position..=end;
            let add = AddBatch { position: 0, added };
            (add, new_range)
        } else {
            // start < position <= end
            let add = AddBatch {
                position: position - start,
                added,
            };
            let new_range = start..=(end + added);
            (add, new_range)
        };

        imp.push_change(add.into());
        *imp.range.borrow_mut() = Some(new_range);
    }

    /// Whether this group has an accumulated batch of changes.
    pub(super) fn has_batch(&self) -> bool {
        !self.imp().batch.borrow().is_empty()
    }

    /// Process the accumulated batch of changes.
    pub(super) fn process_batch(&self) {
        let batch = self.imp().batch.take();

        // Do not process removals right away, to batch a removal with the corresponding
        // addition, if any.
        let mut previous_remove = None;

        for changes in batch {
            match changes {
                ChangesBatch::Remove(remove) => {
                    if let Some(remove) = previous_remove.replace(remove) {
                        self.items_changed(remove.position, remove.removed, 0);
                    }
                }
                ChangesBatch::Add(add) => {
                    let removed = if let Some(remove) = previous_remove.take() {
                        if remove.position == add.position {
                            remove.removed
                        } else {
                            self.items_changed(remove.position, remove.removed, 0);
                            0
                        }
                    } else {
                        0
                    };
                    self.items_changed(add.position, removed, add.added);
                }
            }
        }

        if let Some(remove) = previous_remove.take() {
            self.items_changed(remove.position, remove.removed, 0);
        }
    }

    /// Drop the accumulated batch of changes.
    pub(super) fn drop_batch(&self) {
        self.imp().batch.take();
    }
}

// A batch of changes.
#[derive(Debug, Clone, Copy)]
enum ChangesBatch {
    // Remove items.
    Remove(RemoveBatch),
    // Add items.
    Add(AddBatch),
}

impl From<RemoveBatch> for ChangesBatch {
    fn from(value: RemoveBatch) -> Self {
        Self::Remove(value)
    }
}

impl From<AddBatch> for ChangesBatch {
    fn from(value: AddBatch) -> Self {
        Self::Add(value)
    }
}

// A batch of removals.
#[derive(Debug, Clone, Copy)]
struct RemoveBatch {
    // The position of the first item that was removed.
    position: u32,
    // The number of items that were removed.
    removed: u32,
}

// A batch of additions.
#[derive(Debug, Clone, Copy)]
struct AddBatch {
    // The position of the first item that was added.
    position: u32,
    // The number of items that were added.
    added: u32,
}
