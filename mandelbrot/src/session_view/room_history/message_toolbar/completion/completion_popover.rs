use gettextrs::gettext;
use gtk::{gdk, glib, glib::clone, prelude::*, subclass::prelude::*};
use pulldown_cmark::{Event, Parser, Tag};
use secular::normalized_lower_lay_string;

use super::{CompletionMemberList, CompletionRoomList};
use crate::{
    components::{AvatarImageSafetySetting, Pill, PillSource, PillSourceRow},
    session::Room,
    session_view::room_history::message_toolbar::MessageToolbar,
    utils::BoundObject,
};

/// The maximum number of rows presented in the popover.
const MAX_ROWS: usize = 32;
/// The sigil for a user ID.
const USER_ID_SIGIL: char = '@';
/// The sigil for a room alias.
const ROOM_ALIAS_SIGIL: char = '#';

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
    };

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_toolbar/completion/completion_popover.ui"
    )]
    #[properties(wrapper_type = super::CompletionPopover)]
    pub struct CompletionPopover {
        #[template_child]
        list: TemplateChild<gtk::ListBox>,
        /// The parent `GtkTextView` to autocomplete.
        #[property(get = Self::view)]
        view: PhantomData<gtk::TextView>,
        /// The current room.
        #[property(get, set = Self::set_room, explicit_notify, nullable)]
        room: glib::WeakRef<Room>,
        /// The sorted and filtered room members.
        #[property(get)]
        member_list: CompletionMemberList,
        /// The sorted and filtered rooms.
        #[property(get)]
        room_list: CompletionRoomList,
        /// The rows in the popover.
        rows: [PillSourceRow; MAX_ROWS],
        /// The selected row in the popover.
        selected: Cell<Option<usize>>,
        /// The current autocompleted word.
        current_word: RefCell<Option<(gtk::TextIter, gtk::TextIter, SearchTerm)>>,
        /// Whether the popover is inhibited for the current word.
        inhibit: Cell<bool>,
        /// The buffer to autocomplete.
        buffer: BoundObject<gtk::TextBuffer>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CompletionPopover {
        const NAME: &'static str = "ContentCompletionPopover";
        type Type = super::CompletionPopover;
        type ParentType = gtk::Popover;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for CompletionPopover {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            for row in &self.rows {
                self.list.append(row);
            }

            obj.connect_parent_notify(|obj| {
                let imp = obj.imp();
                imp.update_buffer();

                if obj.parent().is_some() {
                    let view = obj.view();

                    view.connect_buffer_notify(clone!(
                        #[weak]
                        imp,
                        move |_| {
                            imp.update_buffer();
                        }
                    ));

                    let key_events = gtk::EventControllerKey::new();
                    key_events.connect_key_pressed(clone!(
                        #[weak]
                        imp,
                        #[upgrade_or]
                        glib::Propagation::Proceed,
                        move |_, key, _, modifier| imp.handle_key_pressed(key, modifier)
                    ));
                    view.add_controller(key_events);

                    // Close popup when the entry is not focused.
                    view.connect_has_focus_notify(clone!(
                        #[weak]
                        obj,
                        move |view| {
                            if !view.has_focus() && obj.get_visible() {
                                obj.popdown();
                            }
                        }
                    ));
                }
            });

            self.list.connect_row_activated(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, row| {
                    if let Some(row) = row.downcast_ref::<PillSourceRow>() {
                        imp.row_activated(row);
                    }
                }
            ));
        }
    }

    impl WidgetImpl for CompletionPopover {}
    impl PopoverImpl for CompletionPopover {}

    impl CompletionPopover {
        /// Set the current room.
        fn set_room(&self, room: Option<&Room>) {
            self.member_list.set_room(room);

            self.room_list
                .set_rooms(room.and_then(Room::session).map(|s| s.room_list()));

            self.room.set(room);
        }

        /// The parent `GtkTextView` to autocomplete.
        fn view(&self) -> gtk::TextView {
            self.obj().parent().and_downcast::<gtk::TextView>().unwrap()
        }

        /// The ancestor `MessageToolbar`.
        fn message_toolbar(&self) -> MessageToolbar {
            self.obj()
                .ancestor(MessageToolbar::static_type())
                .and_downcast::<MessageToolbar>()
                .unwrap()
        }

        /// Handle a change of buffer.
        fn update_buffer(&self) {
            self.buffer.disconnect_signals();

            if self.obj().parent().is_some() {
                let buffer = self.view().buffer();
                let handler_id = buffer.connect_cursor_position_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_completion(false);
                    }
                ));
                self.buffer.set(buffer, vec![handler_id]);

                self.update_completion(false);
            }
        }

        /// The number of visible rows.
        fn visible_rows_count(&self) -> usize {
            self.rows
                .iter()
                .filter(|row| row.get_visible())
                .fuse()
                .count()
        }

        /// Handle when a key was pressed.
        fn handle_key_pressed(
            &self,
            key: gdk::Key,
            modifier: gdk::ModifierType,
        ) -> glib::Propagation {
            // Do not capture key press if there is a mask other than CapsLock.
            if modifier != gdk::ModifierType::NO_MODIFIER_MASK
                && modifier != gdk::ModifierType::LOCK_MASK
            {
                return glib::Propagation::Proceed;
            }

            // If the popover is not visible, we only handle tab to open the popover.
            if !self.obj().is_visible() {
                if matches!(key, gdk::Key::Tab | gdk::Key::KP_Tab) {
                    self.update_completion(true);
                    return glib::Propagation::Stop;
                }

                return glib::Propagation::Proceed;
            }

            // Activate the selected row on enter or tab.
            if matches!(
                key,
                gdk::Key::Return
                    | gdk::Key::KP_Enter
                    | gdk::Key::ISO_Enter
                    | gdk::Key::Tab
                    | gdk::Key::KP_Tab
            ) {
                self.activate_selected_row();
                return glib::Propagation::Stop;
            }

            // Move up in the list on key up, if possible.
            if matches!(key, gdk::Key::Up | gdk::Key::KP_Up) {
                let idx = self.selected_row_index().unwrap_or_default();
                if idx > 0 {
                    self.select_row_at_index(Some(idx - 1));
                }
                return glib::Propagation::Stop;
            }

            // Move down in the list on key down, if possible.
            if matches!(key, gdk::Key::Down | gdk::Key::KP_Down) {
                let new_idx = if let Some(idx) = self.selected_row_index() {
                    idx + 1
                } else {
                    0
                };

                let max = self.visible_rows_count();

                if new_idx < max {
                    self.select_row_at_index(Some(new_idx));
                }
                return glib::Propagation::Stop;
            }

            // Close the popover on escape.
            if matches!(key, gdk::Key::Escape) {
                self.inhibit();
                return glib::Propagation::Stop;
            }

            glib::Propagation::Proceed
        }

        /// The word that is currently used for filtering.
        ///
        /// Returns the start and end position of the word, as well as the
        /// search term.
        fn current_word(&self) -> Option<(gtk::TextIter, gtk::TextIter, SearchTerm)> {
            self.current_word.borrow().clone()
        }

        /// Set the word that is currently used for filtering.
        fn set_current_word(&self, word: Option<(gtk::TextIter, gtk::TextIter, SearchTerm)>) {
            if self.current_word() == word {
                return;
            }

            self.current_word.replace(word);
        }

        /// Update completion.
        ///
        /// If trigger is `true`, the search term will not look for `@` at the
        /// start of the word.
        fn update_completion(&self, trigger: bool) {
            let search = self.find_search_term(trigger);

            if self.is_inhibited() && search.is_none() {
                self.inhibit.set(false);
            } else if !self.is_inhibited() {
                if let Some((start, end, term)) = search {
                    self.set_current_word(Some((start, end, term)));
                    self.update_accessible_label();
                    self.update_search();
                } else {
                    self.obj().popdown();
                    self.select_row_at_index(None);
                    self.set_current_word(None);
                }
            }
        }

        /// Find the current search term in the underlying buffer.
        ///
        /// Returns the start and end of the search word and the term to search
        /// for.
        ///
        /// If trigger is `true`, the search term will not look for `@` at the
        /// start of the word.
        fn find_search_term(
            &self,
            trigger: bool,
        ) -> Option<(gtk::TextIter, gtk::TextIter, SearchTerm)> {
            // Vocabular used in this method:
            // - `word`: sequence of characters that form a valid ID or display name. This
            //   includes characters that are usually not considered to be in words because
            //   of the grammar of Matrix IDs.
            // - `trigger`: character used to trigger the popover, usually the first
            //   character of the corresponding ID.

            let (word_start, word_end) = self.cursor_word_boundaries(trigger)?;

            let mut term_start = word_start;
            let term_start_char = term_start.char();
            let is_room = term_start_char == ROOM_ALIAS_SIGIL;

            // Remove the starting sigil for searching.
            if matches!(term_start_char, USER_ID_SIGIL | ROOM_ALIAS_SIGIL) {
                term_start.forward_cursor_position();
            }

            let term = self.view().buffer().text(&term_start, &word_end, true);

            // If the cursor jumped to another word, abort the completion.
            if self.current_word().is_some_and(|(_, _, prev_term)| {
                !term.contains(&prev_term.term) && !prev_term.term.contains(term.as_str())
            }) {
                return None;
            }

            let target = if is_room {
                SearchTermTarget::Room
            } else {
                SearchTermTarget::Member
            };
            let term = SearchTerm {
                target,
                term: term.into(),
            };

            Some((word_start, word_end, term))
        }

        /// Find the word boundaries for the current cursor position.
        ///
        /// If trigger is `true`, the search term will not look for `@` at the
        /// start of the word.
        ///
        /// Returns a `(start, end)` tuple.
        fn cursor_word_boundaries(&self, trigger: bool) -> Option<(gtk::TextIter, gtk::TextIter)> {
            let buffer = self.view().buffer();
            let cursor = buffer.iter_at_mark(&buffer.get_insert());
            let mut word_start = cursor;

            // Search for the beginning of the word.
            while word_start.backward_cursor_position() {
                let c = word_start.char();
                if !is_possible_word_char(c) {
                    word_start.forward_cursor_position();
                    break;
                }
            }

            if !matches!(word_start.char(), USER_ID_SIGIL | ROOM_ALIAS_SIGIL)
                && !trigger
                && (cursor == word_start || self.current_word().is_none())
            {
                // No trigger or not updating the word.
                return None;
            }

            let mut ctx = SearchContext::default();
            let mut word_end = word_start;
            while word_end.forward_cursor_position() {
                let c = word_end.char();
                if ctx.has_id_separator {
                    // The server name of an ID.
                    if ctx.has_port_separator {
                        // The port number
                        if ctx.port.len() <= 5 && c.is_ascii_digit() {
                            ctx.port.push(c);
                        } else {
                            break;
                        }
                    } else {
                        // An IPv6 address, IPv4 address, or a domain name.
                        if matches!(ctx.server_name, ServerNameContext::Unknown) {
                            if c == '[' {
                                ctx.server_name = ServerNameContext::Ipv6(c.into());
                            } else if c.is_alphanumeric() {
                                ctx.server_name = ServerNameContext::Ipv4OrDomain(c.into());
                            } else {
                                break;
                            }
                        } else if let ServerNameContext::Ipv6(address) = &mut ctx.server_name {
                            if address.ends_with(']') {
                                if c == ':' {
                                    ctx.has_port_separator = true;
                                } else {
                                    break;
                                }
                            } else if address.len() > 46 {
                                break;
                            } else if c.is_ascii_hexdigit() || matches!(c, ':' | '.' | ']') {
                                address.push(c);
                            } else {
                                break;
                            }
                        } else if let ServerNameContext::Ipv4OrDomain(address) =
                            &mut ctx.server_name
                        {
                            if c == ':' {
                                ctx.has_port_separator = true;
                            } else if c.is_ascii_alphanumeric() || matches!(c, '-' | '.') {
                                address.push(c);
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                } else {
                    // Localpart or display name.
                    if !ctx.is_outside_ascii
                        && (c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '=' | '-' | '/'))
                    {
                        ctx.localpart.push(c);
                    } else if c.is_alphanumeric() {
                        ctx.is_outside_ascii = true;
                    } else if !ctx.is_outside_ascii && c == ':' {
                        ctx.has_id_separator = true;
                    } else {
                        break;
                    }
                }
            }

            // It the cursor is not at the word, there is no need for completion.
            if cursor != word_end && !cursor.in_range(&word_start, &word_end) {
                return None;
            }

            // If we are in markdown that would be escaped, there is no need for completion.
            if self.in_escaped_markdown(&word_start, &word_end) {
                return None;
            }

            Some((word_start, word_end))
        }

        /// Check if the text is in markdown that would be escaped.
        ///
        /// This includes:
        /// - Inline code
        /// - Block code
        /// - Links (because nested links are not allowed in HTML)
        /// - Images
        fn in_escaped_markdown(
            &self,
            word_start: &gtk::TextIter,
            word_end: &gtk::TextIter,
        ) -> bool {
            let buffer = self.view().buffer();
            let (buf_start, buf_end) = buffer.bounds();

            // If the word is at the start or the end of the buffer, it cannot be escaped.
            if *word_start == buf_start || *word_end == buf_end {
                return false;
            }

            let text = buffer.slice(&buf_start, &buf_end, true);

            // Find the word string slice indexes, because GtkTextIter only gives us
            // the char offset but the parser gives us indexes.
            let word_start_offset = usize::try_from(word_start.offset()).unwrap_or_default();
            let word_end_offset = usize::try_from(word_end.offset()).unwrap_or_default();
            let mut word_start_index = 0;
            let mut word_end_index = 0;
            if word_start_offset != 0 && word_end_offset != 0 {
                for (offset, (index, _char)) in text.char_indices().enumerate() {
                    if word_start_offset == offset {
                        word_start_index = index;
                    }
                    if word_end_offset == offset {
                        word_end_index = index;
                    }

                    if word_start_index != 0 && word_end_index != 0 {
                        break;
                    }
                }
            }

            // Look if word is in escaped markdown.
            let mut in_escaped_tag = false;
            for (event, range) in Parser::new(&text).into_offset_iter() {
                match event {
                    Event::Start(tag) => {
                        in_escaped_tag = matches!(
                            tag,
                            Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Image { .. }
                        );
                    }
                    Event::End(_) => {
                        // A link or a code block only contains text so an end tag
                        // always means the end of an escaped part.
                        in_escaped_tag = false;
                    }
                    Event::Code(_) if range.contains(&word_start_index) => {
                        return true;
                    }
                    Event::Text(_) if in_escaped_tag && range.contains(&word_start_index) => {
                        return true;
                    }
                    _ => {}
                }

                if range.end <= word_end_index {
                    break;
                }
            }

            false
        }

        /// Update the popover for the current search term.
        fn update_search(&self) {
            let term = self
                .current_word()
                .map(|(_, _, term)| term.into_normalized_parts());

            let list = match term {
                Some((SearchTermTarget::Room, term)) => {
                    self.room_list.set_search_term(term.as_deref());
                    self.room_list.list()
                }
                term => {
                    self.member_list
                        .set_search_term(term.and_then(|(_, t)| t).as_deref());
                    self.member_list.list()
                }
            };

            let obj = self.obj();
            let new_len = list.n_items();
            if new_len == 0 {
                obj.popdown();
                self.select_row_at_index(None);
            } else {
                for (idx, row) in self.rows.iter().enumerate() {
                    let item = list.item(idx as u32);
                    if let Some(source) = item.clone().and_downcast::<PillSource>() {
                        row.set_source(Some(source));
                        row.set_visible(true);
                    } else if row.get_visible() {
                        row.set_visible(false);
                    } else {
                        // All remaining rows should be hidden too.
                        break;
                    }
                }

                self.update_pointing_to();
                self.popup();
            }
        }

        /// Show the popover.
        fn popup(&self) {
            if self
                .selected_row_index()
                .is_none_or(|index| index >= self.visible_rows_count())
            {
                self.select_row_at_index(Some(0));
            }
            self.obj().popup();
        }

        /// Update the location where the popover is pointing to.
        fn update_pointing_to(&self) {
            let view = self.view();
            let (start, ..) = self.current_word().expect("the current word is known");
            let location = view.iter_location(&start);
            let (x, y) = view.buffer_to_window_coords(
                gtk::TextWindowType::Widget,
                location.x(),
                location.y(),
            );
            self.obj()
                .set_pointing_to(Some(&gdk::Rectangle::new(x - 6, y - 2, 0, 0)));
        }

        /// The index of the selected row.
        fn selected_row_index(&self) -> Option<usize> {
            self.selected.get()
        }

        /// Select the row at the given index.
        fn select_row_at_index(&self, idx: Option<usize>) {
            if self.selected_row_index() == idx || idx >= Some(self.visible_rows_count()) {
                return;
            }

            if let Some(row) = idx.map(|idx| &self.rows[idx]) {
                // Make sure the row is visible.
                let row_bounds = row.compute_bounds(&*self.list).unwrap();
                let lower = row_bounds.top_left().y().into();
                let upper = row_bounds.bottom_left().y().into();
                self.list.adjustment().unwrap().clamp_page(lower, upper);

                self.list.select_row(Some(row));
            } else {
                self.list.select_row(gtk::ListBoxRow::NONE);
            }
            self.selected.set(idx);
        }

        /// Activate the row that is currently selected.
        fn activate_selected_row(&self) {
            if let Some(idx) = self.selected_row_index() {
                self.rows[idx].activate();
            } else {
                self.inhibit();
            }
        }

        /// Handle a row being activated.
        fn row_activated(&self, row: &PillSourceRow) {
            let Some(source) = row.source() else {
                return;
            };

            let Some((mut start, mut end, _)) = self.current_word.take() else {
                return;
            };

            let view = self.view();
            let buffer = view.buffer();

            buffer.delete(&mut start, &mut end);

            // We do not need to watch safety settings for mentions, rooms will be watched
            // automatically.
            let pill = Pill::new(&source, AvatarImageSafetySetting::None, None);
            self.message_toolbar()
                .current_composer_state()
                .add_widget(pill, &mut start);

            self.obj().popdown();
            self.select_row_at_index(None);
            view.grab_focus();
        }

        /// Whether the completion is inhibited.
        fn is_inhibited(&self) -> bool {
            self.inhibit.get()
        }

        /// Inhibit the completion.
        fn inhibit(&self) {
            if !self.is_inhibited() {
                self.inhibit.set(true);
                self.obj().popdown();
                self.select_row_at_index(None);
            }
        }

        /// Update the accessible label of the popover.
        fn update_accessible_label(&self) {
            let Some((_, _, term)) = self.current_word() else {
                return;
            };

            let label = if matches!(term.target, SearchTermTarget::Room) {
                gettext("Public Room Mention Auto-completion")
            } else {
                gettext("Room Member Mention Auto-completion")
            };
            self.obj()
                .update_property(&[gtk::accessible::Property::Label(&label)]);
        }
    }
}

