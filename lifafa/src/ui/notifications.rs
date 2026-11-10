//! Top-of-window status bar (notification area).
//!
//! A transient bar slides down for new messages and auto-collapses after a few
//! seconds. Messages that need attention (errors) are also kept in a list that
//! can be expanded into toast-like cards via a button in the header. The bar is
//! tinted with the desktop accent color (`@accent_bg_color`, default GNOME
//! blue) and turns amber for errors.

use std::time::Duration;

use crate::i18n::{i18n, i18n_f};
use gtk::glib;
use gtk::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;

mod bar;
mod cards;
mod console;

use console::draw_crt_grain;

const TRANSIENT_SECS: u64 = 5;

/// A single persisted notification (currently always an error).
#[derive(Debug, Clone)]
struct Notification {
    id: u32,
    text: String,
    /// Connection/sync error — cleared automatically once connectivity returns.
    connectivity: bool,
}

/// One expandable card.
struct NotificationCard {
    note: Notification,
}

#[derive(Debug)]
enum CardOutput {
    Dismiss(u32),
}

/// The card is built once from its note and never changes; this is what
/// `#[relm4::factory]`'s `view!` block used to generate.
impl FactoryComponent for NotificationCard {
    type Init = Notification;
    type Input = ();
    type Output = CardOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::Box;
    type Root = gtk::Box;
    type Widgets = ();
    type Index = DynamicIndex;

    fn init_model(note: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        Self { note }
    }

    fn init_root(&self) -> Self::Root {
        let card = gtk::Box::builder().spacing(10).css_classes(["toast-card"]).build();
        card.append(
            &gtk::Image::builder()
                .icon_name("dialog-warning-symbolic")
                .valign(gtk::Align::Start)
                .css_classes(["toast-card-icon"])
                .build(),
        );
        card.append(
            &gtk::Label::builder()
                .label(self.note.text.as_str())
                .wrap(true)
                // Copyable (#246): an error is something to paste into a report.
                .selectable(true)
                .xalign(0.0)
                .hexpand(true)
                .halign(gtk::Align::Start)
                .build(),
        );
        card
    }

    fn init_widgets(
        &mut self,
        _index: &DynamicIndex,
        root: Self::Root,
        _returned_widget: &gtk::Widget,
        sender: FactorySender<Self>,
    ) -> Self::Widgets {
        let dismiss = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .valign(gtk::Align::Start)
            .css_classes(["flat", "circular"])
            .build();
        let id = self.note.id;
        dismiss.connect_clicked(move |_| {
            let _ = sender.output(CardOutput::Dismiss(id));
        });
        root.append(&dismiss);
    }
}

pub(crate) struct NotificationCenter {
    cards: FactoryVecDeque<NotificationCard>,
    /// Card ids, index-aligned with the factory (newest first).
    ids: Vec<u32>,
    next_id: u32,
    /// Bumped on each new transient message so stale timers are ignored.
    epoch: u32,
    transient_text: String,
    transient_error: bool,
    transient_visible: bool,
    /// Current ongoing activity (connecting/syncing). Never auto-shown; only
    /// visible when the user opens the panel.
    status_text: String,
    /// The network is down: the opened bar explains it.
    offline: bool,
    panel_open: bool,
    /// Console mode (verbose live log): offered at all (Settings), and open.
    console_enabled: bool,
    console_open: bool,
    /// The dracula styling, applied the moment console mode is requested —
    /// the console's own reveal is staged (see ShowConsole), and the bar must
    /// never map in its normal colors first and fade over.
    console_theme: bool,
    /// Whether opening the console is what opened the bar — closing the
    /// console then takes the bar back down with it.
    console_opened_bar: bool,
    /// Newest console_log sequence already rendered.
    console_seq: u64,
    console_buf: gtk::TextBuffer,
    /// Whether the console sticks to its newest line: true until the user
    /// scrolls up to read, true again once they scroll back to the bottom.
    console_follow: std::rc::Rc<std::cell::Cell<bool>>,
    console_scroll: Option<gtk::ScrolledWindow>,
    console_grain: Option<gtk::DrawingArea>,
    /// Whether the bar sits at the top of the window rather than its foot
    /// (Settings, #433); shared with the console grip's drag.
    top: std::rc::Rc<std::cell::Cell<bool>>,
    /// What [`NotificationCenter::place`] puts in order for either end.
    layout: Option<Layout>,
}

