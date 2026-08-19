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
pub use suite_common_core::{actions, palette, format, undo, events, string_pool, units, props, style, search, print, atomic_save, autosave, recent, templates, session};

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

/// Translate a user-facing string through gettext. All new UI strings
/// go through this; `scripts/update-pot.sh` extracts them.
pub fn i18n(s: &str) -> String {
    gettextrs::gettext(s)
}

/// Initialize gettext for the suite. Called once from SuiteApp::new.
/// Locale files install to <prefix>/share/locale (Flatpak: /app).
fn init_i18n() {
    use gettextrs::{bind_textdomain_codeset, bindtextdomain, setlocale, textdomain, LocaleCategory};
    // SAFETY: setlocale is safe in single-threaded startup; gettext-rs 0.8 marks it unsafe.
    unsafe { setlocale(LocaleCategory::LcAll, ""); }
    let dir = if std::path::Path::new("/app/share/locale").exists() {
        "/app/share/locale"
    } else {
        "/usr/share/locale"
    };
    let _ = bindtextdomain("gtk-office-suite", dir);
    let _ = bind_textdomain_codeset("gtk-office-suite", "UTF-8");
    let _ = textdomain("gtk-office-suite");
}

impl SuiteApp {
    /// Create a new SuiteApp with the given application id.
    /// Registers standard actions: new, open, save, save-as, preferences,
    /// about, keyboard-shortcuts, toggle-dark-mode, quit.
    pub fn new(app_id: &str) -> Self {
        init_i18n();
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

        let act_help = gio::SimpleAction::new("help", None);
        let app_weak = app.downgrade();
        act_help.connect_activate(move |_, _| {
            if let Some(app) = app_weak.upgrade() {
                let win = app.active_window().as_ref().and_then(|w| w.downcast_ref::<adw::ApplicationWindow>());
                show_help_dialog(win);
            }
        });
        app.add_action(&act_help);

        let act_new_template = gio::SimpleAction::new("new-from-template", None);
        let app_weak = app.downgrade();
        act_new_template.connect_activate(move |_, _| {
            if let Some(app) = app_weak.upgrade() {
                let app_id = app.application_id().unwrap_or_default();
                let app_name = app_id.split('.').last().unwrap_or("letters").to_string();
                let app_for_tmpl = app.clone();
                let win = app.active_window().as_ref().and_then(|w| w.downcast_ref::<adw::ApplicationWindow>());
                show_templates_dialog(win, &app_name, move |_name, _content| {
                    app_for_tmpl.activate_action("new-document", None);
                });
            }
        });
        app.add_action(&act_new_template);

        let act_clear_recent = gio::SimpleAction::new("clear-recent-files", None);
        let app_weak = app.downgrade();
        act_clear_recent.connect_activate(move |_, _| {
            if let Some(app) = app_weak.upgrade() {
                if let Some(app_id) = app.application_id() {
                    let settings = gio::Settings::new(&app_id);
                    clear_recent_files(&settings);
                }
            }
        });
        app.add_action(&act_clear_recent);

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
        app.set_accels_for_action("app.new-from-template", &["<Control><Shift>n"]);
        app.set_accels_for_action("app.open",        &["<Control>o"]);
        app.set_accels_for_action("app.save",        &["<Control>s"]);
        app.set_accels_for_action("app.save-as",     &["<Control><Shift>s"]);
        app.set_accels_for_action("app.preferences", &["<Control>comma"]);
        app.set_accels_for_action("app.shortcuts",   &["<Control>question"]);
        app.set_accels_for_action("app.help",        &["F1"]);
        app.set_accels_for_action("app.quit",        &["<Control>q"]);
        app.set_accels_for_action("app.command-palette", &["<Control>k"]);

        actions::register_labels(&[
            ("app.command-palette", "Command Palette"),
            ("app.new", "New Document"),
            ("app.new-from-template", "New from Template…"),
            ("app.open", "Open…"),
            ("app.save", "Save"),
            ("app.save-as", "Save As…"),
            ("app.preferences", "Preferences"),
            ("app.about", "About"),
            ("app.help", "Help"),
            ("app.shortcuts", "Keyboard Shortcuts"),
            ("app.clear-recent-files", "Clear Recent Files"),
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

/// Show a contextual help dialog explaining formats, interoperability, crash recovery, and shortcuts.
pub fn show_help_dialog(parent: Option<&adw::ApplicationWindow>) {
    let dialog = adw::AlertDialog::new(
        Some(&i18n("Help & System Diagnostics")),
        Some(&i18n("Overview of supported formats, crash recovery, and shortcuts.")),
    );

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(12);
    content.set_margin_bottom(12);

    let formats_group = adw::PreferencesGroup::builder()
        .title(i18n("Supported Formats & Interoperability"))
        .description(i18n("Native OpenDocument (.odt, .ods, .odp) and Microsoft Office (.docx, .xlsx, .pptx) with lossless round-tripping and Markdown/plain-text import/export."))
        .build();
    content.append(&formats_group);

    let recovery_group = adw::PreferencesGroup::builder()
        .title(i18n("Crash Recovery & Autosave"))
        .description(i18n("Documents are safely captured to atomic recovery slots in XDG state directory. If the app terminates unexpectedly, the next session will offer automatic restoration."))
        .build();
    content.append(&recovery_group);

    let shortcuts_group = adw::PreferencesGroup::builder()
        .title(i18n("Keyboard Shortcuts & Command Palette"))
        .description(i18n("Press Ctrl+K anytime to open the searchable Command Palette, or Ctrl+? for the complete Keyboard Shortcuts table."))
        .build();
    content.append(&shortcuts_group);

    dialog.set_extra_child(Some(&content));
    dialog.add_response("close", &i18n("_Close"));
    dialog.set_default_response(Some("close"));
    dialog.present(parent.map(|w| w.upcast_ref::<gtk::Widget>()));
}

/// Show a template picker dialog for creating new documents from predefined templates.
pub fn show_templates_dialog<F>(parent: Option<&adw::ApplicationWindow>, app_name: &str, on_select: F)
where
    F: Fn(&'static str, &'static str) + 'static,
{
    let templates = suite_common_core::templates::templates_for_app(app_name);
    let dialog = adw::Dialog::builder()
        .title(i18n("New from Template"))
        .content_width(520)
        .content_height(400)
        .build();

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("boxed-list");
    list.set_margin_start(12);
    list.set_margin_end(12);
    list.set_margin_top(12);
    list.set_margin_bottom(12);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_vexpand(true);

    for (idx, tmpl) in templates.iter().enumerate() {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row_box.set_margin_start(12);
        row_box.set_margin_end(12);
        row_box.set_margin_top(8);
        row_box.set_margin_bottom(8);

        let icon = gtk::Image::from_icon_name(tmpl.icon_name);
        icon.set_pixel_size(32);
        row_box.append(&icon);

        let text_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text_box.set_hexpand(true);
        let title = gtk::Label::new(Some(&i18n(tmpl.name)));
        title.set_halign(gtk::Align::Start);
        title.add_css_class("heading");
        let desc = gtk::Label::new(Some(&i18n(tmpl.description)));
        desc.set_halign(gtk::Align::Start);
        desc.add_css_class("dim-label");
        desc.add_css_class("caption");
        desc.set_wrap(true);
        text_box.append(&title);
        text_box.append(&desc);
        row_box.append(&text_box);

        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&row_box));
        unsafe { row.set_data("template-idx", idx); }
        list.append(&row);
    }

    let on_select = std::rc::Rc::new(on_select);
    let dlg = dialog.clone();
    let on_sel = on_select.clone();
    list.connect_row_activated(move |_, row| {
        let idx = unsafe { row.data::<usize>("template-idx").map(|p| *p.as_ref()) };
        if let Some(idx) = idx {
            if let Some(tmpl) = templates.get(idx) {
                dlg.close();
                on_sel(tmpl.name, tmpl.content);
            }
        }
    });

    let main_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let header = adw::HeaderBar::new();
    main_box.append(&header);
    main_box.append(&scroll);
    dialog.set_child(Some(&main_box));

    dialog.present(parent.map(|w| w.upcast_ref::<gtk::Widget>()));
}

/// Attach a file drop target to a widget to handle opening dragged files.
pub fn attach_file_drop_target<F>(widget: &impl IsA<gtk::Widget>, on_files_dropped: F) -> gtk::DropTarget
where
    F: Fn(Vec<std::path::PathBuf>) + 'static,
{
    let target = gtk::DropTarget::new(gtk4::gdk::FileList::static_type(), gtk4::gdk::DragAction::COPY);
    target.connect_drop(move |_, val, _, _| {
        if let Ok(file_list) = val.get::<gtk4::gdk::FileList>() {
            let paths: Vec<std::path::PathBuf> = file_list
                .files()
                .into_iter()
                .filter_map(|f| f.path())
                .collect();
            if !paths.is_empty() {
                on_files_dropped(paths);
                return true;
            }
        }
        false
    });
    widget.add_controller(target.clone());
    target
}

/// Read recent files from GSettings, pruning non-existent files.
pub fn get_recent_files(settings: &gio::Settings) -> Vec<String> {
    let files: Vec<String> = settings
        .strv("recent-files")
        .iter()
        .map(|s| s.to_string())
        .filter(|p| std::path::Path::new(p).exists())
        .collect();
    files
}

/// Push a file to the recent-files GSettings list (most recent first, up to 10).
pub fn push_recent_file(settings: &gio::Settings, path: &str) {
    let mut files = get_recent_files(settings);
    files.retain(|p| p != path);
    files.insert(0, path.to_string());
    files.truncate(10);
    let strv: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    let _ = settings.set_strv("recent-files", strv.as_slice());
}

/// Clear recent files in GSettings (privacy wipe).
pub fn clear_recent_files(settings: &gio::Settings) {
    let empty: [&str; 0] = [];
    let _ = settings.set_strv("recent-files", empty.as_slice());
}

/// Show a generic about dialog (apps override with their own metadata).
fn show_about_dialog() {
    let about = adw::AboutDialog::new();
    about.set_application_name(&i18n("Hanthor Office"));
    about.set_version("0.1.0");
    about.set_developer_name(&i18n("Hanthor Contributors"));
    about.set_license_type(gtk::License::Gpl30);
    about.set_comments(&i18n("A GNOME-native office suite written in Rust."));
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

/// Shared GNOME adaptive-shell contract.
pub const TOUCH_TARGET_SP: i32 = 44;
pub const NARROW_WIDTH_SP: f64 = 500.0;
pub const MEDIUM_WIDTH_SP: f64 = 800.0;
pub const WIDE_AUDIT_WIDTH_SP: f64 = 1280.0;

pub fn apply_touch_target(widget: &impl IsA<gtk::Widget>) {
    widget.set_size_request(TOUCH_TARGET_SP, TOUCH_TARGET_SP);
}

/// Read the active libadwaita accent through GTK's stable named-color API.
/// Custom Cairo renderers must not invent a second accent palette: this keeps
/// canvas selection and contextual controls aligned with the shell theme.
#[allow(deprecated)]
pub fn accent_rgb(widget: &impl IsA<gtk::Widget>) -> (f64, f64, f64) {
    use gtk::prelude::StyleContextExt;
    widget
        .style_context()
        .lookup_color("accent_bg_color")
        .map(|c| (c.red() as f64, c.green() as f64, c.blue() as f64))
        .unwrap_or((0.0, 0.5, 1.0))
}

/// Theme-aware foreground color for canvas text. Keep the fallback contrast
/// explicit for high-contrast themes where a named color may be unavailable.
pub fn canvas_foreground(is_dark: bool) -> (f64, f64, f64) {
    if is_dark { (0.94, 0.94, 0.94) } else { (0.08, 0.08, 0.08) }
}

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
            apply_touch_target(&b);
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
            .tooltip_text(i18n("More"))
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
    pub header_bar: adw::HeaderBar,
    /// Narrow breakpoint (≤ 500sp). AdwWindow applies at most one
    /// breakpoint at a time, so this one includes all medium setters
    /// too — apps add their narrow-mode setters here.
    pub narrow_breakpoint: adw::Breakpoint,
    /// Medium breakpoint (≤ 800sp) for toolbar collapse and sidebar
    /// folding. When the narrow breakpoint fires this one does not,
    /// so apps should only add medium-specific setters here.
    pub medium_breakpoint: adw::Breakpoint,
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

        // ---- Adaptive collapse (HIG breakpoints) ----
        // Medium (≤ 800sp): the extended toolbar section folds into "More".
        let medium_bp = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            MEDIUM_WIDTH_SP,
            adw::LengthUnit::Sp,
        ));
        medium_bp.add_setter(&toolbar.extended_box, "visible", Some(&false.to_value()));
        medium_bp.add_setter(&toolbar.more_button, "visible", Some(&true.to_value()));
        win.add_breakpoint(medium_bp.clone());

