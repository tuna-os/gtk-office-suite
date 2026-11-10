//! Compose window: write a new message, reply, or forward.

use adw::prelude::*;
use relm4::prelude::*;

use crate::config::{ComposeFormat, SignaturePosition};
use crate::i18n::{i18n, i18n_f};
use crate::ui::recipient_row::RecipientRow;
use crate::ui::rich_editor::{RichEditor, SourceKind};

mod assistant;
mod assistant_edit;
mod attachment_check;
mod attachments;
mod cloud;
mod format;
mod history;
mod host;
mod keys;
mod merge;
mod merge_fields;
mod messages;
mod model;
mod send;
mod send_menu;
mod setup;
mod showcase;
mod signature;
mod subject;
mod suggest;
#[cfg(test)]
mod tests;
mod text_diff;
mod translate;
mod tuta;
mod types;

use cloud::not_signed_in;
pub(crate) use format::forget_zoom;
use format::{markdown_body, session_zoom, zoom_chip, SHOWCASE_PLAIN_LOG};
use host::{fold_on_first_map, pgp_tooltip, set_toggle_icon, size_for_host, COMPOSE_MIN_WIDTH};
pub(crate) use keys::composer_key;
pub(crate) use messages::{ComposeInput, ComposeOutput};
pub(crate) use model::Compose;
use send::{pick_send_time, start_autosave};
use setup::{add_toolbar_end_buttons, connect_editor};
use showcase::showcase_hooks;
use signature::sig_html;
use subject::wire_subject;
use suggest::{load_suggestions, wire_recipient_rows};
pub(crate) use types::{ComposeAccount, ComposeInit, ComposePrefill};

/// The composer's widgets: the ones the rest of the module reaches by field,
/// and the ones that follow the model, which `apply` sets from it at build and
/// after every message. This is what `#[relm4::component]`'s `view!` block and
/// its `#[watch]` attributes used to generate.
pub(crate) struct ComposeWidgets {
    drop_overlay: gtk::Overlay,
    toolbar_root: adw::ToolbarView,
    header: adw::HeaderBar,
    delete_draft_btn: gtk::Button,
    send_btn: gtk::Button,
    overflow_btn: gtk::Button,
    confidential_btn: gtk::ToggleButton,
    encrypt_btn: gtk::ToggleButton,
    sign_btn: gtk::ToggleButton,
    attach_btn: gtk::Button,
    cloud_btn: gtk::Button,
    contacts_btn: gtk::Button,
    toggle_btn: gtk::Button,
    fields_btn: gtk::ToggleButton,
    schedule_bar: gtk::Box,
    schedule_label: gtk::Label,
    password_bar: gtk::Box,
    password_label: gtk::Label,
    fields_list: gtk::ListBox,
    from_row: adw::ComboRow,
    custom_from_row: adw::EntryRow,
    to_row: RecipientRow,
    cc_row: RecipientRow,
    bcc_row: RecipientRow,
    reply_to_row: adw::EntryRow,
    subject_row: adw::EntryRow,
    attach_box: gtk::FlowBox,
    editor_holder: gtk::Box,
}

