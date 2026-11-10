//! The graph of the joined spaces of a session, and the pure logic built on
//! it to navigate spaces in the sidebar.
//!
//! Everything here is plain data with no `GObject`, so it can be unit-tested
//! without a display or a Matrix client.

use std::collections::{HashMap, HashSet, VecDeque};

use ruma::{OwnedRoomId, RoomId};

use crate::session::{HighlightFlags, RoomCategory};

/// The maximum length of a valid MSC3230 `order`.
const MAX_ORDER_LEN: usize = 50;

/// The data of a joined space needed to build a [`SpaceGraph`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinedSpace {
    /// The ID of the space.
    pub(crate) room_id: OwnedRoomId,
    /// The IDs of the children of the space, from its valid `m.space.child`
    /// state events.
    pub(crate) children: Vec<OwnedRoomId>,
    /// The MSC3230 `order` of the space, from its `m.space_order` room
    /// account data.
    pub(crate) order: Option<String>,
}

/// The graph of the joined spaces of a session.
///
/// Edges come from the `m.space.child` state events of the joined spaces,
/// which are authoritative for the content of a space per the Matrix
/// specification. The graph can contain cycles since they are not prevented by
/// homeservers, so every traversal keeps track of the visited nodes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SpaceGraph {
    /// The children of each joined space.
    children: HashMap<OwnedRoomId, Vec<OwnedRoomId>>,
    /// The top-level spaces, in display order.
    top_level: Vec<OwnedRoomId>,
}

impl SpaceGraph {
    /// Build the graph from the given joined spaces.
    pub(crate) fn new(spaces: Vec<JoinedSpace>) -> Self {
        let mut orders = HashMap::with_capacity(spaces.len());
        let mut children = HashMap::with_capacity(spaces.len());

        for space in spaces {
            let mut seen = HashSet::new();
            // Ignore duplicates and self-references.
            let space_children = space
                .children
                .into_iter()
                .filter(|child| *child != space.room_id && seen.insert(child.clone()))
                .collect::<Vec<_>>();

            orders.insert(
                space.room_id.clone(),
                space.order.filter(|o| is_valid_order(o)),
            );
            children.insert(space.room_id, space_children);
        }

        let mut graph = Self {
            children,
            top_level: Vec::new(),
        };
        graph.top_level = graph.compute_top_level(&orders);
        graph
    }

    /// Compute the top-level spaces.
    ///
    /// A top-level space is a joined space that is not the child of another
    /// joined space. Spaces that are only reachable through a cycle would never
    /// be reachable from the top-level, so one space of each such cycle is
    /// promoted to the top-level.
    fn compute_top_level(&self, orders: &HashMap<OwnedRoomId, Option<String>>) -> Vec<OwnedRoomId> {
        let child_spaces = self
            .children
            .values()
            .flatten()
            .filter(|child| self.is_space(child))
            .collect::<HashSet<_>>();

        // Sort to make the choice of the space promoted in a cycle
        // deterministic.
        let mut candidates = self.children.keys().collect::<Vec<_>>();
        candidates.sort_by(|a, b| compare_space_order(a, orders.get(*a), b, orders.get(*b)));

        let mut top_level = candidates
            .iter()
            .filter(|space| !child_spaces.contains(*space))
            .map(|space| (*space).clone())
            .collect::<Vec<_>>();

        let mut reachable = HashSet::new();
        for space in &top_level {
            reachable.insert(space.clone());
            reachable.extend(self.descendants(space));
        }

        for space in candidates {
            if reachable.contains(space) {
                continue;
            }

            top_level.push(space.clone());
            reachable.insert(space.clone());
            reachable.extend(self.descendants(space));
        }

        top_level.sort_by(|a, b| compare_space_order(a, orders.get(a), b, orders.get(b)));
        top_level
    }

    /// Whether the room with the given ID is a joined space of this graph.
    pub(crate) fn is_space(&self, room_id: &RoomId) -> bool {
        self.children.contains_key(room_id)
    }

    /// The top-level spaces, in display order.
    pub(crate) fn top_level_spaces(&self) -> &[OwnedRoomId] {
        &self.top_level
    }

