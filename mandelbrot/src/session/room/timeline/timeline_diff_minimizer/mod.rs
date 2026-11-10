use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use gtk::prelude::*;
use matrix_sdk_ui::{eyeball_im::VectorDiff, timeline::TimelineItem as SdkTimelineItem};

mod tests;

use super::TimelineItem;

/// Trait to access data from a type that stores [`TimelineDiffItem`]s.
pub(super) trait TimelineDiffItemStore: Sized {
    type Item: TimelineDiffItem + std::fmt::Debug;
    type Data: TimelineDiffItemData;

    /// The current list of items.
    fn items(&self) -> Vec<Self::Item>;

    /// Create a `TimelineItem` with the given `TimelineItemData`.
    fn create_item(&self, data: &Self::Data) -> Self::Item;

    /// Update the given item with the given timeline ID.
    fn update_item(&self, item: &Self::Item, data: &Self::Data);

    /// Apply the given list of item diffs to this store.
    fn apply_item_diff_list(&self, item_diff_list: Vec<TimelineDiff<Self::Item>>);

    /// Whether the given diff list can be minimized by calling
    /// `minimize_diff_list`.
    ///
    /// It can be minimized if there is more than 1 item in the list and if the
    /// list only includes supported `VectorDiff` variants.
    fn can_minimize_diff_list(&self, diff_list: &[VectorDiff<Self::Data>]) -> bool {
        diff_list.len() > 1
            && !diff_list.iter().any(|diff| {
                matches!(
                    diff,
                    VectorDiff::Clear | VectorDiff::Truncate { .. } | VectorDiff::Reset { .. }
                )
            })
    }

    /// Minimize the given diff list and apply it to this store.
    ///
    /// Panics if the diff list contains unsupported `VectorDiff` variants. This
    /// will never panic if `can_minimize_diff_list` returns `true`.
    fn minimize_diff_list(&self, diff_list: Vec<VectorDiff<Self::Data>>) {
        TimelineDiffMinimizer::new(self).apply(diff_list);
    }
}

/// Trait implemented by types that provide data for [`TimelineDiffItem`]s.
pub(super) trait TimelineDiffItemData {
    /// The unique timeline ID of the data.
    fn timeline_id(&self) -> &str;
}

impl TimelineDiffItemData for SdkTimelineItem {
    fn timeline_id(&self) -> &str {
        &self.unique_id().0
    }
}

impl<T> TimelineDiffItemData for Arc<T>
where
    T: TimelineDiffItemData,
{
    fn timeline_id(&self) -> &str {
        (**self).timeline_id()
    }
}

/// Trait implemented by items in the timeline.
pub(super) trait TimelineDiffItem: Clone {
    /// The unique timeline ID of the item.
    fn timeline_id(&self) -> String;
}

impl<T> TimelineDiffItem for T
where
    T: IsA<TimelineItem>,
{
    fn timeline_id(&self) -> String {
        self.upcast_ref().timeline_id()
    }
}

/// A helper struct to minimize a list of `VectorDiff`.
///
/// This does not support `VectorDiff::Clear`, `VectorDiff::Truncate` and
/// `VectorDiff::Reset` as we assume that lists including those cannot be
/// minimized in an optimal way.
struct TimelineDiffMinimizer<'a, S, I> {
    store: &'a S,
    item_map: HashMap<String, I>,
    updated_item_ids: HashSet<String>,
}

impl<'a, S, I> TimelineDiffMinimizer<'a, S, I> {
    /// Construct a `TimelineDiffMinimizer` with the given store.
    fn new(store: &'a S) -> Self {
        Self {
            store,
            item_map: HashMap::new(),
            updated_item_ids: HashSet::new(),
        }
    }
}