impl ComposeWidgets {
    /// Everything that was `#[watch]`ed: set from the model.
    fn apply(&self, model: &Compose) {
        self.delete_draft_btn
            .set_visible(model.draft_origin.is_some());
        self.send_btn.set_label(&if model.send_at.is_some() {
            i18n("Schedule")
        } else {
            i18n("Send")
        });
        self.overflow_btn.set_visible(model.narrow);

        self.confidential_btn
            .set_icon_name(tuta::lock_icon(model.tuta.confidential));
        self.confidential_btn
            .set_tooltip_text(Some(&tuta::confidential_tooltip(model.tuta.confidential)));
        self.confidential_btn
            .set_visible(model.tuta.active && !model.narrow);

        let pgp = crate::mail::pgp::available() && !model.narrow;
        self.encrypt_btn.set_tooltip_text(Some(&pgp_tooltip(
            &i18n("Encrypt with OpenPGP to every recipient's key"),
            model.encrypt,
        )));
        self.encrypt_btn.set_visible(pgp);
        self.sign_btn.set_tooltip_text(Some(&pgp_tooltip(
            &i18n("Sign with your OpenPGP key"),
            model.sign,
        )));
        self.sign_btn.set_visible(pgp);

        self.attach_btn.set_visible(!model.narrow);
        self.cloud_btn
            .set_visible(!model.cloud_accounts.is_empty() && !model.narrow);
        self.cloud_btn.set_sensitive(model.cloud_busy == 0);
        self.contacts_btn.set_visible(!model.narrow);
        self.toggle_btn
            .set_visible(model.can_toggle && !model.narrow);
        self.fields_btn
            .set_visible(model.compact && !model.windowed);

        self.schedule_bar.set_visible(model.send_at.is_some());
        self.schedule_label.set_label(
            &model
                .send_at
                .map(|t| {
                    i18n_f(
                        "Scheduled for {when}",
                        &[("when", &crate::datefmt::date_time(t))],
                    )
                })
                .unwrap_or_default(),
        );
        self.password_bar
            .set_visible(!model.cloud_passwords.is_empty());
        self.password_label.set_label(
            &model
                .cloud_passwords
                .iter()
                .map(|(n, p)| {
                    i18n_f(
                        "Download password for {name}: {password}",
                        &[("name", n), ("password", p)],
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
}

/// A flat entry of a context menu that closes its popover, then sends `input`.
fn menu_item(
    sender: &ComponentSender<Compose>,
    label: &str,
    tooltip: &str,
    input: fn() -> ComposeInput,
) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class("flat");
    b.add_css_class("context-menu-item");
    b.set_halign(gtk::Align::Fill);
    b.set_tooltip_text(Some(tooltip));
    let l = gtk::Label::new(Some(label));
    l.set_halign(gtk::Align::Start);
    b.set_child(Some(&l));
    let tx = sender.input_sender().clone();
    b.connect_clicked(move |b| {
        if let Some(p) = b
            .ancestor(gtk::Popover::static_type())
            .and_downcast::<gtk::Popover>()
        {
            p.popdown();
        }
        tx.emit(input());
    });
    b
}

/// A plain button sending `input` when clicked.
fn action_button(
    sender: &ComponentSender<Compose>,
    icon: &str,
    tooltip: &str,
    input: fn() -> ComposeInput,
) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tooltip));
    let tx = sender.input_sender().clone();
    b.connect_clicked(move |_| tx.emit(input()));
    b
}

/// A header-bar toggle sending `input(active)` when toggled.
fn header_toggle(
    sender: &ComponentSender<Compose>,
    icon: &str,
    input: fn(bool) -> ComposeInput,
) -> gtk::ToggleButton {
    let b = gtk::ToggleButton::new();
    b.set_icon_name(icon);
    let tx = sender.input_sender().clone();
    b.connect_toggled(move |b| tx.emit(input(b.is_active())));
    b
}

/// A bar of `icon`, then `label`, then `trailing` (the schedule and
/// password bars under the header).
fn info_bar(icon: &str, label: &gtk::Label, trailing: &gtk::Button) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bar.set_margin_start(12);
    bar.set_margin_end(12);
    bar.set_margin_top(4);
    bar.set_margin_bottom(4);
    bar.append(&gtk::Image::from_icon_name(icon));
    label.set_hexpand(true);
    label.set_halign(gtk::Align::Start);
    bar.append(label);
    bar.append(trailing);
    bar
}

