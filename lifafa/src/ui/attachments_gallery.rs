//! Attachments gallery: a grid of every cached attachment across the connected
//! inboxes, with a lightbox preview (prev/next, open, go to message).
//!
//! Data comes from the SQLite cache (what the background prefetch has already
//! downloaded), fed in a page at a time via [`GalleryInput::Page`]. Image attachments and PDFs
//! (rendered from their first page) show as thumbnails; other files show a type
//! icon. Clicking a cell opens a large overlay preview.

use adw::prelude::*;
use gtk::gdk;
use gtk::glib;
use relm4::prelude::*;

use std::collections::HashMap;

use crate::i18n::i18n;
use crate::models::{GalleryItem, GallerySort};

mod cell;
mod items;
mod labels;
mod msg;
mod ratio_box;
mod scope;
mod table;
mod update;

use cell::build_cell;
pub(crate) use cell::{deleting_label, deleting_over, deleting_veil, fade_out};
use labels::{caption, column_header, count_text, scan_text};
use msg::key_of;
pub(crate) use msg::{FileKey, GalleryInput, GalleryOutput, GalleryRequest};

use crate::ui::attachment_io::open_bytes;
use crate::ui::thumbnails::{
    icon_color_class, icon_for, is_pdf_name, lightbox_pdf_texture, spawn_thumbnail_render,
    texture_from, thumb_cache_key, thumbnail_spinner, thumbnail_texture, Thumbnail, PDF_PREVIEWS,
};
use ratio_box::RatioBox;
pub(crate) use scope::{GalleryAccount, GalleryFolder};
use table::build_row;

/// Width of the table's trailing quick-actions column (three icon buttons);
/// the header carries a spacer of the same width so the columns line up.
const TABLE_ACTIONS_WIDTH: i32 = 100;

/// How many attachments one page holds. Big enough that scrolling rarely waits,
/// small enough that the first screen appears at once and a page's worth of
/// cached thumbnails is not a burden.
const PAGE_SIZE: u32 = 120;

/// How close to the end of the list the scroll has to come before the next page
/// is asked for, in px. Roughly two rows of thumbnails, so the page is usually
/// already there by the time the user reaches it.
const LOAD_MORE_MARGIN: f64 = 700.0;

/// How long typing has to pause before the search runs, in ms. Each search is a
/// query over the whole archive and a rebuilt grid, so running one per keystroke
/// would spend most of its time on words half-typed.
const SEARCH_DEBOUNCE_MS: u32 = 250;

pub(crate) struct AttachmentsGallery {
    /// The pages loaded so far, already filtered and ordered by the database.
    /// This is a window onto `total`, not the whole set: an archive of two
    /// decades holds far more attachments than the UI can hold widgets for, so
    /// the list grows a page at a time as the view is scrolled.
    all_items: Vec<GalleryItem>,
    /// How many attachments match the current query in total, across every
    /// page — what the footer counts and how `has_more` is known.
    total: u32,
    /// A page request is in flight; the footer shows its spinner and no second
    /// request is sent until it lands.
    loading_more: bool,
    /// Messages the scan has still to describe across the folders in scope.
    /// Non-zero means the gallery is not yet showing everything there is.
    scan_remaining: u32,
    /// An attachment that was never downloaded is on its way from the server.
    fetching: bool,
    query: String,
    sort: GallerySort,
    /// Index into `items` currently shown in the lightbox, if any.
    preview: Option<usize>,
    /// What the lightbox shows for the current item: a decoded image, or a
    /// PDF's first page once its full-size render lands (None while a PDF
    /// render is in flight, and for types with nothing to show).
    preview_texture: Option<gdk::Texture>,
    /// The PDF being previewed could not be rendered, such as one locked
    /// with a password: its page is not on its way.
    preview_failed: bool,
    loading: bool,
    /// Show the sortable table instead of the thumbnail grid (persisted).
    view_table: bool,
    /// Grid thumbnail cell width in px, driven by the footer slider (persisted).
    thumb_width: i32,
    /// The footer type dropdown's row: 0 = all, then one bucket per row.
    type_filter: u32,
    /// Show only one account's attachments; `None` shows every account. Unlike
    /// the folder scope this is a view filter, not a setting, so it resets to
    /// "All accounts" each time the gallery is opened.
    account_filter: Option<u32>,
    /// Every account and the folders it offers, as fed by the app.
    accounts: Vec<GalleryAccount>,
    /// Pull from Archive folders (persisted master switch).
    include_archive: bool,
    /// Pull from folders that are neither Inbox nor Archive (persisted).
    include_other: bool,
    /// The folders the user ticked or unticked by hand, overriding
    /// [`kind_default`]: account id -> path -> on. Persisted.
    overrides: HashMap<u32, HashMap<String, bool>>,
    /// Effective per-folder verdict, recomputed from the master switches and
    /// `overrides` whenever either changes, so filtering is a lookup.
    included: HashMap<u32, HashMap<String, bool>>,
    /// Debounce for the size slider — one rebuild after the drag settles.
    resize_timer: Option<glib::SourceId>,
    /// Debounce for the search box — one query after the typing settles.
    query_timer: Option<glib::SourceId>,
    flow: gtk::FlowBox,
    /// The table view's rows (the grid's sibling stack page).
    table: gtk::ListBox,
    /// The folder list inside the footer's "Folders" popover, rebuilt whenever
    /// the account/folder set changes.
    folder_list: gtk::ListBox,
    /// The account filter dropdown's model: "All accounts" then one row per
    /// account, in `accounts` order.
    account_names: gtk::StringList,
    /// The component's root widget, used to anchor the right-click context menu.
    root: gtk::Widget,
    /// Files being deleted from the server (#289), drawn pale red with
    /// "Deleting…" until the server answers.
    deleting: Vec<FileKey>,
}

