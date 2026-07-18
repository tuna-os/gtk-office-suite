// SPDX-License-Identifier: GPL-3.0-or-later
//
// suite-common — Shared chrome for the hanthor Rust office suite.
// Provides GNOME-compliant window chrome, toolbar, menus, and application plumbing.
//
// Architecture:
//   SuiteApp          — wraps adw::Application with common Gio actions, shortcuts, dark mode
//   SuiteWindow       — helper to assemble AdwApplicationWindow + ToolbarView + HeaderBar
//   SuiteToolbar      — responsive formatting toolbar (primary/extended/more pattern)
//   FileDialogHelper  — shared open/save/export file dialogs with office format filters
//   ToastManager      — toast notification system for save/error feedback

pub mod file_dialogs;
pub mod toast_manager;
pub use suite_common_core::{actions, palette, format, undo, events, string_pool, units, props, style, search, print};

pub use file_dialogs::FileDialogHelper;
pub use toast_manager::ToastManager;

// The crate is named `libadwaita` in Cargo.toml; imported as `adw` for conciseness.
use libadwaita as adw;
use adw::prelude::*;
use gtk4::{self as gtk, gio, glib};
use std::cell::Cell;

// ---------------------------------------------------------------------------
// SuiteApp — application-level scaffolding
// ---------------------------------------------------------------------------

/// Wraps an `adw::Application` and registers common keyboard shortcuts,
/// the common action namespace, and a dark-mode manager.
pub struct SuiteApp {
    pub app: adw::Application,
}

impl SuiteApp {
    /// Create a new SuiteApp with the given application id.
    /// Registers standard actions: new, open, save, save-as, preferences,
    /// about, keyboard-shortcuts, toggle-dark-mode, quit.
    pub fn new(app_id: &str) -> Self {
        let app = adw::Application::builder()
            .application_id(app_id)
            // Apps open documents from the CLI / file manager ("Open with…").
            .flags(gio::ApplicationFlags::HANDLES_OPEN)
            .build();

        // ---- Register actions (using closure captures) ----
        let act_new = gio::SimpleAction::new("new", None);
        let app_weak = app.downgrade();
        act_new.connect_activate(move |_, _| {
            if let Some(app) = app_weak.upgrade() {
                app.activate_action("new-document", None);
            }
        });
        app.add_action(&act_new);

        let act_open = gio::SimpleAction::new("open", None);
        let app_weak = app.downgrade();
        act_open.connect_activate(move |_, _| {
            if let Some(app) = app_weak.upgrade() {
                app.activate_action("open-file", None);
            }
        });
        app.add_action(&act_open);

        let act_save = gio::SimpleAction::new("save", None);
        let app_weak = app.downgrade();
        act_save.connect_activate(move |_, _| {
            if let Some(app) = app_weak.upgrade() {
                app.activate_action("save-file", None);
            }
        });
        app.add_action(&act_save);

        let act_save_as = gio::SimpleAction::new("save-as", None);
        let app_weak = app.downgrade();
        act_save_as.connect_activate(move |_, _| {
            if let Some(app) = app_weak.upgrade() {
                app.activate_action("save-file-as", None);
            }
        });
        app.add_action(&act_save_as);

        let act_prefs = gio::SimpleAction::new("preferences", None);
        let app_weak = app.downgrade();
        act_prefs.connect_activate(move |_, _| {
            if let Some(app) = app_weak.upgrade() {
                app.activate_action("show-preferences", None);
            }
        });
        app.add_action(&act_prefs);

        let act_about = gio::SimpleAction::new("about", None);
        act_about.connect_activate(move |_, _| {
            show_about_dialog();
        });
        app.add_action(&act_about);

        let act_shortcuts = gio::SimpleAction::new("shortcuts", None);
        let app_weak = app.downgrade();
        act_shortcuts.connect_activate(move |_, _| {
            if let Some(app) = app_weak.upgrade() {
                show_shortcuts_from_registry(&app);
            }
        });
        app.add_action(&act_shortcuts);

        let act_dark = gio::SimpleAction::new("toggle-dark-mode", None);
        act_dark.connect_activate(move |_, _| {
            let sm = adw::StyleManager::default();
            let is_dark = sm.is_dark();
            sm.set_color_scheme(if is_dark {
                adw::ColorScheme::ForceLight
            } else {
                adw::ColorScheme::ForceDark
            });
        });
        app.add_action(&act_dark);

        let act_palette = gio::SimpleAction::new("command-palette", None);
        let app_weak = app.downgrade();
        act_palette.connect_activate(move |_, _| {
            if let Some(app) = app_weak.upgrade() {
                show_command_palette(&app);
            }
        });
        app.add_action(&act_palette);

        let act_quit = gio::SimpleAction::new("quit", None);
        let app_weak = app.downgrade();
        act_quit.connect_activate(move |_, _| {
            if let Some(app) = app_weak.upgrade() {
                app.quit();
            }
        });
        app.add_action(&act_quit);

        // ---- Keyboard accelerators ----
        app.set_accels_for_action("app.new",        &["<Control>n"]);
        app.set_accels_for_action("app.open",        &["<Control>o"]);
        app.set_accels_for_action("app.save",        &["<Control>s"]);
        app.set_accels_for_action("app.save-as",     &["<Control><Shift>s"]);
        app.set_accels_for_action("app.preferences", &["<Control>comma"]);
        app.set_accels_for_action("app.shortcuts",   &["<Control>question"]);
        app.set_accels_for_action("app.quit",        &["<Control>q"]);
        app.set_accels_for_action("app.command-palette", &["<Control>k"]);

        actions::register_labels(&[
            ("app.command-palette", "Command Palette"),
            ("app.new", "New Document"),
            ("app.open", "Open…"),
            ("app.save", "Save"),
            ("app.save-as", "Save As…"),
            ("app.preferences", "Preferences"),
            ("app.about", "About"),
            ("app.shortcuts", "Keyboard Shortcuts"),
            ("app.toggle-dark-mode", "Toggle Dark Mode"),
            ("app.quit", "Quit"),
        ]);

        SuiteApp { app }
    }

