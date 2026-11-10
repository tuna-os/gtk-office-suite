//! Settings → Cloud Storage (#144): the Nextcloud, Dropbox and Seafile
//! accounts that "Upload to cloud" in the composer can put files on. Each
//! row is an account; the editor is a dialog whose fields follow the kind,
//! with a connection check (a browser sign-in, for Dropbox).

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use crate::relm::prelude::*;

use crate::i18n::{i18n, i18n_f};
use crate::services::cloud::{self, CloudAccount, CloudKind};

mod editor;

use editor::build_editor;

/// How a kind is named in the editor and the list.
pub(super) struct CloudAccounts {
    accounts: Vec<CloudAccount>,
    /// Keyring keys of the servers already asked which product they are
    /// (accounts from before the picker told them apart), so a list
    /// rebuild does not ask again.
    probed: std::collections::HashSet<String>,
    list: gtk::ListBox,
    toasts: Option<adw::ToastOverlay>,
    nav: Option<adw::NavigationView>,
    editor_page: Option<adw::NavigationPage>,
    editor_slot: Option<adw::Bin>,
    /// The editor header's Remove, shown while an existing account is up.
    remove_btn: Option<gtk::Button>,
    /// Which account the editor shows (none for a new one).
    editing: Option<usize>,
    /// What the editor's Save does, while one is up: reads the form and
    /// sends `Save` (or starts the sign-in that will); answers whether it
    /// accepted the form.
    save_action: SaveAction,
}

/// The open editor's Save (see `CloudAccounts::save_action`).
type SaveAction = Rc<RefCell<Option<Rc<dyn Fn() -> bool>>>>;

#[derive(Debug)]
pub(super) enum CloudAccountsInput {
    Add,
    /// The editor for a new account of that kind.
    AddOf(CloudKind),
    Edit(usize),
    /// The editor header's Remove: ask about the account being edited.
    RemoveCurrent,
    /// Ask before removing that account.
    ConfirmRemove(usize),
    /// A row's switch: keep the account, but offer it (or not) in the
    /// composer.
    ToggleEnabled {
        index: usize,
        enabled: bool,
    },
    Remove(usize),
    /// The editor page's Save button.
    SaveClicked,
    /// Leave the editor without saving (the settings window asks to, when
    /// the user moves on).
    CloseEditor,
    /// The editor's Save: `index` is the row being replaced, or none for a
    /// new account. The password is stored only when given. Boxed: an
    /// account is far larger than any other message.
    Save {
        index: Option<usize>,
        account: Box<CloudAccount>,
        password: String,
    },
    /// A sign-in the editor finished after closing went wrong.
    Failed(String),
    /// A server said which product it is (`cloud::detect_product`), for
    /// the account with that keyring key.
    ProductDetected {
        key: String,
        product: String,
    },
}

#[derive(Debug)]
pub(super) enum CloudAccountsOutput {
    /// The editor page is up (or gone): the settings window hides its
    /// shared header meanwhile and asks before moving on.
    EditorOpen(bool),
}

/// The widgets `init` reads back out of the page it builds. Nothing in the
/// page follows the model on its own (the list is rebuilt by `rebuild`), so
/// there is no `update_view`; this is what `#[relm4::component]`'s `view!`
/// block used to generate.
pub(crate) struct CloudAccountsWidgets {
    nav: adw::NavigationView,
    toasts: adw::ToastOverlay,
    brands: gtk::Box,
    list: gtk::ListBox,
    empty: gtk::Label,
    editor_page: adw::NavigationPage,
    remove_btn: gtk::Button,
    editor_slot: adw::Bin,
}