glib::wrapper! {
    /// A popover to autocomplete Matrix IDs for its parent `gtk::TextView`.
    pub struct CompletionPopover(ObjectSubclass<imp::CompletionPopover>)
        @extends gtk::Widget, gtk::Popover,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native, gtk::ShortcutManager;
}

impl CompletionPopover {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for CompletionPopover {
    fn default() -> Self {
        Self::new()
    }
}

/// A search term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchTerm {
    /// The target of the search.
    target: SearchTermTarget,
    /// The term to search for.
    term: String,
}

impl SearchTerm {
    /// Normalize and return the parts of this search term.
    fn into_normalized_parts(self) -> (SearchTermTarget, Option<String>) {
        let term = (!self.term.is_empty()).then(|| normalized_lower_lay_string(&self.term));
        (self.target, term)
    }
}

/// The possible targets of a search term.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchTermTarget {
    /// A room member.
    Member,
    /// A room.
    Room,
}

/// The context for a search.
#[derive(Default)]
struct SearchContext {
    localpart: String,
    is_outside_ascii: bool,
    has_id_separator: bool,
    server_name: ServerNameContext,
    has_port_separator: bool,
    port: String,
}

/// The context for a server name.
#[derive(Default)]
enum ServerNameContext {
    Ipv6(String),
    // According to the Matrix spec definition, the IPv4 grammar is a
    // subset of the domain name grammar.
    Ipv4OrDomain(String),
    #[default]
    Unknown,
}

/// Whether the given char can be counted as a word char.
fn is_possible_word_char(c: char) -> bool {
    c.is_alphanumeric()
        || matches!(
            c,
            '.' | '_' | '=' | '-' | '/' | ':' | '[' | ']' | USER_ID_SIGIL | ROOM_ALIAS_SIGIL
        )
}