    /// Run the application (calls `app.run()`).
    pub fn run(&self) -> glib::ExitCode {
        self.app.run()
    }

    /// Create a Gio::Settings handle for this app's schema.
    /// The schema XML must be installed (e.g., via Flatpak manifest).
    /// Panics if the schema doesn't exist — catch at development time.
    pub fn settings(&self) -> gio::Settings {
        let schema_id = self.app.application_id().unwrap_or_default();
        gio::Settings::new(&schema_id)
    }

    /// Restore dark mode from GSettings on startup.
    /// Must be called after GTK initialization (e.g. in connect_activate).
    pub fn restore_dark_mode(&self) {
        let settings = self.settings();
        let dark = settings.boolean("dark-mode");
        let sm = adw::StyleManager::default();
        if dark {
            sm.set_color_scheme(adw::ColorScheme::ForceDark);
        }
    }
}

/// Show a generic about dialog (apps override with their own metadata).
fn show_about_dialog() {
    let about = adw::AboutDialog::new();
    about.set_application_name("Hanthor Office");
    about.set_version("0.1.0");
    about.set_developer_name("Hanthor Contributors");
    about.set_license_type(gtk::License::Gpl30);
    about.set_comments("A GNOME-native office suite written in Rust.");
    about.set_website("https://github.com/tuna-os/gtk-office-suite");
    about.set_issue_url("https://github.com/tuna-os/gtk-office-suite/issues");
    about.present(gtk::Window::NONE);
}

// ---------------------------------------------------------------------------
// SuiteToolbar — responsive formatting toolbar
// ---------------------------------------------------------------------------

