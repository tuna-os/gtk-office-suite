//! In-message attachment drawer: a resizable footer beneath the reader body that
//! shows every attachment on the open message as a wrapping grid of thumbnails
//! (images and PDFs) or color-coded type icons (everything else), each with the
//! filename beneath it.
//!
//! It reuses the gallery's thumbnail/icon/open helpers ([`texture_from`],
//! [`thumbnail_texture`], [`icon_for`], [`icon_color_class`], [`open_bytes`]).
//! Hovering a cell reveals
//! the same Download/Open quick actions used in the gallery; right-clicking opens
//! a matching context menu; double-clicking previews images and PDFs in the
//! app's full-window lightbox (via [`DrawerOutput::ShowLightbox`]) and opens
//! any other type in its default app. Single clicks do nothing — a first click
//! must never steal the second. The header's "Save All…" button writes every
//! attachment into a chosen folder.
//!
//! Sizing: the drawer *owns* a vertical `GtkPaned` whose top pane is the reader
//! body (passed in via [`DrawerInit`]) and whose bottom pane is the drawer. The
//! divider is an invisible grabbable strip — native drag, and because the
//! reader pane is allowed to shrink, resizing the drawer never grows the
//! window; while the drawer is collapsed a drag snaps back, so only the
//! expanded drawer resizes. The size slider in the header scales the
//! thumbnails only; a click on the grab pill — or on the header's
//! "N attachments" count, which is a button for exactly this — collapses the
//! grid to just the header, and clicking again brings it back. The dragged
//! height, collapsed state and view settings are persisted; thumbnail size is
//! per-session.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use relm4::prelude::*;

use crate::config::{self, DrawerState};
use crate::i18n::{i18n, ni18n_f};
use crate::models::Attachment;
use crate::ui::attachment_io::open_bytes;

mod actions;
mod cells;
mod motion;
mod setup;

pub(crate) use cells::previewable;
use setup::{
    activate_on_keys, grab_pill, remove_paned_gestures, showcase_clicks, wire_drawer_edge,
};

const MIN_THUMB: f64 = 56.0;
const MAX_THUMB: f64 = 220.0;
/// Minimum expanded drawer height in px (the drag can't go below this).
const MIN_HEIGHT: i32 = 96;

/// What the drawer needs at launch: its remembered state plus the reader widget
/// it docks beneath (which becomes the top pane of its internal `GtkPaned`).
pub(crate) struct DrawerInit {
    pub(crate) state: DrawerState,
    pub(crate) reader: gtk::Widget,
}

pub(crate) struct AttachmentDrawer {
    items: Vec<Attachment>,
    /// Display position → index into `items`. Identity for the grid; the list
    /// view is sorted alphabetically (flipped by `sort_desc`).
    display_order: Vec<usize>,
    /// Show an alphabetical list instead of the thumbnail grid (persisted).
    list_view: bool,
    /// Sort the list Z→A instead of A→Z (persisted).
    sort_desc: bool,
    /// Thumbnail edge in px (size slider); does not affect drawer height.
    thumb: i32,
    /// Expanded drawer height in px (the Paned split); persisted.
    height: i32,
    /// Whether the thumbnail area is hidden, leaving only the header.
    collapsed: bool,
    /// The wrapping thumbnail grid (rebuilt on item/size changes).
    flow: gtk::FlowBox,
    /// Vertical split: reader body on top, drawer body below. The divider is the
    /// resize grip — native drag, and the reader shrinks rather than the window
    /// growing.
    paned: gtk::Paned,
    /// The visible resize affordance: the shared grab pill floated just above
    /// the seam (over the reader's bottom edge), like the split reply's.
    /// Shown whenever the drawer is. A click toggles collapsed/expanded; a
    /// drag resizes live — out of the collapse too, where the release point
    /// becomes the drawer's new height.
    pill: gtk::Box,
    /// True while we set the Paned position programmatically, so the resulting
    /// position-notify isn't mistaken for a user drag.
    adjusting: Rc<Cell<bool>>,
    /// Set once the initial split has been applied (needs a realized Paned).
    positioned: Rc<Cell<bool>>,
    /// Debounces persisting the dragged height: position-notify fires for
    /// every pixel of a drag, and each save is a config write.
    height_save: Rc<std::cell::RefCell<Option<glib::SourceId>>>,
    /// Whether the press currently on the pill has actually dragged (moved
    /// past the slop). Tells a release apart from a click: a click toggles
    /// the drawer, a drag's release must not.
    pill_dragged: bool,
    /// The running expand/collapse slide and its direction (`true` = closing
    /// toward the header). Held so a toggle or drag mid-slide can skip it to
    /// its end instead of fighting it over the divider.
    toggle_anim: Rc<std::cell::RefCell<Option<(adw::TimedAnimation, bool)>>>,
    /// Bumped whenever a slide is superseded, so a stale `ToggleSettled`
    /// from a skipped animation is recognised and ignored.
    toggle_gen: u64,
    /// Files being deleted from the server (#289), by name and size: drawn
    /// pale red with "Deleting…" until the server answers.
    deleting: Vec<(String, usize)>,
    /// Deleted files still fading out. A new list that arrives meanwhile
    /// waits in `pending_items`, so a fade is never cut short by a rebuild.
    fading: usize,
    pending_items: Option<Vec<Attachment>>,
    /// The drawer's body, shown when a message with attachments opens.
    drawer_body: Option<gtk::Box>,
    /// Bumped by every list, so an empty one held back is dropped when the
    /// next message's list follows it.
    empty_gen: u64,
}