fn build_widgets(root: &adw::Bin, sender: &ComponentSender<CloudAccounts>) -> CloudAccountsWidgets {
    // ---- list page ----
    // Across from the heading: the services' marks (picker order), with Add
    // Account under them.
    let brands = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::End)
        .build();
    let add = gtk::Button::builder().label(i18n("Add Account…")).halign(gtk::Align::End).build();
    {
        let sender = sender.clone();
        add.connect_clicked(move |_| sender.input(CloudAccountsInput::Add));
    }
    let suffix = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(20)
        .valign(gtk::Align::Start)
        .margin_start(24)
        .build();
    suffix.append(&brands);
    suffix.append(&add);

    // A row opens its editor, as the Mail Accounts list does.
    let list = gtk::ListBox::builder()
        .css_classes(["boxed-list"])
        .selection_mode(gtk::SelectionMode::None)
        .margin_top(16)
        .build();
    {
        let sender = sender.clone();
        list.connect_row_activated(move |_, row| {
            sender.input(CloudAccountsInput::Edit(row.index() as usize));
        });
    }
    let empty = gtk::Label::builder()
        .label(i18n("No cloud accounts yet."))
        .css_classes(["dim-label"])
        .margin_top(12)
        .build();

    let group = adw::PreferencesGroup::builder()
        .title(i18n("Cloud storage"))
        .description(i18n(
            "Upload large files and send a link instead of attaching them.\n\nSupported providers: Nextcloud, ownCloud, OpenCloud, OneDrive, Dropbox and Seafile.",
        ))
        .header_suffix(&suffix)
        .build();
    group.add(&list);
    group.add(&empty);
    let prefs = adw::PreferencesPage::new();
    prefs.add(&group);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&prefs));
    let list_toolbar = adw::ToolbarView::new();
    list_toolbar.set_content(Some(&toasts));
    let list_page = adw::NavigationPage::builder()
        .title(i18n("Cloud Storage"))
        .tag("list")
        .child(&list_toolbar)
        .build();

    // ---- editor page ----
    // The window's close button lives in this header while the editor is up
    // (the shared header hides).
    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(true);
    let save = gtk::Button::builder()
        .label(i18n("Save"))
        .css_classes(["suggested-action"])
        .build();
    {
        let sender = sender.clone();
        save.connect_clicked(move |_| sender.input(CloudAccountsInput::SaveClicked));
    }
    header.pack_end(&save);
    // Left of Save, only while editing an existing account; asks before
    // removing.
    let remove_btn = gtk::Button::builder()
        .label(i18n("Remove"))
        .css_classes(["destructive-action"])
        .visible(false)
        .build();
    {
        let sender = sender.clone();
        remove_btn.connect_clicked(move |_| sender.input(CloudAccountsInput::RemoveCurrent));
    }
    header.pack_end(&remove_btn);

    let editor_slot = adw::Bin::new();
    let clamp = adw::Clamp::builder()
        .maximum_size(640)
        .tightening_threshold(480)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .child(&editor_slot)
        .build();
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp)
        .build();
    let editor_toolbar = adw::ToolbarView::new();
    editor_toolbar.add_top_bar(&header);
    editor_toolbar.set_content(Some(&scrolled));
    let editor_page = adw::NavigationPage::builder()
        .title(i18n("Cloud Account"))
        .tag("editor")
        .child(&editor_toolbar)
        .build();

    let nav = adw::NavigationView::new();
    nav.add(&list_page);
    nav.add(&editor_page);
    root.set_child(Some(&nav));

    CloudAccountsWidgets { nav, toasts, brands, list, empty, editor_page, remove_btn, editor_slot }
}

impl SimpleComponent for CloudAccounts {
    type Init = ();
    type Input = CloudAccountsInput;
    type Output = CloudAccountsOutput;
    type Root = adw::Bin;
    type Widgets = CloudAccountsWidgets;

    fn init_root() -> Self::Root {
        adw::Bin::new()
    }

