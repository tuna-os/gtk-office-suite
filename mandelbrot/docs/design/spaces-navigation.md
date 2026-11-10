# Spaces navigation

Status: implemented (branch `feat/spaces-navigation`).

This document records why Mandelbrot's spaces navigation looks the way it
does: the state it replaced, what other clients do, the chosen design, and
what is deliberately left out.

## 1. Previous state and its problems

Spaces support arrived in Mandelbrot as "space hierarchy browsing" (commit
`2f10e61b`, see `docs/UPSTREAM.md`). Upstream Fractal has no space navigation:
its sidebar has no section for spaces at all. Before this change, Mandelbrot
worked like this (line numbers refer to `main` before this change):

* **Spaces were one more flat section.** `SidebarItemList` adds a
  `SidebarSectionName::Space` section next to Favorites/Rooms/Low Priority
  (`src/session/sidebar_data/item_list.rs:65`). Every joined space, top-level
  or nested, gets `RoomCategory::Space` (`src/session/room/mod.rs:579-580`), so
  subspaces sat in that section next to their parents. Nothing showed the
  nesting.
* **No per-space room list.** Room sections filter only by category through
  `RoomCategoryFilter` (`src/session/sidebar_data/section/mod.rs:32,114`). The
  rooms of every space were mixed together in "Rooms" and "Favorites". The
  only way to see which rooms belong to a space was the overview page.
* **High switching cost.** Selecting a space replaced the room view with the
  space overview (`src/session_view/content.rs:268`). Opening a room from
  there meant: pick the space in the sidebar, find the room in the overview,
  press "View", and then find your way back to the space through the sidebar.
  On narrow windows each of those steps is a page transition, because the
  overview is a content page in the collapsed `AdwNavigationSplitView`.
* **Network round-trips just to browse.** The overview lists children
  through `SpaceRoomList::new` + `paginate()`, which is the `/hierarchy`
  endpoint (`src/session/space_hierarchy/mod.rs:121,187`). Each subspace
  opened another one (`src/session_view/space_overview/mod.rs:73`). That is
  right for discovering rooms you have not joined. It is wasteful for
  switching between rooms you are already in, and it does not work offline.
* **No unread aggregation.** A space row shows the space room's own counters
  (`src/session_view/sidebar/room_row.blp:37,67`). A space has no messages,
  so its row never showed unread activity in its rooms. Section badges add
  up per category, not per space.
* **Ordering ignored MSC3230.** The Spaces section sorts by
  `latest-activity` (`section/mod.rs:119`), which is meaningless for spaces.
  Other clients order top-level spaces with the `m.space_order` account data.
* **No keyboard path.** `Ctrl+PgUp/PgDn` (`src/session_view/mod.rs:415`)
  walks the whole room list. There was no shortcut to switch spaces.

## 2. How other clients do it

Read 2026-11-07 from current `develop`/`main` sources and official posts.

