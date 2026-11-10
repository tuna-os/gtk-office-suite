//! In-app Contacts view, shown in the main content area (like the attachments
//! gallery): a searchable, sortable list of every GNOME Contacts entry beside
//! the selected contact's full card, with editing, creation and deletion done
//! right here (writes go through EDS, so GNOME Contacts and CardDAV stay in
//! sync). Anything Hylki doesn't do — linking, photos — is one click away via
//! a contact's "Open in GNOME Contacts".

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use crate::relm::prelude::*;

use crate::i18n::i18n;

mod actions;
mod detail;
mod forms;
mod list;
mod member_editor;
mod menus;
mod rows;
mod types;

use menus::{book_label_factory, new_menu, FilterMenu};
use rows::{row_factory, DragKeys};

pub(crate) use menus::choose_vcf_files;
pub(crate) use rows::dragged_contacts;
pub(crate) use types::*;

mod update_edit;
mod update_groups;
mod update_list;

#[cfg(test)]
mod tests;

/// What a message handler leaves to `update_with_view`: `Continue` runs
/// `update_view` after it, `Break` (a handler that returned early) skips it,
/// as the early returns of the original single `match` did.
type Flow = std::ops::ControlFlow<()>;

/// The page's widgets: the ones `init` and the update modules read back by
/// field, and the two that follow the model, which `apply` sets from it at
/// build and after every message. This is what `#[relm4::component]`'s
/// `view!` block and its `#[watch]` attributes used to generate.
pub(crate) struct ContactsPageWidgets {
    page_stack: gtk::Stack,
    status: adw::StatusPage,
    search_revealer: gtk::Revealer,
    filter_btn: gtk::MenuButton,
    count_label: gtk::Label,
    search: gtk::SearchEntry,
    book_filter: gtk::DropDown,
    list: gtk::ListView,
    detail_overlay: gtk::Overlay,
    edit_btn: gtk::Button,
    detail_box: gtk::Box,
}

impl ContactsPageWidgets {
    /// Everything that was `#[watch]`ed: set from the model.
    fn apply(&self, model: &ContactsPage) {
        self.status.set_title(&if model.loading { i18n("Loading Contacts…") } else { i18n("No Contacts") });
        self.status.set_description(Some(
            &if model.loading {
                String::new()
            } else {
                i18n("Add a contact with the + button, or import vCard files. Contacts are kept in Hylki's own address book, and the address books of GNOME Contacts are listed here too.")
            },
        ));
        self.search_revealer.set_reveal_child(model.search_open || model.search_pinned);
    }
}

/// A flat icon button in a header, with a tooltip.
fn header_button(icon: &str, tooltip: String) -> gtk::Button {
    gtk::Button::builder().icon_name(icon).tooltip_text(tooltip).css_classes(["flat"]).build()
}

