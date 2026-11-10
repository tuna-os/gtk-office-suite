//! The OpenPGP page of the settings window (#133, key management): the
//! user's own keys and everyone else's, with the actions that otherwise
//! need a terminal — generate, import, export, fetch by address, trust,
//! remove. Every action is the user's own gpg on the user's own keyring
//! (see `crate::mail::pgp`); passphrases for a new key travel down a pipe to
//! gpg and are not kept.

use adw::prelude::*;
use crate::relm::prelude::*;

use crate::i18n::{i18n, i18n_f, ni18n_f};
use crate::mail::pgp::{self, Gpg, ImportSummary, KeyInfo};

mod rows;

use rows::{key_row, KeyShow};

/// What the page needs at launch: the identities keys can be made for.
pub(super) struct PgpKeysInit {
    /// (name, address) per account and alias.
    pub(super) identities: Vec<(String, String)>,
}

pub(super) struct PgpKeys {
    identities: Vec<(String, String)>,
    own: Vec<KeyInfo>,
    others: Vec<KeyInfo>,
    own_list: gtk::ListBox,
    others_list: gtk::ListBox,
    /// What the list says when nothing is in it, or nothing matches.
    others_empty: gtk::Label,
    others_query: String,
    others_show: KeyShow,
    toasts: Option<adw::ToastOverlay>,
    /// A background action (generate, fetch) is running.
    busy: bool,
    /// Bumped per reload, so a slower, older listing never lands on top
    /// of a newer one.
    reload_generation: u64,
}

#[derive(Debug)]
pub(super) enum PgpKeysInput {
    /// Re-read the keyring and rebuild both lists.
    Refresh,
    /// The search over other people's keys changed.
    SearchOthers(String),
    /// The show filter's choice, by position in `KeyShow::choices`.
    ShowOthers(u32),
    Generate,
    GenerateConfirmed {
        name: String,
        email: String,
        expire: String,
        passphrase: String,
    },
    /// Pick a key file to import.
    Import,
    ImportBytes(Vec<u8>),
    /// Save a key's public half to a file.
    Export(String),
    Delete {
        fingerprint: String,
        secret: bool,
    },
    DeleteConfirmed {
        fingerprint: String,
        secret: bool,
    },
    Fetch,
    FetchConfirmed(String),
    Trust(String),
    TrustConfirmed(String),
}

#[derive(Debug)]
pub(super) enum PgpKeysCmd {
    Generated(Result<String, String>),
    Fetched(Result<ImportSummary, String>),
    /// What an import, delete or trust has to say, if anything.
    Done(Option<String>),
    Listed {
        generation: u64,
        own: Vec<KeyInfo>,
        others: Vec<KeyInfo>,
    },
}

/// The expiry choices offered when generating a key, in combo order.
const EXPIRIES: &[(&str, &str)] = &[
    (crate::i18n::i18n_noop("2 years"), "2y"),
    (crate::i18n::i18n_noop("1 year"), "1y"),
    (crate::i18n::i18n_noop("5 years"), "5y"),
    (crate::i18n::i18n_noop("Never"), "never"),
];

/// The widgets `init` reads back out of the page it builds. Nothing in the
/// page follows the model on its own (the lists are rebuilt as keys load),
/// so there is no `update_view`; this is what `#[relm4::component]`'s
/// `view!` block used to generate.
pub(crate) struct PgpKeysWidgets {
    toasts: adw::ToastOverlay,
    status_row: adw::ActionRow,
    own_group: adw::PreferencesGroup,
    others_group: adw::PreferencesGroup,
}

/// A button in a group's header: the same 130 pixels wide as its neighbours.
fn action_button(sender: &ComponentSender<PgpKeys>, label: String, message: fn() -> PgpKeysInput) -> gtk::Button {
    let button = gtk::Button::builder().label(label).width_request(130).build();
    let sender = sender.clone();
    button.connect_clicked(move |_| sender.input(message()));
    button
}

