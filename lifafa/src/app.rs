//! Root component: the application window, three-pane adaptive layout, and the
//! routing between the sidebar, list, reader, and the per-account mail workers.

use adw::prelude::*;
use relm4::prelude::*;

use crate::config;
use crate::i18n::i18n;
use crate::ui::notifications::NotifyInput;
use crate::ui::FOCUS_ANIM_MS;

// The message list's opening width now comes from config (the remembered pane
// width, #28); its floor lives with the pane in message_list/layout.rs
// (LIST_MIN_WIDTH).

relm4::new_action_group!(WindowActionGroup, "win");
relm4::new_stateless_action!(AccountsAction, WindowActionGroup, "accounts");
relm4::new_stateless_action!(PreferencesAction, WindowActionGroup, "preferences");
relm4::new_stateless_action!(AboutAction, WindowActionGroup, "about");
relm4::new_stateless_action!(ShortcutsAction, WindowActionGroup, "shortcuts");
relm4::new_stateless_action!(PrintAction, WindowActionGroup, "print");
relm4::new_stateless_action!(PrintPreviewAction, WindowActionGroup, "print-preview");
/// Store a setting the user changed and save the settings, if it really
/// changed; says whether it did, so the caller can apply it.
macro_rules! pref {
    ($self:ident . $($field:ident).+ = $value:expr) => {{
        let value = $value;
        let changed = $self.$($field).+ != value;
        if changed {
            $self.$($field).+ = value;
            $self.save_settings();
        }
        changed
    }};
}

mod about;
mod accounts;
mod actions;
mod assistant;
mod assistant_organize;
mod attachments;
mod boot;
mod card_menu;
mod children;
mod compose_host;
mod compose_init;
mod compose_open;
mod consts;
mod contacts;
mod conversation;
mod counts;
mod demo;
mod diagnostics;
mod draft_copies;
mod drafts;
mod events_bodies;
mod events_folders;
mod events_messages;
mod filter_actions;
mod filter_forward;
mod filter_run;
mod filters;
mod flags;
mod focus;
mod folder_menu;
mod folders;
mod gallery;
mod hand_off;
mod hierarchy;
mod identity;
mod invites;
mod keymap;
mod keyring_alert;
mod list_header;
mod list_sort;
mod merge_send;
mod model;
mod model_new;
mod move_tag;
mod msg;
mod navigation;
mod notifications;
mod outbox;
mod outgoing;
mod palette;
mod pending;
mod picker;
mod popout;
mod prefill;
mod prefs_compose;
mod prefs_data;
mod prefs_general;
mod prefs_list;
mod prefs_reader;
mod prefs_sidebar;
mod reader_select;
mod reader_toolbar;
mod reading_pane;
mod receipts;
mod receipts_returned;
mod release_notes;
mod removal;
mod router;
mod search;
mod selection;
mod send;
mod send_morph;
mod settings_store;
mod settings_window;
mod shortcuts;
mod showcase;
mod sidebar;
mod sidebar_contents;
mod sidebar_peek;
mod smart;
mod snooze;
mod split_reply;
mod tags;
#[cfg(test)]
mod test_support;
mod text_search;
mod theme;
mod to_calendar;
mod transfer;
mod tray;
mod types;
mod undo;
mod unified;
mod unified_starred;
mod unsubscribe;
mod uri_queue;
mod view_flags;
mod window;
mod workers;

pub(crate) use accounts::finish_keyring_writes;
pub(crate) use model::AppModel;
pub(crate) use msg::AppMsg;
pub(crate) use uri_queue::{
    is_message_file, queue_attach_files, queue_mailto, queue_message_file, queue_mid,
};

use attachments::schedule_download_prune;
use boot::{build_main_menu, Boot};
use children::Children;
use consts::READER_MIN_WIDTH;
use contacts::start_contacts_sync;
use diagnostics::start_heap_trim;
use palette::palette_key;
use theme::{apply_app_theme, follow_motion_setting};
use window::{exit_now, install_window_actions, save_window_geometry};
use workers::watch_system_events;

relm4::new_stateless_action!(StatusBarAction, WindowActionGroup, "status-bar");
relm4::new_stateless_action!(ConsoleAction, WindowActionGroup, "console");
relm4::new_stateless_action!(FindAction, WindowActionGroup, "find");
relm4::new_stateless_action!(UndoAction, WindowActionGroup, "undo");
relm4::new_stateless_action!(RedoAction, WindowActionGroup, "redo");
relm4::new_stateful_action!(FullscreenAction, WindowActionGroup, "fullscreen", (), bool);