/// What the drawer asks the app to do.
#[derive(Debug)]
pub(crate) enum DrawerOutput {
    /// Show the app's full-window lightbox over these previewable
    /// attachments, starting at `start`.
    ShowLightbox {
        items: Vec<Attachment>,
        start: usize,
    },
    /// Scroll the reader to the message this attachment came with (#213).
    ShowInMessage(Attachment),
    /// Take this attachment out of its message on the server (#289); the
    /// app asks first.
    DeleteFromServer(Attachment),
}

#[derive(Debug)]
pub(crate) enum AttachmentDrawerInput {
    /// Replace the shown attachments (empty hides the drawer).
    SetItems(Vec<Attachment>),
    /// New thumbnail edge from the size slider (thumbnails only, not height).
    SetThumbSize(i32),
    /// The Paned divider moved (user drag) — capture the new height.
    PositionChanged,
    /// Switch between the thumbnail grid and the alphabetical list.
    ToggleListView,
    /// Flip the list's sort direction (A→Z / Z→A).
    ToggleSortOrder,
    /// Open an attachment in its default application.
    Open(usize),
    /// Import an attached OpenPGP public key into the keyring (#133).
    ImportKey(usize),
    /// Activate a cell/row (display order) — double-click, or Space/Enter on
    /// the highlighted one: preview images and PDFs in the lightbox, open
    /// anything else in its default app.
    Activate(usize),
    /// Save an attachment to disk (file chooser).
    Download(usize),
    /// Scroll the reader to the message the attachment belongs to (#213).
    ShowInMessage(usize),
    /// Remove the attachment from its message on the server (#289).
    DeleteFromServer(usize),
    /// The app is deleting this file (name, size) from the server: show it
    /// as being deleted.
    MarkDeleting(String, usize),
    /// The server refused: show the file as it was.
    NotDeleted(String, usize),
    /// The server deleted it: fade it out.
    Deleted(String, usize),
    /// A fade finished: take the file away.
    FadeDone(String, usize),
    /// Save every attachment into a chosen folder ("Save All" header button).
    SaveAll,
    /// Right-click at (x, y) within the cell.
    ContextMenu { index: usize, x: f64, y: f64 },
    /// A press landed on the grab pill (a drag may or may not follow).
    PillDragBegin,
    /// The grab pill was dragged to this candidate divider position.
    PillDrag { want: i32 },
    /// The press on the grab pill was released. A plain click (no drag in
    /// between) toggles the drawer collapsed/expanded.
    PillReleased,
    /// A click landed on the seam itself (the Paned separator, or the
    /// drawer's topmost strip) — toggles like a pill click.
    EdgeClicked,
    /// The header's "N attachments" button was clicked — toggles the drawer
    /// like the seam does, from the one part of the header that is always
    /// visible (collapsed included).
    CountClicked,
    /// The expand/collapse slide finished. Carries the state it was heading
    /// for and the generation it belongs to (see `toggle_gen`).
    ToggleSettled { collapsed: bool, gen: u64 },
    /// Fold the drawer at once, its files out of the way (the reader below
    /// the list, #475).
    Fold,
    /// An empty list held back (see `SetItems`) is due, unless a newer list
    /// came since: carries the `empty_gen` it was held under.
    SettleEmpty(u64),
}