fn build_widgets(root: &gtk::Stack, sender: &ComponentSender<ContactsPage>, model: &ContactsPage) -> ContactsPageWidgets {
    root.set_transition_type(gtk::StackTransitionType::Crossfade);

    // No contacts at all (or still loading the first read).
    let none_header = adw::HeaderBar::builder().css_classes(["flat"]).build();
    none_header.set_title_widget(Some(&gtk::Label::builder().label(i18n("Contacts")).css_classes(["pane-title"]).build()));
    let toggle = header_button("sidebar-show-symbolic", i18n("Toggle sidebar"));
    {
        let sender = sender.clone();
        toggle.connect_clicked(move |_| {
            let _ = sender.output(ContactsPageOutput::ToggleSidebar);
        });
    }
    none_header.pack_start(&toggle);
    let import = header_button("document-open-symbolic", i18n("Import vCard files into the Hylki address book"));
    {
        let sender = sender.clone();
        import.connect_clicked(move |_| sender.input(ContactsPageInput::ImportContacts));
    }
    none_header.pack_end(&import);
    let add = header_button("list-add-symbolic", i18n("New contact or group"));
    {
        let sender = sender.clone();
        add.connect_clicked(move |b| new_menu(b, &sender));
    }
    none_header.pack_end(&add);
    let status = adw::StatusPage::builder().icon_name("x-office-address-book-symbolic").build();
    let none_page = adw::ToolbarView::new();
    none_page.add_top_bar(&none_header);
    none_page.set_content(Some(&status));
    root.add_named(&none_page, Some("none"));

    // Each pane carries its own header (like the mail view), so the divider
    // between them runs the full height of the window; the paned handle lets
    // the list grow, with its launch width as the floor.
    let browser = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .wide_handle(false)
        .position(crate::config::load_contacts_pane_width())
        .resize_start_child(false)
        .shrink_start_child(false)
        .resize_end_child(true)
        .shrink_end_child(false)
        .build();
    // Remember the width the user drags to. Debounced: one write once the
    // drag settles (see the mail view's list pane).
    {
        let pending = std::rc::Rc::new(std::cell::RefCell::new(None::<gtk::glib::SourceId>));
        browser.connect_position_notify(move |p| {
            let pos = p.position();
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
            let armed = pending.clone();
            *pending.borrow_mut() = Some(gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || {
                *armed.borrow_mut() = None;
                crate::config::save_contacts_pane_width(pos);
            }));
        });
    }

    // ---- the list pane ----
    // Left pane: no window controls (the detail pane's header carries the
    // window's close button).
    let list_header = adw::HeaderBar::builder()
        .css_classes(["flat"])
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    list_header.set_title_widget(Some(&gtk::Label::new(Some(""))));
    // Leftmost, mirroring the pane it acts on: same spot as in the message
    // list's header.
    let list_toggle = header_button("sidebar-show-symbolic", i18n("Toggle sidebar"));
    {
        let sender = sender.clone();
        list_toggle.connect_clicked(move |_| {
            let _ = sender.output(ContactsPageOutput::ToggleSidebar);
        });
    }
    list_header.pack_start(&list_toggle);
    // Beside it, as in the message list's header.
    let focus_search = header_button("system-search-symbolic", i18n("Search contacts (Ctrl+F)"));
    {
        let sender = sender.clone();
        focus_search.connect_clicked(move |_| sender.input(ContactsPageInput::FocusSearch));
    }
    list_header.pack_start(&focus_search);
    // pack_end packs right-to-left: sort and filter at the far right
    // (matching the message list), then the count, then import, then "+",
    // first on the right.
    let filter_btn = gtk::MenuButton::builder()
        .icon_name("filter-symbolic")
        .tooltip_text(i18n("Sort and filter contacts"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    list_header.pack_end(&filter_btn);
    let count_label = gtk::Label::builder().valign(gtk::Align::Center).css_classes(["list-count"]).build();
    list_header.pack_end(&count_label);
    let list_import = header_button("document-open-symbolic", i18n("Import vCard files into the Hylki address book"));
    {
        let sender = sender.clone();
        list_import.connect_clicked(move |_| sender.input(ContactsPageInput::ImportContacts));
    }
    list_header.pack_end(&list_import);
    let list_add = header_button("list-add-symbolic", i18n("New contact or group"));
    {
        let sender = sender.clone();
        list_add.connect_clicked(move |b| new_menu(b, &sender));
    }
    list_header.pack_end(&list_add);

    // The search, shown by the header's search button: the message list's
    // search bar.
    let search = gtk::SearchEntry::builder()
        .hexpand(true)
        .width_chars(3)
        .placeholder_text(i18n("Search contacts"))
        .build();
    {
        let sender = sender.clone();
        search.connect_search_changed(move |e| sender.input(ContactsPageInput::Query(e.text().to_string())));
    }
    {
        let sender = sender.clone();
        search.connect_stop_search(move |_| sender.input(ContactsPageInput::CloseSearch));
    }
    // Leaving it empty closes it too.
    let focus = gtk::EventControllerFocus::new();
    {
        let sender = sender.clone();
        focus.connect_leave(move |ctl| {
            let Some(entry) = ctl.widget().and_downcast::<gtk::SearchEntry>() else { return };
            let sender = sender.clone();
            crate::ui::close_if_focus_left_row(&entry, move || {
                sender.input(ContactsPageInput::CloseSearch);
            });
        });
    }
    search.add_controller(focus);
    let search_row = gtk::Box::builder().spacing(6).css_classes(["list-search-row"]).build();
    search_row.append(&search);
    let search_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .child(&search_row)
        .build();

    // Which address book the list shows, while the sidebar does not list
    // them: every one, or one.
    let book_filter = gtk::DropDown::builder()
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(10)
        .margin_end(10)
        .visible(false)
        .tooltip_text(i18n("Address book"))
        .build();
    let list = gtk::ListView::builder().css_classes(["navigation-sidebar", "contacts-listbox"]).build();
    let list_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .build();
    let list_column = gtk::Box::builder().orientation(gtk::Orientation::Vertical).width_request(280).build();
    list_column.append(&search_revealer);
    list_column.append(&book_filter);
    list_column.append(&list_scroll);
    let list_pane = adw::ToolbarView::new();
    list_pane.add_top_bar(&list_header);
    list_pane.set_content(Some(&list_column));
    browser.set_start_child(Some(&list_pane));

    // ---- the detail pane ----
    let detail_header = adw::HeaderBar::builder().css_classes(["flat"]).show_start_title_buttons(false).build();
    // Top-left of the card pane: edit the shown contact.
    let edit_btn = header_button("document-edit-symbolic", i18n("Edit contact"));
    {
        let sender = sender.clone();
        edit_btn.connect_clicked(move |_| sender.input(ContactsPageInput::Edit));
    }
    detail_header.pack_start(&edit_btn);
    detail_header.set_title_widget(Some(&gtk::Label::builder().label(i18n("Contacts")).css_classes(["pane-title"]).build()));
    let detail_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(24)
        .margin_bottom(32)
        .margin_start(18)
        .margin_end(18)
        .build();
    let clamp = adw::Clamp::builder().maximum_size(560).tightening_threshold(420).child(&detail_box).build();
    let detail_scroll = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .css_classes(["contact-detail-pane"])
        .child(&clamp)
        .build();
    let detail_pane = adw::ToolbarView::builder().hexpand(true).build();
    detail_pane.add_top_bar(&detail_header);
    detail_pane.set_content(Some(&detail_scroll));
    // The app mounts its contacts compose slot over this in init, so "New
    // message" slides down right here.
    let detail_overlay = gtk::Overlay::new();
    detail_overlay.set_child(Some(&detail_pane));
    browser.set_end_child(Some(&detail_overlay));
    root.add_named(&browser, Some("browser"));

    let widgets = ContactsPageWidgets {
        page_stack: root.clone(),
        status,
        search_revealer,
        filter_btn,
        count_label,
        search,
        book_filter,
        list,
        detail_overlay,
        edit_btn,
        detail_box,
    };
    widgets.apply(model);
    widgets
}

impl Component for ContactsPage {
    /// The app's compose revealer, mounted over the detail pane.
    type Init = gtk::Revealer;
    type Input = ContactsPageInput;
    type Output = ContactsPageOutput;
    type CommandOutput = ();
    type Root = gtk::Stack;
    type Widgets = ContactsPageWidgets;

    fn init_root() -> Self::Root {
        gtk::Stack::new()
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.apply(self);
    }

    fn init(
        compose_slot: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mut model = ContactsPage {
            contacts: Vec::new(),
            query: String::new(),
            sort: ContactSort::FirstName,
            scope: ContactScope::All,
            tree: None,
            drag: Rc::new(RefCell::new(DragKeys::default())),
            filters: 0,
            filter_menu: None,
            book_picker: false,
            picker_labels: Vec::new(),
            picker_guard: Rc::new(std::cell::Cell::new(false)),
            search_open: false,
            search_pinned: false,
            search_closed_at: None,
            selected: None,
            multi: Vec::new(),
            row_map: Vec::new(),
            store: gtk::gio::ListStore::new::<gtk::glib::BoxedAnyObject>(),
            selection: gtk::MultiSelection::new(None::<gtk::gio::ListStore>),
            rows: Rc::new(RefCell::new(HashMap::new())),
            items: Vec::new(),
            loading: true,
            editor: None,
            editing_target: None,
            deleted: std::collections::HashSet::new(),
            books: Vec::new(),
            filter_books: Vec::new(),
            directory_hits: Vec::new(),
            directory_generation: 0,
            directories: Vec::new(),
            directory_failures: Vec::new(),
            suggestions: Rc::new(RefCell::new(Vec::new())),
            pending: None,
            decoding: std::collections::HashSet::new(),
        };
        let widgets = build_widgets(&root, &sender, &model);
        widgets.detail_overlay.add_overlay(&compose_slot);

        // Delete on the list asks to remove the contact being looked at. The
        // controller sits on the list, not the page, so Delete inside the
        // search box or the editor still edits text.
        {
            let keys = gtk::EventControllerKey::new();
            let s = sender.input_sender().clone();
            keys.connect_key_pressed(move |_, keyval, _, _| {
                if !matches!(keyval, gtk::gdk::Key::Delete | gtk::gdk::Key::KP_Delete) {
                    return gtk::glib::Propagation::Proceed;
                }
                let _ = s.send(ContactsPageInput::DeleteSelected);
                gtk::glib::Propagation::Stop
            });
            widgets.list.add_controller(keys);
        }

        model.filter_menu = Some(FilterMenu::new(&widgets.filter_btn, sender.input_sender()));

        // The dropdown's own label gives way in a narrow pane; its list
        // shows each name whole.
        widgets
            .book_filter
            .set_factory(Some(&book_label_factory(true)));
        widgets
            .book_filter
            .set_list_factory(Some(&book_label_factory(false)));
        {
            let s = sender.input_sender().clone();
            let guard = model.picker_guard.clone();
            widgets.book_filter.connect_selected_notify(move |d| {
                if !guard.get() {
                    let _ = s.send(ContactsPageInput::PickBook(d.selected()));
                }
            });
        }

        model.selection.set_model(Some(&model.store));
        {
            let s = sender.input_sender().clone();
            model.selection.connect_selection_changed(move |_, _, _| {
                let _ = s.send(ContactsPageInput::SelectionChanged);
            });
        }
        widgets.list.set_model(Some(&model.selection));
        widgets.list.set_factory(Some(&row_factory(
            &model.rows,
            sender.input_sender(),
            &model.drag,
        )));

        ComponentParts { model, widgets }
    }

    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::Input,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        let flow = match message {
            ContactsPageInput::SetLoading => self.on_set_loading(widgets, sender.clone()),
            // A read that comes in while a form is open waits in `pending`
            // until the form closes.
            ContactsPageInput::SetContacts(contacts) if self.editor.is_some() => {
                self.pending = Some(contacts);
                Flow::Continue(())
            }
            ContactsPageInput::SetContacts(contacts) => {
                self.on_set_contacts(widgets, sender.clone(), contacts)
            }
            ContactsPageInput::Query(q) if q.to_lowercase() == self.query => Flow::Continue(()),
            ContactsPageInput::Query(q) => self.on_query(widgets, sender.clone(), q),
            ContactsPageInput::SetSearch(text) => self.on_set_search(widgets, text),
            ContactsPageInput::FocusSearch => self.on_focus_search(widgets, sender.clone()),
            ContactsPageInput::CloseSearch => self.on_close_search(widgets, sender.clone()),
            ContactsPageInput::SetBookPicker(on) => self.on_set_book_picker(widgets, on),
            ContactsPageInput::SetSearchPinned(on) => self.on_set_search_pinned(on),
            ContactsPageInput::SetFilter(filter, on) => {
                self.on_set_filter(widgets, sender.clone(), filter, on)
            }
            ContactsPageInput::ClearFilters => self.on_clear_filters(widgets, sender.clone()),
            ContactsPageInput::SuggestionsLoaded(suggestions) => {
                self.on_suggestions_loaded(suggestions)
            }
            ContactsPageInput::PickBook(n) => self.on_pick_book(sender.clone(), n),
            ContactsPageInput::DirectoriesKnown(directories) => {
                self.on_directories_known(widgets, sender.clone(), directories)
            }
            ContactsPageInput::DirectoryLookup { generation, query } => {
                self.on_directory_lookup(sender.clone(), generation, query)
            }
            ContactsPageInput::DirectoryResults {
                generation,
                hits,
                failed,
            } => self.on_directory_results(widgets, sender.clone(), generation, hits, failed),
            ContactsPageInput::AddDirectoryHit(idx) => {
                self.on_add_directory_hit(sender.clone(), idx)
            }
            ContactsPageInput::SetSort(sort) => self.on_set_sort(widgets, sender.clone(), sort),
            ContactsPageInput::SetScope(scope) => self.on_set_scope(widgets, sender.clone(), scope),
            ContactsPageInput::DropContacts { keys, on, moving } => {
                self.on_drop_contacts(widgets, sender.clone(), keys, on, moving)
            }
            ContactsPageInput::Tree(action) => self.on_tree(widgets, sender.clone(), action),
            ContactsPageInput::RemoveFromGroup(people) => {
                self.on_remove_from_group(sender.clone(), people)
            }
            ContactsPageInput::SelectionChanged => {
                self.on_selection_changed(widgets, sender.clone())
            }
            ContactsPageInput::SelectFirst(n) => self.on_select_first(n),
            ContactsPageInput::BulkAddToGroup(g) => self.on_bulk_add_to_group(sender.clone(), g),
            ContactsPageInput::BulkMoveRequest(to) => {
                self.on_bulk_move_request(widgets, sender.clone(), to)
            }
            ContactsPageInput::MoveRequest { idx, to } => {
                self.on_move_request(widgets, sender.clone(), idx, to)
            }
            ContactsPageInput::EditMoveConfirmed { to, key, vcard } => {
                self.on_edit_move_confirmed(widgets, sender.clone(), to, key, vcard)
            }
            ContactsPageInput::BulkMoveConfirmed { to, keys } => {
                self.on_bulk_move_confirmed(widgets, sender.clone(), to, keys)
            }
            ContactsPageInput::WriteFailed => self.on_write_failed(),
            ContactsPageInput::BulkDeleteRequest => {
                self.on_bulk_delete_request(widgets, sender.clone())
            }
            ContactsPageInput::Compose(email) => self.on_compose(sender.clone(), email),
            ContactsPageInput::OpenUrl(url) => self.on_open_url(url),
            ContactsPageInput::Edit => self.on_edit(widgets, sender.clone()),
            ContactsPageInput::EditIndex(idx) => self.on_edit_index(widgets, sender.clone(), idx),
            ContactsPageInput::NewContact => self.on_new_contact(widgets, sender.clone()),
            ContactsPageInput::NewGroup => self.on_new_group(widgets, sender.clone()),
            ContactsPageInput::ComposeGroup(idx) => self.on_compose_group(sender.clone(), idx),
            ContactsPageInput::AddToGroup { group, person } => {
                self.on_add_to_group(sender.clone(), group, person)
            }
            ContactsPageInput::ImportContacts => self.on_import_contacts(sender.clone()),
            ContactsPageInput::CancelEdit => self.on_cancel_edit(widgets, sender.clone()),
            ContactsPageInput::SaveEdit => self.on_save_edit(widgets, sender.clone()),
            ContactsPageInput::OpenInGnome(idx) => self.on_open_in_gnome(idx),
            ContactsPageInput::DeleteSelected => self.on_delete_selected(sender.clone()),
            ContactsPageInput::DeleteRequest(idx) => {
                self.on_delete_request(widgets, sender.clone(), idx)
            }
            ContactsPageInput::SetBooks(books) => self.on_set_books(sender.clone(), books),
            ContactsPageInput::PhotosDecoded => self.on_photos_decoded(),
            ContactsPageInput::RowMenu { idx, x, y } => {
                self.on_row_menu(widgets, sender.clone(), idx, x, y)
            }
            ContactsPageInput::OpenPhoto { name, data } => {
                self.on_open_photo(sender.clone(), name, data)
            }
            ContactsPageInput::DeleteConfirmed(keys) => {
                self.on_delete_confirmed(widgets, sender.clone(), keys)
            }
        };
        if flow.is_continue() {
            // Overriding `update_with_view` replaces the generated body that ran
            // `update_view` after every message; without this the `#[watch]`
            // bindings above (the "Loading Contacts…" / "No Contacts" face) only
            // ever hold their `init` values, and an empty address book looked
            // like a load that never finished.
            self.update_view(widgets, sender);
        }
    }
}