    /// The direct children of the space with the given ID.
    pub(crate) fn children(&self, space_id: &RoomId) -> &[OwnedRoomId] {
        self.children
            .get(space_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// The direct children of the space with the given ID that are joined
    /// spaces.
    pub(crate) fn child_spaces(&self, space_id: &RoomId) -> HashSet<OwnedRoomId> {
        self.children(space_id)
            .iter()
            .filter(|child| self.is_space(child))
            .cloned()
            .collect()
    }

    /// All the rooms and spaces that are in the space with the given ID,
    /// directly or through any depth of subspaces.
    ///
    /// The space itself is never part of its descendants, even if it is part
    /// of a cycle.
    pub(crate) fn descendants(&self, space_id: &RoomId) -> HashSet<OwnedRoomId> {
        let mut descendants = HashSet::new();
        let mut queue = VecDeque::from([space_id]);

        while let Some(current) = queue.pop_front() {
            for child in self.children(current) {
                if child != space_id && descendants.insert(child.clone()) {
                    queue.push_back(child);
                }
            }
        }

        descendants
    }

    /// The longest valid prefix of the given navigation path.
    ///
    /// A valid path starts with a top-level space and each following space is
    /// a child space of the previous one. The empty path is the "Home" of the
    /// sidebar and is always valid.
    pub(crate) fn valid_path(&self, path: &[OwnedRoomId]) -> Vec<OwnedRoomId> {
        path[..self.valid_path_len(path)].to_vec()
    }

    /// The length of the longest valid prefix of the given navigation path.
    fn valid_path_len(&self, path: &[OwnedRoomId]) -> usize {
        let Some(first) = path.first() else {
            return 0;
        };
        if !self.top_level.contains(first) {
            return 0;
        }

        let mut len = 1;
        for pair in path.windows(2) {
            let [parent, child] = pair else {
                unreachable!("windows(2) yields slices of 2 items");
            };

            if self.is_space(child) && self.children(parent).contains(child) {
                len += 1;
            } else {
                break;
            }
        }

        len
    }

    /// The shortest navigation path from a top-level space to the joined
    /// space with the given ID, both included.
    ///
    /// Returns `None` if the ID is not a joined space of this graph.
    pub(crate) fn path_to(&self, space_id: &RoomId) -> Option<Vec<OwnedRoomId>> {
        if !self.is_space(space_id) {
            return None;
        }

        let mut parents = HashMap::<&RoomId, &RoomId>::new();
        let mut visited = HashSet::<&RoomId>::new();
        let mut queue = VecDeque::new();

        for top_level in &self.top_level {
            visited.insert(top_level);
            queue.push_back(&**top_level);
        }

        while let Some(current) = queue.pop_front() {
            if current == space_id {
                let mut path = vec![current.to_owned()];
                let mut node = current;
                while let Some(parent) = parents.get(node) {
                    path.push((*parent).to_owned());
                    node = *parent;
                }
                path.reverse();
                return Some(path);
            }

            for child in self.children(current) {
                if self.is_space(child) && visited.insert(child) {
                    parents.insert(child, current);
                    queue.push_back(child);
                }
            }
        }

        None
    }

    /// Keep only the top-level spaces for which the given function returns
    /// `true`.
    ///
    /// This is used to drop the spaces that cannot be shown yet, so the
    /// positions in the space switcher always match the top-level spaces.
    pub(crate) fn retain_top_level(&mut self, f: impl FnMut(&OwnedRoomId) -> bool) {
        self.top_level.retain(f);
    }

    /// The position in the space switcher of the item to select for the
    /// given navigation path.
    ///
    /// Position `0` is "Home", the top-level spaces follow.
    pub(crate) fn switcher_position(&self, path: &[OwnedRoomId]) -> u32 {
        path.first()
            .and_then(|id| self.top_level.iter().position(|top| top == id))
            .map_or(0, |pos| pos as u32 + 1)
    }

    /// The ID of the top-level space at the given position in the space
    /// switcher, or `None` for "Home" or an unknown position.
    pub(crate) fn switcher_space_id(&self, position: u32) -> Option<&OwnedRoomId> {
        self.top_level.get(position.checked_sub(1)? as usize)
    }

    /// Resolve the navigation path requested by the user in this graph.
    ///
    /// Returns the requested path if it is valid. If the selected space was
    /// moved, for example a top-level space that became a subspace, the
    /// shortest path to it is returned. Otherwise, the longest valid prefix is
    /// returned.
    pub(crate) fn resolve_path(&self, requested: &[OwnedRoomId]) -> Vec<OwnedRoomId> {
        let valid = self.valid_path(requested);

        if valid.len() < requested.len()
            && let Some(path) = requested.last().and_then(|id| self.path_to(id))
        {
            return path;
        }

        valid
    }

    /// The navigation path to enter the space with the given ID from the
    /// given path.
    ///
    /// If it is a child space of the last space of the path, it is appended
    /// to the path, unless it is already part of the path because of a
    /// cycle. Otherwise, the shortest path to it is used.
    ///
    /// Returns `None` if the ID is not a joined space of this graph.
    pub(crate) fn path_into(
        &self,
        current: &[OwnedRoomId],
        space_id: &RoomId,
    ) -> Option<Vec<OwnedRoomId>> {
        if let Some(last) = current.last()
            && self.child_spaces(last).contains(space_id)
            && !current.iter().any(|id| **id == *space_id)
        {
            let mut path = current.to_vec();
            path.push(space_id.to_owned());
            return Some(path);
        }

        self.path_to(space_id)
    }

    /// The navigation path to use so that the room with the given ID, and the
    /// given predecessor, is visible in the sidebar.
    ///
    /// If the room is a joined space, navigate to it. If the room is not
    /// visible in the current space, go to "Home".
    ///
    /// Returns `None` if the current path should not change.
    pub(crate) fn path_to_show_room(
        &self,
        current: &[OwnedRoomId],
        room_id: &RoomId,
        predecessor: Option<&RoomId>,
    ) -> Option<Vec<OwnedRoomId>> {
        if self.is_space(room_id) {
            if current.last().is_some_and(|id| **id == *room_id) {
                return None;
            }
            return self.path_into(current, room_id);
        }

        if SpaceVisibility::new(self, current).is_room_visible(room_id, predecessor) {
            None
        } else {
            Some(Vec::new())
        }
    }
}

/// The position of the item before or after the given position in a list of
/// the given length, wrapping around.
pub(crate) fn relative_position(current: u32, n_items: u32, forward: bool) -> u32 {
    if n_items == 0 {
        return 0;
    }
    let current = current.min(n_items - 1);

    if forward {
        (current + 1) % n_items
    } else {
        (current + n_items - 1) % n_items
    }
}

/// Whether the given room, or its predecessor, is in the given set of rooms.
///
/// The predecessor is checked because spaces keep the ID of a room after it is
/// upgraded, until an admin updates the `m.space.child` event.
pub(crate) fn contains_room(
    rooms: &HashSet<OwnedRoomId>,
    room_id: &RoomId,
    predecessor: Option<&RoomId>,
) -> bool {
    rooms.contains(room_id) || predecessor.is_some_and(|id| rooms.contains(id))
}

/// Whether the given MSC3230 `order` is valid.
///
/// It must consist only of ASCII characters in the range `\x20` to `\x7E`, and
/// be at most 50 characters long.
pub(crate) fn is_valid_order(order: &str) -> bool {
    order.len() <= MAX_ORDER_LEN && order.bytes().all(|b| (0x20..=0x7E).contains(&b))
}

/// Compare two spaces for their display order, according to MSC3230.
///
/// Spaces with an `order` come first, sorted lexicographically by `order`, then
/// spaces without an `order`. Ties are broken by room ID.
fn compare_space_order(
    a: &RoomId,
    a_order: Option<&Option<String>>,
    b: &RoomId,
    b_order: Option<&Option<String>>,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let a_order = a_order.and_then(Option::as_deref);
    let b_order = b_order.and_then(Option::as_deref);

    match (a_order, b_order) {
        (Some(a_order), Some(b_order)) => a_order.cmp(b_order),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then_with(|| a.cmp(b))
}

/// Which rooms of the sidebar are visible for a navigation path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SpaceVisibility {
    /// The rooms that are visible, or `None` if all the rooms are visible.
    pub(crate) rooms: Option<HashSet<OwnedRoomId>>,
    /// The spaces that are visible in the section of spaces.
    pub(crate) subspaces: HashSet<OwnedRoomId>,
}

impl SpaceVisibility {
    /// Compute the visibility for the given navigation path in the given
    /// graph.
    ///
    /// With an empty path ("Home"), all the rooms are visible and no space is
    /// visible, since the space switcher lists the spaces. Otherwise, the rooms
    /// of the selected space and of all its subspaces are visible, and only
    /// the direct child spaces of the selected space are visible.
    pub(crate) fn new(graph: &SpaceGraph, path: &[OwnedRoomId]) -> Self {
        match path.last() {
            Some(space_id) => Self {
                rooms: Some(graph.descendants(space_id)),
                subspaces: graph.child_spaces(space_id),
            },
            None => Self::default(),
        }
    }

    /// Whether the room with the given ID and predecessor is visible.
    pub(crate) fn is_room_visible(&self, room_id: &RoomId, predecessor: Option<&RoomId>) -> bool {
        self.rooms
            .as_ref()
            .is_none_or(|rooms| contains_room(rooms, room_id, predecessor))
    }

    /// Whether the space with the given ID is visible in the section of
    /// spaces.
    pub(crate) fn is_subspace_visible(&self, room_id: &RoomId) -> bool {
        self.subspaces.contains(room_id)
    }

    /// Whether a room that is not a space passes the filter of the sidebar.
    ///
    /// During a search, all the rooms pass.
    pub(crate) fn room_matches(
        &self,
        search_active: bool,
        room_id: &RoomId,
        predecessor: Option<&RoomId>,
    ) -> bool {
        search_active || self.is_room_visible(room_id, predecessor)
    }

    /// Whether a space passes the filter of the section of spaces.
    ///
    /// During a search, all the spaces pass.
    pub(crate) fn subspace_matches(&self, search_active: bool, room_id: &RoomId) -> bool {
        search_active || self.is_subspace_visible(room_id)
    }
}

/// The aggregated unread state of a group of rooms, like a space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnreadSummary {
    /// The total number of unread notifications.
    pub(crate) notification_count: u64,
    /// The union of the highlight flags.
    pub(crate) highlight: HighlightFlags,
    /// Whether all the rooms are read.
    pub(crate) is_read: bool,
}

impl Default for UnreadSummary {
    fn default() -> Self {
        Self {
            notification_count: 0,
            highlight: HighlightFlags::empty(),
            is_read: true,
        }
    }
}

impl UnreadSummary {
    /// Whether a room with the given category counts towards the unread state
    /// of a space.
    ///
    /// Spaces themselves, low-priority rooms (like in Element Web), rooms that
    /// were left and rooms that are not shown in the sidebar are ignored.
    pub(crate) fn counts_category(category: RoomCategory) -> bool {
        matches!(
            category,
            RoomCategory::Invited | RoomCategory::Favorite | RoomCategory::Normal
        )
    }