/// The drawer's widgets: the ones `init` wires afterwards (`drawer_body`,
/// `drawer_edge`, `count_btn`) and the ones that follow the model, which
/// `apply` sets from it at build and after every message. This is what
/// `#[relm4::component]`'s `view!` block and its `#[watch]` attributes used
/// to generate.
pub(crate) struct AttachmentDrawerWidgets {
    drawer_body: gtk::Box,
    drawer_edge: gtk::Box,
    count_btn: gtk::Button,
    count_label: gtk::Label,
    chevron: gtk::Image,
    small_icon: gtk::Image,
    scale: gtk::Scale,
    large_icon: gtk::Image,
    sort_btn: gtk::Button,
    view_btn: gtk::Button,
    scroller: gtk::ScrolledWindow,
    flow: gtk::FlowBox,
}

impl AttachmentDrawerWidgets {
    /// Everything that was `#[watch]`ed: set from the model.
    fn apply(&self, model: &AttachmentDrawer) {
        // Hidden (and the divider disappears) when there's nothing to show.
        self.drawer_body.set_visible(!model.items.is_empty());
        self.count_btn.set_tooltip_text(Some(
            &if model.collapsed { i18n("Show the attachments") } else { i18n("Hide the attachments") },
        ));
        self.count_label.set_label(&ni18n_f(
            "{n} attachment",
            "{n} attachments",
            model.items.len() as u32,
            &[("n", &model.items.len().to_string())],
        ));
        self.chevron.set_icon_name(Some(if model.collapsed { "pan-up-symbolic" } else { "pan-down-symbolic" }));
        // The size slider is meaningless with the grid hidden (and sizes
        // thumbnails only, so also in the list view).
        let sizing = !model.collapsed && !model.list_view;
        self.small_icon.set_visible(sizing);
        self.scale.set_visible(sizing);
        self.large_icon.set_visible(sizing);
        // Only the list has an order to flip.
        self.sort_btn.set_visible(!model.collapsed && model.list_view);
        self.sort_btn.set_icon_name(if model.sort_desc { "view-sort-descending-symbolic" } else { "view-sort-ascending-symbolic" });
        self.sort_btn.set_tooltip_text(Some(&if model.sort_desc {
            i18n("Sorted Z to A — switch to A to Z")
        } else {
            i18n("Sorted A to Z — switch to Z to A")
        }));
        self.view_btn.set_visible(!model.collapsed);
        self.view_btn.set_icon_name(if model.list_view { "view-grid-symbolic" } else { "view-list-bullet-symbolic" });
        self.view_btn.set_tooltip_text(Some(&if model.list_view { i18n("Show as thumbnails") } else { i18n("Show as a list") }));
        self.scroller.set_visible(!model.collapsed);
        self.flow.set_max_children_per_line(if model.list_view { 1 } else { 40 });
    }
}

/// An icon-only flat button in the drawer's header.
fn header_icon_button(sender: &ComponentSender<AttachmentDrawer>, message: fn() -> AttachmentDrawerInput) -> gtk::Button {
    let button = gtk::Button::builder().css_classes(["flat"]).valign(gtk::Align::Center).build();
    let sender = sender.clone();
    button.connect_clicked(move |_| sender.input_sender().emit(message()));
    button
}