/// The parts of the bar whose order follows the end of the window it is at:
/// the bar nearest the window's edge, the cards and the console opening
/// away from it, and the console's grip on its far side.
struct Layout {
    root: gtk::Revealer,
    area: gtk::Box,
    bar: gtk::WindowHandle,
    console_box: gtk::Box,
    grip: gtk::Box,
}

#[derive(Debug)]
pub(crate) enum NotifyInput {
    /// Auto-shows the transient bar for a few seconds; errors also add a card.
    Push {
        text: String,
        error: bool,
        connectivity: bool,
    },
    /// Updates the ongoing-activity text without auto-showing anything.
    SetStatus(String),
    /// Connectivity restored: drop any connection/sync error cards.
    ClearConnectivity,
    /// The network went down or came back. Never shows the bar by itself:
    /// the Refresh button turns orange, and the bar, once opened, says why
    /// (#442).
    SetOffline(bool),
    TogglePanel,
    /// The menu item, Ctrl+Shift+S and a long-press on Refresh: hide the bar
    /// if it shows at all, a passing message included, or open it (#294).
    ToggleBar,
    /// Open the panel straight into console mode.
    ShowConsole,
    /// The console's Export button (#132).
    ExportLog,
    /// Settings: whether console mode is offered (button + menu).
    SetConsoleEnabled(bool),
    /// Settings: the bar at the top of the window (`true`) or its foot.
    SetAtTop(bool),
    /// Console tick: pull new log lines (and reschedule while open).
    PollConsole,
    /// Second stage of ShowConsole: slide the console open once the bar
    /// itself is mapped (an unmapped revealer skips its transition).
    RevealConsole,
    ClearAll,
    Dismiss(u32),
    CollapseTransient(u32),
}

#[derive(Debug)]
pub(crate) enum NotifyOutput {
    CountChanged(usize),
    /// The console's Export button: save the log to a file (#132).
    ExportLog,
    /// The bar came down or went up, for the menu item that names what it
    /// will do (#294).
    Shown(bool),
}

/// The bar's widgets: the ones `init` and the console module wire
/// afterwards, and the ones that follow the model, which `apply` sets from
/// it at build and after every message. This is what
/// `#[relm4::component]`'s `view!` block and its `#[watch]` attributes used
/// to generate.
pub(crate) struct NotificationCenterWidgets {
    root: gtk::Revealer,
    area: gtk::Box,
    cards_revealer: gtk::Revealer,
    console_revealer: gtk::Revealer,
    console_box: gtk::Box,
    console_grip: gtk::Box,
    console_scroll: gtk::ScrolledWindow,
    console_grain: gtk::DrawingArea,
    bar: gtk::WindowHandle,
    bar_icon: gtk::Image,
    bar_label: gtk::Label,
    clear_btn: gtk::Button,
    export_btn: gtk::Button,
    console_btn: gtk::Button,
    chevron_btn: gtk::Button,
}

impl NotificationCenterWidgets {
    /// Everything that was `#[watch]`ed: set from the model.
    fn apply(&self, model: &NotificationCenter) {
        self.root.set_transition_type(model.slide());
        self.root.set_reveal_child(model.transient_visible || model.panel_open);
        self.area.set_css_classes(&model.area_classes());
        // Only the cards section expands; an empty panel stays at bar
        // height (just shows the status / "Status bar (0)" row).
        self.cards_revealer.set_transition_type(model.slide());
        self.cards_revealer.set_reveal_child(model.panel_open && !model.console_open && !model.ids.is_empty());
        self.console_revealer.set_transition_type(model.slide());
        self.console_revealer.set_reveal_child(model.console_open);
        self.bar_icon.set_icon_name(Some(model.bar_icon()));
        self.bar_label.set_label(&model.bar_label());
        // The offline explanation is read whole; anything else is one line.
        self.bar_label.set_wrap(model.showing_offline());
        self.bar_label.set_ellipsize(if model.showing_offline() {
            gtk::pango::EllipsizeMode::None
        } else {
            gtk::pango::EllipsizeMode::End
        });
        self.clear_btn.set_visible(model.panel_open && !model.ids.is_empty());
        self.export_btn.set_visible(model.console_enabled && model.console_open);
        self.console_btn.set_visible(model.console_enabled);
        self.chevron_btn.set_icon_name(model.chevron());
    }
}