impl<S, I> TimelineDiffMinimizer<'_, S, I>
where
    S: TimelineDiffItemStore<Item = I>,
    I: TimelineDiffItem + std::fmt::Debug,
{
    /// Load the items from the store.
    ///
    /// Returns the list of timeline IDs of the items.
    fn load_items(&mut self) -> Vec<String> {
        let items = self.store.items();
        let item_ids = items.iter().map(S::Item::timeline_id).collect();

        self.item_map
            .extend(items.into_iter().map(|item| (item.timeline_id(), item)));

        item_ids
    }

    /// Update or create an item in the store using the given data.
    ///
    /// Returns the timeline ID of the item.
    fn update_or_create_item(&mut self, data: &S::Data) -> String {
        let timeline_id = data.timeline_id().to_owned();
        self.item_map
            .entry(timeline_id)
            .and_modify(|item| {
                self.store.update_item(item, data);
                self.updated_item_ids.insert(item.timeline_id());
            })
            .or_insert_with(|| self.store.create_item(data))
            .timeline_id()
    }

    /// Apply the given diff to the given items.
    fn apply_diff_to_items(
        &mut self,
        item_ids: &[String],
        diff_list: Vec<VectorDiff<S::Data>>,
    ) -> Vec<String> {
        let mut new_item_ids = VecDeque::from(item_ids.to_owned());

        // Get the new state by applying the diffs.
        for diff in diff_list {
            match diff {
                VectorDiff::Append { values } => {
                    let items = values
                        .into_iter()
                        .map(|data| self.update_or_create_item(data));
                    new_item_ids.extend(items);
                }
                VectorDiff::PushFront { value } => {
                    let item = self.update_or_create_item(&value);
                    new_item_ids.push_front(item);
                }
                VectorDiff::PushBack { value } => {
                    let item = self.update_or_create_item(&value);
                    new_item_ids.push_back(item);
                }
                VectorDiff::PopFront => {
                    new_item_ids.pop_front();
                }
                VectorDiff::PopBack => {
                    new_item_ids.pop_back();
                }
                VectorDiff::Insert { index, value } => {
                    let item = self.update_or_create_item(&value);
                    new_item_ids.insert(index, item);
                }
                VectorDiff::Set { index, value } => {
                    let item_id = self.update_or_create_item(&value);
                    *new_item_ids
                        .get_mut(index)
                        .expect("an item should already exist at the given index") = item_id;
                }
                VectorDiff::Remove { index } => {
                    new_item_ids.remove(index);
                }
                VectorDiff::Clear | VectorDiff::Truncate { .. } | VectorDiff::Reset { .. } => {
                    unreachable!()
                }
            }
        }

        new_item_ids.into()
    }

    /// Compute the list of item diffs between the two given lists.
    ///
    /// Uses a diff algorithm to minimize the removals and additions.
    fn item_diff_list(
        &self,
        old_item_ids: &[String],
        new_item_ids: &[String],
    ) -> Vec<TimelineDiff<S::Item>> {
        if old_item_ids == new_item_ids && self.updated_item_ids.is_empty() {
            // No items have changed.
            return Vec::new();
        }

        // First, ignore unchanged items at the beginning and end of the list, it will
        // make the diff algorithm faster.
        let start = old_item_ids
            .iter()
            .zip(new_item_ids)
            .position(|(old_item_id, new_item_id)| {
                old_item_id != new_item_id || self.updated_item_ids.contains(old_item_id)
            })
            .unwrap_or_default();
        let end = old_item_ids[start..]
            .iter()
            .rev()
            .zip(new_item_ids[start..].iter().rev())
            .position(|(old_item_id, new_item_id)| {
                old_item_id != new_item_id || self.updated_item_ids.contains(old_item_id)
            })
            .unwrap_or_default();

        let old_item_ids = &old_item_ids[start..old_item_ids.len() - end];
        let new_item_ids = &new_item_ids[start..new_item_ids.len() - end];

        // Get the per-item diff.
        let per_item_diff = Self::per_item_diff(old_item_ids, new_item_ids);

        // Now we can batch the diffs.
        let mut diff_batches = TimelineDiffBatches::new();
        let mut pos = start as u32;

        for item_diff in per_item_diff {
            match item_diff {
                ItemDiff::Added(timeline_id) => {
                    let item = self
                        .item_map
                        .get(timeline_id)
                        .expect("item should exist in map")
                        .clone();

                    diff_batches.push_addition(&mut pos, item);
                }
                ItemDiff::Removed => {
                    diff_batches.push_removal(&mut pos);
                }
                ItemDiff::Common(timeline_id) => {
                    if self.updated_item_ids.contains(timeline_id) {
                        diff_batches.push_update(&mut pos);
                    } else {
                        diff_batches.skip_item(&mut pos);
                    }
                }
            }
        }

        diff_batches.finalize()
    }

    /// Get a per-item diff by implementing a simple longest common subsequence
    /// (LCS) algorithm to find the minimal diff. Given that the list should not
    /// be too long, we do not need an efficient and more complicated algorithm.
    ///
    /// Source: <https://en.wikipedia.org/wiki/Longest_common_subsequence>
    fn per_item_diff<'a>(
        old_item_ids: &'a [String],
        new_item_ids: &'a [String],
    ) -> Vec<ItemDiff<'a>> {
        // Early return if either of the lists is empty.
        if old_item_ids.is_empty() {
            return new_item_ids
                .iter()
                .map(|item_id| ItemDiff::Added(item_id.as_str()))
                .collect();
        }
        if new_item_ids.is_empty() {
            return old_item_ids.iter().map(|_| ItemDiff::Removed).collect();
        }

        // First, we create a table and fill it to track the differences.
        let mut table = Table::new(old_item_ids.len() + 1, new_item_ids.len() + 1);

        for (row, old_item_id) in old_item_ids.iter().enumerate() {
            for (col, new_item_id) in new_item_ids.iter().enumerate() {
                table.set(
                    row + 1,
                    col + 1,
                    if old_item_id == new_item_id {
                        table.get(row, col) + 1
                    } else {
                        table.get(row, col + 1).max(table.get(row + 1, col))
                    },
                );
            }
        }

        // Then we go through the table from the bottom-right and go back to the
        // top-left.
        let mut row = old_item_ids.len();
        let mut col = new_item_ids.len();
        let mut diff = Vec::with_capacity(row.max(col));

        loop {
            if col > 0 && (row == 0 || table.get(row, col) == table.get(row, col - 1)) {
                col -= 1;
                diff.push(ItemDiff::Added(&new_item_ids[col]));
            } else if row > 0 && (col == 0 || table.get(row, col) == table.get(row - 1, col)) {
                row -= 1;
                diff.push(ItemDiff::Removed);
            } else if row > 0 && col > 0 {
                row -= 1;
                col -= 1;
                diff.push(ItemDiff::Common(&old_item_ids[row]));
            } else {
                break;
            }
        }

        // Reverse the list to go from beginning to end now.
        diff.reverse();

        diff
    }

    /// Minimize the given diff and apply it to the store.
    fn apply(mut self, diff_list: Vec<VectorDiff<S::Data>>) {
        let old_item_ids = self.load_items();
        let new_item_ids = self.apply_diff_to_items(&old_item_ids, diff_list);
        let item_diff_list = self.item_diff_list(&old_item_ids, &new_item_ids);
        self.store.apply_item_diff_list(item_diff_list);
    }
}