/// One toolbar item: (symbolic icon name, tooltip, fully qualified action
/// name like `"app.bold"`). Buttons activate the named GioAction, so every
/// toolbar capability is automatically keyboard-reachable, palette-listable,
/// and collapsible into a real menu.
pub type ToolbarItem = (&'static str, &'static str, &'static str);

/// A responsive toolbar with a primary (always-visible) section and an
/// extended section that collapses into a "More" menu on narrow windows.
///
/// Layout:
///   [ Bold | Italic | Underline ]  [ Strikethrough | ... | More▾ ]
///    ← always visible →              ← collapses on narrow →
pub struct SuiteToolbar {
    pub container: gtk::Box,
    pub extended_box: gtk::Box,
    pub more_button: gtk::MenuButton,
    collapse_callback: Option<Box<dyn Fn(bool)>>,
    expanded: Cell<bool>,
}

/// Menu label for an action: registry label if present, else the tooltip
/// with any trailing accelerator hint ("Bold (<Control>b)") stripped.
fn menu_label(action: &str, tooltip: &str) -> String {
    actions::label_for(action)
        .unwrap_or_else(|| tooltip.split(" (").next().unwrap_or(tooltip).to_string())
}

impl SuiteToolbar {
    /// Build a responsive toolbar from action-named items.
    pub fn new(primary: Vec<ToolbarItem>, extended: Vec<ToolbarItem>) -> Self {
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        container.set_margin_start(6);
        container.set_margin_end(6);

        if primary.is_empty() && extended.is_empty() {
            container.set_visible(false);
        }

        let make_button = |icon: &str, tooltip: &str, action: &str| -> gtk::Button {
            let b = gtk::Button::new();
            if icon.ends_with("-symbolic") {
                b.set_icon_name(icon);
                // GNOME HIG: symbolic icons with tooltips, NOT text labels
                b.add_css_class("image-button");
            } else {
                b.set_label(icon);
            }
            b.set_tooltip_text(Some(tooltip));
            b.set_action_name(Some(action));
            b
        };

        // ---- Primary section (always visible) ----
        let primary_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        primary_box.add_css_class("linked");
        for (icon, tooltip, action) in &primary {
            primary_box.append(&make_button(icon, tooltip, action));
        }
        container.append(&primary_box);

        // ---- Extended section + More button ----
        let extended_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        extended_box.add_css_class("linked");
        let more_menu = gio::Menu::new();
        for (icon, tooltip, action) in &extended {
            extended_box.append(&make_button(icon, tooltip, action));
            // Same action in the overflow menu, shown when collapsed.
            more_menu.append(Some(&menu_label(action, tooltip)), Some(action));
        }

        let more_button = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("More")
            .menu_model(&more_menu)
            .build();

        container.append(&extended_box);
        container.append(&more_button);

        let toolbar = SuiteToolbar {
            container,
            extended_box,
            more_button,
            collapse_callback: None,
            expanded: Cell::new(true),
        };

        toolbar.set_narrow(false);
        toolbar
    }

    /// Switch between wide and narrow layout.
    pub fn set_narrow(&self, narrow: bool) {
        self.extended_box.set_visible(!narrow);
        self.more_button.set_visible(narrow);
        if let Some(ref cb) = self.collapse_callback {
            cb(narrow);
        }
        self.expanded.set(!narrow);
    }

    /// Set a callback invoked when the toolbar collapses/expands.
    pub fn on_collapse_changed<F: Fn(bool) + 'static>(&mut self, f: F) {
        self.collapse_callback = Some(Box::new(f));
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded.get()
    }
}

// ---------------------------------------------------------------------------
// SuiteWindow — helper to build the common window chrome
// ---------------------------------------------------------------------------

/// Helper to construct a standard `adw::ApplicationWindow` with toolbar
/// view, header bar, and toolbar.
pub struct SuiteWindow {
    pub window: adw::ApplicationWindow,
    pub toolbar_view: adw::ToolbarView,
    pub toolbar: SuiteToolbar,
}