    /// Add the unread state of a room with the given properties.
    pub(crate) fn add(
        &mut self,
        category: RoomCategory,
        notification_count: u64,
        highlight: HighlightFlags,
        is_read: bool,
    ) {
        if !Self::counts_category(category) {
            return;
        }

        self.notification_count = self.notification_count.saturating_add(notification_count);
        self.highlight |= highlight;
        self.is_read &= is_read;
    }

    /// Whether any room has unread notifications.
    pub(crate) fn has_notifications(&self) -> bool {
        self.notification_count > 0
    }
}

#[cfg(test)]
mod tests {
    use ruma::owned_room_id;

    use super::*;

    fn space(id: &str, children: &[&str]) -> JoinedSpace {
        JoinedSpace {
            room_id: id.try_into().unwrap(),
            children: children.iter().map(|c| (*c).try_into().unwrap()).collect(),
            order: None,
        }
    }

    fn ordered_space(id: &str, children: &[&str], order: &str) -> JoinedSpace {
        JoinedSpace {
            order: Some(order.to_owned()),
            ..space(id, children)
        }
    }

    fn ids(ids: &[&str]) -> HashSet<OwnedRoomId> {
        ids.iter().map(|id| (*id).try_into().unwrap()).collect()
    }

    fn id_vec(ids: &[&str]) -> Vec<OwnedRoomId> {
        ids.iter().map(|id| (*id).try_into().unwrap()).collect()
    }