/// A minimized diff for timeline items.
#[derive(Debug, Clone)]
pub(super) enum TimelineDiff<T> {
    /// Remove then add items.
    Splice(SpliceDiff<T>),

    /// Update items.
    Update(UpdateDiff),
}

impl<T> From<SpliceDiff<T>> for TimelineDiff<T> {
    fn from(value: SpliceDiff<T>) -> Self {
        Self::Splice(value)
    }
}

impl<T> From<UpdateDiff> for TimelineDiff<T> {
    fn from(value: UpdateDiff) -> Self {
        Self::Update(value)
    }
}

/// A diff to remove then add items.
#[derive(Debug, Clone)]
pub(super) struct SpliceDiff<T> {
    /// The position where the change happens
    pub(super) pos: u32,
    /// The number of items to remove.
    pub(super) n_removals: u32,
    /// The items to add.
    pub(super) additions: Vec<T>,
}

/// A diff to update items.
#[derive(Debug, Clone)]
pub(super) struct UpdateDiff {
    /// The position from where to start updating items.
    pub(super) pos: u32,
    /// The number of items to update.
    pub(super) n_items: u32,
}

/// A struct grouping compatible diffs.
struct TimelineDiffBatches<T> {
    /// The final list of diffs.
    list: Vec<TimelineDiff<T>>,