impl SuiteWindow {
    /// Build a standard application window with chrome.
    pub fn new(
        app: &adw::Application,
        title: &str,
        primary_toolbar: Vec<ToolbarItem>,
        extended_toolbar: Vec<ToolbarItem>,
    ) -> Self {
        let win = adw::ApplicationWindow::builder()
            .application(app)
            .default_width(960)
            .default_height(680)
            .title(title)
            .build();

        // ---- Header bar ----
        let header_bar = make_header_bar();

        // ---- Toolbar ----
        let toolbar = SuiteToolbar::new(primary_toolbar, extended_toolbar);

        // ---- Toolbar view ----
        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.set_top_bar_style(adw::ToolbarStyle::Raised);
        toolbar_view.add_top_bar(&header_bar);
        toolbar_view.add_top_bar(&toolbar.container);

        win.set_content(Some(&toolbar_view));

        // ---- Adaptive collapse (HIG: adapt below 600sp) ----
        // The extended toolbar section folds into the "More" menu.
        let bp = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            600.0,
            adw::LengthUnit::Sp,
        ));
        bp.add_setter(&toolbar.extended_box, "visible", Some(&false.to_value()));
        bp.add_setter(&toolbar.more_button, "visible", Some(&true.to_value()));
        win.add_breakpoint(bp);

        // ---- Window sizing ----
        win.set_size_request(360, 300);

        SuiteWindow {
            window: win,
            toolbar_view,
            toolbar,
        }
    }

    /// Set the content widget of the toolbar view (the document area).
    pub fn set_content(&self, widget: &impl IsA<gtk::Widget>) {
        self.toolbar_view.set_content(Some(widget));
    }

    /// Add a bottom bar (e.g. formula bar for Tables).
    pub fn add_bottom_bar(&self, widget: &impl IsA<gtk::Widget>) {
        self.toolbar_view.add_bottom_bar(widget);
    }

    /// Add a top bar above the toolbar.
    pub fn add_top_bar(&self, widget: &impl IsA<gtk::Widget>) {
        self.toolbar_view.add_top_bar(widget);
    }

    /// Display the window.
    pub fn present(&self) {
        self.window.present();
    }
}

// ---------------------------------------------------------------------------
// Header bar builder
// ---------------------------------------------------------------------------

/// Build a standard GNOME header bar:
///   [start] New Document button
///   [center] Window title (implicit via AdwApplicationWindow)
///   [end]   Style toggle, Menu button (Preferences, Keyboard Shortcuts, About)
pub fn make_header_bar() -> adw::HeaderBar {
    // ---- Start: New Document ----
    let new_btn = gtk::Button::builder()
        .icon_name("document-new-symbolic")
        .tooltip_text("New Document")
        .action_name("app.new")
        .build();
    new_btn.add_css_class("flat");

    // ---- End: Primary menu ----
    let menu = gio::Menu::new();

    let file_section = gio::Menu::new();
    file_section.append(Some("_New"), Some("app.new"));
    file_section.append(Some("_Open\u{2026}"), Some("app.open"));
    file_section.append(Some("_Save"), Some("app.save"));
    file_section.append(Some("Save _as\u{2026}"), Some("app.save-as"));
    file_section.append(Some("Page set_up\u{2026}"), Some("app.page-setup"));
    file_section.append(Some("Print pre_view\u{2026}"), Some("app.print-preview"));
    file_section.append(Some("_Print\u{2026}"), Some("app.print"));
    menu.append_section(Some("File"), &file_section);

    let edit_section = gio::Menu::new();
    edit_section.append(Some("_Preferences"), Some("app.preferences"));
    menu.append_section(Some("Edit"), &edit_section);

    let help_section = gio::Menu::new();
    help_section.append(Some("_Keyboard shortcuts"), Some("app.shortcuts"));
    help_section.append(Some("_About"), Some("app.about"));
    menu.append_section(Some("Help"), &help_section);

    let menu_btn = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Menu")
        .build();

    let hb = adw::HeaderBar::new();
    hb.pack_start(&new_btn);
    hb.pack_end(&menu_btn);
    hb
}

// ---------------------------------------------------------------------------
// Preference helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Keyboard Shortcuts Dialog
// ---------------------------------------------------------------------------