| Client | Space switcher | Selecting a space shows | Home / "all" | Unread per space | Lobby / joining | Narrow widths |
|---|---|---|---|---|---|---|
| **Element Web** | Left rail of avatars: Home, meta-spaces, top-level spaces; subspaces expand in the rail as a tree; drag to reorder ([SpacePanel.tsx][ew-panel], [SpaceTreeLevel.tsx][ew-tree]) | Rooms of the space **and all its subspaces** (`includeDescendantSpaces = true`), plus DMs with members ([SpaceStore.ts][ew-store]) | Rooms outside any space + DMs + invites by default; "Show all rooms in Home" setting ([SpaceStore.ts][ew-store], [Settings.tsx][ew-settings]) | Recursive sum over the space's rooms, skipping low priority ([SpaceNotificationState.ts][ew-notif]) | Space home: `SpaceRoomView` → `SpaceHierarchy` ([SpaceRoomView.tsx][ew-room-view]) | Desktop/web only |
| **Element X** (2026) | "Spaces" tab in the bottom bar + space filter row in the chat list ([blog][ex-blog], [HomeNavigationBarItem.kt][ex-tabs]) | SDK `SpaceFilter`: level 0 = direct children, level 1 = all descendants ([matrix-rust-sdk spaces/mod.rs][sdk-spaces]) | All chats | — | Space screen with Join / Accept / Decline over `/hierarchy` ([SpaceEvent.kt][ex-space-event]) | Mobile-first: tabs |
| **Cinny** | Rail: Home, Direct, top-level spaces, user folders ([SidebarNav.tsx][cinny-nav]) | Room list grouped with subspaces as collapsible categories ([Space.tsx][cinny-space]) | Rooms outside any space ([useHomeRooms.ts][cinny-home]) | Recursive ([SpaceTabs.tsx][cinny-tabs]) | Lobby with the full hierarchy ([Lobby.tsx][cinny-lobby]) | Under 750px: one pane at a time ([MobileFriendly.tsx][cinny-mobile]) |
| **FluffyChat** | 80px rail: All chats + every joined space, flat ([navigation_rail.dart][ff-rail]) | Space view built from `/hierarchy` depth 2 ([space_view.dart][ff-space]) | All chats | Direct children only | Inline join in the space view | Rail is optional below 2×380+80px ([themes.dart][ff-themes]) |
| **Nheko** | "Communities" column: All rooms, Direct chats, space tree, tags ([CommunitiesModel.cpp][nheko-comm]) | Direct children ([RoomlistModel.cpp][nheko-list]) | All rooms | Direct children | — | Columns shrink to avatars, then single page ([ChatPage.qml][nheko-page]) |
| **Discord** | Server rail with folders ([Server Folders][dc-folders]) | Channels in collapsible categories ([Channel Categories][dc-cats]) | DMs | Unread dot, red mention count ([mute help][dc-mute]) | — | Bottom tabs on mobile ([Tabs][dc-tabs]) |

GNOME guidance: the HIG recommends sidebars for dynamic locations such as a
messaging app, implemented with `AdwNavigationSplitView`
([HIG sidebars][hig-sidebars]), and a back button at the top left for nested
browsing ([HIG browsing][hig-browsing]). `AdwOverlaySplitView` is meant for
utility panes: when collapsed, its sidebar overlays the content, can be
swiped in from the edge, and hides automatically ([OverlaySplitView][adw-osv]).
For three panes, libadwaita documents nesting split views that collapse one
at a time with `AdwBreakpoint` ([adaptive layouts][adw-adaptive],
[Breakpoint][adw-bp]).

Patterns most of these clients share:

1. A vertical **avatar rail** with an "all"/Home entry first.
2. Selecting a space **filters the room list**. Element Web and Cinny include
   subspace rooms recursively; FluffyChat and Nheko use direct children only.
3. **Unread state is added up per space** and shown on the rail avatar, with
   mentions stronger than plain unread.
4. A **lobby** (space home) built on `/hierarchy` is where you discover and
   join rooms. It is not where you switch between rooms you already joined.
5. On narrow widths the rail is **hidden or made optional**: one pane at a
   time, bottom tabs, or an opt-in rail.

## 3. Chosen design

A **space rail** inside the sidebar page, built on the existing sidebar data
model, plus in-place drill-down into subspaces. The existing overview page is
kept as the space lobby.