/// Builds the widget tree under `root`.
fn build_widgets(
    root: &adw::BreakpointBin,
    sender: &ComponentSender<Compose>,
    model: &Compose,
    send_menu: &gtk::Popover,
) -> ComposeWidgets {
    // The floor the pane can shrink to (inline in a narrow reader, or a small
    // window): below the measured full-toolbar width the breakpoint set in
    // `init` folds the actions into the ⋯ menu.
    root.set_size_request(COMPOSE_MIN_WIDTH, 200);

    // The overlay carries the drop surfaces shown while files are dragged over
    // the composer (drop_zones.rs).
    let drop_overlay = gtk::Overlay::new();
    root.set_child(Some(&drop_overlay));
    let toolbar_root = adw::ToolbarView::new();
    drop_overlay.set_child(Some(&toolbar_root));

    let header = adw::HeaderBar::new();
    header.add_css_class("compose-toolbar");
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    // No "Hylki" branding on the compose bar.
    header.set_title_widget(Some(&gtk::Label::new(Some(""))));
    toolbar_root.add_top_bar(&header);

    let cancel = gtk::Button::with_label(&i18n("Cancel"));
    let tx = sender.input_sender().clone();
    cancel.connect_clicked(move |_| tx.emit(ComposeInput::Cancel));
    header.pack_start(&cancel);

    // Save Draft stays, label and all, however narrow the pane: it is the one
    // action worth a click in a hurry. Save as Template (#360) beside it, as
    // Send Later is beside Send; a template being edited has the one way to
    // be saved.
    let save_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    save_box.add_css_class("linked");
    let save_btn = gtk::Button::with_label(&if model.template {
        i18n("Save Template")
    } else {
        i18n("Save Draft")
    });
    save_btn.set_tooltip_text(Some(&if model.template {
        i18n("Save to Templates")
    } else {
        i18n("Save to Drafts")
    }));
    let tx = sender.input_sender().clone();
    save_btn.connect_clicked(move |_| tx.emit(ComposeInput::SaveDraft));
    save_box.append(&save_btn);
    let save_more = gtk::MenuButton::new();
    save_more.set_icon_name("pan-down-symbolic");
    save_more.set_tooltip_text(Some(&i18n("More ways to save")));
    save_more.set_can_focus(false);
    save_more.set_visible(!model.template);
    let save_popover = gtk::Popover::new();
    let save_list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    save_list.add_css_class("context-menu-list");
    save_list.append(&menu_item(
        sender,
        &i18n("Save as Template"),
        &i18n("Keep a copy in Templates to start new messages from"),
        || ComposeInput::SaveAsTemplate,
    ));
    // Only while editing an existing draft: the message is moved to Trash, not
    // saved, and the editor closes.
    let delete_draft_btn = menu_item(
        sender,
        &i18n("Delete Draft"),
        &i18n("Move this draft to Trash"),
        || ComposeInput::DeleteDraft,
    );
    save_list.append(&delete_draft_btn);
    save_popover.set_child(Some(&save_list));
    save_more.set_popover(Some(&save_popover));
    save_box.append(&save_more);
    header.pack_start(&save_box);

    // Send, with Send Later beside it (#145): presets, or a date and time of
    // your own.
    let send_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    send_box.add_css_class("linked");
    send_box.add_css_class("send-split");
    let send_btn = gtk::Button::new();
    send_btn.add_css_class("suggested-action");
    let tx = sender.input_sender().clone();
    send_btn.connect_clicked(move |_| tx.emit(ComposeInput::Send));
    send_box.append(&send_btn);
    // A floating divider, not a seam: the box paints the accent behind it so
    // the two read as one control.
    send_box.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    let send_options = gtk::MenuButton::new();
    send_options.set_icon_name("pan-down-symbolic");
    send_options.add_css_class("suggested-action");
    send_options.set_tooltip_text(Some(&i18n("Send options")));
    send_options.set_can_focus(false);
    send_options.set_popover(Some(send_menu));
    send_box.append(&send_options);
    header.pack_end(&send_box);

    // The folded toolbar (narrow pane): every action that is not Cancel or
    // Send lives in this menu.
    let overflow_btn = action_button(
        sender,
        "view-more-horizontal-symbolic",
        &i18n("Actions"),
        || ComposeInput::OverflowMenu,
    );
    header.pack_end(&overflow_btn);

    // Tuta's Confidential (a Tuta From only), a lock as in Tuta's own app:
    // closed while it is on.
    let confidential_btn = header_toggle(sender, "changes-allow-symbolic", |on| {
        ComposeInput::ToggleConfidential(on)
    });
    confidential_btn.add_css_class("pgp-toggle");
    header.pack_end(&confidential_btn);

    // OpenPGP (#133): only offered where a gpg exists.
    let encrypt_btn = header_toggle(
        sender,
        "channel-secure-symbolic",
        ComposeInput::ToggleEncrypt,
    );
    encrypt_btn.add_css_class("pgp-toggle");
    header.pack_end(&encrypt_btn);
    let sign_btn = header_toggle(sender, "security-high-symbolic", ComposeInput::ToggleSign);
    sign_btn.add_css_class("pgp-toggle");
    header.pack_end(&sign_btn);

    let attach_btn = action_button(
        sender,
        "mail-attachment-symbolic",
        &i18n("Attach files"),
        || ComposeInput::AttachFiles,
    );
    header.pack_end(&attach_btn);
    // Cloud attachments (#144): only with an account set up.
    let cloud_btn = action_button(
        sender,
        "cloud-symbolic",
        &i18n("Upload to cloud storage and share a link"),
        || ComposeInput::CloudAttach,
    );
    header.pack_end(&cloud_btn);
    let contacts_btn = action_button(
        sender,
        "x-office-address-book-symbolic",
        &i18n("Open Contacts"),
        || ComposeInput::OpenContacts,
    );
    header.pack_end(&contacts_btn);
    // Promote inline reply → window, or collapse window → inline. Icon set in
    // `init` and on SetWindowed.
    let toggle_btn = gtk::Button::new();
    toggle_btn.set_tooltip_text(Some(&i18n("Open in window")));
    let tx = sender.input_sender().clone();
    toggle_btn.connect_clicked(move |_| tx.emit(ComposeInput::ToggleWindowed));
    header.pack_end(&toggle_btn);
    // The compact reply's From/To/Subject rows (#154): folded away by default,
    // one press brings them back.
    let fields_btn = header_toggle(sender, "pan-down-symbolic", ComposeInput::ShowFields);
    fields_btn.add_css_class("fields-chevron");
    fields_btn.set_tooltip_text(Some(&i18n("Show From, To and Subject")));
    fields_btn.set_can_focus(false);
    header.pack_end(&fields_btn);

    // Send Later (#145): says when a scheduled message goes, with a way back
    // to sending at once.
    let schedule_label = gtk::Label::new(None);
    schedule_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let send_now = gtk::Button::with_label(&i18n("Send now instead"));
    send_now.add_css_class("flat");
    let tx = sender.input_sender().clone();
    send_now.connect_clicked(move |_| tx.emit(ComposeInput::ClearSendAt));
    let schedule_bar = info_bar("alarm-symbolic", &schedule_label, &send_now);
    schedule_bar.add_css_class("schedule-bar");
    toolbar_root.add_top_bar(&schedule_bar);

    // Cloud attachments (#144): the download passwords, which stay out of the
    // message and go to the recipient some other way.
    let password_label = gtk::Label::new(None);
    password_label.set_wrap(true);
    password_label.set_selectable(true);
    let copy = gtk::Button::with_label(&i18n("Copy"));
    copy.add_css_class("flat");
    let tx = sender.input_sender().clone();
    copy.connect_clicked(move |_| tx.emit(ComposeInput::CopyCloudPasswords));
    let password_bar = info_bar("dialog-password-symbolic", &password_label, &copy);
    toolbar_root.add_top_bar(&password_bar);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.add_css_class("compose-pane");
    toolbar_root.set_content(Some(&content));

    // From and To are always offered, inline included — a forward is
    // unaddressable without To (#25, #52). Cc, Bcc, and (for replies/forwards)
    // the prefilled Subject wait behind the To row's "More" button; per-row
    // visibility is set in `init`.
    let fields_list = gtk::ListBox::new();
    fields_list.add_css_class("boxed-list");
    fields_list.add_css_class("compose-fields");
    fields_list.set_selection_mode(gtk::SelectionMode::None);

    let from_row = adw::ComboRow::new();
    from_row.set_title(&i18n("From"));
    let tx = sender.input_sender().clone();
    from_row.connect_selected_notify(move |_| tx.emit(ComposeInput::AccountChanged));
    fields_list.append(&from_row);
    // The From as text (#347): any name and address, sent through the account
    // chosen above.
    let custom_from_row = adw::EntryRow::new();
    custom_from_row.set_title(&i18n("From"));
    custom_from_row.set_input_purpose(gtk::InputPurpose::Email);
    custom_from_row.set_visible(false);
    fields_list.append(&custom_from_row);
    let recipient_row = |title: String| {
        let r = RecipientRow::default();
        r.set_title(&title);
        r.set_input_purpose(gtk::InputPurpose::Email);
        r
    };
    let to_row = recipient_row(i18n("To"));
    let cc_row = recipient_row(i18n("Cc"));
    let bcc_row = recipient_row(i18n("Bcc"));
    fields_list.append(&to_row);
    fields_list.append(&cc_row);
    fields_list.append(&bcc_row);
    let reply_to_row = adw::EntryRow::new();
    reply_to_row.set_title(&i18n("Reply-To"));
    reply_to_row.set_input_purpose(gtk::InputPurpose::Email);
    fields_list.append(&reply_to_row);
    let subject_row = adw::EntryRow::new();
    subject_row.set_title(&i18n("Subject"));
    fields_list.append(&subject_row);
    content.append(&fields_list);

    // Attachments in up to three equal columns (#299), filling left to right
    // and wrapping.
    let attach_box = gtk::FlowBox::new();
    attach_box.set_selection_mode(gtk::SelectionMode::None);
    attach_box.set_column_spacing(6);
    attach_box.set_row_spacing(6);
    attach_box.set_homogeneous(true);
    attach_box.set_max_children_per_line(3);
    attach_box.add_css_class("attach-flow");
    attach_box.set_visible(false);
    content.append(&attach_box);

    // Holder for the shared rich-text editor (toolbar + body), appended in
    // `init`.
    let editor_holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
    editor_holder.set_vexpand(true);
    content.append(&editor_holder);

    let widgets = ComposeWidgets {
        drop_overlay,
        toolbar_root,
        header,
        delete_draft_btn,
        send_btn,
        overflow_btn,
        confidential_btn,
        encrypt_btn,
        sign_btn,
        attach_btn,
        cloud_btn,
        contacts_btn,
        toggle_btn,
        fields_btn,
        schedule_bar,
        schedule_label,
        password_bar,
        password_label,
        fields_list,
        from_row,
        custom_from_row,
        to_row,
        cc_row,
        bcc_row,
        reply_to_row,
        subject_row,
        attach_box,
        editor_holder,
    };
    widgets.apply(model);
    widgets
}