/// The column of buttons across from a group's heading.
fn header_buttons(top: gtk::Button, bottom: gtk::Button) -> gtk::Box {
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .valign(gtk::Align::Start)
        .halign(gtk::Align::End)
        .margin_start(24)
        .build();
    column.append(&top);
    column.append(&bottom);
    column
}

fn build_widgets(
    root: &adw::ToastOverlay,
    sender: &ComponentSender<PgpKeys>,
    own_list: &gtk::ListBox,
    others_list: &gtk::ListBox,
) -> PgpKeysWidgets {
    let status_row = adw::ActionRow::builder()
        .title(i18n("GnuPG"))
        .subtitle(i18n("The same keyring the gpg command and other mail apps use."))
        .build();
    let status_group = adw::PreferencesGroup::builder().title(i18n("GnuPG")).build();
    status_group.add(&status_row);

    own_list.add_css_class("boxed-list");
    own_list.set_selection_mode(gtk::SelectionMode::None);
    let own_group = adw::PreferencesGroup::builder()
        .title(i18n("Your keys"))
        .description(i18n("Your key signs what you send and opens what is encrypted to you. Share its public half with people who write to you."))
        .header_suffix(&header_buttons(
            action_button(sender, i18n("Import…"), || PgpKeysInput::Import),
            action_button(sender, i18n("Generate…"), || PgpKeysInput::Generate),
        ))
        .build();
    own_group.add(own_list);

    let search = gtk::SearchEntry::builder()
        .hexpand(true)
        .placeholder_text(i18n("Search by name, address or key ID"))
        .build();
    {
        let sender = sender.clone();
        search.connect_search_changed(move |e| sender.input(PgpKeysInput::SearchOthers(e.text().to_string())));
    }
    let show = gtk::DropDown::builder()
        .valign(gtk::Align::Center)
        .model(&KeyShow::choices())
        .build();
    {
        let sender = sender.clone();
        show.connect_selected_notify(move |d| sender.input(PgpKeysInput::ShowOthers(d.selected())));
    }
    let filters = gtk::Box::builder().spacing(6).margin_bottom(12).build();
    filters.append(&search);
    filters.append(&show);
    others_list.add_css_class("boxed-list");
    others_list.set_selection_mode(gtk::SelectionMode::None);
    let others_group = adw::PreferencesGroup::builder()
        .title(i18n("Other people's keys"))
        .description(i18n("Keys you encrypt to and check signatures with. Check a fingerprint with its owner before trusting it."))
        .header_suffix(&header_buttons(
            action_button(sender, i18n("Import…"), || PgpKeysInput::Import),
            action_button(sender, i18n("Fetch by address…"), || PgpKeysInput::Fetch),
        ))
        .build();
    others_group.add(&filters);
    others_group.add(others_list);

    let page = adw::PreferencesPage::new();
    page.add(&status_group);
    page.add(&own_group);
    page.add(&others_group);
    root.set_child(Some(&page));

    PgpKeysWidgets { toasts: root.clone(), status_row, own_group, others_group }
}

impl Component for PgpKeys {
    type Init = PgpKeysInit;
    type Input = PgpKeysInput;
    type Output = ();
    type CommandOutput = PgpKeysCmd;
    type Root = adw::ToastOverlay;
    type Widgets = PgpKeysWidgets;