/// A flat button in the bar that sends one message.
fn bar_button(sender: &ComponentSender<NotificationCenter>, label: BarFace, tooltip: Option<String>, message: fn() -> NotifyInput) -> gtk::Button {
    let button = gtk::Button::builder().css_classes(["flat"]).build();
    match label {
        BarFace::Label(text) => button.set_label(&text),
        BarFace::Icon(name) => button.set_icon_name(name),
    }
    if let Some(tip) = tooltip {
        button.set_tooltip_text(Some(&tip));
    }
    let sender = sender.clone();
    button.connect_clicked(move |_| sender.input(message()));
    button
}

/// What a bar button shows.
enum BarFace {
    Label(String),
    Icon(&'static str),
}

fn build_widgets(
    root: &gtk::Revealer,
    sender: &ComponentSender<NotificationCenter>,
    model: &NotificationCenter,
    cards_box: &gtk::Box,
    console_buf: &gtk::TextBuffer,
) -> NotificationCenterWidgets {
    let area = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();

    cards_box.set_orientation(gtk::Orientation::Vertical);
    cards_box.set_spacing(8);
    cards_box.add_css_class("notify-cards");
    let cards_scroll = gtk::ScrolledWindow::builder()
        .propagate_natural_height(true)
        .max_content_height(320)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(cards_box)
        .build();
    let cards_revealer = gtk::Revealer::builder().child(&cards_scroll).build();
    area.append(&cards_revealer);

    // Console mode: the dracula terminal, expanding the bar further.
    // Selectable text; a CSS-scanline + drawn-grain overlay gives it the
    // old-CRT cast.
    // Drag grip: pull the console taller (160px floor).
    let grip_bar = gtk::Box::builder()
        .css_classes(["console-grip-bar"])
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    let console_grip = gtk::Box::builder().css_classes(["console-grip"]).halign(gtk::Align::Fill).build();
    console_grip.set_cursor_from_name(Some("ns-resize"));
    console_grip.append(&grip_bar);
    let console_view = gtk::TextView::builder()
        .buffer(console_buf)
        .css_classes(["console-view"])
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(10)
        .right_margin(10)
        .top_margin(8)
        .bottom_margin(8)
        .build();
    let console_scroll = gtk::ScrolledWindow::builder()
        .min_content_height(160)
        .max_content_height(160)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&console_view)
        .build();
    let scanlines = gtk::Box::builder().css_classes(["console-scanlines"]).can_target(false).build();
    let console_grain = gtk::DrawingArea::builder().can_target(false).build();
    let console_overlay = gtk::Overlay::new();
    console_overlay.set_child(Some(&console_scroll));
    console_overlay.add_overlay(&scanlines);
    console_overlay.add_overlay(&console_grain);
    let console_box = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
    console_box.append(&console_grip);
    console_box.append(&console_overlay);
    let console_revealer = gtk::Revealer::builder().transition_duration(350).child(&console_box).build();
    area.append(&console_revealer);

    // A WindowHandle so the bar can drag the window, like a title bar.
    // Interactive children (the buttons) still receive their clicks.
    let bar_icon = gtk::Image::new();
    let bar_label = gtk::Label::builder().hexpand(true).halign(gtk::Align::Start).xalign(0.0).build();
    let clear_btn = bar_button(sender, BarFace::Label(i18n("Clear all")), None, || NotifyInput::ClearAll);
    // While the console is open: save its log to a file.
    let export_btn = bar_button(sender, BarFace::Icon("document-save-symbolic"), Some(i18n("Export log")), || NotifyInput::ExportLog);
    // Console mode: the verbose live log.
    let console_btn = bar_button(sender, BarFace::Icon("code-symbolic"), Some(i18n("Console")), || NotifyInput::ShowConsole);
    // The bar's own toggle, always last.
    let chevron_btn = bar_button(sender, BarFace::Icon("pan-down-symbolic"), Some(i18n("Collapse status bar")), || NotifyInput::TogglePanel);
    let bar_row = gtk::Box::builder().css_classes(["notify-bar"]).spacing(8).build();
    bar_row.append(&bar_icon);
    bar_row.append(&bar_label);
    bar_row.append(&clear_btn);
    bar_row.append(&export_btn);
    bar_row.append(&console_btn);
    bar_row.append(&chevron_btn);
    let bar = gtk::WindowHandle::new();
    bar.set_child(Some(&bar_row));
    area.append(&bar);