impl Component for Compose {
    type Init = ComposeInit;
    type Input = ComposeInput;
    type Output = ComposeOutput;
    type CommandOutput = ();
    // Host-agnostic root: the same pane is shown inline (in a reader Revealer)
    // or set as the content of an app-owned window. Hosting/close is the app's
    // job (see ComposeOutput::ToggleWindow / Close).
    type Root = adw::BreakpointBin;
    type Widgets = ComposeWidgets;

    fn init_root() -> Self::Root {
        adw::BreakpointBin::new()
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.apply(self);
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let ComposeInit {
            compose_id,
            prefill,
            accounts,
            selected,
            own_addresses,
            windowed,
            can_toggle,
            compact,
            decorations,
            format,
            signature_position,
        } = init;
        let in_reply_to = prefill.in_reply_to.clone();
        let references = prefill.references.clone();
        let forwards = prefill.forwards.clone();
        let draft_origin = prefill.draft_origin.clone();
        let template = prefill.template && draft_origin.is_some();
        let outbox_origin = prefill.outbox_origin;
        let prefill_attachments = prefill.attachments.clone();
        let handed_files =
            !prefill_attachments.is_empty() && draft_origin.is_none() && outbox_origin.is_none();
        let prefill_encrypt = prefill.encrypt;
        let request_receipt = prefill.receipt.unwrap_or(false);
        let importance = prefill.importance;
        let send_at = prefill.send_at;
        // A message picked up again keeps how it was to go, and any
        // passwords still in memory; a new one starts as its From says.
        let tuta = match prefill.tuta.clone() {
            Some(delivery) => tuta::TutaCompose {
                confidential: delivery.confidential,
                touched: true,
                passwords: delivery.passwords,
                ..Default::default()
            },
            None => tuta::TutaCompose::default(),
        };
        let current_sig = accounts
            .get(selected)
            .map(|a| a.signature.clone())
            .unwrap_or_default();
        let sign_default = accounts.get(selected).is_some_and(|a| a.sign_default);
        // The user's own domains, which the assistant banner does not flag.
        let own_domains = assistant::own_domains(&accounts);

        let completion = gtk::Popover::new();
        completion.set_autohide(false); // don't steal focus from the entry
        completion.set_can_focus(false);
        completion.set_position(gtk::PositionType::Bottom);
        completion.add_css_class("menu");

        // Initial editor content: a blank line to type on, then the
        // signature and the quoted reply/forward (if any), in the order
        // the setting says (#237). A draft or a queued message already has
        // both and gets neither again: each reopening of a draft used to add
        // another blank line (#349). With Return set to start paragraphs the
        // line is a paragraph too, so the first Return splits it into two.
        let resumed = prefill.resumed || draft_origin.is_some() || outbox_origin.is_some();
        let blank_line = match (resumed, crate::config::load_privacy().return_paragraph) {
            (true, _) => "",
            (false, true) => "<p><br></p>",
            (false, false) => "<div><br></div>",
        };
        // Text written for the user (an assistant's) stands where they would
        // have typed, with the blank line under it to part it from the rest.
        let mut content = if prefill.typed_html.is_empty() {
            String::from(blank_line)
        } else {
            format!("{}{blank_line}", prefill.typed_html)
        };
        let sig_dashes = crate::config::load_privacy().signature_dashes;
        let sig = if !resumed && !prefill.no_signature && !current_sig.is_empty() {
            sig_html(&current_sig, sig_dashes)
        } else {
            String::new()
        };
        match signature_position {
            SignaturePosition::AboveQuote => {
                content.push_str(&sig);
                content.push_str(&prefill.body_html);
            }
            SignaturePosition::BelowQuote => {
                content.push_str(&prefill.body_html);
                content.push_str(&sig);
            }
        }
        // Blocking the quote's pictures must not blank the user's own
        // signature, whichever identity it ends up sent from.
        let remote_images = prefill.block_remote_images.then(|| {
            accounts
                .iter()
                .flat_map(|a| a.signatures.iter())
                .flat_map(|sig| super::rich_editor::remote_image_urls(&sig.html))
                .collect::<Vec<_>>()
        });
        let editor = RichEditor::new(
            &content,
            remote_images.as_deref(),
            format == ComposeFormat::Plain,
        );
        editor.set_formatting_visible(format == ComposeFormat::Rich);
        editor.set_zoom(session_zoom());
        let zoom_chip = zoom_chip(&sender);
        editor.toolbar_end().prepend(&zoom_chip);
        {
            let s = sender.input_sender().clone();
            editor.widget.add_controller(super::ctrl_scroll_zoom(
                || true,
                move |step| {
                    let _ = s.send(ComposeInput::Zoom(step));
                },
            ));
        }
        // A source format starts from the same content, written out as
        // source: a reply's quoted original becomes `> ` lines in Markdown,
        // or the HTML it already was.
        match format {
            ComposeFormat::Markdown => {
                editor.set_source(SourceKind::Markdown, &markdown_body(&content))
            }
            ComposeFormat::Html => editor.set_source(
                SourceKind::Html,
                &crate::mail::markdown::pretty_html(&content),
            ),
            ComposeFormat::Rich | ComposeFormat::Plain => {}
        }
        // HYLKI_SHOWCASE_PLAIN_LOG also logs the body a composer opens with
        // (demo only), to check a reply started in plain text (#436).
        if std::env::var_os("HYLKI_SHOWCASE_PLAIN_LOG").is_some()
            && std::env::var_os("HYLKI_DEMO").is_some()
        {
            let ed = editor.clone();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(2500), move || {
                ed.eval(SHOWCASE_PLAIN_LOG, move |v| {
                    tracing::info!("showcase plain: opened {format:?} {v}")
                });
            });
        }
        let (format_btn, preview_btn, preview_content, translate_btn, signature_btn) =
            add_toolbar_end_buttons(
                &editor,
                format,
                accounts
                    .get(selected)
                    .is_some_and(|a| !a.signatures.is_empty()),
                &sender,
            );