/// The main window's widgets: the ones the rest of the app reaches by field,
/// and the ones that follow the model, which `apply` sets from it at build and
/// after every message. This is what `#[relm4::component]`'s `view!` block and
/// its `#[watch]` attributes used to generate.
#[allow(dead_code)]
pub(crate) struct AppModelWidgets {
    toasts: adw::ToastOverlay,
    window_overlay: gtk::Overlay,
    peek_split: adw::OverlaySplitView,
    sidebar_split: adw::OverlaySplitView,
    sidebar_header: adw::HeaderBar,
    app_title: gtk::Label,
    sidebar_menu: gtk::MenuButton,
    content_stack: gtk::Stack,
    mail_split: gtk::Paned,
    list_header: adw::HeaderBar,
    sidebar_toggle: gtk::Button,
    lh_search: gtk::Revealer,
    list_tools: gtk::Box,
    lh_overflow: gtk::Revealer,
    lh_filter: gtk::Revealer,
    list_filter_btn: gtk::MenuButton,
    lh_count: gtk::Revealer,
    count_label: gtk::Label,
    reader_bin: adw::BreakpointBin,
    reader_header: adw::HeaderBar,
    tb_outbox_edit: gtk::Revealer,
    outbox_edit_btn: gtk::Button,
    tb_outbox_send: gtk::Revealer,
    outbox_send_btn: gtk::Button,
    tb_outbox_send_all: gtk::Revealer,
    outbox_send_all_btn: gtk::Button,
    tb_reply: gtk::Revealer,
    reply_btn: gtk::Button,
    tb_reply_all: gtk::Revealer,
    reply_all_btn: gtk::Button,
    tb_forward: gtk::Revealer,
    forward_btn: gtk::Button,
    tb_star: gtk::Revealer,
    star_btn: gtk::Button,
    tb_archive: gtk::Revealer,
    archive_btn: gtk::Button,
    tb_delete: gtk::Revealer,
    delete_btn: gtk::Button,
    tb_print: gtk::Revealer,
    print_btn: gtk::Button,
    tb_find: gtk::Revealer,
    find_btn: gtk::Button,
    tb_move: gtk::Revealer,
    tb_snooze: gtk::Revealer,
    tb_spam: gtk::Revealer,
    spam_btn: gtk::Button,
    tb_read: gtk::Revealer,
    read_btn: gtk::Button,
    tb_tags: gtk::Revealer,
    tb_spinner: gtk::Spinner,
    reader_content_box: gtk::Box,
}