fn build_widgets(
    root: &gtk::Paned,
    sender: &ComponentSender<AttachmentDrawer>,
    model: &AttachmentDrawer,
    flow: &gtk::FlowBox,
    scroller: &gtk::ScrolledWindow,
) -> AttachmentDrawerWidgets {
    root.set_orientation(gtk::Orientation::Vertical);
    // No wide handle: its stock chrome is a bordered band with a grip. The
    // plain separator is a permanently inert hairline (see `.drawer-split`
    // and set_divider_draggable): the grab pill floated over the reader's
    // bottom edge and the drawer's own edge strip carry every click and
    // drag.
    root.add_css_class("drawer-split");
    // The reader (start child) shrinks to make room; the drawer keeps its
    // set size. This is what prevents the window from growing.
    root.set_resize_start_child(true);
    root.set_shrink_start_child(true);
    root.set_resize_end_child(false);
    root.set_shrink_end_child(false);
    {
        let sender = sender.clone();
        root.connect_position_notify(move |_| sender.input_sender().emit(AttachmentDrawerInput::PositionChanged));
    }

    // The drawer's own top edge as a real widget: it clicks (toggle), drags
    // (resize), and carries the same ns-resize cursor as the pill zone
    // above, so the seam feels like one continuous handle. The native Paned
    // separator is shrunk to a hairline: its drag gesture claims every
    // press, which made it a click-dead band with its own cursor.
    let drawer_edge = gtk::Box::builder().height_request(8).build();
    drawer_edge.set_cursor_from_name(Some("ns-resize"));

    // The count is the drawer's other handle: clicking it expands or
    // collapses exactly like a click on the seam, with the chevron showing
    // which way it goes next. GTK hands a button with a plain label the
    // `text-button` padding and one with an icon the `image-button` padding;
    // a button with a child of its own (this one: clip, count, chevron) gets
    // neither and ends up tighter than "Save All…" beside it. Ask for the
    // text one by hand so the two pills match. A hair off the header's own
    // inset at each end (Jason, 2026-09-17): the count and "Save All…" are
    // the row's two pills, so they sit the same distance from the edge they
    // face.
    let clip = gtk::Image::builder().icon_name("mail-attachment-symbolic").css_classes(["dim-label"]).build();
    let count_label = gtk::Label::builder().css_classes(["heading"]).build();
    let chevron = gtk::Image::builder().css_classes(["dim-label"]).pixel_size(12).build();
    let count_content = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(8).build();
    count_content.append(&clip);
    count_content.append(&count_label);
    count_content.append(&chevron);
    let count_btn = gtk::Button::builder()
        .css_classes(["flat", "text-button"])
        .valign(gtk::Align::Center)
        .margin_start(3)
        .child(&count_content)
        .build();
    {
        let sender = sender.clone();
        count_btn.connect_clicked(move |_| sender.input_sender().emit(AttachmentDrawerInput::CountClicked));
    }

    let small_icon = gtk::Image::builder()
        .icon_name("image-x-generic-symbolic")
        .css_classes(["dim-label"])
        .pixel_size(12)
        .build();
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, None::<&gtk::Adjustment>);
    scale.set_width_request(130);
    scale.set_range(MIN_THUMB, MAX_THUMB);
    scale.set_value(model.thumb as f64);
    scale.set_round_digits(0);
    scale.set_draw_value(false);
    scale.set_tooltip_text(Some(&i18n("Thumbnail size")));
    scale.set_valign(gtk::Align::Center);
    {
        let sender = sender.clone();
        scale.connect_value_changed(move |s| sender.input_sender().emit(AttachmentDrawerInput::SetThumbSize(s.value() as i32)));
    }
    let large_icon = gtk::Image::builder()
        .icon_name("image-x-generic-symbolic")
        .css_classes(["dim-label"])
        .pixel_size(22)
        .build();
    let sort_btn = header_icon_button(sender, || AttachmentDrawerInput::ToggleSortOrder);
    let view_btn = header_icon_button(sender, || AttachmentDrawerInput::ToggleListView);
    // Rightmost by design: the header's one real action, with the view
    // controls (slider, sort, list/grid) to its left. A standing button, not
    // a flat hover-reveal; its margin matches the count's inset at the other
    // end.
    let save_all = gtk::Button::builder()
        .label(i18n("Save All…"))
        .valign(gtk::Align::Center)
        .margin_end(3)
        .tooltip_text(i18n("Save every attachment to a folder"))
        .build();
    {
        let sender = sender.clone();
        save_all.connect_clicked(move |_| sender.input_sender().emit(AttachmentDrawerInput::SaveAll));
    }

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["attachment-drawer-header"])
        .build();
    header.append(&count_btn);
    header.append(&gtk::Box::builder().hexpand(true).build());
    header.append(&small_icon);
    header.append(&scale);
    header.append(&large_icon);
    header.append(&sort_btn);
    header.append(&view_btn);
    header.append(&save_all);

    scroller.set_vexpand(true);
    scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroller.set_vscrollbar_policy(gtk::PolicyType::Automatic);
    // Single-click highlights an attachment; Space or Enter then previews
    // it, Nautilus-style (issue #37).
    flow.set_orientation(gtk::Orientation::Horizontal);
    flow.set_selection_mode(gtk::SelectionMode::Single);
    flow.set_activate_on_single_click(true);
    flow.set_homogeneous(false);
    flow.set_valign(gtk::Align::Start);
    flow.set_row_spacing(6);
    flow.set_column_spacing(6);
    flow.set_margin_top(8);
    flow.set_margin_bottom(8);
    flow.set_margin_start(8);
    flow.set_margin_end(8);
    flow.set_min_children_per_line(1);
    flow.add_css_class("attachment-drawer-flow");
    scroller.set_child(Some(flow));

    let drawer_body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["attachment-drawer"])
        .build();
    drawer_body.append(&drawer_edge);
    drawer_body.append(&header);
    drawer_body.append(scroller);
    root.set_end_child(Some(&drawer_body));

    let widgets = AttachmentDrawerWidgets {
        drawer_body,
        drawer_edge,
        count_btn,
        count_label,
        chevron,
        small_icon,
        scale,
        large_icon,
        sort_btn,
        view_btn,
        scroller: scroller.clone(),
        flow: flow.clone(),
    };
    widgets.apply(model);
    widgets
}