    fn is_valid_path(graph: &SpaceGraph, path: &[OwnedRoomId]) -> bool {
        graph.valid_path(path) == path
    }

    /// A company space with a team subspace that has its own subspace.
    fn nested_graph() -> SpaceGraph {
        SpaceGraph::new(vec![
            space("!company:x", &["!general:x", "!team:x"]),
            space("!team:x", &["!standup:x", "!project:x"]),
            space("!project:x", &["!design:x"]),
            space("!hobby:x", &["!games:x"]),
        ])
    }

    #[test]
    fn descendants_include_nested_subspaces() {
        let graph = nested_graph();

        assert_eq!(
            graph.descendants(&owned_room_id!("!company:x")),
            ids(&[
                "!general:x",
                "!team:x",
                "!standup:x",
                "!project:x",
                "!design:x"
            ])
        );
        assert_eq!(
            graph.descendants(&owned_room_id!("!team:x")),
            ids(&["!standup:x", "!project:x", "!design:x"])
        );
        assert_eq!(
            graph.descendants(&owned_room_id!("!hobby:x")),
            ids(&["!games:x"])
        );
    }

    #[test]
    fn descendants_of_unknown_space_is_empty() {
        let graph = nested_graph();
        assert!(graph.descendants(&owned_room_id!("!unknown:x")).is_empty());
        // A plain room has no children either.
        assert!(graph.descendants(&owned_room_id!("!general:x")).is_empty());
    }