    fn init_root() -> Self::Root {
        adw::ToastOverlay::new()
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let own_list = gtk::ListBox::new();
        let others_list = gtk::ListBox::new();
        let empty_others = gtk::Label::new(Some(&i18n("No keys from other people yet.")));
        let mut model = PgpKeys {
            identities: init.identities,
            own: Vec::new(),
            others: Vec::new(),
            own_list: own_list.clone(),
            others_list: others_list.clone(),
            others_empty: empty_others.clone(),
            others_query: String::new(),
            others_show: KeyShow::All,
            toasts: None,
            busy: false,
            reload_generation: 0,
        };
        let widgets = build_widgets(&root, &sender, &own_list, &others_list);
        model.toasts = Some(widgets.toasts.clone());

        // Probing gpg runs it; done a moment after the Settings window is
        // up rather than before it can appear.
        {
            let status_row = widgets.status_row.clone();
            let own_group = widgets.own_group.clone();
            let others_group = widgets.others_group.clone();
            let s = sender.clone();
            gtk::glib::idle_add_local_full(gtk::glib::Priority::LOW, move || {
                let available = pgp::available();
                if available {
                    let version = gpg_version().unwrap_or_default();
                    status_row
                        .set_subtitle(&i18n_f("GnuPG {version} found.", &[("version", &version)]));
                } else {
                    status_row.set_subtitle(&i18n(
                        "GnuPG was not found. Install the gnupg package to read encrypted mail and check signatures.",
                    ));
                    status_row.add_css_class("error");
                }
                own_group.set_sensitive(available);
                others_group.set_sensitive(available);
                if available {
                    s.input(PgpKeysInput::Refresh);
                }
                gtk::glib::ControlFlow::Break
            });
        }

        let empty_own = gtk::Label::new(Some(&i18n(
            "No key of your own yet. Generate one for your address.",
        )));
        empty_own.add_css_class("dim-label");
        empty_own.set_margin_top(14);
        empty_own.set_margin_bottom(14);
        own_list.set_placeholder(Some(&empty_own));
        empty_others.add_css_class("dim-label");
        empty_others.set_margin_top(14);
        empty_others.set_margin_bottom(14);
        others_list.set_placeholder(Some(&empty_others));

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            PgpKeysInput::Refresh => self.reload(&sender),
            PgpKeysInput::SearchOthers(query) => {
                self.others_query = query;
                self.filter_others();
            }
            PgpKeysInput::ShowOthers(i) => {
                self.others_show = KeyShow::from_index(i);
                self.filter_others();
            }

            PgpKeysInput::Generate => self.generate_dialog(&sender),
            PgpKeysInput::GenerateConfirmed {
                name,
                email,
                expire,
                passphrase,
            } => self.on_generate_confirmed(name, email, expire, passphrase, &sender),

            PgpKeysInput::Import => self.on_import(&sender),
            PgpKeysInput::ImportBytes(bytes) => self.on_import_bytes(bytes, &sender),

            PgpKeysInput::Export(fpr) => self.on_export(fpr, &sender),

            PgpKeysInput::Delete {
                fingerprint,
                secret,
            } => self.on_delete(fingerprint, secret, &sender),
            PgpKeysInput::DeleteConfirmed {
                fingerprint,
                secret,
            } => self.on_delete_confirmed(fingerprint, secret, &sender),

            PgpKeysInput::Fetch => self.on_fetch(&sender),
            PgpKeysInput::FetchConfirmed(addr) => self.on_fetch_confirmed(addr, &sender),

            PgpKeysInput::Trust(fpr) => self.on_trust(fpr, &sender),
            PgpKeysInput::TrustConfirmed(fpr) => self.on_trust_confirmed(fpr, &sender),
        }
    }

    fn update_cmd(
        &mut self,
        message: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            PgpKeysCmd::Listed {
                generation,
                own,
                others,
            } => {
                if generation == self.reload_generation {
                    self.show_keys(own, others, &sender);
                }
                return;
            }
            PgpKeysCmd::Generated(Ok(_)) => self.toast(&i18n("Your key is ready.")),
            PgpKeysCmd::Generated(Err(e)) => {
                self.toast(&i18n_f("Could not generate the key: {e}", &[("e", &e)]))
            }
            PgpKeysCmd::Fetched(Ok(s)) => self.toast(&import_message(&s)),
            PgpKeysCmd::Fetched(Err(e)) => self.toast(&i18n_f("No key found: {e}", &[("e", &e)])),
            PgpKeysCmd::Done(text) => {
                if let Some(text) = text {
                    self.toast(&text);
                }
                self.reload(&sender);
                return;
            }
        }
        self.busy = false;
        self.reload(&sender);
    }
}

impl PgpKeys {
    fn toast(&self, text: &str) {
        if let Some(t) = &self.toasts {
            t.add_toast(adw::Toast::new(text));
        }
    }