/// The gallery's widgets: the ones other code reaches by field, and the ones
/// that follow the model, which `apply` sets from it at build and after every
/// message. This is what `#[relm4::component]`'s `view!` block and its
/// `#[watch]` attributes used to generate.
pub(crate) struct AttachmentsGalleryWidgets {
    toolbar: gtk::Box,
    search_entry: gtk::SearchEntry,
    stack: gtk::Stack,
    name_header: gtk::Label,
    sender_header: gtk::Label,
    type_header: gtk::Label,
    date_header: gtk::Label,
    size_header: gtk::Label,
    footer: gtk::ActionBar,
    grid_toggle: gtk::ToggleButton,
    grid_handler: glib::SignalHandlerId,
    table_toggle: gtk::ToggleButton,
    table_handler: glib::SignalHandlerId,
    account_dropdown: gtk::DropDown,
    folders_button: gtk::MenuButton,
    folders_box: gtk::Box,
    archive_row: adw::SwitchRow,
    archive_handler: glib::SignalHandlerId,
    other_row: adw::SwitchRow,
    other_handler: glib::SignalHandlerId,
    sort_dropdown: gtk::DropDown,
    count_label: gtk::Label,
    status_box: gtk::Box,
    status_spinner: gtk::Spinner,
    status_label: gtk::Label,
    scale: gtk::Scale,
    lightbox: gtk::Box,
    lightbox_title: gtk::Label,
    prev_button: gtk::Button,
    next_button: gtk::Button,
    preview_stack: gtk::Stack,
    preview_picture: gtk::Picture,
    file_icon: gtk::Image,
    file_name: gtk::Label,
    no_preview: gtk::Label,
    caption_label: gtk::Label,
    open_button: gtk::Button,
}