        // Narrow (≤ 500sp): includes all medium setters plus any
        // app-specific aggressive simplification.  Apps add their own
        // setters (hide sidebars, compact formula bar, etc.) post-construction.
        let narrow_bp = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            NARROW_WIDTH_SP,
            adw::LengthUnit::Sp,
        ));
        narrow_bp.add_setter(&toolbar.extended_box, "visible", Some(&false.to_value()));
        narrow_bp.add_setter(&toolbar.more_button, "visible", Some(&true.to_value()));
        win.add_breakpoint(narrow_bp.clone());

        // ---- Window sizing ----
        win.set_size_request(360, 300);

        SuiteWindow {
            window: win,
            toolbar_view,
            toolbar,
            header_bar,
            narrow_breakpoint: narrow_bp,
            medium_breakpoint: medium_bp,
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

/// Restore window size/maximized state from `window-width`/`window-height`/
/// `window-maximized` GSettings keys, and persist it back on every close
/// attempt (whether or not the close actually proceeds — a dirty-document
/// close guard may cancel it, but the size at that moment is still worth
/// keeping). Call this right after `SuiteWindow::new()`, before the app
/// registers its own `close-request` handler: GTK's boolean-return signal
/// accumulator stops calling further handlers once one returns "stop", so
/// this handler must run first or a close-guard Stop would skip it.
pub fn bind_window_geometry(window: &adw::ApplicationWindow, settings: &gio::Settings) {
    let w = settings.int("window-width");
    let h = settings.int("window-height");
    if w > 0 && h > 0 {
        window.set_default_size(w, h);
    }
    if settings.boolean("window-maximized") {
        window.set_maximized(true);
    }
    let s = settings.clone();
    window.connect_close_request(move |win| {
        let _ = s.set_int("window-width", win.default_width());
        let _ = s.set_int("window-height", win.default_height());
        let _ = s.set_boolean("window-maximized", win.is_maximized());
        glib::Propagation::Proceed
    });
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
        .tooltip_text(i18n("New Document"))
        .action_name("app.new")
        .build();
    new_btn.add_css_class("flat");

    // ---- End: Primary menu ----
    let menu = gio::Menu::new();

    let file_section = gio::Menu::new();
    file_section.append(Some(&i18n("_New")), Some("app.new"));
    file_section.append(Some(&i18n("New from _template\u{2026}")), Some("app.new-from-template"));
    file_section.append(Some(&i18n("_Open\u{2026}")), Some("app.open"));
    file_section.append(Some(&i18n("_Save")), Some("app.save"));
    file_section.append(Some(&i18n("Save _as\u{2026}")), Some("app.save-as"));
    file_section.append(Some(&i18n("Page set_up\u{2026}")), Some("app.page-setup"));
    file_section.append(Some(&i18n("Print pre_view\u{2026}")), Some("app.print-preview"));
    file_section.append(Some(&i18n("_Print\u{2026}")), Some("app.print"));
    menu.append_section(Some(&i18n("File")), &file_section);

    let edit_section = gio::Menu::new();
    edit_section.append(Some(&i18n("_Preferences")), Some("app.preferences"));
    menu.append_section(Some(&i18n("Edit")), &edit_section);

    let help_section = gio::Menu::new();
    help_section.append(Some(&i18n("_Help")), Some("app.help"));
    help_section.append(Some(&i18n("_Keyboard shortcuts")), Some("app.shortcuts"));
    help_section.append(Some(&i18n("_About")), Some("app.about"));
    menu.append_section(Some(&i18n("Help")), &help_section);

    let menu_btn = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text(i18n("Menu"))
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

    // Recently used actions rank first (persisted per app).
    let settings = app
        .application_id()
        .map(|id| gio::Settings::new(&id));
    let recent: Vec<String> = settings
        .as_ref()
        .map(|s| s.strv("palette-recent").iter().map(|g| g.to_string()).collect())
        .unwrap_or_default();

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some(&i18n("Type a command…")));
    search.update_property(&[gtk4::accessible::Property::Label(&i18n("Command Palette"))]);
    search.set_margin_start(6);
    search.set_margin_end(6);
    search.set_margin_top(6);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("boxed-list");
    list.set_margin_start(6);
    list.set_margin_end(6);
    list.set_margin_bottom(6);
    list.update_property(&[gtk4::accessible::Property::Label(&i18n("Command list"))]);

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
        .title(i18n("Command Palette"))
        .content_width(480)
        .content_height(420)
        .build();
    dialog.set_child(Some(&content));

    // (Re)populate rows for a query. Row widget: label left, accel right.
    let populate = {
        let list = list.clone();
        let entries = entries.clone();
        let recent = recent.clone();
        move |query: &str| {
            while let Some(row) = list.row_at_index(0) {
                list.remove(&row);
            }
            for e in palette::filter_entries_with_recency(query, &entries, &recent) {
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
                if let Some(s) = &settings {
                    let mut r: Vec<String> =
                        s.strv("palette-recent").iter().map(|g| g.to_string()).collect();
                    palette::push_recent(&mut r, &full, 8);
                    let _ = s.set_strv(
                        "palette-recent",
                        r.iter().map(|x| x.as_str()).collect::<Vec<_>>(),
                    );
                }
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
    let word_count = gtk::Label::new(Some(&i18n("0 words")));
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

impl Default for SuiteTabView {
    fn default() -> Self {
        Self::new()
    }
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