    fn init(_init: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let widgets = build_widgets(&root, &sender);
        for service in cloud::SERVICES.iter() {
            let mark = crate::desktop::brand::image(service.brand(), 22);
            mark.set_tooltip_text(Some(service.name));
            widgets.brands.append(&mark);
        }
        let mut model = CloudAccounts {
            accounts: cloud::load_accounts(),
            probed: std::collections::HashSet::new(),
            list: widgets.list.clone(),
            toasts: Some(widgets.toasts.clone()),
            nav: Some(widgets.nav.clone()),
            editor_page: Some(widgets.editor_page.clone()),
            editor_slot: Some(widgets.editor_slot.clone()),
            remove_btn: Some(widgets.remove_btn.clone()),
            editing: None,
            save_action: Rc::new(RefCell::new(None)),
        };
        model.rebuild(&sender);
        widgets.empty.set_visible(model.accounts.is_empty());
        // Tell the settings window when the editor page is up, every way
        // in or out (Save, the back button, a swipe).
        {
            let s = sender.output_sender().clone();
            let save_action = model.save_action.clone();
            let slot = widgets.editor_slot.clone();
            widgets.nav.connect_visible_page_notify(move |nav| {
                let editor = nav
                    .visible_page()
                    .and_then(|p| p.tag())
                    .is_some_and(|t| t == "editor");
                if !editor {
                    // The form is done with: drop it and what Save would do.
                    *save_action.borrow_mut() = None;
                    slot.set_child(None::<&gtk::Widget>);
                }
                let _ = s.send(CloudAccountsOutput::EditorOpen(editor));
            });
        }
        // HYLKI_SHOWCASE_EDIT_CLOUD=<index> opens that account's editor
        // for a capture (demo only); "add", "add:dropbox", "add:seafile"
        // or "add:onedrive" opens the Add page on that kind.
        if let Ok(what) = std::env::var("HYLKI_SHOWCASE_EDIT_CLOUD") {
            if std::env::var_os("HYLKI_DEMO").is_some() {
                let s = sender.input_sender().clone();
                gtk::glib::timeout_add_seconds_local_once(2, move || {
                    let _ = s.send(match what.as_str() {
                        "add" | "add:nextcloud" => CloudAccountsInput::AddOf(CloudKind::Nextcloud),
                        "add:dropbox" => CloudAccountsInput::AddOf(CloudKind::Dropbox),
                        "add:seafile" => CloudAccountsInput::AddOf(CloudKind::Seafile),
                        "add:onedrive" => CloudAccountsInput::AddOf(CloudKind::OneDrive),
                        i => CloudAccountsInput::Edit(i.parse().unwrap_or(0)),
                    });
                });
            }
        }
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            CloudAccountsInput::Add => {
                sender.input(CloudAccountsInput::AddOf(CloudKind::Nextcloud))
            }
            CloudAccountsInput::AddOf(kind) => {
                let mut a = CloudAccount::empty();
                a.kind = kind;
                self.open_editor(None, a, &sender);
            }
            CloudAccountsInput::Edit(i) => {
                if let Some(a) = self.accounts.get(i).cloned() {
                    self.open_editor(Some(i), a, &sender);
                }
            }
            CloudAccountsInput::ToggleEnabled { index, enabled } => {
                if let Some(a) = self.accounts.get_mut(index) {
                    if a.enabled != enabled {
                        a.enabled = enabled;
                        cloud::save_accounts(&self.accounts);
                    }
                }
            }
            CloudAccountsInput::RemoveCurrent => {
                if let Some(i) = self.editing {
                    sender.input(CloudAccountsInput::ConfirmRemove(i));
                }
            }
            CloudAccountsInput::ConfirmRemove(i) => {
                let Some(a) = self.accounts.get(i) else {
                    return;
                };
                let name = if a.name.trim().is_empty() {
                    a.where_shown()
                } else {
                    a.name.clone()
                };
                let parent = crate::relm::main_application().active_window();
                let s = sender.input_sender().clone();
                crate::ui::dialog::confirm_destructive(
                    parent.as_ref(),
                    &i18n_f("Remove {name}?", &[("name", &name)]),
                    &i18n("Files already uploaded and links sent stay active on the provider's service."),
                    "remove",
                    &i18n("Remove"),
                    move || {
                        let _ = s.send(CloudAccountsInput::Remove(i));
                    },
                );
            }
            CloudAccountsInput::Remove(i) => {
                if i < self.accounts.len() {
                    if self.editing == Some(i) {
                        self.editing = None;
                        self.close_editor();
                    }
                    let a = self.accounts.remove(i);
                    let key = a.key();
                    crate::config::keyring_later(move || {
                        crate::config::delete_cloud_password(&key)
                    });
                    cloud::save_accounts(&self.accounts);
                    self.rebuild(&sender);
                    self.toast(&i18n_f("Removed {name}", &[("name", &a.name)]));
                }
            }
            CloudAccountsInput::SaveClicked => {
                let action = self.save_action.borrow().clone();
                if let Some(f) = action {
                    if f() {
                        self.close_editor();
                    }
                }
            }
            CloudAccountsInput::CloseEditor => self.close_editor(),
            CloudAccountsInput::Failed(e) => self.toast(&e),
            CloudAccountsInput::ProductDetected { key, product } => {
                let mut changed = false;
                for a in self
                    .accounts
                    .iter_mut()
                    .filter(|a| a.key() == key && a.product.is_empty())
                {
                    a.product = product.clone();
                    changed = true;
                }
                if changed {
                    cloud::save_accounts(&self.accounts);
                    self.rebuild(&sender);
                }
            }
            CloudAccountsInput::Save {
                index,
                account,
                password,
            } => {
                let account = *account;
                // On the keyring thread, in order: the new entry, then the
                // old one dropped.
                if !password.is_empty() && account.has_secret() {
                    let key = account.key();
                    let s = sender.input_sender().clone();
                    crate::config::keyring_then(
                        move || {
                            crate::config::store_cloud_password(&key, &password)
                                .map_err(|e| e.to_string())
                        },
                        move |stored| {
                            if let Err(e) = stored {
                                s.emit(CloudAccountsInput::Failed(i18n_f(
                                    "Could not store the sign-in in the keyring: {e}",
                                    &[("e", &e)],
                                )));
                            }
                        },
                    );
                }
                match index {
                    Some(i) if i < self.accounts.len() => {
                        // A changed server or user name moves the keyring
                        // entry: drop the old one.
                        if self.accounts[i].key() != account.key() {
                            let old = self.accounts[i].key();
                            crate::config::keyring_later(move || {
                                crate::config::delete_cloud_password(&old)
                            });
                        }
                        self.accounts[i] = account;
                    }
                    _ => self.accounts.push(account),
                }
                cloud::save_accounts(&self.accounts);
                self.rebuild(&sender);
            }
        }
    }
}