impl AttachmentsGalleryWidgets {
    /// Everything that was `#[watch]`ed: set from the model.
    fn apply(&self, model: &AttachmentsGallery) {
        let show_chrome = model.show_chrome();
        self.toolbar.set_visible(show_chrome);
        self.stack.set_visible_child_name(model.page());
        self.name_header.set_label(&column_header(
            &i18n("Name"),
            model.sort,
            GallerySort::Name,
            GallerySort::NameDesc,
        ));
        self.sender_header.set_label(&column_header(
            &i18n("Sender"),
            model.sort,
            GallerySort::Sender,
            GallerySort::SenderDesc,
        ));
        self.type_header.set_label(&column_header(
            &i18n("Type"),
            model.sort,
            GallerySort::Type,
            GallerySort::TypeDesc,
        ));
        self.date_header.set_label(&column_header(
            &i18n("Date"),
            model.sort,
            GallerySort::Oldest,
            GallerySort::Newest,
        ));
        self.size_header.set_label(&column_header(
            &i18n("Size"),
            model.sort,
            GallerySort::Smallest,
            GallerySort::Largest,
        ));
        self.footer.set_revealed(show_chrome);

        self.grid_toggle.block_signal(&self.grid_handler);
        self.grid_toggle.set_active(!model.view_table);
        self.grid_toggle.unblock_signal(&self.grid_handler);
        self.table_toggle.block_signal(&self.table_handler);
        self.table_toggle.set_active(model.view_table);
        self.table_toggle.unblock_signal(&self.table_handler);

        // Pointless with one account, and it would be the only control there
        // that could never change anything.
        self.account_dropdown.set_visible(model.accounts.len() > 1);
        self.archive_row.block_signal(&self.archive_handler);
        self.archive_row.set_active(model.include_archive);
        self.archive_row.unblock_signal(&self.archive_handler);
        self.other_row.block_signal(&self.other_handler);
        self.other_row.set_active(model.include_other);
        self.other_row.unblock_signal(&self.other_handler);

        self.count_label
            .set_label(&count_text(model.all_items.len(), model.total as usize));
        let busy = model.loading_more || model.scan_remaining > 0;
        self.status_box.set_visible(busy);
        self.status_spinner.set_spinning(busy);
        self.status_label
            .set_label(&scan_text(model.loading_more, model.scan_remaining));
        self.scale.set_visible(!model.view_table);

        // The lightbox.
        let current = model.current();
        self.lightbox.set_visible(model.preview.is_some());
        let name = current.map(|i| i.name.clone()).unwrap_or_default();
        self.lightbox_title.set_label(&name);
        let several = model.all_items.len() > 1;
        self.prev_button.set_sensitive(several);
        self.next_button.set_sensitive(several);
        self.preview_stack
            .set_visible_child_name(if model.preview_texture.is_some() {
                "image"
            } else if !model.preview_failed
                && current.is_some_and(|i| is_pdf_name(&i.name) && i.data.is_some())
            {
                // A PDF whose full-size render is still on its way.
                "rendering"
            } else {
                "file"
            });
        self.preview_picture
            .set_paintable(model.preview_texture.as_ref());
        self.file_icon
            .set_icon_name(current.map(|i| icon_for(&i.name)));
        self.file_icon.set_css_classes(&[
            "gallery-file-icon",
            current
                .map(|i| icon_color_class(&i.name))
                .unwrap_or("ftype-generic"),
        ]);
        self.file_name.set_label(&name);
        self.no_preview.set_visible(model.preview_failed);
        self.caption_label
            .set_label(&current.map(caption).unwrap_or_default());
        self.open_button
            .set_sensitive(current.is_some_and(|i| i.data.is_some()));
    }
}

/// A scroller over `child` that asks for the next page near its end.
fn paging_scroller(
    sender: &ComponentSender<AttachmentsGallery>,
    child: &impl IsA<gtk::Widget>,
) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroller.set_vexpand(true);
    scroller.set_child(Some(child));
    let tx = sender.input_sender().clone();
    scroller.connect_edge_reached(move |_, pos| {
        if pos == gtk::PositionType::Bottom {
            tx.emit(GalleryInput::LoadMore);
        }
    });
    let tx = sender.input_sender().clone();
    scroller.vadjustment().connect_value_changed(move |a| {
        if near_end(a) {
            tx.emit(GalleryInput::LoadMore);
        }
    });
    scroller
}

/// A flat, sortable column header button around `label`.
fn header_button(
    sender: &ComponentSender<AttachmentsGallery>,
    label: &gtk::Label,
    column: u32,
    width: Option<i32>,
) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    match width {
        Some(w) => {
            button.set_width_request(w);
            button.set_hexpand(false);
        }
        None => button.set_hexpand(true),
    }
    button.set_child(Some(label));
    let tx = sender.input_sender().clone();
    button.connect_clicked(move |_| tx.emit(GalleryInput::SortColumn(column)));
    button
}

fn dropdown(tooltip: &str, labels: &[String]) -> gtk::DropDown {
    let d = gtk::DropDown::new(
        Some(gtk::StringList::new(
            &labels.iter().map(String::as_str).collect::<Vec<_>>(),
        )),
        gtk::Expression::NONE,
    );
    d.set_tooltip_text(Some(tooltip));
    d
}

fn text_button(label: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_label(label);
    b
}