    #[test]
    fn descendants_terminate_on_cycles_and_exclude_self() {
        let graph = SpaceGraph::new(vec![
            space("!a:x", &["!b:x", "!room-a:x"]),
            space("!b:x", &["!c:x", "!room-b:x"]),
            space("!c:x", &["!a:x", "!room-c:x"]),
        ]);

        assert_eq!(
            graph.descendants(&owned_room_id!("!a:x")),
            ids(&["!b:x", "!c:x", "!room-a:x", "!room-b:x", "!room-c:x"])
        );
        assert_eq!(
            graph.descendants(&owned_room_id!("!b:x")),
            ids(&["!a:x", "!c:x", "!room-a:x", "!room-b:x", "!room-c:x"])
        );
    }

    #[test]
    fn self_references_and_duplicates_are_ignored() {
        let graph = SpaceGraph::new(vec![space("!a:x", &["!a:x", "!r:x", "!r:x"])]);

        assert_eq!(graph.children(&owned_room_id!("!a:x")), id_vec(&["!r:x"]));
        assert_eq!(graph.top_level_spaces(), id_vec(&["!a:x"]));
    }

    #[test]
    fn room_in_several_spaces_is_in_each() {
        let graph = SpaceGraph::new(vec![
            space("!a:x", &["!shared:x"]),
            space("!b:x", &["!shared:x"]),
        ]);

        assert!(
            graph
                .descendants(&owned_room_id!("!a:x"))
                .contains(&owned_room_id!("!shared:x"))
        );
        assert!(
            graph
                .descendants(&owned_room_id!("!b:x"))
                .contains(&owned_room_id!("!shared:x"))
        );
    }

    #[test]
    fn top_level_excludes_joined_subspaces() {
        let graph = nested_graph();
        assert_eq!(
            graph.top_level_spaces(),
            id_vec(&["!company:x", "!hobby:x"])
        );
    }

    #[test]
    fn subspace_of_unjoined_parent_is_top_level() {
        // `!team:x` claims to be in `!company:x`, but we are not in it.
        let graph = SpaceGraph::new(vec![space("!team:x", &["!standup:x"])]);
        assert_eq!(graph.top_level_spaces(), id_vec(&["!team:x"]));
    }

    #[test]
    fn pure_cycle_promotes_one_space_to_top_level() {
        let graph = SpaceGraph::new(vec![
            space("!b:x", &["!a:x"]),
            space("!a:x", &["!b:x"]),
            space("!other:x", &[]),
        ]);

        // `!a:x` is chosen deterministically, `!b:x` stays reachable inside it.
        assert_eq!(graph.top_level_spaces(), id_vec(&["!a:x", "!other:x"]));
        assert_eq!(
            graph.path_to(&owned_room_id!("!b:x")),
            Some(id_vec(&["!a:x", "!b:x"]))
        );
    }

    #[test]
    fn cycle_below_a_root_does_not_add_top_level_spaces() {
        let graph = SpaceGraph::new(vec![
            space("!root:x", &["!a:x"]),
            space("!a:x", &["!b:x"]),
            space("!b:x", &["!a:x"]),
        ]);

        assert_eq!(graph.top_level_spaces(), id_vec(&["!root:x"]));
    }

    #[test]
    fn top_level_is_sorted_by_order_then_room_id() {
        let graph = SpaceGraph::new(vec![
            space("!c:x", &[]),
            ordered_space("!z:x", &[], "a"),
            space("!a:x", &[]),
            ordered_space("!y:x", &[], "b"),
            // Invalid order, treated as missing.
            ordered_space("!b:x", &[], "\u{e9}"),
        ]);

        assert_eq!(
            graph.top_level_spaces(),
            id_vec(&["!z:x", "!y:x", "!a:x", "!b:x", "!c:x"])
        );
    }

    #[test]
    fn order_validation() {
        assert!(is_valid_order(""));
        assert!(is_valid_order(" ~azAZ09"));
        assert!(is_valid_order(&"a".repeat(50)));
        assert!(!is_valid_order(&"a".repeat(51)));
        assert!(!is_valid_order("tab\t"));
        assert!(!is_valid_order("\u{e9}"));
    }

    #[test]
    fn child_spaces_only_returns_joined_spaces() {
        let graph = nested_graph();
        assert_eq!(
            graph.child_spaces(&owned_room_id!("!company:x")),
            ids(&["!team:x"])
        );
        assert!(graph.child_spaces(&owned_room_id!("!hobby:x")).is_empty());
    }