    /// Re-read the keyring, then rebuild both lists. gpg runs on a worker
    /// thread: the hidden Settings window does this at startup, and a
    /// keyring lock left by a stale gpg held the whole app for twenty
    /// seconds (#316).
    fn reload(&mut self, sender: &ComponentSender<Self>) {
        self.reload_generation += 1;
        let generation = self.reload_generation;
        sender.oneshot_command(async move {
            let (own, others) = tokio::task::spawn_blocking(|| {
                let gpg = Gpg::system();
                let own = pgp::list_keys(&gpg, true);
                let others = pgp::list_keys(&gpg, false)
                    .into_iter()
                    .filter(|k| !own.iter().any(|o| o.fingerprint == k.fingerprint))
                    .collect();
                (own, others)
            })
            .await
            .unwrap_or_default();
            PgpKeysCmd::Listed {
                generation,
                own,
                others,
            }
        });
    }

    fn show_keys(
        &mut self,
        own: Vec<KeyInfo>,
        others: Vec<KeyInfo>,
        sender: &ComponentSender<Self>,
    ) {
        self.own = own;
        self.others = others;
        // gpg lists keys in the order they were imported; by name, a long
        // list can be scanned.
        self.others
            .sort_by_cached_key(|k| k.primary_uid().to_lowercase());
        for list in [&self.own_list, &self.others_list] {
            while let Some(row) = list.row_at_index(0) {
                list.remove(&row);
            }
        }
        for key in &self.own {
            self.own_list.append(&key_row(key, true, sender));
        }
        for key in &self.others {
            self.others_list.append(&key_row(key, false, sender));
        }
        self.filter_others();
    }

    /// Hide the rows of other people's keys the search or the show filter
    /// leaves out.
    fn filter_others(&self) {
        let mut i = 0;
        while let Some(row) = self.others_list.row_at_index(i) {
            let shown = self
                .others
                .get(i as usize)
                .is_none_or(|k| self.others_show.shows(k) && k.matches_search(&self.others_query));
            row.set_visible(shown);
            i += 1;
        }
        self.others_empty.set_label(&if self.others.is_empty() {
            i18n("No keys from other people yet.")
        } else {
            i18n("No matches")
        });
    }

    /// The form for a new key: which identity, how long it lasts, and the
    /// passphrase that protects it.
    #[allow(deprecated)] // adw::MessageDialog: AlertDialog would change how it looks
    fn generate_dialog(&self, sender: &ComponentSender<Self>) {
        let parent = crate::relm::main_application().active_window();
        let dialog = adw::MessageDialog::new(
            parent.as_ref(),
            Some(&i18n("Generate a Key")),
            Some(&i18n("This will be kept in the GnuPG keyring. The passphrase is asked for when the key is used.")),
        );
        dialog.add_response("cancel", &i18n("Cancel"));
        dialog.add_response("generate", &i18n("Generate"));
        dialog.set_response_appearance("generate", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("generate"));

        let form = gtk::ListBox::new();
        form.add_css_class("boxed-list");
        form.set_selection_mode(gtk::SelectionMode::None);

        let name_row = adw::EntryRow::new();
        name_row.set_title(&i18n("Name"));
        let email_row = adw::ComboRow::new();
        email_row.set_title(&i18n("Address"));
        let emails: Vec<String> = self.identities.iter().map(|(_, e)| e.clone()).collect();
        let email_refs: Vec<&str> = emails.iter().map(String::as_str).collect();
        email_row.set_model(Some(&gtk::StringList::new(&email_refs)));
        if let Some((name, _)) = self.identities.first() {
            name_row.set_text(name);
        }
        {
            let identities = self.identities.clone();
            let name_row = name_row.clone();
            email_row.connect_selected_notify(move |row| {
                if let Some((name, _)) = identities.get(row.selected() as usize) {
                    name_row.set_text(name);
                }
            });
        }
        let expiry_row = adw::ComboRow::new();
        expiry_row.set_title(&i18n("Expires"));
        let labels: Vec<String> = EXPIRIES.iter().map(|(l, _)| i18n(l)).collect();
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        expiry_row.set_model(Some(&gtk::StringList::new(&label_refs)));
        let pass_row = adw::PasswordEntryRow::new();
        pass_row.set_title(&i18n("Passphrase"));
        let confirm_row = adw::PasswordEntryRow::new();
        confirm_row.set_title(&i18n("Confirm passphrase"));
        for row in [
            name_row.clone().upcast::<gtk::Widget>(),
            email_row.clone().upcast(),
            expiry_row.clone().upcast(),
            pass_row.clone().upcast(),
            confirm_row.clone().upcast(),
        ] {
            form.append(&row);
        }
        dialog.set_extra_child(Some(&form));

        let s = sender.clone();
        let identities = self.identities.clone();
        dialog.connect_response(None, move |dialog, resp| {
            if resp != "generate" {
                return;
            }
            let email = identities
                .get(email_row.selected() as usize)
                .map(|(_, e)| e.clone())
                .unwrap_or_default();
            let pass = pass_row.text().to_string();
            if pass != confirm_row.text() {
                let parent = dialog.transient_for();
                crate::ui::dialog::alert(
                    parent.as_ref(),
                    &i18n("The passphrases differ"),
                    &i18n("Type the same passphrase twice."),
                );
                return;
            }
            let expire = EXPIRIES
                .get(expiry_row.selected() as usize)
                .map(|(_, v)| v.to_string())
                .unwrap_or_else(|| "2y".into());
            s.input(PgpKeysInput::GenerateConfirmed {
                name: name_row.text().to_string(),
                email,
                expire,
                passphrase: pass,
            });
        });
        dialog.present();
    }