/// Builds the widget tree under `root` (the overlay).
fn build_widgets(
    root: &gtk::Overlay,
    sender: &ComponentSender<AttachmentsGallery>,
    model: &AttachmentsGallery,
) -> AttachmentsGalleryWidgets {
    root.add_css_class("attachments-gallery");
    let tx = sender.input_sender().clone();

    // Base layer: a search/sort toolbar above the scrolling grid.
    let base = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_child(Some(&base));

    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toolbar.add_css_class("gallery-toolbar");
    let search_entry = gtk::SearchEntry::new();
    search_entry.set_hexpand(true);
    search_entry.set_placeholder_text(Some(&i18n("Search by sender, subject, type, filename…")));
    let t = tx.clone();
    search_entry.connect_search_changed(move |e| {
        t.emit(GalleryInput::SetQuery(e.text().to_string()));
    });
    toolbar.append(&search_entry);
    base.append(&toolbar);

    let stack = gtk::Stack::new();
    stack.set_vexpand(true);
    base.append(&stack);

    let loading = gtk::Box::new(gtk::Orientation::Vertical, 14);
    loading.set_halign(gtk::Align::Center);
    loading.set_valign(gtk::Align::Center);
    let spinner = gtk::Spinner::new();
    spinner.set_spinning(true);
    spinner.set_width_request(36);
    spinner.set_height_request(36);
    loading.append(&spinner);
    let loading_label = gtk::Label::new(Some(&i18n("Loading attachments…")));
    loading_label.add_css_class("dim-label");
    loading.append(&loading_label);
    stack.add_named(&loading, Some("loading"));

    let empty = adw::StatusPage::new();
    empty.set_icon_name(Some("mail-attachment-symbolic"));
    empty.set_title(&i18n("No attachments"));
    empty.set_description(Some(&i18n(
        "Attachments from your inboxes will appear here.",
    )));
    stack.add_named(&empty, Some("empty"));

    let noresults = adw::StatusPage::new();
    noresults.set_icon_name(Some("system-search-symbolic"));
    noresults.set_title(&i18n("No matching attachments"));
    noresults.set_description(Some(&i18n(
        "Try a different search, or check which accounts and folders the footer is pulling from.",
    )));
    stack.add_named(&noresults, Some("noresults"));

    let flow = &model.flow;
    flow.set_valign(gtk::Align::Start);
    flow.set_max_children_per_line(8);
    flow.set_min_children_per_line(3);
    flow.set_row_spacing(14);
    flow.set_column_spacing(14);
    flow.set_homogeneous(true);
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_activate_on_single_click(true);
    flow.add_css_class("gallery-flow");
    let t = tx.clone();
    flow.connect_child_activated(move |_, child| {
        t.emit(GalleryInput::Activate(child.index() as u32));
    });
    stack.add_named(&paging_scroller(sender, flow), Some("grid"));

    // Table view: fixed sortable column headers over the rows.
    let table_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    header.add_css_class("gallery-table-header");
    // Aligns the headers with the rows' leading thumbnail.
    let lead = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    lead.set_width_request(28);
    header.append(&lead);
    let header_label = |xalign: f32| {
        let l = gtk::Label::new(None);
        l.set_xalign(xalign);
        l
    };
    let name_header = header_label(0.0);
    let sender_header = header_label(0.0);
    let type_header = header_label(0.0);
    let date_header = header_label(0.0);
    let size_header = header_label(1.0);
    // Only Name expands, as in the rows: a header that also took spare width
    // (a child's hexpand propagates to its button) would shift every column
    // between them off its cells.
    header.append(&header_button(sender, &name_header, 0, None));
    header.append(&header_button(sender, &sender_header, 1, Some(170)));
    header.append(&header_button(sender, &type_header, 4, Some(90)));
    header.append(&header_button(sender, &date_header, 2, Some(110)));
    header.append(&header_button(sender, &size_header, 3, Some(90)));
    // Aligns with the rows' trailing actions column.
    let trail = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    trail.set_width_request(TABLE_ACTIONS_WIDTH);
    header.append(&trail);
    table_page.append(&header);

    let table = &model.table;
    table.set_selection_mode(gtk::SelectionMode::None);
    table.set_activate_on_single_click(true);
    table.add_css_class("gallery-table");
    let t = tx.clone();
    table.connect_row_activated(move |_, row| {
        t.emit(GalleryInput::Activate(row.index() as u32));
    });
    table_page.append(&paging_scroller(sender, table));
    stack.add_named(&table_page, Some("table"));

    // Footer: view toggle, filtering/ordering, size, count — the gallery's
    // controls in one place, out of the content's way.
    let footer = gtk::ActionBar::new();
    footer.add_css_class("gallery-footer");
    let toggles = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    toggles.add_css_class("linked");
    let grid_toggle = gtk::ToggleButton::new();
    grid_toggle.set_icon_name("view-grid-symbolic");
    grid_toggle.set_tooltip_text(Some(&i18n("Thumbnail grid")));
    let t = tx.clone();
    let grid_handler = grid_toggle.connect_clicked(move |_| {
        t.emit(GalleryInput::SetViewTable(false));
    });
    let table_toggle = gtk::ToggleButton::new();
    table_toggle.set_icon_name("view-list-bullet-symbolic");
    table_toggle.set_tooltip_text(Some(&i18n("Table")));
    let t = tx.clone();
    let table_handler = table_toggle.connect_clicked(move |_| {
        t.emit(GalleryInput::SetViewTable(true));
    });
    toggles.append(&grid_toggle);
    toggles.append(&table_toggle);
    footer.pack_start(&toggles);

    // Scope: which account and which folders feed the gallery.
    let account_dropdown =
        gtk::DropDown::new(Some(model.account_names.clone()), gtk::Expression::NONE);
    account_dropdown.set_tooltip_text(Some(&i18n("Show only this account")));
    let t = tx.clone();
    account_dropdown.connect_selected_notify(move |d| {
        t.emit(GalleryInput::SetAccountFilter(d.selected()));
    });
    footer.pack_start(&account_dropdown);

    let folders_button = gtk::MenuButton::new();
    folders_button.set_icon_name("folder-symbolic");
    folders_button.set_tooltip_text(Some(&i18n("Folders to pull from")));
    let popover = gtk::Popover::new();
    popover.add_css_class("gallery-folders-popover");
    let folders_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
    folders_box.set_width_request(320);
    let switches = gtk::ListBox::new();
    switches.add_css_class("boxed-list");
    switches.set_selection_mode(gtk::SelectionMode::None);
    let archive_row = adw::SwitchRow::new();
    archive_row.set_title(&i18n("Include Archive"));
    let t = tx.clone();
    let archive_handler = archive_row.connect_active_notify(move |r| {
        t.emit(GalleryInput::SetIncludeArchive(r.is_active()));
    });
    let other_row = adw::SwitchRow::new();
    other_row.set_title(&i18n("Include other folders"));
    other_row.set_subtitle(&i18n("Everything that is not an inbox or an archive"));
    let t = tx.clone();
    let other_handler = other_row.connect_active_notify(move |r| {
        t.emit(GalleryInput::SetIncludeOther(r.is_active()));
    });
    switches.append(&archive_row);
    switches.append(&other_row);
    folders_box.append(&switches);
    let pulling = gtk::Label::new(Some(&i18n("Pulling attachments from")));
    pulling.set_xalign(0.0);
    pulling.add_css_class("heading");
    folders_box.append(&pulling);
    let folder_scroll = gtk::ScrolledWindow::new();
    folder_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    folder_scroll.set_propagate_natural_height(true);
    folder_scroll.set_max_content_height(380);
    let folder_list = &model.folder_list;
    folder_list.add_css_class("boxed-list");
    folder_list.set_selection_mode(gtk::SelectionMode::None);
    folder_scroll.set_child(Some(folder_list));
    folders_box.append(&folder_scroll);
    popover.set_child(Some(&folders_box));
    folders_button.set_popover(Some(&popover));
    footer.pack_start(&folders_button);

    let type_dropdown = dropdown(
        &i18n("Show only this type"),
        &[
            i18n("All types"),
            i18n("Images"),
            i18n("PDFs"),
            i18n("Documents"),
            i18n("Archives"),
            i18n("Audio & Video"),
            i18n("Other"),
        ],
    );
    let t = tx.clone();
    type_dropdown.connect_selected_notify(move |d| {
        t.emit(GalleryInput::SetTypeFilter(d.selected()));
    });
    footer.pack_start(&type_dropdown);

    let sort_dropdown = dropdown(
        &i18n("Sort"),
        &[
            i18n("Newest first"),
            i18n("Oldest first"),
            i18n("Name (A–Z)"),
            i18n("Name (Z–A)"),
            i18n("Sender (A–Z)"),
            i18n("Sender (Z–A)"),
            i18n("Largest first"),
            i18n("Smallest first"),
            i18n("Type (A–Z)"),
            i18n("Type (Z–A)"),
        ],
    );
    sort_dropdown.set_selected(model.sort.index());
    let t = tx.clone();
    sort_dropdown.connect_selected_notify(move |d| {
        t.emit(GalleryInput::SetSort(d.selected()));
    });
    footer.pack_start(&sort_dropdown);

    let count_label = gtk::Label::new(None);
    count_label.add_css_class("dim-label");
    footer.pack_end(&count_label);

    // What the network is doing, in the one place the gallery keeps its
    // status: a page on its way, or the scan still working back through the
    // archive.
    let status_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    status_box.set_valign(gtk::Align::Center);
    let status_spinner = gtk::Spinner::new();
    status_spinner.set_width_request(16);
    status_spinner.set_height_request(16);
    let status_label = gtk::Label::new(None);
    status_label.add_css_class("dim-label");
    status_box.append(&status_spinner);
    status_box.append(&status_label);
    footer.pack_end(&status_box);

    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 140.0, 380.0, 10.0);
    scale.set_increments(10.0, 40.0);
    scale.set_value(model.thumb_width as f64);
    scale.set_width_request(140);
    scale.set_draw_value(false);
    scale.set_tooltip_text(Some(&i18n("Thumbnail size")));
    let t = tx.clone();
    scale.connect_value_changed(move |s| {
        t.emit(GalleryInput::SetThumbWidth(s.value()));
    });
    footer.pack_end(&scale);
    base.append(&footer);

    // Lightbox overlay, shown while previewing an item.
    let lightbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    lightbox.add_css_class("gallery-lightbox");
    root.add_overlay(&lightbox);

    // Top bar: title + close.
    let top = gtk::CenterBox::new();
    top.add_css_class("gallery-lightbox-bar");
    let lightbox_title = gtk::Label::new(None);
    lightbox_title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    lightbox_title.set_halign(gtk::Align::Start);
    lightbox_title.add_css_class("gallery-lightbox-title");
    top.set_start_widget(Some(&lightbox_title));
    let close = gtk::Button::from_icon_name("window-close-symbolic");
    close.set_tooltip_text(Some(&i18n("Close")));
    close.add_css_class("circular");
    close.add_css_class("flat");
    let t = tx.clone();
    close.connect_clicked(move |_| t.emit(GalleryInput::ClosePreview));
    top.set_end_widget(Some(&close));
    lightbox.append(&top);

    // Middle: prev  |  image/icon  |  next.
    let middle = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    middle.set_vexpand(true);
    let nav = |icon: &str, tooltip: &str, input: fn() -> GalleryInput| {
        let b = gtk::Button::from_icon_name(icon);
        b.set_tooltip_text(Some(tooltip));
        b.set_valign(gtk::Align::Center);
        b.add_css_class("circular");
        b.add_css_class("osd");
        let t = tx.clone();
        b.connect_clicked(move |_| t.emit(input()));
        b
    };
    let prev_button = nav("go-previous-symbolic", &i18n("Previous"), || {
        GalleryInput::Prev
    });
    let next_button = nav("go-next-symbolic", &i18n("Next"), || GalleryInput::Next);
    middle.append(&prev_button);

    let preview_stack = gtk::Stack::new();
    preview_stack.set_hexpand(true);
    preview_stack.set_vexpand(true);
    let preview_picture = gtk::Picture::new();
    preview_picture.set_can_shrink(true);
    preview_picture.set_content_fit(gtk::ContentFit::Contain);
    preview_stack.add_named(&preview_picture, Some("image"));
    let rendering = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    rendering.set_halign(gtk::Align::Center);
    rendering.set_valign(gtk::Align::Center);
    let render_spinner = gtk::Spinner::new();
    render_spinner.set_spinning(true);
    render_spinner.set_width_request(36);
    render_spinner.set_height_request(36);
    rendering.append(&render_spinner);
    preview_stack.add_named(&rendering, Some("rendering"));
    let file_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    file_page.add_css_class("gallery-lightbox-file");
    file_page.set_halign(gtk::Align::Center);
    file_page.set_valign(gtk::Align::Center);
    let file_icon = gtk::Image::new();
    file_icon.set_pixel_size(96);
    let file_name = gtk::Label::new(None);
    file_name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    file_name.add_css_class("title-3");
    let no_preview = gtk::Label::new(Some(&i18n("No preview available")));
    no_preview.add_css_class("dim-label");
    file_page.append(&file_icon);
    file_page.append(&file_name);
    file_page.append(&no_preview);
    preview_stack.add_named(&file_page, Some("file"));
    middle.append(&preview_stack);
    middle.append(&next_button);
    lightbox.append(&middle);

    // Bottom bar: caption + actions.
    let bottom = gtk::CenterBox::new();
    bottom.add_css_class("gallery-lightbox-bar");
    let caption_label = gtk::Label::new(None);
    caption_label.set_halign(gtk::Align::Start);
    caption_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    caption_label.add_css_class("dim-label");
    bottom.set_start_widget(Some(&caption_label));
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let open_button = text_button(&i18n("Open"));
    open_button.set_tooltip_text(Some(&i18n("Open in the default app")));
    let t = tx.clone();
    open_button.connect_clicked(move |_| t.emit(GalleryInput::OpenCurrent));
    let goto_button = text_button(&i18n("Go to Message"));
    goto_button.add_css_class("suggested-action");
    let t = tx.clone();
    goto_button.connect_clicked(move |_| t.emit(GalleryInput::GoToCurrent));
    actions.append(&open_button);
    actions.append(&goto_button);
    bottom.set_end_widget(Some(&actions));
    lightbox.append(&bottom);

    let widgets = AttachmentsGalleryWidgets {
        toolbar,
        search_entry,
        stack,
        name_header,
        sender_header,
        type_header,
        date_header,
        size_header,
        footer,
        grid_toggle,
        grid_handler,
        table_toggle,
        table_handler,
        account_dropdown,
        folders_button,
        folders_box,
        archive_row,
        archive_handler,
        other_row,
        other_handler,
        sort_dropdown,
        count_label,
        status_box,
        status_spinner,
        status_label,
        scale,
        lightbox,
        lightbox_title,
        prev_button,
        next_button,
        preview_stack,
        preview_picture,
        file_icon,
        file_name,
        no_preview,
        caption_label,
        open_button,
    };
    widgets.apply(model);
    widgets
}