impl AppModelWidgets {
    /// Everything that was `#[watch]`ed: set from the model.
    fn apply(&self, model: &AppModel) {
        let stack_page = if model.showing_gallery {
            "gallery"
        } else if model.showing_contacts {
            "contacts"
        } else {
            "mail"
        };
        self.content_stack.set_visible_child_name(stack_page);

        self.list_header
            .set_show_end_title_buttons(model.controls_in_list());
        self.sidebar_toggle
            .set_tooltip_text(Some(&if model.rail_active {
                i18n("Expand sidebar")
            } else {
                i18n("Collapse sidebar")
            }));
        self.list_tools
            .set_visible(model.toolbar_in_list() && !model.many_selected());
        self.count_label.set_label(&model.list_count);

        self.reader_bin.set_visible(model.reader_shown());
        self.reader_header
            .set_show_end_title_buttons(!model.controls_in_list());

        let outbox = model.showing_outbox && model.reader_compose.is_none();
        let has_current = model.current.is_some();
        self.outbox_edit_btn.set_visible(outbox);
        self.outbox_edit_btn.set_sensitive(has_current);
        self.outbox_send_btn.set_visible(outbox);
        self.outbox_send_btn.set_sensitive(has_current);
        self.outbox_send_all_btn.set_visible(outbox);

        // In a conversation these act on the one highlighted card; with none
        // (or several) highlighted they grey out — no way to say which message
        // they'd mean.
        let reply = model.reply_target().is_some();
        self.reply_btn
            .set_visible(model.toolbar_visible(config::ToolbarItem::Reply));
        self.reply_btn.set_sensitive(reply);
        self.reply_all_btn
            .set_visible(model.toolbar_visible(config::ToolbarItem::ReplyAll));
        self.reply_all_btn.set_sensitive(reply);
        self.forward_btn
            .set_visible(model.toolbar_visible(config::ToolbarItem::Forward));
        self.forward_btn.set_sensitive(reply);

        let acts = model.acts_on_selection();
        let lit = model.toolbar_star_lit();
        self.star_btn.set_tooltip_text(Some(&if lit {
            i18n("Remove Star")
        } else {
            i18n("Star")
        }));
        self.star_btn.set_css_classes(if lit {
            &["flat", "star-active"]
        } else {
            &["flat"]
        });
        self.star_btn
            .set_visible(model.toolbar_visible(config::ToolbarItem::Star));
        self.star_btn.set_sensitive(acts);
        self.archive_btn
            .set_visible(model.toolbar_visible(config::ToolbarItem::Archive));
        self.archive_btn.set_sensitive(acts);
        self.delete_btn
            .set_tooltip_text(Some(&model.delete_tooltip()));
        // Shown for the Outbox too (a queued message can still be binned) —
        // see toolbar_visible.
        self.delete_btn
            .set_visible(model.toolbar_visible(config::ToolbarItem::Delete));
        self.delete_btn
            .set_sensitive(reply || model.many_selected());

        self.print_btn
            .set_visible(model.toolbar_visible(config::ToolbarItem::Print));
        self.print_btn.set_sensitive(has_current);
        // Greyed out, not hidden, with no message open: the toolbar must not
        // shift.
        self.find_btn
            .set_visible(model.toolbar_visible(config::ToolbarItem::Find));
        self.find_btn.set_sensitive(has_current);
        model
            .reader_move_btn
            .set_visible(model.toolbar_visible(config::ToolbarItem::MoveTo));
        model
            .reader_move_btn
            .set_sensitive(reply || model.many_selected());
        model
            .reader_snooze_btn
            .set_visible(model.toolbar_visible(config::ToolbarItem::Snooze));
        model
            .reader_snooze_btn
            .set_sensitive(model.snooze_offered());

        let in_junk = model.target_in_junk();
        self.spam_btn.set_icon_name(if in_junk {
            "mail-mark-notjunk-symbolic"
        } else {
            "mail-mark-junk-symbolic"
        });
        self.spam_btn.set_tooltip_text(Some(&if in_junk {
            i18n("Not Spam")
        } else {
            i18n("Mark as Spam")
        }));
        self.spam_btn
            .set_visible(model.toolbar_visible(config::ToolbarItem::Spam));
        self.spam_btn.set_sensitive(acts);

        // The icon shows the ACTION (read envelope = "mark as read"), matching
        // the menus.
        let unread = model.reply_target().is_some_and(|m| m.unread);
        self.read_btn
            .set_visible(model.toolbar_visible(config::ToolbarItem::ReadUnread));
        self.read_btn.set_icon_name(if unread {
            "hylki-mail-read-symbolic"
        } else {
            "mail-unread-symbolic"
        });
        self.read_btn.set_tooltip_text(Some(&if unread {
            i18n("Mark as Read")
        } else {
            i18n("Mark as Unread")
        }));
        self.read_btn.set_sensitive(acts);

        model.reader_tag_btn.set_visible(
            model.toolbar_visible(config::ToolbarItem::Tags) && !model.tags.is_empty(),
        );
        model.reader_tag_btn.set_sensitive(acts);

        self.tb_spinner.set_spinning(model.attachments_loading);
        self.tb_spinner
            .set_visible(model.attachments_loading && model.reader_compose.is_none());
    }
}

/// A flat button for a header bar, sending `input()` when clicked.
fn flat_button(
    sender: &ComponentSender<AppModel>,
    icon: Option<&str>,
    label: Option<&str>,
    tooltip: Option<&str>,
    input: fn() -> AppMsg,
) -> gtk::Button {
    let b = gtk::Button::new();
    if let Some(icon) = icon {
        b.set_icon_name(icon);
    }
    if let Some(label) = label {
        b.set_label(label);
    }
    b.set_tooltip_text(tooltip);
    b.add_css_class("flat");
    let tx = sender.input_sender().clone();
    b.connect_clicked(move |_| tx.emit(input()));
    b
}

/// The Revealer a reader/list header action sits in, so Focus Mode can fold it
/// away. `left` slides in from the left (packed at the start).
fn focus_revealer(child: &impl IsA<gtk::Widget>, left: bool) -> gtk::Revealer {
    let r = gtk::Revealer::new();
    r.set_transition_type(if left {
        gtk::RevealerTransitionType::SlideRight
    } else {
        gtk::RevealerTransitionType::SlideLeft
    });
    r.set_transition_duration(FOCUS_ANIM_MS);
    r.add_css_class("focus-fade");
    r.set_child(Some(child));
    r
}

/// A header action wrapped in a Box, as the Move/Snooze/Tags buttons are.
fn boxed(child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.append(child);
    b
}

/// An app-titled, flat header with the main menu at its end: the sidebar's
/// and the peek panel's headers.
fn title_label() -> gtk::Label {
    let l = gtk::Label::new(Some(crate::APP_NAME));
    l.add_css_class("app-title");
    l
}