    /// Make a key pair off the main thread.
    fn on_generate_confirmed(
        &mut self,
        name: String,
        email: String,
        expire: String,
        passphrase: String,
        sender: &ComponentSender<Self>,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.toast(&i18n("Generating the key…"));
        sender.oneshot_command(async move {
            let r = tokio::task::spawn_blocking(move || {
                pgp::generate_key(&Gpg::system(), &name, &email, &expire, &passphrase)
            })
            .await
            .unwrap_or_else(|_| Err("task failed".into()));
            PgpKeysCmd::Generated(r)
        });
    }

    /// Choose a key file to import.
    fn on_import(&mut self, sender: &ComponentSender<Self>) {
        let dialog = gtk::FileDialog::builder().title(i18n("Import Key")).build();
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&i18n("OpenPGP keys")));
        for pattern in ["*.asc", "*.gpg", "*.pgp", "*.key", "*.pub"] {
            filter.add_pattern(pattern);
        }
        let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let any = gtk::FileFilter::new();
        any.set_name(Some(&i18n("All files")));
        any.add_pattern("*");
        filters.append(&any);
        dialog.set_filters(Some(&filters));
        let parent = crate::relm::main_application().active_window();
        let s = sender.clone();
        dialog.open(parent.as_ref(), gtk::gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res {
                if let Some(path) = file.path() {
                    match std::fs::read(&path) {
                        Ok(bytes) => s.input(PgpKeysInput::ImportBytes(bytes)),
                        Err(e) => tracing::warn!("could not read key file: {e}"),
                    }
                }
            }
        });
    }

    /// Import the keys in a file's bytes.
    fn on_import_bytes(&mut self, bytes: Vec<u8>, sender: &ComponentSender<Self>) {
        sender.oneshot_command(async move {
            let r = tokio::task::spawn_blocking(move || pgp::import_keys(&Gpg::system(), &bytes))
                .await
                .unwrap_or_else(|_| Err("task failed".into()));
            PgpKeysCmd::Done(Some(match r {
                Ok(s) => import_message(&s),
                Err(e) => i18n_f("Could not import: {e}", &[("e", &e)]),
            }))
        });
    }

    /// Save a public key to a file.
    fn on_export(&mut self, fpr: String, sender: &ComponentSender<Self>) {
        let key = self
            .own
            .iter()
            .chain(self.others.iter())
            .find(|k| k.fingerprint == fpr)
            .cloned();
        let name = key
            .as_ref()
            .and_then(|k| k.emails().into_iter().next())
            .map(|e| format!("{e}-public.asc"))
            .unwrap_or_else(|| "public-key.asc".to_string());
        let dialog = gtk::FileDialog::builder()
            .title(i18n("Export Public Key"))
            .initial_name(&name)
            .build();
        let parent = crate::relm::main_application().active_window();
        let s = sender.clone();
        dialog.save(parent.as_ref(), gtk::gio::Cancellable::NONE, move |res| {
            let Some(path) = res.ok().and_then(|file| file.path()) else {
                return;
            };
            let s = s.clone();
            let fpr = fpr.clone();
            gtk::glib::spawn_future_local(async move {
                let r = gtk::gio::spawn_blocking(move || pgp::export_public(&Gpg::system(), &fpr))
                    .await;
                match r {
                    Ok(Ok(bytes)) => {
                        if let Err(e) = std::fs::write(&path, bytes) {
                            tracing::warn!("could not write key file: {e}");
                        }
                    }
                    Ok(Err(e)) => tracing::warn!("could not export key: {e}"),
                    Err(_) => tracing::warn!("could not export key: task failed"),
                }
                // Nothing to say on success beyond the file's existence.
                s.input(PgpKeysInput::Refresh);
            });
        });
    }

    /// Ask before deleting a key.
    #[allow(deprecated)] // adw::MessageDialog: AlertDialog would change how it looks
    fn on_delete(&mut self, fingerprint: String, secret: bool, sender: &ComponentSender<Self>) {
        let Some(key) = self
            .own
            .iter()
            .chain(self.others.iter())
            .find(|k| k.fingerprint == fingerprint)
            .cloned()
        else {
            return;
        };
        let parent = crate::relm::main_application().active_window();
        let (title, body) = if secret {
            (
                i18n("Delete your key?"),
                i18n_f(
                    "{uid} will be removed, secret half included. Mail encrypted to it \
                     can never be read again, unless you have a backup of the key.",
                    &[("uid", key.primary_uid())],
                ),
            )
        } else {
            (
                i18n("Remove this key?"),
                i18n_f("{uid} will be removed from your keyring. It can be imported or fetched again later.", &[("uid", key.primary_uid())]),
            )
        };
        let dialog = adw::MessageDialog::new(parent.as_ref(), Some(&title), Some(&body));
        dialog.add_response("cancel", &i18n("Cancel"));
        dialog.add_response(
            "delete",
            &if secret {
                i18n("Delete")
            } else {
                i18n("Remove")
            },
        );
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        let s = sender.clone();
        dialog.connect_response(None, move |_, resp| {
            if resp == "delete" {
                s.input(PgpKeysInput::DeleteConfirmed {
                    fingerprint: fingerprint.clone(),
                    secret,
                });
            }
        });
        dialog.present();
    }

    /// Delete a key off the main thread.
    fn on_delete_confirmed(
        &mut self,
        fingerprint: String,
        secret: bool,
        sender: &ComponentSender<Self>,
    ) {
        sender.oneshot_command(async move {
            let r = tokio::task::spawn_blocking(move || {
                pgp::delete_key(&Gpg::system(), &fingerprint, secret)
            })
            .await
            .unwrap_or_else(|_| Err("task failed".into()));
            PgpKeysCmd::Done(
                r.err()
                    .map(|e| i18n_f("Could not delete: {e}", &[("e", &e)])),
            )
        });
    }

    /// Ask which address to look a key up for.
    #[allow(deprecated)] // adw::MessageDialog: AlertDialog would change how it looks
    fn on_fetch(&mut self, sender: &ComponentSender<Self>) {
        let parent = crate::relm::main_application().active_window();
        let dialog = adw::MessageDialog::new(
            parent.as_ref(),
            Some(&i18n("Fetch a Key")),
            Some(&i18n("Looks the address up where its provider publishes keys (WKD), then on the keyservers.")),
        );
        dialog.add_response("cancel", &i18n("Cancel"));
        dialog.add_response("fetch", &i18n("Fetch"));
        dialog.set_response_appearance("fetch", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("fetch"));
        let form = gtk::ListBox::new();
        form.add_css_class("boxed-list");
        form.set_selection_mode(gtk::SelectionMode::None);
        let addr_row = adw::EntryRow::new();
        addr_row.set_title(&i18n("Email address"));
        addr_row.set_input_purpose(gtk::InputPurpose::Email);
        form.append(&addr_row);
        dialog.set_extra_child(Some(&form));
        let s = sender.clone();
        dialog.connect_response(None, move |_, resp| {
            if resp == "fetch" {
                s.input(PgpKeysInput::FetchConfirmed(
                    addr_row.text().trim().to_string(),
                ));
            }
        });
        dialog.present();
    }

    /// Look a key up off the main thread.
    fn on_fetch_confirmed(&mut self, addr: String, sender: &ComponentSender<Self>) {
        if addr.is_empty() || self.busy {
            return;
        }
        self.busy = true;
        self.toast(&i18n_f("Looking for a key for {addr}…", &[("addr", &addr)]));
        sender.oneshot_command(async move {
            let r =
                tokio::task::spawn_blocking(move || pgp::fetch_key(&Gpg::system(), &addr, None))
                    .await
                    .unwrap_or_else(|_| Err("task failed".into()));
            PgpKeysCmd::Fetched(r)
        });
    }

    /// Ask before vouching for a key.
    #[allow(deprecated)] // adw::MessageDialog: AlertDialog would change how it looks
    fn on_trust(&mut self, fpr: String, sender: &ComponentSender<Self>) {
        let Some(key) = self.others.iter().find(|k| k.fingerprint == fpr).cloned() else {
            return;
        };
        if self.own.iter().all(|k| !(k.usable() && k.can_sign)) {
            self.toast(&i18n(
                "Generate a key of your own first: trusting a key means signing it with yours.",
            ));
            return;
        }
        let parent = crate::relm::main_application().active_window();
        let dialog = adw::MessageDialog::new(
            parent.as_ref(),
            Some(&i18n("Trust this key?")),
            Some(&i18n_f(
                "Compare the fingerprint with the one {uid} gives you in person or another way. Trusting an unchecked key lets an impostor's signature pass.",
                &[("uid", key.primary_uid())],
            )),
        );
        let fpr_label = gtk::Label::new(Some(&key.fingerprint_display()));
        fpr_label.add_css_class("monospace");
        fpr_label.set_wrap(true);
        fpr_label.set_selectable(true);
        fpr_label.set_justify(gtk::Justification::Center);
        dialog.set_extra_child(Some(&fpr_label));
        dialog.add_response("cancel", &i18n("Cancel"));
        dialog.add_response("trust", &i18n("Trust"));
        dialog.set_response_appearance("trust", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("cancel"));
        let s = sender.clone();
        dialog.connect_response(None, move |_, resp| {
            if resp == "trust" {
                s.input(PgpKeysInput::TrustConfirmed(fpr.clone()));
            }
        });
        dialog.present();
    }

    /// Vouch for a key off the main thread.
    fn on_trust_confirmed(&mut self, fpr: String, sender: &ComponentSender<Self>) {
        let signer = self
            .own
            .iter()
            .find(|k| k.usable() && k.can_sign)
            .map(|k| k.fingerprint.clone());
        sender.oneshot_command(async move {
            let r = tokio::task::spawn_blocking(move || {
                pgp::trust_key(&Gpg::system(), &fpr, signer.as_deref())
            })
            .await
            .unwrap_or_else(|_| Err("task failed".into()));
            PgpKeysCmd::Done(Some(match r {
                Ok(()) => i18n("Key trusted."),
                Err(e) => i18n_f("Could not sign the key: {e}", &[("e", &e)]),
            }))
        });
    }
}

fn import_message(s: &ImportSummary) -> String {
    if s.imported > 0 {
        ni18n_f(
            "{n} key imported.",
            "{n} keys imported.",
            s.imported,
            &[("n", &s.imported.to_string())],
        )
    } else {
        i18n("That key was already in your keyring.")
    }
}

/// "2.4.9", from `gpg --version`'s first line.
fn gpg_version() -> Option<String> {
    let out = std::process::Command::new("gpg")
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .next()?
        .split_whitespace()
        .last()
        .map(str::to_string)
}