    #[test]
    fn path_validation() {
        let graph = nested_graph();

        assert!(is_valid_path(&graph, &[]));
        assert!(is_valid_path(&graph, &id_vec(&["!company:x"])));
        assert!(is_valid_path(
            &graph,
            &id_vec(&["!company:x", "!team:x", "!project:x"])
        ));

        // Does not start at the top-level.
        assert!(!is_valid_path(&graph, &id_vec(&["!team:x"])));
        assert_eq!(
            graph.valid_path(&id_vec(&["!team:x", "!project:x"])),
            id_vec(&[])
        );

        // `!project:x` is not a child of `!company:x`.
        assert_eq!(
            graph.valid_path(&id_vec(&["!company:x", "!project:x"])),
            id_vec(&["!company:x"])
        );

        // Plain rooms cannot be part of a path.
        assert_eq!(
            graph.valid_path(&id_vec(&["!company:x", "!general:x"])),
            id_vec(&["!company:x"])
        );
    }

    #[test]
    fn path_to_finds_shortest_path_from_top_level() {
        let graph = nested_graph();

        assert_eq!(
            graph.path_to(&owned_room_id!("!project:x")),
            Some(id_vec(&["!company:x", "!team:x", "!project:x"]))
        );
        assert_eq!(
            graph.path_to(&owned_room_id!("!hobby:x")),
            Some(id_vec(&["!hobby:x"]))
        );
        assert_eq!(graph.path_to(&owned_room_id!("!general:x")), None);
        assert_eq!(graph.path_to(&owned_room_id!("!unknown:x")), None);
    }

    #[test]
    fn empty_graph() {
        let graph = SpaceGraph::new(vec![]);
        assert!(graph.top_level_spaces().is_empty());
        assert!(is_valid_path(&graph, &[]));
        assert!(!is_valid_path(&graph, &id_vec(&["!a:x"])));
    }

    fn room_visible(visibility: &SpaceVisibility, id: &str) -> bool {
        visibility.is_room_visible(&id_vec(&[id])[0], None)
    }

    fn subspace_visible(visibility: &SpaceVisibility, id: &str) -> bool {
        visibility.is_subspace_visible(&id_vec(&[id])[0])
    }

    #[test]
    fn home_shows_all_rooms_and_no_spaces() {
        let graph = nested_graph();
        let visibility = SpaceVisibility::new(&graph, &[]);

        assert!(room_visible(&visibility, "!general:x"));
        assert!(room_visible(&visibility, "!games:x"));
        assert!(room_visible(&visibility, "!not-in-a-space:x"));
        assert!(!subspace_visible(&visibility, "!company:x"));
        assert!(!subspace_visible(&visibility, "!team:x"));
    }

    #[test]
    fn space_shows_rooms_of_nested_subspaces_and_direct_child_spaces() {
        let graph = nested_graph();
        let visibility = SpaceVisibility::new(&graph, &id_vec(&["!company:x"]));

        // Direct child, child of subspace, child of sub-subspace.
        assert!(room_visible(&visibility, "!general:x"));
        assert!(room_visible(&visibility, "!standup:x"));
        assert!(room_visible(&visibility, "!design:x"));
        // Rooms of other spaces and of no space.
        assert!(!room_visible(&visibility, "!games:x"));
        assert!(!room_visible(&visibility, "!not-in-a-space:x"));
        // The space itself.
        assert!(!room_visible(&visibility, "!company:x"));

        // Only the direct child space is listed.
        assert!(subspace_visible(&visibility, "!team:x"));
        assert!(!subspace_visible(&visibility, "!project:x"));
    }

    #[test]
    fn subspace_narrows_the_rooms() {
        let graph = nested_graph();
        let visibility = SpaceVisibility::new(&graph, &id_vec(&["!company:x", "!team:x"]));

        assert!(room_visible(&visibility, "!standup:x"));
        assert!(room_visible(&visibility, "!design:x"));
        assert!(!room_visible(&visibility, "!general:x"));
        assert!(subspace_visible(&visibility, "!project:x"));
        assert!(!subspace_visible(&visibility, "!team:x"));
    }

    #[test]
    fn visibility_in_a_cycle_terminates() {
        let graph = SpaceGraph::new(vec![
            space("!a:x", &["!b:x", "!room-a:x"]),
            space("!b:x", &["!a:x", "!room-b:x"]),
        ]);
        let path = graph.path_to(&owned_room_id!("!b:x")).unwrap();
        let visibility = SpaceVisibility::new(&graph, &path);

        assert!(room_visible(&visibility, "!room-a:x"));
        assert!(room_visible(&visibility, "!room-b:x"));
        // `!a:x` is a child of `!b:x` in the cycle.
        assert!(subspace_visible(&visibility, "!a:x"));
    }