    // At the window's foot by default, so the bar coming up never moves what
    // is being read or written (#433); the cards and the console open away
    // from the window's edge. Settings can put it at the top.
    root.set_child(Some(&area));

    let widgets = NotificationCenterWidgets {
        root: root.clone(),
        area,
        cards_revealer,
        console_revealer,
        console_box,
        console_grip,
        console_scroll,
        console_grain,
        bar,
        bar_icon,
        bar_label,
        clear_btn,
        export_btn,
        console_btn,
        chevron_btn,
    };
    widgets.apply(model);
    widgets
}

impl SimpleComponent for NotificationCenter {
    type Init = ();
    type Input = NotifyInput;
    type Output = NotifyOutput;
    type Root = gtk::Revealer;
    type Widgets = NotificationCenterWidgets;

    fn init_root() -> Self::Root {
        gtk::Revealer::new()
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.apply(self);
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let cards = FactoryVecDeque::builder()
            .launch(gtk::Box::default())
            .forward(sender.input_sender(), |out| match out {
                CardOutput::Dismiss(id) => NotifyInput::Dismiss(id),
            });

        let console_buf = gtk::TextBuffer::new(None);

        let mut model = NotificationCenter {
            cards,
            ids: Vec::new(),
            next_id: 1,
            epoch: 0,
            transient_text: String::new(),
            transient_error: false,
            transient_visible: false,
            status_text: String::new(),
            offline: !crate::desktop::power::online(),
            panel_open: false,
            console_enabled: false,
            console_open: false,
            console_theme: false,
            console_opened_bar: false,
            console_seq: 0,
            console_buf: console_buf.clone(),
            console_follow: std::rc::Rc::new(std::cell::Cell::new(true)),
            console_scroll: None,
            console_grain: None,
            top: std::rc::Rc::default(),
            layout: None,
        };

        let cards_box = model.cards.widget();
        let widgets = build_widgets(&root, &sender, &model, cards_box, &console_buf);
        model.console_scroll = Some(widgets.console_scroll.clone());
        model.console_grain = Some(widgets.console_grain.clone());
        widgets.console_grain.set_draw_func(draw_crt_grain);
        model.follow_console_until_user_scrolls(&widgets);
        model.layout = Some(Layout {
            root: root.clone(),
            area: widgets.area.clone(),
            bar: widgets.bar.clone(),
            console_box: widgets.console_box.clone(),
            grip: widgets.console_grip.clone(),
        });
        model.attach_grip_drag(&widgets);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        let was_shown = self.shown();
        self.handle(msg, &sender);
        if self.shown() != was_shown {
            let _ = sender.output(NotifyOutput::Shown(self.shown()));
        }
    }
}

impl NotificationCenter {
    /// How the bar and its panels slide: away from the window's edge.
    fn slide(&self) -> gtk::RevealerTransitionType {
        if self.top.get() {
            gtk::RevealerTransitionType::SlideDown
        } else {
            gtk::RevealerTransitionType::SlideUp
        }
    }