impl CloudAccounts {
    fn toast(&self, text: &str) {
        if let Some(t) = &self.toasts {
            t.add_toast(adw::Toast::new(text));
        }
    }

    /// Slide the editor page in with the form for `account`.
    fn open_editor(
        &mut self,
        index: Option<usize>,
        account: CloudAccount,
        sender: &ComponentSender<Self>,
    ) {
        let (Some(nav), Some(page), Some(slot)) = (&self.nav, &self.editor_page, &self.editor_slot)
        else {
            return;
        };
        page.set_title(&if index.is_some() {
            i18n("Edit Cloud Account")
        } else {
            i18n("Add Cloud Account")
        });
        self.editing = index;
        if let Some(b) = &self.remove_btn {
            b.set_visible(index.is_some());
        }
        let (form, save) = build_editor(index, account, sender.input_sender().clone());
        slot.set_child(Some(&form));
        *self.save_action.borrow_mut() = Some(save);
        if nav
            .visible_page()
            .and_then(|p| p.tag())
            .is_some_and(|t| t == "editor")
        {
            return;
        }
        nav.push_by_tag("editor");
    }

    fn close_editor(&self) {
        if let Some(nav) = &self.nav {
            if nav
                .visible_page()
                .and_then(|p| p.tag())
                .is_some_and(|t| t == "editor")
            {
                nav.pop();
            }
        }
    }

    fn rebuild(&mut self, sender: &ComponentSender<Self>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for (i, a) in self.accounts.iter().enumerate() {
            // The same card as a Mail Accounts row: mark, name over
            // details, then a chevron; the row itself opens the editor.
            let row = gtk::ListBoxRow::new();
            row.set_activatable(true);
            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            hbox.add_css_class("account-list-row");
            hbox.append(&crate::desktop::brand::image(a.brand(), 28));
            let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
            vbox.set_hexpand(true);
            vbox.set_valign(gtk::Align::Center);
            let title = gtk::Label::new(Some(&if a.name.trim().is_empty() {
                a.where_shown()
            } else {
                a.name.clone()
            }));
            title.set_halign(gtk::Align::Start);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            title.add_css_class("account-name");
            let mut sub = format!("{} · {}", a.where_shown(), a.user);
            if a.expire_days > 0 {
                sub.push_str(&format!(
                    " · {}",
                    i18n_f(
                        "links expire after {n} days",
                        &[("n", &a.expire_days.to_string())]
                    )
                ));
            }
            if a.password {
                sub.push_str(&format!(" · {}", i18n("password-protected")));
            }
            let subtitle = gtk::Label::new(Some(&sub));
            subtitle.set_halign(gtk::Align::Start);
            subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
            subtitle.add_css_class("account-email");
            vbox.append(&title);
            vbox.append(&subtitle);
            hbox.append(&vbox);
            // A server from before the picker told Nextcloud, ownCloud and
            // OpenCloud apart: ask it once, in the background, and fill in
            // its mark when it answers.
            if a.kind == CloudKind::Nextcloud && a.product.is_empty() && self.probed.insert(a.key())
            {
                let (key, base) = (a.key(), a.base());
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    if let Some(product) = cloud::detect_product(&base) {
                        let _ = s.send(CloudAccountsInput::ProductDetected { key, product });
                    }
                });
            }
            // On/off without removing, as a mail account's row has.
            let toggle = gtk::Switch::new();
            toggle.set_valign(gtk::Align::Center);
            toggle.set_tooltip_text(Some(&i18n("Offer this account in the composer")));
            toggle.set_active(a.enabled);
            let s = sender.input_sender().clone();
            toggle.connect_state_set(move |_, state| {
                let _ = s.send(CloudAccountsInput::ToggleEnabled {
                    index: i,
                    enabled: state,
                });
                gtk::glib::Propagation::Proceed
            });
            hbox.append(&toggle);
            let next = gtk::Image::from_icon_name("go-next-symbolic");
            next.add_css_class("dim-label");
            hbox.append(&next);
            row.set_child(Some(&hbox));
            self.list.append(&row);
        }
        self.list.set_visible(!self.accounts.is_empty());
        if let Some(next) = self.list.next_sibling() {
            next.set_visible(self.accounts.is_empty());
        }
    }
}