    #[test]
    fn upgraded_room_stays_visible_through_its_predecessor() {
        let graph = nested_graph();
        let visibility = SpaceVisibility::new(&graph, &id_vec(&["!company:x"]));
        let successor = owned_room_id!("!general-v2:x");
        let predecessor = owned_room_id!("!general:x");
        let unrelated = owned_room_id!("!games:x");

        assert!(!visibility.is_room_visible(&successor, None));
        assert!(visibility.is_room_visible(&successor, Some(&predecessor)));
        assert!(!visibility.is_room_visible(&successor, Some(&unrelated)));

        let rooms = graph.descendants(&owned_room_id!("!company:x"));
        assert!(contains_room(&rooms, &successor, Some(&predecessor)));
        assert!(!contains_room(&rooms, &successor, None));

        // Showing the successor does not leave the space.
        assert_eq!(
            graph.path_to_show_room(&id_vec(&["!company:x"]), &successor, Some(&predecessor)),
            None
        );
    }

    #[test]
    fn search_loosens_and_tightens_the_filters() {
        let graph = nested_graph();
        let visibility = SpaceVisibility::new(&graph, &id_vec(&["!company:x"]));
        let other_room = owned_room_id!("!games:x");
        let other_space = owned_room_id!("!hobby:x");
        let nested_space = owned_room_id!("!project:x");

        // Search active: everything passes, including all spaces.
        assert!(visibility.room_matches(true, &other_room, None));
        assert!(visibility.subspace_matches(true, &other_space));
        assert!(visibility.subspace_matches(true, &nested_space));

        // Search inactive: back to the selected space.
        assert!(!visibility.room_matches(false, &other_room, None));
        assert!(!visibility.subspace_matches(false, &other_space));
        assert!(!visibility.subspace_matches(false, &nested_space));
        assert!(visibility.subspace_matches(false, &owned_room_id!("!team:x")));

        // In Home, all the spaces show during search, none otherwise.
        let home = SpaceVisibility::new(&graph, &[]);
        assert!(home.subspace_matches(true, &other_space));
        assert!(!home.subspace_matches(false, &other_space));
    }

    #[test]
    fn switcher_positions_match_shown_top_level_spaces() {
        let mut graph = SpaceGraph::new(vec![
            ordered_space("!a:x", &[], "a"),
            ordered_space("!b:x", &[], "b"),
            ordered_space("!c:x", &[], "c"),
        ]);
        // `!b:x` is not in the room list yet, so it is not shown.
        graph.retain_top_level(|id| id.as_str() != "!b:x");

        assert_eq!(graph.top_level_spaces(), id_vec(&["!a:x", "!c:x"]));
        assert_eq!(graph.switcher_space_id(0), None);
        assert_eq!(graph.switcher_space_id(1), Some(&owned_room_id!("!a:x")));
        assert_eq!(graph.switcher_space_id(2), Some(&owned_room_id!("!c:x")));
        assert_eq!(graph.switcher_space_id(3), None);

        assert_eq!(graph.switcher_position(&[]), 0);
        assert_eq!(graph.switcher_position(&id_vec(&["!c:x"])), 2);
        // A path through a space that is not shown resolves to "Home".
        assert_eq!(graph.resolve_path(&id_vec(&["!b:x"])), id_vec(&[]));
        assert_eq!(graph.switcher_position(&id_vec(&["!b:x"])), 0);
    }

    #[test]
    fn requested_path_is_restored_when_space_is_rejoined() {
        let requested = id_vec(&["!company:x", "!team:x", "!project:x"]);

        // `!team:x` was left: fall back to its parent.
        let left = SpaceGraph::new(vec![
            space("!company:x", &["!general:x", "!team:x"]),
            space("!hobby:x", &["!games:x"]),
        ]);
        assert_eq!(left.resolve_path(&requested), id_vec(&["!company:x"]));

        // Everything was left: "Home".
        let empty = SpaceGraph::new(vec![]);
        assert_eq!(empty.resolve_path(&requested), id_vec(&[]));

        // Rejoined: the same requested path resolves fully again.
        assert_eq!(nested_graph().resolve_path(&requested), requested);
    }

    #[test]
    fn top_level_space_that_became_a_subspace_stays_selected() {
        let before = nested_graph();
        assert_eq!(
            before.resolve_path(&id_vec(&["!hobby:x"])),
            id_vec(&["!hobby:x"])
        );

        let after = SpaceGraph::new(vec![
            space("!company:x", &["!general:x", "!team:x", "!hobby:x"]),
            space("!team:x", &["!standup:x"]),
            space("!hobby:x", &["!games:x"]),
        ]);
        assert_eq!(
            after.resolve_path(&id_vec(&["!hobby:x"])),
            id_vec(&["!company:x", "!hobby:x"])
        );
    }