    /// The bar's toggle points where the panel opens, or back where it folds.
    fn chevron(&self) -> &'static str {
        match (self.top.get(), self.panel_open) {
            (true, true) | (false, false) => "pan-up-symbolic",
            (true, false) | (false, true) => "pan-down-symbolic",
        }
    }

    /// Open the console on its newest line, whatever was being read the last
    /// time. The hidden view keeps its last measurements, so the jump lands
    /// even when nothing new arrives to resize it.
    fn follow_console(&self) {
        self.console_follow.set(true);
        if let Some(sw) = &self.console_scroll {
            let adj = sw.vadjustment();
            adj.set_value(adj.upper() - adj.page_size());
        }
    }

    /// Put the bar at its end of the window, and its parts in order for it.
    fn place(&self) {
        let Some(l) = &self.layout else { return };
        let top = self.top.get();
        let none = None::<&gtk::Widget>;
        if top {
            l.area.reorder_child_after(&l.bar, none);
            l.console_box
                .reorder_child_after(&l.grip, l.console_box.last_child().as_ref());
        } else {
            l.area
                .reorder_child_after(&l.bar, l.area.last_child().as_ref());
            l.console_box.reorder_child_after(&l.grip, none);
        }
        // The window's column: first child at the top, last at the foot.
        if let Some(column) = l.root.parent().and_downcast::<gtk::Box>() {
            if top {
                column.reorder_child_after(&l.root, none);
            } else if let Some(last) = column
                .last_child()
                .filter(|w| w != l.root.upcast_ref::<gtk::Widget>())
            {
                column.reorder_child_after(&l.root, Some(&last));
            }
        }
    }

    fn handle(&mut self, msg: NotifyInput, sender: &ComponentSender<Self>) {
        match msg {
            NotifyInput::Push {
                text,
                error,
                connectivity,
            } => self.on_push(text, error, connectivity, sender),
            NotifyInput::SetStatus(text) => {
                self.status_text = text;
            }
            NotifyInput::SetOffline(offline) => {
                self.offline = offline;
            }
            NotifyInput::ClearConnectivity => self.on_clear_connectivity(sender),
            NotifyInput::CollapseTransient(epoch) => self.on_collapse_transient(epoch),
            NotifyInput::TogglePanel => self.on_toggle_panel(),
            NotifyInput::ToggleBar => self.on_toggle_bar(),
            NotifyInput::ExportLog => {
                let _ = sender.output(NotifyOutput::ExportLog);
            }
            NotifyInput::ShowConsole => self.on_show_console(sender),
            NotifyInput::RevealConsole => self.on_reveal_console(sender),
            NotifyInput::SetAtTop(on) => self.on_set_at_top(on),
            NotifyInput::SetConsoleEnabled(on) => self.on_set_console_enabled(on),
            NotifyInput::PollConsole => self.on_poll_console(sender),
            NotifyInput::Dismiss(id) => self.on_dismiss(id, sender),
            NotifyInput::ClearAll => self.on_clear_all(sender),
        }
    }

    /// Whether the bar is down: a passing message, or the panel.
    fn shown(&self) -> bool {
        self.transient_visible || self.panel_open
    }

    /// Remove every connectivity-flagged error card (newest-first indices).
    fn remove_connectivity_cards(&mut self) {
        let to_remove: Vec<usize> = (0..self.ids.len())
            .filter(|&i| self.cards.get(i).is_some_and(|c| c.note.connectivity))
            .collect();
        if to_remove.is_empty() {
            return;
        }
        {
            let mut guard = self.cards.guard();
            for &i in to_remove.iter().rev() {
                guard.remove(i);
            }
        }
        for &i in to_remove.iter().rev() {
            self.ids.remove(i);
        }
    }

    fn is_error_state(&self) -> bool {
        // Amber only when something error-related is actually present: an active
        // error transient, or existing error cards. Never the stale flag, which
        // used to tint the bar amber while collapsing an already-cleared panel.
        (self.transient_visible && self.transient_error)
            || !self.ids.is_empty()
            || self.showing_offline()
    }

    /// Whether the bar's own line is the offline explanation: the bar was
    /// opened while the network is down.
    fn showing_offline(&self) -> bool {
        self.offline && self.panel_open && !self.console_open
    }

    fn area_classes(&self) -> Vec<&'static str> {
        let mut classes = if self.is_error_state() {
            vec!["notify-area", "error"]
        } else {
            vec!["notify-area"]
        };
        if self.console_theme || self.console_open {
            // CSS transitions on the bar colors fade it into (and out of)
            // the dracula terminal look — console_theme is set before the
            // bar maps, so a shortcut-opened console never flashes the
            // normal bar colors first.
            classes.push("console-on");
        }
        classes
    }

    fn bar_icon(&self) -> &'static str {
        if self.showing_offline() {
            "network-offline-symbolic"
        } else if self.is_error_state() {
            "dialog-warning-symbolic"
        } else {
            // The bell — the same icon as the toolbar button that opens this
            // panel, so the two read as one feature.
            "preferences-system-notifications-symbolic"
        }
    }

    fn bar_label(&self) -> String {
        if self.panel_open {
            // Nothing syncs while the network is down, so that is the
            // activity to report.
            if self.showing_offline() {
                i18n(
                    "Offline: there is no network connection, so mail is not being checked or sent. Messages you send wait in the Outbox. Everything syncs again once the network is back.",
                )
            // Reveal the current activity when the user opens the panel.
            } else if !self.status_text.is_empty() {
                self.status_text.clone()
            } else {
                i18n_f("Status bar ({n})", &[("n", &self.ids.len().to_string())])
            }
        } else {
            self.transient_text.clone()
        }
    }
}
