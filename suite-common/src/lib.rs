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
pub mod undo;
pub mod format;

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
                app.activate_action("show-shortcuts", None);
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
    about.set_website("https://github.com/tuna-os/hanthor-rust");
    about.set_issue_url("https://github.com/tuna-os/hanthor-rust/issues");
    about.present(gtk::Window::NONE);
}

// ---------------------------------------------------------------------------
// SuiteToolbar — responsive formatting toolbar
// ---------------------------------------------------------------------------

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

impl SuiteToolbar {
    /// Build a responsive toolbar.
    pub fn new(
        primary: Vec<(&'static str, &'static str, Box<dyn Fn(bool)>)>,
        extended: Vec<(&'static str, &'static str, Box<dyn Fn()>)>,
    ) -> Self {
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        container.set_margin_start(6);
        container.set_margin_end(6);

        let is_empty = primary.is_empty() && extended.is_empty();
        println!("SuiteToolbar::new: primary len = {}, extended len = {}, is_empty = {}", primary.len(), extended.len(), is_empty);
        if is_empty {
            println!("SuiteToolbar::new: setting container visible to false");
            container.set_visible(false);
        } else {
            println!("SuiteToolbar::new: container remains visible (true)");
        }

        // ---- Primary section (always visible) ----
        let primary_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        primary_box.add_css_class("linked");
        for (label, tooltip, cb) in primary {
            let btn = if label.ends_with("-symbolic") {
                let b = gtk::ToggleButton::new();
                b.set_icon_name(label);
                b.set_label(&label.replace("-symbolic", "").replace("format-text-", "").replace("format-justify-", "").replace("format-list-", "").replace("insert-", ""));
                b
            } else {
                gtk::ToggleButton::with_label(label)
            };
            btn.set_tooltip_text(Some(tooltip));
            let cb = cb;
            btn.connect_toggled(move |b| {
                cb(b.is_active());
            });
            primary_box.append(&btn);
        }
        container.append(&primary_box);

        // ---- Extended section + More button ----
        let extended_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        extended_box.add_css_class("linked");
        for (label, tooltip, cb) in extended {
            let btn = if label.ends_with("-symbolic") {
                let b = gtk::Button::new();
                b.set_icon_name(label);
                b.set_label(&label.replace("-symbolic", "").replace("format-text-", "").replace("format-justify-", "").replace("format-list-", "").replace("insert-", ""));
                b
            } else {
                gtk::Button::with_label(label)
            };
            btn.set_tooltip_text(Some(tooltip));
            btn.connect_clicked(move |_| cb());
            extended_box.append(&btn);
        }

        let more_menu = gio::Menu::new();
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
        primary_toolbar: Vec<(&'static str, &'static str, Box<dyn Fn(bool)>)>,
        extended_toolbar: Vec<(&'static str, &'static str, Box<dyn Fn()>)>,
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

    // ---- End: Dark mode toggle ----
    let dark_btn = gtk::ToggleButton::builder()
        .icon_name("night-light-symbolic")
        .tooltip_text("Toggle Dark Mode")
        .build();
    dark_btn.add_css_class("flat");
    dark_btn.connect_toggled(|btn| {
        let sm = adw::StyleManager::default();
        if btn.is_active() {
            sm.set_color_scheme(adw::ColorScheme::ForceDark);
        } else {
            sm.set_color_scheme(adw::ColorScheme::Default);
        }
    });

    // ---- End: Primary menu ----
    let menu = gio::Menu::new();

    let file_section = gio::Menu::new();
    file_section.append(Some("_New"), Some("app.new"));
    file_section.append(Some("_Open\u{2026}"), Some("app.open"));
    file_section.append(Some("_Save"), Some("app.save"));
    file_section.append(Some("Save _As\u{2026}"), Some("app.save-as"));
    file_section.append(Some("Page Set_up\u{2026}"), Some("app.page-setup"));
    file_section.append(Some("Print Pre_view\u{2026}"), Some("app.print-preview"));
    file_section.append(Some("_Print\u{2026}"), Some("app.print"));
    menu.append_section(Some("File"), &file_section);

    let edit_section = gio::Menu::new();
    edit_section.append(Some("_Preferences"), Some("app.preferences"));
    menu.append_section(Some("Edit"), &edit_section);

    let help_section = gio::Menu::new();
    help_section.append(Some("_Keyboard Shortcuts"), Some("app.shortcuts"));
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
    hb.pack_end(&dark_btn);
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