    /// The current batch of diffs.
    current_batch: Option<TimelineDiff<T>>,
}

impl<T> TimelineDiffBatches<T> {
    /// Create an empty `TimelineDiffBatches`.
    fn new() -> Self {
        Self {
            list: Vec::new(),
            current_batch: None,
        }
    }

    /// Add the given addition to the list.
    fn push_addition(&mut self, pos: &mut u32, item: T) {
        // End the previous batch if it is not the right kind.
        if !matches!(self.current_batch, Some(TimelineDiff::Splice(_))) {
            self.end_batch(pos);
        }

        match self.current_batch.get_or_insert_with(|| {
            TimelineDiff::Splice(SpliceDiff {
                pos: *pos,
                n_removals: 0,
                additions: Vec::new(),
            })
        }) {
            TimelineDiff::Splice(splice) => splice.additions.push(item),
            TimelineDiff::Update(_) => unreachable!(),
        }
    }

    /// Add a removal to the list.
    fn push_removal(&mut self, pos: &mut u32) {
        // End the previous batch if it is not the right kind.
        if !matches!(self.current_batch, Some(TimelineDiff::Splice(_))) {
            self.end_batch(pos);
        }

        match self.current_batch.get_or_insert_with(|| {
            TimelineDiff::Splice(SpliceDiff {
                pos: *pos,
                n_removals: 0,
                additions: Vec::new(),
            })
        }) {
            TimelineDiff::Splice(splice) => splice.n_removals += 1,
            TimelineDiff::Update(_) => unreachable!(),
        }
    }

    /// Add an update to the list.
    fn push_update(&mut self, pos: &mut u32) {
        // End the previous batch if it is not the right kind.
        if !matches!(self.current_batch, Some(TimelineDiff::Update(_))) {
            self.end_batch(pos);
        }

        match self.current_batch.get_or_insert_with(|| {
            TimelineDiff::Update(UpdateDiff {
                pos: *pos,
                n_items: 0,
            })
        }) {
            TimelineDiff::Splice(_) => unreachable!(),
            TimelineDiff::Update(update) => update.n_items += 1,
        }
    }

    /// Skip an item.
    fn skip_item(&mut self, pos: &mut u32) {
        // End the previous batch if any.
        self.end_batch(pos);
        // Take the skipped item into account for the position.
        *pos += 1;
    }

    /// End the current batch, if any.
    fn end_batch(&mut self, pos: &mut u32) {
        if let Some(batch) = self.current_batch.take() {
            match &batch {
                TimelineDiff::Splice(splice) => *pos += splice.additions.len() as u32,
                TimelineDiff::Update(update) => *pos += update.n_items,
            }

            self.list.push(batch);
        }
    }

    /// Finalize the list.
    fn finalize(mut self) -> Vec<TimelineDiff<T>> {
        if let Some(batch) = self.current_batch.take() {
            self.list.push(batch);
        }

        self.list
    }
}

/// A table containing `u32`s.
struct Table {
    /// The number of columns.
    n_cols: usize,

    /// The data.
    data: Vec<u32>,
}

impl Table {
    /// Construct a table with the given number of rows and columns initialized
    /// with zeroes.
    fn new(n_rows: usize, n_cols: usize) -> Self {
        Self {
            n_cols,
            data: vec![0; n_rows * n_cols],
        }
    }

    /// Get the value at the given position.
    fn get(&self, row: usize, col: usize) -> u32 {
        self.data[row * self.n_cols + col]
    }

    /// Set the value at the given position.
    fn set(&mut self, row: usize, col: usize, value: u32) {
        self.data[row * self.n_cols + col] = value;
    }
}

/// The diff of an item.
enum ItemDiff<'a> {
    /// The item was added.
    Added(&'a str),

    /// The item was removed.
    Removed,

    /// The item is in both lists.
    Common(&'a str),
}