impl SimpleComponent for AttachmentDrawer {
    type Init = DrawerInit;
    type Input = AttachmentDrawerInput;
    type Output = DrawerOutput;
    type Root = gtk::Paned;
    type Widgets = AttachmentDrawerWidgets;

    fn init_root() -> Self::Root {
        gtk::Paned::new(gtk::Orientation::Vertical)
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.apply(self);
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let flow = gtk::FlowBox::default();
        let scroller = gtk::ScrolledWindow::default();

        activate_on_keys(&flow, &sender);

        let pill = grab_pill(&root, &sender);

        let mut model = AttachmentDrawer {
            items: Vec::new(),
            display_order: Vec::new(),
            list_view: init.state.list_view,
            sort_desc: init.state.sort_desc,
            thumb: init.state.thumb.clamp(MIN_THUMB as i32, MAX_THUMB as i32),
            height: init.state.height.max(MIN_HEIGHT),
            collapsed: init.state.collapsed,
            flow: flow.clone(),
            // The root of this component IS the Paned.
            paned: root.clone(),
            pill: pill.clone(),
            adjusting: Rc::new(Cell::new(false)),
            positioned: Rc::new(Cell::new(false)),
            height_save: Rc::new(std::cell::RefCell::new(None)),
            pill_dragged: false,
            toggle_anim: Rc::new(std::cell::RefCell::new(None)),
            toggle_gen: 0,
            deleting: Vec::new(),
            fading: 0,
            pending_items: None,
            drawer_body: None,
            empty_gen: 0,
        };

        let widgets = build_widgets(&root, &sender, &model, &flow, &scroller);
        model.drawer_body = Some(widgets.drawer_body.clone());
        // The body moves into an overlay of its own (`header_height` reads
        // it there). The overlay comes and goes with the body, so the split
        // still hides its seam with no drawer.
        {
            let body = widgets.drawer_body.clone();
            let frame = gtk::Overlay::new();
            root.set_end_child(None::<&gtk::Widget>);
            frame.set_child(Some(&body));
            body.bind_property("visible", &frame, "visible")
                .sync_create()
                .build();
            root.set_end_child(Some(&frame));
            root.set_resize_end_child(false);
            root.set_shrink_end_child(false);
        }
        // Dock the reader as the top pane (the drawer body is the bottom pane),
        // wrapped in the Overlay that floats the grab pill over its lower edge.
        let reader_wrap = gtk::Overlay::new();
        reader_wrap.set_child(Some(&init.reader));
        reader_wrap.add_overlay(&pill);
        root.set_start_child(Some(&reader_wrap));
        // A collapsed drawer starts with its divider locked.
        // The native separator is permanently inert: GTK gives it an
        // enlarged invisible input region that wears its own row-resize
        // cursor and steals presses landing near the seam — the pill zone
        // and the drawer's edge strip carry every click and drag instead.
        model.set_divider_draggable(false);
        remove_paned_gestures(&root);

        wire_drawer_edge(&widgets.drawer_edge, &pill, &root, &sender);
        showcase_clicks(&widgets.count_btn);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            AttachmentDrawerInput::SetItems(items) if self.fading > 0 => {
                self.pending_items = Some(items);
            }
            AttachmentDrawerInput::SetItems(items) => self.set_items(items, &sender),
            AttachmentDrawerInput::SettleEmpty(gen) => {
                if gen == self.empty_gen {
                    self.apply_items(Vec::new(), &sender);
                }
            }
            AttachmentDrawerInput::SetThumbSize(size) => {
                // Thumbnail size is a live setting only — never persisted.
                let size = size.clamp(MIN_THUMB as i32, MAX_THUMB as i32);
                if size != self.thumb {
                    self.thumb = size;
                    self.rebuild(&sender);
                }
            }
            AttachmentDrawerInput::PositionChanged => self.position_changed(),
            AttachmentDrawerInput::ToggleListView => {
                self.list_view = !self.list_view;
                self.rebuild(&sender);
                config::save_drawer_list_view(self.list_view);
            }
            AttachmentDrawerInput::ToggleSortOrder => {
                self.sort_desc = !self.sort_desc;
                self.rebuild(&sender);
                config::save_drawer_sort_desc(self.sort_desc);
            }
            AttachmentDrawerInput::Open(i) => {
                if let Some(att) = self.item_at(i) {
                    open_bytes(&att.name, &att.data, self.window().as_ref());
                }
            }
            AttachmentDrawerInput::ImportKey(i) => self.import_key(i),
            AttachmentDrawerInput::Download(i) => {
                if let Some(att) = self.item_at(i).cloned() {
                    self.save_attachment(&att);
                }
            }
            AttachmentDrawerInput::ShowInMessage(i) => {
                if let Some(att) = self.item_at(i).cloned() {
                    let _ = sender.output(DrawerOutput::ShowInMessage(att));
                }
            }
            AttachmentDrawerInput::MarkDeleting(name, len) => {
                self.deleting.push((name, len));
                self.rebuild_unless_fading(&sender);
            }
            AttachmentDrawerInput::NotDeleted(name, len) => {
                if let Some(at) = self.deleting.iter().position(|d| *d == (name.clone(), len)) {
                    self.deleting.remove(at);
                    self.rebuild_unless_fading(&sender);
                }
            }
            AttachmentDrawerInput::Deleted(name, len) => self.deleted(name, len, &sender),
            AttachmentDrawerInput::FadeDone(name, len) => self.fade_done(name, len, &sender),
            AttachmentDrawerInput::DeleteFromServer(i) => {
                if let Some(att) = self.item_at(i).cloned() {
                    let _ = sender.output(DrawerOutput::DeleteFromServer(att));
                }
            }
            AttachmentDrawerInput::Activate(i) => self.activate(i, &sender),
            AttachmentDrawerInput::SaveAll => {
                self.save_all();
            }
            AttachmentDrawerInput::ContextMenu { index, x, y } => {
                self.show_context_menu(index, x, y, &sender);
            }
            AttachmentDrawerInput::PillDragBegin => {
                self.pill_dragged = false;
                // A drag takes over from any running slide: jump it to its
                // end (settling the collapse it was heading for) so the two
                // don't fight over the divider.
                self.finish_running_toggle();
            }
            AttachmentDrawerInput::PillDrag { want } => self.pill_drag(want),
            AttachmentDrawerInput::PillReleased => {
                // A plain click — the press never travelled past the drag
                // slop — toggles the drawer. A drag's release must not: the
                // user just set a height.
                if !self.pill_dragged {
                    self.toggle_collapsed(&sender);
                }
            }
            AttachmentDrawerInput::EdgeClicked => {
                // The edge strip shares the pill's drag plumbing: a release
                // that ended a drag just set a height and must not toggle.
                if !self.pill_dragged {
                    self.toggle_collapsed(&sender);
                }
            }
            AttachmentDrawerInput::CountClicked => {
                // A real button, never part of a drag: it toggles
                // unconditionally (the seam's `pill_dragged` guard would
                // otherwise swallow the first click after any resize).
                self.toggle_collapsed(&sender);
            }
            AttachmentDrawerInput::ToggleSettled { collapsed, gen } => {
                self.toggle_settled(collapsed, gen)
            }
            AttachmentDrawerInput::Fold => self.fold(),
        }
    }
}

impl AttachmentDrawer {}