fn main_menu_button(model: &AppModel) -> gtk::MenuButton {
    let b = gtk::MenuButton::new();
    // Hylki's own copy of the three bars: an icon of the GNOME name from a
    // theme or another program drew a gear on some systems.
    b.set_icon_name("hylki-open-menu-symbolic");
    b.set_tooltip_text(Some(&i18n("Main Menu")));
    b.add_css_class("flat");
    b.set_menu_model(Some(&model.menu));
    b
}

/// Builds the widget tree under `root` (the application window).
fn build_widgets(
    root: &adw::ApplicationWindow,
    sender: &ComponentSender<AppModel>,
    model: &AppModel,
) -> AppModelWidgets {
    // Persist the window size + maximized state on close. (Position and which
    // monitor can't be restored on Wayland — the compositor owns placement —
    // so only the geometry is saved.)
    {
        let background = model.run_in_background.clone();
        let outgoing = model.outgoing.clone();
        let merges = model.merges.clone();
        root.connect_close_request(move |w| {
            save_window_geometry(w);
            // Running in the background: hide the window and stay alive, so
            // mail keeps arriving and GNOME lists Hylki under Background Apps
            // (issue #3). The window is kept rather than rebuilt, so reopening
            // is instant and nothing is torn down — which is also what makes
            // this safe, given the exit below.
            if background.get() {
                w.set_visible(false);
                return gtk::glib::Propagation::Stop;
            }
            exit_now(&outgoing, &merges)
        });
    }

    // Send's toasts (#434) float over everything, the status bar included, and
    // never move the layout.
    let toasts = adw::ToastOverlay::new();
    root.set_content(Some(&toasts));
    let window_overlay = gtk::Overlay::new();
    toasts.set_child(Some(&window_overlay));
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    window_overlay.set_child(Some(&outer));

    // The peek panel's split view: permanently collapsed, so showing its
    // sidebar slides the expanded panel in from the window's left edge over the
    // rail and the panes — with libadwaita's scrim, shadow and swipe — while
    // the docked rail underneath stays exactly as it is (see
    // set_sidebar_peek). The account split view is its content.
    let peek_split = adw::OverlaySplitView::new();
    peek_split.set_vexpand(true);
    peek_split.set_collapsed(true);
    peek_split.set_show_sidebar(false);
    peek_split.set_min_sidebar_width(280.0);
    peek_split.set_max_sidebar_width(280.0);
    // No edge-swipe to open: it would fight the message list's own swipe
    // actions.
    peek_split.set_enable_show_gesture(false);
    outer.append(&peek_split);

    let peek_view = adw::ToolbarView::new();
    // Laid out like the expanded sidebar's header: Refresh top-left, the title
    // centred, the menu top-right.
    let peek_header = adw::HeaderBar::new();
    peek_header.add_css_class("flat");
    peek_header.set_show_start_title_buttons(false);
    peek_header.set_show_end_title_buttons(false);
    peek_header.set_title_widget(Some(&title_label()));
    let peek_refresh = gtk::Button::new();
    peek_refresh.set_tooltip_text(Some(&i18n("Refresh or long-press for Status Bar")));
    peek_refresh.add_css_class("flat");
    peek_refresh.set_valign(gtk::Align::Center);
    peek_refresh.set_child(Some(&model.peek_refresh_stack));
    let tx = sender.input_sender().clone();
    peek_refresh.connect_clicked(move |_| tx.emit(AppMsg::Refresh));
    let long_press = gtk::GestureLongPress::new();
    let tx = sender.input_sender().clone();
    long_press.connect_pressed(move |gesture, _, _| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        tx.emit(AppMsg::ToggleNotifications);
    });
    peek_refresh.add_controller(long_press);
    peek_header.pack_start(&peek_refresh);
    peek_header.pack_end(&main_menu_button(model));
    peek_view.add_top_bar(&peek_header);
    peek_view.set_content(Some(model.peek_sidebar.widget()));
    peek_split.set_sidebar(Some(&peek_view));

    let sidebar_split = adw::OverlaySplitView::new();
    sidebar_split.set_vexpand(true);
    sidebar_split.set_max_sidebar_width(280.0);
    // Low enough (with the panes' own minimums) that the whole window fits half
    // of a 1920px screen — see READER_MIN_WIDTH.
    sidebar_split.set_min_sidebar_width(180.0);
    sidebar_split.set_sidebar_width_fraction(0.2);
    peek_split.set_content(Some(&sidebar_split));

    let sidebar_view = adw::ToolbarView::new();
    let sidebar_header = adw::HeaderBar::new();
    sidebar_header.add_css_class("flat");
    let app_title = title_label();
    sidebar_header.set_title_widget(Some(&app_title));
    sidebar_header.pack_start(&model.sidebar_refresh);
    let sidebar_menu = main_menu_button(model);
    sidebar_header.pack_end(&sidebar_menu);
    sidebar_view.add_top_bar(&sidebar_header);
    sidebar_view.set_content(Some(model.sidebar.widget()));
    sidebar_split.set_sidebar(Some(&sidebar_view));

    let panes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    sidebar_split.set_content(Some(&panes));

    let content_stack = gtk::Stack::new();
    content_stack.set_hexpand(true);
    panes.append(&content_stack);

    let mail_split = gtk::Paned::new(gtk::Orientation::Horizontal);
    // Thin handle so the panes sit flush (just a 1px divider), no wide-handle
    // gap between them. A theme paints that divider through this class (see
    // theme::css); without one it keeps the stock separator.
    mail_split.add_css_class("mail-split");
    mail_split.set_wide_handle(false);
    // Launch at the remembered width; `shrink_start_child: false` keeps it
    // above the list's floor.
    mail_split.set_position(crate::config::load_list_pane_width());
    // Remember the width the user drags to (#28). Debounced: position-notify
    // fires per pixel of a drag (and when the window squeezes the pane), one
    // write once it settles.
    {
        let tx = sender.input_sender().clone();
        let pending = std::rc::Rc::new(std::cell::RefCell::new(None::<gtk::glib::SourceId>));
        mail_split.connect_position_notify(move |p| {
            let pos = p.position();
            // With the reader below, the list's height (#475).
            let below = p.orientation() == gtk::Orientation::Vertical;
            // Automatic layout follows the width as it is dragged, not once the
            // drag has settled (#334).
            if !below {
                tx.emit(AppMsg::ListPaneWidth(pos));
            }
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
            let armed = pending.clone();
            *pending.borrow_mut() = Some(gtk::glib::timeout_add_local_once(
                std::time::Duration::from_millis(600),
                move || {
                    *armed.borrow_mut() = None;
                    if below {
                        crate::config::save_list_pane_height(pos);
                    } else {
                        crate::config::save_list_pane_width(pos);
                    }
                },
            ));
        });
    }
    // The list keeps its width as the window resizes (the reader absorbs the
    // change), and can't be dragged narrower than its own minimum. The reader
    // has a floor of its own: its header carries a full row of actions, and
    // squeezing it pushed them off the right-hand edge of the window.
    mail_split.set_resize_start_child(false);
    mail_split.set_shrink_start_child(false);
    mail_split.set_resize_end_child(true);
    mail_split.set_shrink_end_child(false);
    content_stack.add_named(&mail_split, Some("mail"));

    let list_overlay = gtk::Overlay::new();
    // A strip across the top that says how wide the list is: with the reading
    // pane off, that decides when the toolbar folds (#475).
    let width_strip = gtk::DrawingArea::new();
    width_strip.set_can_target(false);
    width_strip.set_valign(gtk::Align::Start);
    width_strip.set_content_height(1);
    {
        let tx = sender.input_sender().clone();
        let fold = model.list_fold.clone();
        width_strip.connect_resize(move |_, width, _| fold.resized(width, &tx));
    }
    list_overlay.add_overlay(&width_strip);
    mail_split.set_start_child(Some(&list_overlay));

    let list_view = adw::ToolbarView::new();
    list_overlay.set_child(Some(&list_view));
    let list_header = adw::HeaderBar::new();
    list_header.add_css_class("flat");
    // Middle pane: no window controls (the reader pane's header carries the
    // window's close button), unless there is no reader pane (#475).
    list_header.set_show_start_title_buttons(false);
    // No folder title here — the sidebar's selection already names it. An empty
    // label keeps the window's "Hylki" title from appearing instead.
    list_header.set_title_widget(Some(&gtk::Label::new(Some(""))));
    // Leftmost, mirroring the pane it acts on: the sidebar expand/collapse
    // toggle (moved here from the sidebar's own footer).
    let sidebar_toggle = flat_button(sender, Some("sidebar-show-symbolic"), None, None, || {
        AppMsg::ToggleSidebar
    });
    list_header.pack_start(&sidebar_toggle);
    // Search lives behind this button (#102); Ctrl+F and / open it too.
    let lh_search = focus_revealer(
        &flat_button(
            sender,
            Some("system-search-symbolic"),
            None,
            Some(&i18n("Search messages (Ctrl+F)")),
            || AppMsg::OpenListSearch,
        ),
        true,
    );
    list_header.pack_start(&lh_search);
    // The reader's toolbar, while there is no reading pane to carry it (#475):
    // beside the window controls, so packed first. Filled by
    // relayout_reader_toolbar.
    let list_tools = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    list_tools.add_css_class("list-tools");
    list_header.pack_end(&list_tools);
    // Focus Mode: the ⋯ that stands in for the search, filters, count and sort
    // while they are folded away.
    let overflow_button = flat_button(
        sender,
        Some("view-more-horizontal-symbolic"),
        None,
        Some(&i18n("Search, filters and sort")),
        || AppMsg::ListOverflowMenu,
    );
    overflow_button.set_valign(gtk::Align::Center);
    let lh_overflow = focus_revealer(&overflow_button, false);
    lh_overflow.set_reveal_child(false);
    list_header.pack_end(&lh_overflow);
    // The sort orders and every quick filter share one popover (#441): a button
    // each would crowd the header.
    let list_filter_btn = gtk::MenuButton::new();
    list_filter_btn.set_icon_name("filter-symbolic");
    list_filter_btn.set_tooltip_text(Some(&i18n("Sort and filter messages")));
    list_filter_btn.set_valign(gtk::Align::Center);
    list_filter_btn.add_css_class("flat");
    let lh_filter = focus_revealer(&list_filter_btn, false);
    list_header.pack_end(&lh_filter);
    let count_label = gtk::Label::new(None);
    count_label.set_valign(gtk::Align::Center);
    count_label.add_css_class("list-count");
    let lh_count = focus_revealer(&count_label, false);
    list_header.pack_end(&lh_count);
    list_view.add_top_bar(&list_header);
    list_view.set_content(Some(model.message_list.widget()));

    // The narrowest the reader may become. The header's actions collapse into
    // the overflow menu before this matters (see the breakpoint added in init).
    // The height floor exists because AdwBreakpointBin insists on an explicit
    // one; the window's own minimum is what really binds vertically.
    let reader_bin = adw::BreakpointBin::new();
    reader_bin.set_size_request(READER_MIN_WIDTH, 200);
    mail_split.set_end_child(Some(&reader_bin));
    let reader_view = adw::ToolbarView::new();
    reader_bin.set_child(Some(&reader_view));
    let reader_header = adw::HeaderBar::new();
    // Rightmost of the packed items, beside the window controls: the overflow ⋯
    // that stands in for the action buttons while collapsed.
    reader_header.pack_end(&model.reader_overflow_btn);
    reader_header.add_css_class("flat");
    // Tighter icon spacing than stock so the full action row fits a narrower
    // pane (see READER_ACTIONS_BREAKPOINT and styles.css).
    reader_header.add_css_class("reader-toolbar");
    // Empty title so the window's "Hylki" title isn't shown here; the app title
    // lives above the sidebar.
    reader_header.set_title_widget(Some(&gtk::Label::new(Some(""))));
    reader_view.add_top_bar(&reader_header);

    // Outbox actions, in place of Reply/Forward/Flag — a message that hasn't
    // been sent can't be replied to, and the questions worth asking about it
    // are whether to edit, send or bin it.
    let outbox_edit_btn = flat_button(
        sender,
        Some("document-edit-symbolic"),
        None,
        Some(&i18n("Edit this message")),
        || AppMsg::EditCurrentOutbox,
    );
    let tb_outbox_edit = focus_revealer(&outbox_edit_btn, true);
    reader_header.pack_start(&tb_outbox_edit);
    let outbox_send_btn = flat_button(
        sender,
        Some("mail-send-symbolic"),
        None,
        Some(&i18n("Try to send this message now")),
        || AppMsg::SendCurrentOutbox,
    );
    let tb_outbox_send = focus_revealer(&outbox_send_btn, true);
    reader_header.pack_start(&tb_outbox_send);
    let outbox_send_all_btn = flat_button(sender, None, Some(&i18n("Send all")), None, || {
        AppMsg::RetryAllOutbox
    });
    let tb_outbox_send_all = focus_revealer(&outbox_send_all_btn, true);
    reader_header.pack_start(&tb_outbox_send_all);

    // ---- The action buttons. Declared here in the default order, then
    // re-packed in init (and on every change) in the user's saved order — see
    // relayout_reader_toolbar. The left group stays at every width; the right
    // group folds into the ⋯ overflow. Default left group: Reply, Reply All,
    // Forward, Star, Archive, Spam, Delete.
    let reply_btn = flat_button(
        sender,
        Some("mail-reply-sender-symbolic"),
        None,
        Some(&i18n("Reply")),
        || AppMsg::Reply,
    );
    let tb_reply = focus_revealer(&reply_btn, true);
    reader_header.pack_start(&tb_reply);
    let reply_all_btn = flat_button(
        sender,
        Some("mail-reply-all-symbolic"),
        None,
        Some(&i18n("Reply All")),
        || AppMsg::ReplyAll,
    );
    let tb_reply_all = focus_revealer(&reply_all_btn, true);
    reader_header.pack_start(&tb_reply_all);
    let forward_btn = flat_button(
        sender,
        Some("mail-forward-symbolic"),
        None,
        Some(&i18n("Forward")),
        || AppMsg::Forward,
    );
    let tb_forward = focus_revealer(&forward_btn, true);
    reader_header.pack_start(&tb_forward);
    // One glyph in both states, like every other icon; the starred state
    // carries color only.
    let star_btn = flat_button(
        sender,
        Some("hylki-non-starred-symbolic"),
        None,
        None,
        || AppMsg::ToggleStar,
    );
    let tb_star = focus_revealer(&star_btn, true);
    reader_header.pack_start(&tb_star);
    let archive_btn = flat_button(
        sender,
        Some("mail-archive-symbolic"),
        None,
        Some(&i18n("Archive")),
        || AppMsg::Archive,
    );
    let tb_archive = focus_revealer(&archive_btn, true);
    reader_header.pack_start(&tb_archive);
    let delete_btn = flat_button(sender, Some("user-trash-symbolic"), None, None, || {
        AppMsg::Delete
    });
    let tb_delete = focus_revealer(&delete_btn, true);
    reader_header.pack_start(&tb_delete);

    // ---- Default right group, left to right: Tags, Read/Unread, Spam, Move To,
    // Find, Print. (The sender-check seal lives in the message header now —
    // #88; no Add-to-Contacts button either: right-click any address in a
    // message header.)
    // The preview, not the print dialog: the button shows what will come out
    // and prints from there, so nobody spends paper to find out.
    let print_btn = flat_button(
        sender,
        Some("printer-symbolic"),
        None,
        Some(&i18n("Print Preview (Ctrl+Shift+P)")),
        || AppMsg::PrintPreview,
    );
    let tb_print = focus_revealer(&print_btn, false);
    reader_header.pack_end(&tb_print);
    // In-message find (#103).
    let find_btn = flat_button(
        sender,
        Some("loupe-with-arrow-symbolic"),
        None,
        Some(&i18n("Find in message (Ctrl+F)")),
        || AppMsg::OpenReaderFind,
    );
    let tb_find = focus_revealer(&find_btn, false);
    reader_header.pack_end(&tb_find);
    // Move To… (#164): a folder picker for the target, or the whole list
    // selection.
    let tb_move = focus_revealer(&boxed(&model.reader_move_btn), false);
    reader_header.pack_end(&tb_move);
    // Snooze (#527): a menu of times, greyed out where nothing can be snoozed
    // (Drafts, POP3).
    let tb_snooze = focus_revealer(&boxed(&model.reader_snooze_btn), false);
    reader_header.pack_end(&tb_snooze);
    let spam_btn = flat_button(sender, None, None, None, || AppMsg::MarkSpam);
    let tb_spam = focus_revealer(&spam_btn, false);
    reader_header.pack_end(&tb_spam);
    let read_btn = flat_button(sender, None, None, None, || AppMsg::ToggleReadCurrent);
    let tb_read = focus_revealer(&read_btn, false);
    reader_header.pack_end(&tb_read);
    // Tags (#71): a menu of the tags, ticked where the target carries them.
    // Only once a tag exists.
    let tb_tags = focus_revealer(&boxed(&model.reader_tag_btn), false);
    reader_header.pack_end(&tb_tags);
    let tb_spinner = gtk::Spinner::new();
    tb_spinner.set_valign(gtk::Align::Center);
    tb_spinner.set_tooltip_text(Some(&i18n("Downloading attachments…")));
    reader_header.pack_end(&tb_spinner);

    // Reader content: the inline reply/forward pane drops down (SlideDown
    // revealer) above the message body, pushing it down to make room. The
    // revealer is prepended in `init`. The drawer's widget is a Paned that
    // holds the reader body + attachment footer.
    let reader_content_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    reader_content_box.append(model.reader_swap.host());
    reader_view.set_content(Some(&reader_content_box));

    // The status bar sits at the foot of the window (#433).
    outer.append(model.notifications.widget());

    // ======== Full-window attachment lightbox ========
    // Covers all three panes; shown for images and PDFs coming from the
    // drawer. Same look as the gallery's own lightbox (same CSS), no extra
    // window.
    window_overlay.add_overlay(model.lightbox.widget());

    let widgets = AppModelWidgets {
        toasts,
        window_overlay,
        peek_split,
        sidebar_split,
        sidebar_header,
        app_title,
        sidebar_menu,
        content_stack,
        mail_split,
        list_header,
        sidebar_toggle,
        lh_search,
        list_tools,
        lh_overflow,
        lh_filter,
        list_filter_btn,
        lh_count,
        count_label,
        reader_bin,
        reader_header,
        tb_outbox_edit,
        outbox_edit_btn,
        tb_outbox_send,
        outbox_send_btn,
        tb_outbox_send_all,
        outbox_send_all_btn,
        tb_reply,
        reply_btn,
        tb_reply_all,
        reply_all_btn,
        tb_forward,
        forward_btn,
        tb_star,
        star_btn,
        tb_archive,
        archive_btn,
        tb_delete,
        delete_btn,
        tb_print,
        print_btn,
        tb_find,
        find_btn,
        tb_move,
        tb_snooze,
        tb_spam,
        spam_btn,
        tb_read,
        tb_tags,
        tb_spinner,
        reader_content_box,
    };
    widgets.apply(model);
    widgets
}