/// Build and show a keyboard shortcuts dialog.
/// `shortcuts` — list of (group_title, [(shortcut_title, accelerator), ...]).
pub fn show_shortcuts_dialog(
    shortcuts: &[(&str, &[(&str, &str)])],
) {
    let win = gtk::ShortcutsWindow::builder()
        .modal(true)
        .build();

    let section = gtk::ShortcutsSection::builder()
        .section_name("main")
        .visible(true)
        .build();

    for (group_title, items) in shortcuts {
        let group = gtk::ShortcutsGroup::builder().title(*group_title).build();
        for (title, accel) in *items {
            group.add_shortcut(
                &gtk::ShortcutsShortcut::builder()
                    .title(*title)
                    .accelerator(*accel)
                    .build(),
            );
        }
        section.add_group(&group);
    }

    win.add_section(&section);
    win.set_visible(true);
}

// ---------------------------------------------------------------------------
// Command palette
// ---------------------------------------------------------------------------

/// Human-readable label for an accelerator string ("<Primary>b" → "Ctrl+B").
fn accel_display(accel: &str) -> Option<String> {
    gtk::accelerator_parse(accel).map(|(key, mods)| gtk::accelerator_get_label(key, mods).to_string())
}

/// Collect every parameterless app action as a palette entry. Actions
/// without a registered label surface as "unlabeled: app.x" — the palette
/// coverage test asserts none exist, which forces new actions to be named.
fn collect_palette_entries(app: &adw::Application) -> Vec<palette::PaletteEntry> {
    // SuiteApp's canonical actions (app.save, app.open, …) forward to these
    // per-app implementation actions; listing both would duplicate rows.
    const FORWARDING_TARGETS: &[&str] = &[
        "save-file", "save-file-as", "open-file", "new-document",
        "show-preferences", "show-shortcuts",
    ];
    let mut entries: Vec<palette::PaletteEntry> = app
        .list_actions()
        .into_iter()
        .filter(|name| app.action_parameter_type(name).is_none())
        .filter(|name| !FORWARDING_TARGETS.contains(&name.as_str()))
        .map(|name| {
            let full = format!("app.{name}");
            let label = actions::label_for(&full)
                .unwrap_or_else(|| format!("unlabeled: {full}"));
            let accel = app.accels_for_action(&full).first().and_then(|a| accel_display(a));
            palette::PaletteEntry { name: full, label, accel }
        })
        .collect();
    entries.sort_by(|a, b| a.label.cmp(&b.label));
    entries
}

/// Show the Ctrl+K command palette: a searchable list of every labeled
/// action with its shortcut. DESIGN-UI.md §surfacing — the power ceiling.
pub fn show_command_palette(app: &adw::Application) {
    let entries = collect_palette_entries(app);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Type a command…"));
    search.update_property(&[gtk4::accessible::Property::Label("Command Palette")]);
    search.set_margin_start(6);
    search.set_margin_end(6);
    search.set_margin_top(6);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("boxed-list");
    list.set_margin_start(6);
    list.set_margin_end(6);
    list.set_margin_bottom(6);
    list.update_property(&[gtk4::accessible::Property::Label("Command list")]);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_vexpand(true);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(&search);
    content.append(&scroll);

    // Chromeless palette (GNOME Text Editor / Builder idiom): the surface
    // IS the search entry plus results — no titlebar, Esc closes.
    let dialog = adw::Dialog::builder()
        .title("Command Palette")
        .content_width(480)
        .content_height(420)
        .build();
    dialog.set_child(Some(&content));

    // (Re)populate rows for a query. Row widget: label left, accel right.
    let populate = {
        let list = list.clone();
        let entries = entries.clone();
        move |query: &str| {
            while let Some(row) = list.row_at_index(0) {
                list.remove(&row);
            }
            for e in palette::filter_entries(query, &entries) {
                let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                row_box.set_margin_start(12);
                row_box.set_margin_end(12);
                row_box.set_margin_top(6);
                row_box.set_margin_bottom(6);
                let label = gtk::Label::new(Some(&e.label));
                label.set_halign(gtk::Align::Start);
                label.set_hexpand(true);
                row_box.append(&label);
                if let Some(accel) = &e.accel {
                    let al = gtk::Label::new(Some(accel));
                    al.add_css_class("dim-label");
                    al.add_css_class("caption");
                    row_box.append(&al);
                }
                let row = gtk::ListBoxRow::new();
                row.set_child(Some(&row_box));
                // Action name travels on the row for activation.
                unsafe { row.set_data("action-name", e.name.clone()) };
                list.append(&row);
            }
            if let Some(first) = list.row_at_index(0) {
                list.select_row(Some(&first));
            }
        }
    };
    populate("");

    {
        let populate = populate.clone();
        search.connect_search_changed(move |s| populate(&s.text()));
    }

    // Row activation → fire the action, close the palette.
    {
        let app = app.clone();
        let dialog = dialog.clone();
        list.connect_row_activated(move |_, row| {
            let name: Option<String> =
                unsafe { row.data::<String>("action-name").map(|p| p.as_ref().clone()) };
            if let Some(full) = name {
                dialog.close();
                if let Some(short) = full.strip_prefix("app.") {
                    app.activate_action(short, None);
                }
            }
        });
    }

    // Enter in the search entry activates the selected (or first) row.
    {
        let list = list.clone();
        search.connect_activate(move |_| {
            if let Some(row) = list.selected_row().or_else(|| list.row_at_index(0)) {
                row.activate();
            }
        });
    }

    let parent = app.active_window();
    dialog.present(parent.as_ref());
    search.grab_focus();
}