        connect_editor(&editor, &sender);

        let mut model = Compose {
            accounts,
            editor,
            current_sig,
            sig_dashes,
            signature_position,
            attachments: prefill_attachments,
            attach_thumbs: Default::default(),
            suggestions: Vec::new(),
            graph_directories: Default::default(),
            own_mail: Default::default(),
            from_account: 0,
            directory_matches: Vec::new(),
            directory_generation: 0,
            directory_errors_shown: std::collections::HashSet::new(),
            completion,
            completion_field: None,
            completion_list: None,
            completion_selected: 0,
            completion_count: 0,
            completion_open: std::rc::Rc::new(std::cell::Cell::new(false)),
            in_reply_to,
            references,
            forwards,
            draft_origin,
            template,
            outbox_origin,
            compose_id,
            windowed,
            can_toggle,
            // A compact (fields-hidden) pane only makes sense once it is
            // addressed: replies arrive with To filled, forwards do not.
            compact: compact && !prefill.to.trim().is_empty(),
            decorations,
            fields_shown: crate::config::load_privacy().reply_fields,
            custom_from: false,
            narrow: false,
            // Files handed in from outside are something to lose: closing
            // asks first, as it would once the user had attached them.
            fields_dirty: handed_files,
            asking_discard: false,
            sign: false,
            sign_touched: false,
            sign_expected: None,
            format,
            zoom_chip,
            format_btn,
            translate_btn,
            signature_btn,
            translating: false,
            autosaving: false,
            autosaved: None,
            autosave_pending: None,
            autosave_made: false,
            // A template is saved when its editor says so: an automatic save
            // would put the half-edited one in its place.
            autosave_off: template,
            tr_subject: None,
            preview_btn,
            preview_content,
            encrypt: false,
            request_receipt,
            importance,
            pgp_checking: false,
            pgp_cleared: None,
            send_at,
            cloud_accounts: crate::services::cloud::load_enabled_accounts(),
            drop_zones: None,
            cloud_links: Vec::new(),
            cloud_busy: 0,
            cloud_passwords: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            by_assistant: prefill.assistant,
            from_encrypted: prefill.from_encrypted,
            own_domains,
            assistant_banner: None,
            assistant_edit: None,
            edit_loss: None,
            edit_bar: assistant_edit::new_edit_bar(),
            last_edit: None,
            tuta,
            tuta_bar: tuta::TutaBar::new(&sender),
        };
        model.from_account = model
            .accounts
            .get(selected)
            .map(|a| a.id)
            .unwrap_or_default();
        let send_menu = send_menu::send_menu(&sender, model.request_receipt, model.importance);
        let widgets = build_widgets(&root, &sender, &model, &send_menu);
        if prefill_encrypt && crate::mail::pgp::available() {
            // Through the buttons, so the toggles and the model agree.
            widgets.encrypt_btn.set_active(true);
        } else if sign_default && crate::mail::pgp::available() {
            // A draft does not record whether it was signed, so a reopened
            // one starts as its account says, like any new message.
            model.sign_expected = Some(true);
            widgets.sign_btn.set_active(true);
        }
        widgets.editor_holder.append(&model.editor.widget);
        widgets.toolbar_root.add_top_bar(&model.tuta_bar.root);
        if prefill.assistant {
            let banner = assistant::banner(&prefill, &model.own_domains);
            widgets.toolbar_root.add_top_bar(&banner);
            model.assistant_banner = Some(banner);
        }
        widgets.toolbar_root.add_top_bar(&model.edit_bar);