### Desktop (window wider than 600sp)

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ [acct]            [🔍][☰] │ Room title                                    │
├────┬──────────────────────┼───────────────────────────────────────────────┤
│ ⌂  │ [<] (A) Acme Corp  > │                                               │
│ 3  │ ▾ SPACES             │                                               │
│(A)●│   (E) Engineering    │            Room history / space lobby         │
│ 1  │ ▾ FAVORITES          │                                               │
│(H) │   (#) announcements  │                                               │
│    │ ▾ ROOMS              │                                               │
│    │   (#) general     2  │                                               │
│    │   (#) random      ·  │                                               │
│rail│ room list (filtered) │                     content                   │
└────┴──────────────────────┴───────────────────────────────────────────────┘
```

* The rail (`AdwOverlaySplitView` sidebar, 60sp) lists **Home** and the
  **top-level joined spaces** in MSC3230 order (`m.space_order` /
  `org.matrix.msc3230.space_order`, then room ID). It is only shown when the
  user has joined at least one space, so users without spaces see exactly
  the old sidebar.
* With the rail visible, the sidebar of the `AdwNavigationSplitView` gets
  60sp more (min 240sp, max 340sp). The room list keeps its old width, and
  240sp sidebar + 360sp content still fits at the 600sp breakpoint.

### Narrow (600sp or less, the window's existing `compact` breakpoint)

```text
┌───────────────────────────┐        ┌───────────────────────────┐
│ [acct][▥]         [🔍][☰] │        │ [acct][▥]         [🔍][☰] │
├───────────────────────────┤        ├────┬──────────────────────┤
│ [<] (A) Acme Corp       > │  ▥ or  │ ⌂  │ [<] (A) Acme Corp  > │
│ ▾ SPACES                  │ swipe  │(A)●│ ▾ SPACES             │
│   (E) Engineering         │ ─────▶ │(H) │   (E) Engineering    │
│ ▾ ROOMS                   │        │    │ ▾ ROOMS              │
│   (#) general           2 │        │rail│   (#) general      2 │
│                           │        │over│                      │
└───────────────────────────┘        └────┴──────────────────────┘
```

* The `AdwOverlaySplitView` collapses (bound to `Window:compact`). The rail
  becomes an overlay, opened by the "Show Spaces" toggle in the header bar
  (`sidebar-show-symbolic`) or by swiping from the left edge, and it closes
  once a space is picked. Without joined spaces, the toggle is hidden and
  the edge swipe is disabled.

### Interaction details

* **Select a space** (activate it in the rail: click, or Enter/Space): the
  room list shows only the rooms in that space **and in all its subspaces,
  recursively**, as in Element Web and Cinny. Sections (Invited, Favorites,
  Rooms, Low Priority, Historical) keep their meaning; they are filtered, and
  their badges add up only the visible rooms. The open room stays open:
  switching spaces never changes the content pane. Activating the selected
  space again goes back to its top level.
* **Upgraded rooms** stay in their space: a room whose predecessor is a
  child of the space is shown, because spaces keep listing the old room ID
  until an admin updates the `m.space.child` event.
* **Home** shows **all rooms**. This differs from Element Web's default
  (rooms outside spaces) on purpose: it keeps the pre-existing behaviour for
  current users and is what FluffyChat and Nheko do. The Spaces section is
  hidden in Home, because the rail already lists the spaces.
* **Nested subspaces**: inside a space, the Spaces section lists its
  **direct** child spaces. Activating one opens it **in the sidebar**: the
  list narrows to that subspace and a header row shows `[<]` (back to the
  parent) and the subspace's avatar and name. This is a sidebar-local
  drill-down, as in Cinny and Element X; nothing changes in the content pane.
  The navigation path is resolved against the space graph: a space that
  moved (for example a top-level space that became a subspace) is found
  again through its shortest path; otherwise leaving a space or losing a
  parent link falls back to the longest valid prefix. A space already in
  the path is never pushed again, so cycles cannot grow the path.
* **Space lobby**: the header row (avatar + name + `>`) opens the existing
  space overview (`src/session_view/space_overview/`) in the content pane.
  There the user can browse unjoined rooms and subspaces through
  `/hierarchy` and join or knock.
* **Unread aggregation**: each rail item sums the notification count, ORs the
  highlight flags, and ANDs the read state over every room in the space and
  its subspaces, including invites. Spaces, low-priority rooms (as in Element
  Web), left rooms, and ignored/outdated rooms are skipped. Home sums over
  all rooms. The badge shows the count, in the accent colour for mentions;
  a dot means unread messages without notifications.
* **Search** (`Ctrl+K`) searches every room, whatever space is selected,
  matching Element X's global search, and the Spaces section lists every
  space. The space header is hidden while searching. Activating a space in
  the results enters it and closes the search.
* **Opening a room from elsewhere** (notification, link, `Ctrl+*`): if the
  room is not in the selected space, the sidebar switches to Home so that the
  selection is visible. Opening a space's link enters that space.
* **Persistence**: the navigation path (top-level space → … → subspace) is
  stored per session in the session settings (`space_path`). It is restored
  once the spaces have loaded. A path that does not resolve yet is kept, so
  a slow first sync does not reset the choice.
* **Keyboard**: the rail is a `GtkListView` whose selection follows the
  navigation state, not the focus: arrow keys move the focus, Enter/Space
  activate (like the room list), and Tab moves between rail, header and
  room list. New shortcuts, listed in
  the shortcuts dialog: `Ctrl+Alt+PgDn` / `Ctrl+Alt+PgUp` for the next /
  previous space (wrapping, the space counterpart of `Ctrl+PgDn/PgUp` for
  rooms; Discord uses `Ctrl+Alt+↑/↓`, but GNOME Shell binds
  `Ctrl+Alt+arrows` to workspace switching) and `Ctrl+Alt+Home` for Home.
* **Accessibility**: the rail has the accessible label "Spaces". Each rail
  row's accessible label carries the name and the aggregated state ("Acme
  Corp 3 notifications, mentioned", "Acme Corp, unread messages"); the
  notification strings reuse the room row's existing translations. Avatars
  and badges are presentation-only. Every rail item has a tooltip with the
  space name. The back button and the lobby button have tooltips ("Parent
  Space", "Space Overview").

### Data model

All of this reuses `src/session/sidebar_data/`:

* `space_graph.rs`: pure Rust, no GObject, unit-tested. `SpaceGraph` is
  built from each joined space's `m.space.child` events (only those with a
  non-empty `via`, per the spec) and its `m.space_order`. It provides
  cycle-safe `descendants()`, `child_spaces()`, MSC3230-ordered
  `top_level_spaces()`, path resolution and `path_to()`. A cycle with no
  root (A→B→A) promotes its smallest space to the top level, so no joined
  space becomes unreachable. `SpaceVisibility` holds the filtering rules and
  `UnreadSummary` the aggregation rules.
* `space_list.rs`: `SidebarSpaceList` (a `GListModel` of Home + top-level
  spaces). It rebuilds the graph from the SDK store, debounced, when the
  room list changes, when a space is left, when a joined space receives
  `m.space.child` state events or MSC3230 order account data, and when the
  update stream lagged (via `Client::subscribe_to_all_room_updates`, the
  same signal the SDK's `SpaceService` uses). Top-level spaces that are not
  in the room list yet are left out until they are. It owns the navigation
  path and the two `GtkCustomFilter`s that the sidebar sections chain after
  their category filter: rooms in the selected space, and its direct child
  spaces.
* `space_item.rs`: `SidebarSpaceItem`, the per-space unread aggregate. It is
  an `ExpressionListModel` over the room list filtered to the space's
  descendants, watching `category`, `notification-count`, `highlight` and
  `is-read`, like `SidebarSection` does for sections.

The SDK's `SpaceService::space_filters()` was not used for filtering: it
stops at two levels, and its level-0 filters hold only direct children
([source][sdk-spaces]), which does not match the recursive semantics chosen
here.

## 4. Out of scope

* DMs with members of a space appearing in that space (Element Web's
  `showPeopleInSpace`).
* A "Home shows only rooms outside spaces" setting, and space-scoped
  search.
* Reordering spaces by drag and drop and writing `m.space_order`, plus
  folders of spaces (Discord, Cinny).
* Showing nested subspaces as an expandable tree inside the rail (Element
  Web). Nesting is navigated in the room list instead, which also works on
  phones.
* Aggregated unread badges on subspace rows in the room list. Those rows
  still show the space room's own state; the rail shows the aggregate.
* Space management (create a space, add or remove children, space
  settings), and the `m.space.parent`-only links that the spec trusts only
  under power-level conditions. The graph uses `m.space.child`.
* Remembering the last room per space (Element Web reopens it on switch).

[ew-panel]: https://github.com/element-hq/element-web/blob/develop/apps/web/src/components/views/spaces/SpacePanel.tsx
[ew-tree]: https://github.com/element-hq/element-web/blob/develop/apps/web/src/components/views/spaces/SpaceTreeLevel.tsx
[ew-store]: https://github.com/element-hq/element-web/blob/develop/apps/web/src/stores/spaces/SpaceStore.ts
[ew-settings]: https://github.com/element-hq/element-web/blob/develop/apps/web/src/settings/Settings.tsx
[ew-notif]: https://github.com/element-hq/element-web/blob/develop/apps/web/src/stores/notifications/SpaceNotificationState.ts
[ew-room-view]: https://github.com/element-hq/element-web/blob/develop/apps/web/src/components/structures/SpaceRoomView.tsx
[ex-blog]: https://element.io/blog/spaces-has-landed-on-element-x/
[ex-tabs]: https://github.com/element-hq/element-x-android/blob/develop/features/home/impl/src/main/kotlin/io/element/android/features/home/impl/HomeNavigationBarItem.kt
[ex-space-event]: https://github.com/element-hq/element-x-android/blob/develop/features/space/impl/src/main/kotlin/io/element/android/features/space/impl/root/SpaceEvent.kt
[sdk-spaces]: https://github.com/matrix-org/matrix-rust-sdk/blob/main/crates/matrix-sdk-ui/src/spaces/mod.rs
[cinny-nav]: https://github.com/cinnyapp/cinny/blob/dev/src/app/pages/client/SidebarNav.tsx
[cinny-space]: https://github.com/cinnyapp/cinny/blob/dev/src/app/pages/client/space/Space.tsx
[cinny-home]: https://github.com/cinnyapp/cinny/blob/dev/src/app/pages/client/home/useHomeRooms.ts
[cinny-tabs]: https://github.com/cinnyapp/cinny/blob/dev/src/app/pages/client/sidebar/SpaceTabs.tsx
[cinny-lobby]: https://github.com/cinnyapp/cinny/blob/dev/src/app/features/lobby/Lobby.tsx
[cinny-mobile]: https://github.com/cinnyapp/cinny/blob/dev/src/app/pages/MobileFriendly.tsx
[ff-rail]: https://github.com/krille-chan/fluffychat/blob/main/lib/pages/chat_list/navigation_rail.dart
[ff-space]: https://github.com/krille-chan/fluffychat/blob/main/lib/pages/chat_list/space_view.dart
[ff-themes]: https://github.com/krille-chan/fluffychat/blob/main/lib/config/themes.dart
[nheko-comm]: https://github.com/Nheko-Reborn/nheko/blob/master/src/timeline/CommunitiesModel.cpp
[nheko-list]: https://github.com/Nheko-Reborn/nheko/blob/master/src/timeline/RoomlistModel.cpp
[nheko-page]: https://github.com/Nheko-Reborn/nheko/blob/master/resources/qml/ChatPage.qml
[dc-folders]: https://support.discord.com/hc/en-us/articles/360030853132-Server-Folders-101
[dc-cats]: https://support.discord.com/hc/en-us/articles/115001580171-Channel-Categories-101
[dc-mute]: https://support.discord.com/hc/en-us/articles/209791877-How-do-I-mute-and-disable-notifications-for-specific-channels
[dc-tabs]: https://support.discord.com/hc/en-us/articles/360042584732-Tabs-to-Android
[hig-sidebars]: https://developer.gnome.org/hig/patterns/nav/sidebars.html
[hig-browsing]: https://developer.gnome.org/hig/patterns/nav/browsing.html
[adw-osv]: https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/class.OverlaySplitView.html
[adw-adaptive]: https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/adaptive-layouts.html
[adw-bp]: https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/class.Breakpoint.html