/// Show the keyboard-shortcuts dialog generated from the action label
/// registry: every labeled action that has accelerators gets a row.
/// Registering labels (which the palette needs anyway) is all an app has
/// to do for Ctrl+? to work.
pub fn show_shortcuts_from_registry(app: &adw::Application) {
    let win = gtk::ShortcutsWindow::builder().modal(true).build();
    let section = gtk::ShortcutsSection::builder()
        .section_name("main")
        .visible(true)
        .build();
    let group = gtk::ShortcutsGroup::builder().title("Actions").build();

    for entry in actions::labeled_actions() {
        let accels = app.accels_for_action(&entry.name);
        if let Some(accel) = accels.first() {
            group.add_shortcut(
                &gtk::ShortcutsShortcut::builder()
                    .title(entry.label.as_str())
                    .accelerator(accel.as_str())
                    .build(),
            );
        }
    }

    section.add_group(&group);
    win.add_section(&section);
    win.set_visible(true);
}

/// Helper to build a simple preferences dialog.
pub fn make_preferences_window() -> adw::PreferencesDialog {
    let prefs = adw::PreferencesDialog::new();
    prefs.set_search_enabled(true);
    prefs
}

/// Build a preferences page with the given title and icon.
pub fn make_preferences_page(title: &str, icon_name: &str) -> adw::PreferencesPage {
    adw::PreferencesPage::builder()
        .title(title)
        .icon_name(icon_name)
        .build()
}

/// Build a preferences group.
pub fn make_preferences_group(title: &str, description: &str) -> adw::PreferencesGroup {
    adw::PreferencesGroup::builder()
        .title(title)
        .description(description)
        .build()
}

// ---------------------------------------------------------------------------
// Empty state builder
// ---------------------------------------------------------------------------

/// Build an `AdwStatusPage` for the empty-state pattern.
/// Shown when no document is open; swapped with editor via GtkStack.
pub fn make_empty_state(
    title: &str,
    description: &str,
    icon_name: &str,
    button_label: &str,
) -> gtk::Box {
    let page = adw::StatusPage::new();
    page.set_title(title);
    page.set_description(Some(description));
    page.set_icon_name(Some(icon_name));

    let open_btn = gtk::Button::with_label(button_label);
    open_btn.add_css_class("suggested-action");
    open_btn.add_css_class("pill");
    open_btn.connect_clicked(|_| {
        // emit app.open action
    });
    page.set_child(Some(&open_btn));

    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 0);
    box_.set_halign(gtk::Align::Center);
    box_.set_valign(gtk::Align::Center);
    box_.append(&page);
    box_
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
// SuiteStatusBar — bottom status bar with word count
// ---------------------------------------------------------------------------