impl Component for AttachmentsGallery {
    type Init = ();
    type Input = GalleryInput;
    type Output = GalleryOutput;
    type CommandOutput = ();
    type Root = gtk::Overlay;
    type Widgets = AttachmentsGalleryWidgets;

    fn init_root() -> Self::Root {
        gtk::Overlay::new()
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.apply(self);
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let (view_table, thumb_width, sort_index) = crate::config::load_gallery_view();
        let (include_archive, include_other, saved_folders) = crate::config::load_gallery_scope();
        let mut overrides: HashMap<u32, HashMap<String, bool>> = HashMap::new();
        for (id, path, on) in saved_folders {
            overrides.entry(id).or_default().insert(path, on);
        }
        let model = AttachmentsGallery {
            all_items: Vec::new(),
            total: 0,
            loading_more: false,
            scan_remaining: 0,
            fetching: false,
            query: String::new(),
            sort: GallerySort::from_index(sort_index),
            preview: None,
            preview_texture: None,
            preview_failed: false,
            loading: false,
            view_table,
            thumb_width,
            type_filter: 0,
            account_filter: None,
            accounts: Vec::new(),
            include_archive,
            include_other,
            overrides,
            included: HashMap::new(),
            resize_timer: None,
            query_timer: None,
            flow: gtk::FlowBox::new(),
            table: gtk::ListBox::new(),
            folder_list: gtk::ListBox::new(),
            account_names: gtk::StringList::new(&[i18n("All accounts").as_str()]),
            root: root.clone().upcast(),
            deleting: Vec::new(),
        };
        let widgets = build_widgets(&root, &sender, &model);

        // Double-clicking the preview opens the document in its external app.
        let dbl = gtk::GestureClick::new();
        dbl.set_button(gtk::gdk::BUTTON_PRIMARY);
        let ds = sender.clone();
        dbl.connect_pressed(move |_, n, _, _| {
            if n == 2 {
                ds.input(GalleryInput::OpenCurrent);
            }
        });
        widgets.preview_picture.add_controller(dbl);

        // Arrow keys navigate the lightbox; Escape closes it.
        let key = gtk::EventControllerKey::new();
        let ks = sender.clone();
        key.connect_key_pressed(move |_, keyval, _, _| {
            match keyval {
                gdk::Key::Left => ks.input(GalleryInput::Prev),
                gdk::Key::Right => ks.input(GalleryInput::Next),
                gdk::Key::Escape => ks.input(GalleryInput::ClosePreview),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        root.add_controller(key);

        ComponentParts { model, widgets }
    }

    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        msg: Self::Input,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            GalleryInput::Page {
                items,
                total,
                offset,
            } => self.on_page(items, total, offset, &sender),
            GalleryInput::LoadMore => self.load_more(&sender),
            GalleryInput::ScanProgress => {
                if self.all_items.len() <= PAGE_SIZE as usize {
                    self.reload(&sender);
                }
            }
            GalleryInput::ScanStatus(remaining) => self.scan_remaining = remaining,
            GalleryInput::SetFetching(on) => self.fetching = on,
            GalleryInput::Fetched {
                account_id,
                path,
                uid,
                items,
            } => self.on_fetched(account_id, path, uid, items, &sender),
            GalleryInput::SetDeleting(key, on) => self.on_set_deleting(key, on, &sender),
            GalleryInput::Removed(key) => self.on_removed(key, &sender),
            GalleryInput::RemoveNow(key) => {
                if !self.on_remove_now(key, &sender) {
                    return;
                }
            }
            GalleryInput::SetQuery(q) => self.on_set_query(q, &sender),
            GalleryInput::ApplyQuery => {
                self.query_timer = None;
                self.reload(&sender);
            }
            GalleryInput::SetSort(i) => self.on_set_sort(i, &sender),
            GalleryInput::SortColumn(col) => self.on_sort_column(col, widgets, &sender),
            GalleryInput::SetViewTable(table) => self.on_set_view_table(table, &sender),
            GalleryInput::SetThumbWidth(v) => self.on_set_thumb_width(v, &sender),
            GalleryInput::ApplyThumbWidth => self.on_apply_thumb_width(&sender),
            GalleryInput::SetTypeFilter(bucket) => {
                if self.type_filter != bucket {
                    self.type_filter = bucket;
                    self.reload(&sender);
                }
            }
            GalleryInput::SetAccounts(accounts) => self.on_set_accounts(accounts, widgets, &sender),
            GalleryInput::SetAccountFilter(row) => {
                self.on_set_account_filter(row, widgets, &sender)
            }
            GalleryInput::SetIncludeArchive(on) => {
                if self.include_archive != on {
                    self.include_archive = on;
                    self.scope_changed(&sender);
                }
            }
            GalleryInput::SetIncludeOther(on) => {
                if self.include_other != on {
                    self.include_other = on;
                    self.scope_changed(&sender);
                }
            }
            GalleryInput::SetFolder {
                account_id,
                path,
                on,
            } => self.on_set_folder(account_id, path, on, &sender),
            GalleryInput::SetLoading(on) => self.loading = on,
            GalleryInput::Activate(i) => {
                if (i as usize) < self.all_items.len() {
                    self.preview = Some(i as usize);
                    self.refresh_preview(&sender);
                }
            }
            GalleryInput::Prev => self.step(-1, &sender),
            GalleryInput::Next => self.step(1, &sender),
            GalleryInput::ClosePreview => {
                self.preview = None;
                self.preview_texture = None;
            }
            GalleryInput::PreviewRendered(key) => self.on_preview_rendered(key, &sender),
            GalleryInput::OpenCurrent => {
                if let Some(i) = self.preview {
                    self.open_item(i, &sender);
                }
            }
            GalleryInput::GoToCurrent => {
                if let Some(i) = self.preview {
                    self.goto_item(i, &sender);
                }
            }
            GalleryInput::OpenItem(i) => self.open_item(i, &sender),
            GalleryInput::DownloadItem(i) => self.download_item(i, &sender),
            GalleryInput::GoToItem(i) => self.goto_item(i, &sender),
            GalleryInput::ShowcaseDelete(name) => self.on_showcase_delete(name, &sender),
            GalleryInput::DeleteItem(i) => self.on_delete_item(i, &sender),
            GalleryInput::OpenExternal(i) => {
                // Double-click: skip/close the preview and open the file directly.
                self.preview = None;
                self.open_item(i, &sender);
            }
            GalleryInput::ContextMenu { index, x, y } => {
                self.show_context_menu(index, x, y, &sender)
            }
            GalleryInput::ShowcaseSearch(text) => self.on_showcase_search(text, widgets),
            GalleryInput::ShowcaseFolders => self.on_showcase_folders(widgets),
        }
        self.update_view(widgets, sender);
    }
}

/// Whether a scroller has come close enough to its end to be worth asking for
/// the next page. `upper - page_size` is the furthest the view can scroll, so
/// this is "within a couple of rows of the bottom".
fn near_end(a: &gtk::Adjustment) -> bool {
    a.upper() - a.page_size() - a.value() < LOAD_MORE_MARGIN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_index_roundtrips_every_criterion() {
        for i in 0..10 {
            assert_eq!(GallerySort::from_index(i).index(), i);
        }
    }
}