impl SimpleComponent for AppModel {
    type Init = ();
    type Input = AppMsg;
    type Output = ();
    type Root = adw::ApplicationWindow;
    type Widgets = AppModelWidgets;

    fn init_root() -> Self::Root {
        let window = adw::ApplicationWindow::default();
        window.set_title(Some(crate::APP_NAME));
        window.set_icon_name(Some(crate::APP_ID));
        window.add_css_class("hylki");
        window
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.apply(self);
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let boot = Boot::read(&root);
        let children = Children::launch(&boot, &sender);
        let main_menu = build_main_menu(
            boot.prefs.show_accounts,
            boot.prefs.reading_pane,
            &boot.focus,
            &sender,
        );
        let mut model = AppModel::new(&root, &sender, boot, children, main_menu);
        model.prime_and_connect(&sender);
        schedule_download_prune();
        text_search::prepare_text_search();
        model.withdraw_stale_notifications();
        if model.tray_enabled {
            model.start_tray(&sender);
        }
        start_contacts_sync(&sender);
        if model.contact_names {
            model.read_contact_names(&sender);
        }
        watch_system_events(&sender);
        model.open_wizard_if_needed(&sender);
        model.push_settings_to_children();
        model.arm_auto_fetch(&sender);

        // The app-wide theme choice must be in force before the first frame.
        apply_app_theme(model.app_theme);
        crate::desktop::text_scale::apply(model.text_scale);
        follow_motion_setting();
        let widgets = build_widgets(&root, &sender, &model);
        let _ = model.reader_header.set(widgets.reader_header.clone());
        let _ = model.mail_split.set(widgets.mail_split.clone());
        let _ = model.toasts.set(widgets.toasts.clone());
        model.init_reader_header_menu(&widgets, &sender);
        model.init_reader_toolbar_fold(&widgets, &sender);
        model.place_reader();
        model.init_compose_overlay(&widgets);
        model.init_gallery_page(&widgets, &sender);
        model.install_notification_actions(&sender);
        model.init_window_size(&root, &widgets, &sender);
        model.init_hover_peek(&widgets, &sender);
        model.sidebar_split = Some(widgets.sidebar_split.clone());
        model.peek_split = Some(widgets.peek_split.clone());
        model.app_title = Some(widgets.app_title.clone());
        model.sidebar_header = Some(widgets.sidebar_header.clone());
        model.sidebar_menu = Some(widgets.sidebar_menu.clone());
        model.init_refresh_buttons(&widgets, &sender);

        model.install_app_actions(&root, &sender);

        let (undo_action, redo_action) = install_window_actions(&root, &sender);

        root.add_controller(palette_key(&sender));

        model.install_key_controller(&root, &sender);

        model.show_startup_notices(&sender);

        start_heap_trim();
        model.install_showcase_probes(&sender);
        model.stage_showcase(&root, &widgets, &sender);

        model.init_list_sort_menu(&widgets, &sender);

        model.rebuild_help_menu();
        model.undo_action = Some(undo_action);
        model.redo_action = Some(redo_action);
        // Both start empty, so both start greyed out.
        model.refresh_undo_menu();
        model
            .notifications
            .emit(NotifyInput::SetConsoleEnabled(model.console_mode));
        model
            .notifications
            .emit(NotifyInput::SetAtTop(model.status_bar_top));
        model.install_showcase_overlays(&sender);
        model.drain_hand_off_queues(&sender);
        model.start_assistant_if_on(&sender);

        model.install_drop_targets(&root, &widgets, &sender);

        model.start_outbox_clocks(&sender);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        self.route(msg, sender)
    }
}