    #[test]
    fn entering_spaces_never_repeats_a_space_in_a_cycle() {
        let graph = SpaceGraph::new(vec![space("!a:x", &["!b:x"]), space("!b:x", &["!a:x"])]);

        let path = graph.path_into(&id_vec(&["!a:x"]), &owned_room_id!("!b:x"));
        assert_eq!(path, Some(id_vec(&["!a:x", "!b:x"])));

        // `!a:x` is a child of `!b:x`, but it is already in the path.
        let path = graph.path_into(&id_vec(&["!a:x", "!b:x"]), &owned_room_id!("!a:x"));
        assert_eq!(path, Some(id_vec(&["!a:x"])));

        assert_eq!(graph.path_into(&[], &owned_room_id!("!unknown:x")), None);
    }

    #[test]
    fn path_to_show_room() {
        let graph = nested_graph();
        let company = id_vec(&["!company:x"]);

        // A room outside the selected space: go to "Home".
        assert_eq!(
            graph.path_to_show_room(&company, &owned_room_id!("!games:x"), None),
            Some(id_vec(&[]))
        );
        // A room in the selected space, nested or not: no change.
        assert_eq!(
            graph.path_to_show_room(&company, &owned_room_id!("!design:x"), None),
            None
        );
        // A room in "Home": no change.
        assert_eq!(
            graph.path_to_show_room(&[], &owned_room_id!("!games:x"), None),
            None
        );
        // A space: navigate into it.
        assert_eq!(
            graph.path_to_show_room(&company, &owned_room_id!("!team:x"), None),
            Some(id_vec(&["!company:x", "!team:x"]))
        );
        assert_eq!(
            graph.path_to_show_room(&[], &owned_room_id!("!project:x"), None),
            Some(id_vec(&["!company:x", "!team:x", "!project:x"]))
        );
        // The selected space itself: no change.
        assert_eq!(
            graph.path_to_show_room(&company, &owned_room_id!("!company:x"), None),
            None
        );

        // Before the graph is loaded, nothing changes.
        let empty = SpaceGraph::new(vec![]);
        let current = empty.resolve_path(&company);
        assert_eq!(
            empty.path_to_show_room(&current, &owned_room_id!("!games:x"), None),
            None
        );
        assert_eq!(
            empty.path_to_show_room(&current, &owned_room_id!("!company:x"), None),
            None
        );
    }

    #[test]
    fn relative_position_wraps() {
        // Only "Home".
        assert_eq!(relative_position(0, 1, true), 0);
        assert_eq!(relative_position(0, 1, false), 0);

        assert_eq!(relative_position(0, 3, true), 1);
        assert_eq!(relative_position(2, 3, true), 0);
        assert_eq!(relative_position(0, 3, false), 2);
        // Out-of-range current position.
        assert_eq!(relative_position(7, 3, true), 0);
        assert_eq!(relative_position(0, 0, true), 0);
    }

    #[test]
    fn unread_summary_aggregates_counted_rooms() {
        let mut summary = UnreadSummary::default();
        assert!(summary.is_read);
        assert!(!summary.has_notifications());

        summary.add(RoomCategory::Normal, 2, HighlightFlags::BOLD, false);
        summary.add(RoomCategory::Favorite, 3, HighlightFlags::empty(), true);
        assert_eq!(summary.notification_count, 5);
        assert_eq!(summary.highlight, HighlightFlags::BOLD);
        assert!(!summary.is_read);
        assert!(summary.has_notifications());

        summary.add(RoomCategory::Invited, 0, HighlightFlags::all(), true);
        assert_eq!(summary.highlight, HighlightFlags::all());
    }

    #[test]
    fn unread_summary_ignores_spaces_low_priority_and_left_rooms() {
        let mut summary = UnreadSummary::default();

        summary.add(RoomCategory::Space, 4, HighlightFlags::all(), false);
        summary.add(RoomCategory::LowPriority, 4, HighlightFlags::all(), false);
        summary.add(RoomCategory::Left, 4, HighlightFlags::all(), false);
        summary.add(RoomCategory::Ignored, 4, HighlightFlags::all(), false);
        summary.add(RoomCategory::Outdated, 4, HighlightFlags::all(), false);
        summary.add(RoomCategory::Knocked, 4, HighlightFlags::all(), false);

        assert_eq!(summary, UnreadSummary::default());
    }

    #[test]
    fn unread_summary_saturates() {
        let mut summary = UnreadSummary::default();
        summary.add(
            RoomCategory::Normal,
            u64::MAX,
            HighlightFlags::empty(),
            false,
        );
        summary.add(RoomCategory::Normal, 1, HighlightFlags::empty(), false);
        assert_eq!(summary.notification_count, u64::MAX);
    }
}