        // The inline/window toggle: only reply/forward panes can toggle. Its icon
        // reflects the current host (fullscreen = "expand to window", restore =
        // "collapse back inline").
        set_toggle_icon(&widgets.toggle_btn, model.windowed);
        size_for_host(
            &widgets.toolbar_root,
            &widgets.header,
            &widgets.editor_holder,
            model.windowed,
            model.decorations,
        );

        fold_on_first_map(&root, &widgets, &sender);

        model.show_initial_rows(&widgets, &prefill);

        model.fill_from_row(&widgets, &prefill, selected, &sender);

        // A draft or template written for a mail merge kept its fields
        // under a stand-in address (#447).
        use crate::mail::merge::fields_from_draft;
        widgets.to_row.set_text(&fields_from_draft(&prefill.to));
        widgets.cc_row.set_text(&fields_from_draft(&prefill.cc));
        widgets.bcc_row.set_text(&fields_from_draft(&prefill.bcc));
        widgets.subject_row.set_text(&prefill.subject);
        if !model.attachments.is_empty() {
            model.rebuild_attachments(&widgets.attach_box, &sender);
        }

        model.install_drop_zones(&root, &widgets, &sender);

        if !prefill.inline_files.is_empty() {
            model.editor.insert_files(&prefill.inline_files);
        }
        // Files handed in over the size limit (Settings → System → GNOME
        // Files): straight into the upload dialog, once the composer has a
        // window for it to be transient for.
        if !prefill.cloud_uploads.is_empty() {
            let s = sender.clone();
            let paths = prefill.cloud_uploads.clone();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || {
                s.input_sender().emit(ComposeInput::CloudPicked(paths));
            });
        }

        showcase_hooks(&root, &model.editor, &sender);

        load_suggestions(own_addresses, &sender);

        wire_recipient_rows(&widgets, &sender);

        // Once the fields are filled: whether the From is a Tuta account,
        // and its question about the recipients.
        model.follow_tuta_account(&widgets, &sender);

        wire_subject(&widgets, &sender);

        merge_fields::add_to_button(&widgets, &sender);

        model.install_keys(&root, &sender);

        start_autosave(&sender);

        ComponentParts { model, widgets }
    }

    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::Input,
        sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        // Each arm handles its message in full, or hands it to an `on_*`
        // method that returns early where it has to; every message ends at
        // `update_view` below.
        {
            match message {
                ComposeInput::Cancel => self.on_cancel(&sender),

                ComposeInput::ConfirmDiscard => self.on_confirm_discard(&sender, root),

                ComposeInput::KeepEditing => self.asking_discard = false,

                ComposeInput::Discard => self.on_discard(&sender),

                ComposeInput::SendAt(at) => {
                    self.send_at = Some(at);
                    sender.input_sender().emit(ComposeInput::Send);
                }

                ComposeInput::ClearSendAt => {
                    self.send_at = None;
                }

                ComposeInput::FocusField(field) => self.on_focus_field(field, widgets),

                ComposeInput::ShowFields(on) => {
                    self.fields_shown = on;
                    widgets
                        .fields_list
                        .set_visible(!(self.compact && !self.windowed) || on);
                }

                ComposeInput::SetNarrow(narrow) => self.on_set_narrow(narrow, widgets, root),

                ComposeInput::OverflowMenu => self.on_overflow_menu(widgets, &sender),

                ComposeInput::CloudAttach => self.on_cloud_attach(&sender, root),

                ComposeInput::DroppedFiles(choice, paths) => {
                    self.on_dropped_files(choice, paths, &sender)
                }

                ComposeInput::CloudPicked(paths) => self.on_cloud_picked(paths, &sender, root),

                ComposeInput::CloudUpload {
                    paths,
                    account,
                    link_password,
                } => self.on_cloud_upload(paths, account, link_password, &sender),

                ComposeInput::CloudUploadSignedIn {
                    paths,
                    account,
                    link_password,
                    password,
                } => match password {
                    Some(password) => self.on_cloud_upload_signed_in(
                        paths,
                        account,
                        link_password,
                        password,
                        widgets,
                        &sender,
                    ),
                    None => not_signed_in(root, &account.name),
                },

                ComposeInput::CloudUploaded { name, result } => {
                    self.on_cloud_uploaded(name, result, widgets, &sender, root)
                }

                ComposeInput::RemoveCloudLink(i) => self.on_remove_cloud_link(i, widgets, &sender),

                ComposeInput::CopyCloudPasswords => self.on_copy_cloud_passwords(),

                ComposeInput::PickSendTime => {
                    let parent = root.root().and_downcast::<gtk::Window>();
                    pick_send_time(parent.as_ref(), self.send_at, sender.input_sender().clone());
                }

                ComposeInput::DeleteDraft => self.on_delete_draft(&sender),

                ComposeInput::ToggleWindowed => {
                    let _ = sender.output(ComposeOutput::ToggleWindow(self.compose_id));
                }

                ComposeInput::SetDecorations(on) => self.on_set_decorations(on, widgets),

                ComposeInput::SetWindowed(windowed) => self.on_set_windowed(windowed, widgets),

                ComposeInput::FocusInitial => self.focus_initial(widgets),

                ComposeInput::MarkFieldsDirty => self.fields_dirty = true,

                ComposeInput::SaveDraftIfDirty => self.on_save_draft_if_dirty(&sender),

                ComposeInput::OpenContacts => self.on_open_contacts(widgets, &sender, root),

                ComposeInput::AttachFiles => self.on_attach_files(&sender, root),

                ComposeInput::AddAttachments(paths) => {
                    self.on_add_attachments(paths, widgets, &sender)
                }

                ComposeInput::RemoveAttachment(i) => self.on_remove_attachment(i, widgets, &sender),

                ComposeInput::Zoom(step) => self.on_zoom(step),

                ComposeInput::History { redo } => {
                    self.history_step(redo, widgets, &sender);
                }

                ComposeInput::ShowcaseHistory(step) => {
                    self.showcase_history(step, widgets, &sender);
                }

                ComposeInput::ShowcaseFrom(n) => {
                    widgets.from_row.set_selected(n);
                }

                ComposeInput::BodyHistory(can_undo) => self.on_body_history(can_undo, &sender),

                ComposeInput::CustomFrom(on) => self.on_custom_from(on, widgets),

                ComposeInput::AccountChanged => {
                    self.on_account_changed(widgets);
                    self.follow_tuta_account(widgets, &sender);
                }

                ComposeInput::InsertSignature(place) => self.on_insert_signature(place, widgets),

                ComposeInput::SignatureClicked => self.on_signature_clicked(widgets, &sender),

                ComposeInput::UseSignature(choice) => self.on_use_signature(choice, widgets),

                ComposeInput::SignatureMenu => self.on_signature_menu(&sender),

                ComposeInput::Suggest(field) => self.on_suggest(field, widgets, &sender),

                ComposeInput::DirectoriesKnown { graph, own_mail } => {
                    self.graph_directories = std::sync::Arc::new(graph);
                    self.own_mail = own_mail;
                }

                ComposeInput::SuggestionsLoaded(loaded) => {
                    self.on_suggestions_loaded(loaded, widgets)
                }

                ComposeInput::DirectoryLookup {
                    field,
                    token,
                    generation,
                } => self.on_directory_lookup(field, token, generation, &sender),

                ComposeInput::DirectoryResults {
                    field,
                    token,
                    generation,
                    matches,
                    failed,
                } => {
                    self.report_directory_failures(failed, &sender);
                    self.on_directory_results(field, token, generation, matches, widgets);
                }

                ComposeInput::AddSuggestions { account_id, new } => {
                    self.on_add_suggestions(account_id, new)
                }

                ComposeInput::CompletionMove(delta) => self.on_completion_move(delta),

                ComposeInput::CompletionAccept => self.on_completion_accept(widgets),

                ComposeInput::CompletionClose => {
                    self.completion_open.set(false);
                    self.completion.popdown();
                }

                ComposeInput::ToggleSign(on) => self.on_toggle_sign(on),

                ComposeInput::FormatMenu => self.on_format_menu(&sender),

                ComposeInput::TranslateMenu => self.on_translate_menu(&sender),

                ComposeInput::ShowTranslateMenu(found) => {
                    self.on_show_translate_menu(found, &sender)
                }

                ComposeInput::TranslateTo(target) => self.on_translate_to(target, &sender, root),

                ComposeInput::TranslateBegun { found, target } => {
                    self.on_translate_begun(found, target, widgets, &sender, root)
                }

                ComposeInput::TranslateToggle { original } => {
                    self.on_translate_toggle(original, widgets, &sender)
                }

                ComposeInput::Translated {
                    result,
                    how,
                    subject,
                } => self.on_translated(result, how, subject, widgets, root),

                ComposeInput::SetFormat(to) => self.on_set_format(to, &sender),

                ComposeInput::AskPlain => self.on_ask_plain(&sender, root),

                ComposeInput::ApplyFormat(to) => self.on_apply_format(to, &sender),

                ComposeInput::LoadAs { from, to, body } => self.on_load_as(from, to, body, &sender),

                ComposeInput::TogglePreview(on) => self.on_toggle_preview(on, &sender),

                ComposeInput::ShowPreview(body) => {
                    let html = match self.format {
                        ComposeFormat::Markdown => {
                            crate::mail::direction::stamp(&crate::mail::markdown::to_html(&body))
                        }
                        ComposeFormat::Html => crate::mail::direction::stamp(&body),
                        _ => body,
                    };
                    self.editor.show_preview(Some(&html));
                }
                ComposeInput::ToggleEncrypt(on) => {
                    self.encrypt = on;
                    self.editor.pause_grammar(on);
                    if on && !self.sign {
                        self.sign = true;
                        widgets.sign_btn.set_active(true);
                    }
                }
                ComposeInput::ToggleConfidential(on) => self.on_toggle_confidential(on, widgets),
                ComposeInput::RecipientsChanged => self.on_recipients_changed(widgets, &sender),
                ComposeInput::TutaLookup(token) => self.on_tuta_lookup(token, widgets, &sender),
                ComposeInput::TutaKnown {
                    generation,
                    asked,
                    result,
                } => self.on_tuta_known(generation, asked, result, widgets, &sender),
                ComposeInput::TutaPasswords => self.on_tuta_passwords(widgets, &sender),
                ComposeInput::TutaPassword { address, typed } => {
                    self.on_tuta_password(&address, &typed, widgets)
                }
                ComposeInput::ConfirmWeakPasswords => {
                    self.tuta.weak_ok = Some(self.tuta.revision);
                    sender.input_sender().emit(ComposeInput::Send);
                }
                // Whatever was asked next is sent behind it.
                ComposeInput::ConsentOrdinary => self.tuta.ordinary_consent = true,
                ComposeInput::ToggleReceipt(on) => {
                    self.request_receipt = on;
                    self.fields_dirty = true;
                }
                ComposeInput::SetImportance(importance) => {
                    self.importance = importance;
                    self.fields_dirty = true;
                }
                ComposeInput::PgpChecked { checked, result } => {
                    self.on_pgp_checked(checked, result, widgets, &sender)
                }
                ComposeInput::Send => self.on_send(widgets, &sender),

                ComposeInput::SendBody(fields) => self.on_send_body(fields, widgets, &sender),

                ComposeInput::SendAnyway(fields) => self.finish_send(fields, &sender),

                ComposeInput::MailMerge => self.on_mail_merge(widgets, &sender),

                ComposeInput::MergeFields(target) => self.on_merge_fields(target, widgets, &sender),

                ComposeInput::MergeBody(fields) => self.on_merge_body(fields, widgets, &sender),

                ComposeInput::MergeSend {
                    fields,
                    letters,
                    interval,
                } => self.on_merge_send(fields, letters, interval, &sender),

                ComposeInput::SaveDraft => self.on_save_draft(false, widgets, &sender),
                ComposeInput::SaveAsTemplate => self.on_save_draft(true, widgets, &sender),

                ComposeInput::SaveDraftBody {
                    fields,
                    as_template,
                } => self.on_save_draft_body(fields, as_template, &sender),

                ComposeInput::AutoSave => self.on_auto_save(&sender),

                ComposeInput::AutoSaveRead => self.on_auto_save_read(widgets, &sender),

                ComposeInput::AutoSaveBody(fields) => self.on_auto_save_body(fields, &sender),

                ComposeInput::Autosaved { saved, origin } => self.on_autosaved(saved, origin),

                input @ (ComposeInput::AssistantRead { .. }
                | ComposeInput::AssistantEdit(_)
                | ComposeInput::ApplyAssistantEdit
                | ComposeInput::RejectAssistantEdit
                | ComposeInput::ReshowAssistantDiff) => {
                    self.on_assistant_input(input, widgets, root, &sender)
                }
            }
        }
        // Overriding `update_with_view` takes over relm4's default, which
        // ran `update_view` after every message; without this the `#[watch]`
        // bindings above only ever hold their `init` values.
        self.update_view(widgets, sender);
    }
}

impl Compose {}