/// Build a status bar widget with a word count label (left) and other info.
pub fn make_status_bar() -> (gtk::Box, gtk::Label) {
    let word_count = gtk::Label::new(Some("0 words"));
    word_count.set_halign(gtk::Align::End);
    word_count.set_margin_start(6);
    word_count.set_margin_end(6);
    word_count.add_css_class("caption");

    let box_ = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    box_.add_css_class("toolbar");
    box_.append(&word_count);
    (box_, word_count)
}

// ---------------------------------------------------------------------------
// SuiteTabView — tabbed documents
// ---------------------------------------------------------------------------

/// A simple wrapper around AdwTabView for tabbed document support.
pub struct SuiteTabView {
    pub tab_view: adw::TabView,
    pub tab_bar: adw::TabBar,
}

impl SuiteTabView {
    pub fn new() -> Self {
        let tab_view = adw::TabView::new();
        let tab_bar = adw::TabBar::new();
        tab_bar.set_view(Some(&tab_view));
        SuiteTabView { tab_view, tab_bar }
    }

    /// Append a page with a title and return the page handle.
    pub fn append_page(&self, child: &impl IsA<gtk::Widget>, title: &str) -> adw::TabPage {
        let page = self.tab_view.append(child);
        page.set_title(title);
        page
    }

    /// Get the selected page.
    pub fn selected_page(&self) -> Option<adw::TabPage> {
        self.tab_view.selected_page()
    }

    /// Number of pages.
    pub fn n_pages(&self) -> i32 {
        self.tab_view.n_pages()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_state_created() {
        // Skip if GTK not available (no display server in test CI)
        if gtk4::init().is_err() || !gtk4::is_initialized() {
            eprintln!("SKIP: GTK not initialized (no display)");
            return;
        }
        let state = make_empty_state("Title", "Description", "icon", "Open");
        assert_eq!(state.width_request(), -1);
    }
}

// ---------------------------------------------------------------------------
// Cross-app clipboard (DESIGN-UI: suite fragment format)
// ---------------------------------------------------------------------------

/// Clipboard plumbing for the suite fragment format. The fragment types
/// and conversions are pure and live in the core crates; this is only
/// the GDK glue.
pub mod clipboard {
    use gtk4::prelude::*;
    use gtk4::{gdk, gio, glib};

    /// A provider offering the suite JSON, HTML, and plain text at once.
    /// Paste order of preference elsewhere: suite JSON → HTML → plain.
    pub fn provider(mime: &str, json: &str, html: &str, plain: &str) -> gdk::ContentProvider {
        gdk::ContentProvider::new_union(&[
            gdk::ContentProvider::for_bytes(mime, &glib::Bytes::from(json.as_bytes())),
            gdk::ContentProvider::for_bytes("text/html", &glib::Bytes::from(html.as_bytes())),
            gdk::ContentProvider::for_value(&glib::Value::from(plain)),
        ])
    }

    /// True when the clipboard currently offers the given MIME type.
    pub fn offers(clipboard: &gdk::Clipboard, mime: &str) -> bool {
        clipboard.formats().contain_mime_type(mime)
    }

    /// Read the given MIME type as a string, asynchronously.
    pub fn read_string<F: Fn(Option<String>) + 'static>(
        clipboard: &gdk::Clipboard,
        mime: &'static str,
        cb: F,
    ) {
        clipboard.read_async(&[mime], glib::Priority::DEFAULT, gio::Cancellable::NONE, move |res| {
            let Ok((stream, _)) = res else {
                cb(None);
                return;
            };
            let out = gio::MemoryOutputStream::new_resizable();
            let flags = gio::OutputStreamSpliceFlags::CLOSE_SOURCE
                | gio::OutputStreamSpliceFlags::CLOSE_TARGET;
            let out2 = out.clone();
            out.splice_async(
                &stream,
                flags,
                glib::Priority::DEFAULT,
                gio::Cancellable::NONE,
                move |res| {
                    if res.is_err() {
                        cb(None);
                        return;
                    }
                    let bytes = out2.steal_as_bytes();
                    cb(String::from_utf8(bytes.to_vec()).ok());
                },
            );
        });
    }
}
