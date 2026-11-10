//! The reading pane as a relm4 component: its widget tree (view!), building
//! it, and `update`, which routes each input to its `on_*` method.

use super::init::link_preview_plaque;
use super::types::{MessageViewInput, MessageViewOutput};
use super::webview::new_webview;
use super::MessageView;
use crate::i18n::{i18n, i18n_f};
use adw::prelude::*;
use crate::relm::prelude::*;

/// The reader's widgets: the ones `init` reads back by field, and the ones
/// that follow the model, which `apply` sets from it at build and after
/// every message. This is what `#[relm4::component]`'s `view!` block and its
/// `#[watch]` attributes used to generate.
pub(crate) struct MessageViewWidgets {
    root: gtk::Stack,
    empty: adw::StatusPage,
    spoof_revealer: gtk::Revealer,
    spoof_label: gtk::Label,
    remote_alert: gtk::Revealer,
    allow_btn: gtk::Button,
    find_bar: gtk::Revealer,
    find_box: gtk::Box,
    find_entry: gtk::SearchEntry,
    find_count: gtk::Label,
    header: gtk::Box,
    account_line: gtk::Box,
    chip_box: gtk::Box,
    account_chip: gtk::Label,
    zoom_chip: gtk::Label,
    theme_box: gtk::Box,
    theme_switch: gtk::Switch,
    reader_box: gtk::Box,
    reader_switch: gtk::Switch,
    subject: gtk::Label,
    header_tags: gtk::Box,
    body_stack: gtk::Stack,
    loading_page: gtk::Box,
}

impl MessageViewWidgets {
    /// Everything that was `#[watch]`ed: set from the model.
    fn apply(&self, model: &MessageView) {
        // Drafts open in the editor, so the empty pane says so there.
        self.empty.set_icon_name(Some(if model.checked.0 > 0 {
            "object-select-symbolic"
        } else if model.drafts_view {
            "document-edit-symbolic"
        } else {
            "hylki-mail-read-symbolic"
        }));
        self.empty.set_title(&model.empty_title());
        self.empty.set_description(Some(&if model.checked.0 > 0 {
            i18n("The actions above the list apply to all of them.")
        } else if model.drafts_view {
            i18n("Choose a draft from the list to edit it here.")
        } else {
            i18n("Choose a message from the list to read it here.")
        }));

        // A banner that comes with the message opening is just there:
        // sliding in, it would push the page down after it shows.
        self.spoof_revealer.set_transition_duration(model.banner_slide_ms());
        self.spoof_revealer.set_reveal_child(model.spoof_alert_shown());
        self.spoof_label.set_label(model.sender_check.as_ref().map(|c| c.summary.as_str()).unwrap_or_default());
        self.remote_alert.set_transition_duration(model.banner_slide_ms());
        self.remote_alert.set_reveal_child(model.blocked && model.show_banner);
        self.allow_btn.set_tooltip_text(model.current.as_ref().map(|m| m.from_addr.as_str()));

        self.find_bar.set_transition_type(if model.find_bottom {
            gtk::RevealerTransitionType::SlideUp
        } else {
            gtk::RevealerTransitionType::SlideDown
        });
        self.find_bar.set_reveal_child(model.find_open);
        self.find_box.set_class_active("bottom", model.find_bottom);
        self.find_count.set_label(&match model.find_matches {
            Some((_, 0)) => i18n("No matches"),
            Some((cur, total)) => i18n_f("{cur} of {total}", &[("cur", &cur.to_string()), ("total", &total.to_string())]),
            None => String::new(),
        });

        // The header's top padding is cut to the 4px that puts the account
        // chip level with the message list's first row. That only reads as
        // alignment when the header is against the toolbar; under a bar it
        // reads as cramped, so give it its air back whenever one is
        // showing. Under the split reply there is no list row to line up
        // with either, only the composer's grab strip: 14px more air than
        // the aligned case.
        self.header.set_class_active(
            "under-bar",
            model.spoof_alert_shown() || (model.blocked && model.show_banner) || (model.find_open && !model.find_bottom),
        );
        self.header.set_class_active("under-split", model.under_split);
        self.account_line.set_visible(model.account_line_shown());
        self.chip_box.set_visible(model.account_name.is_some());
        self.account_chip.set_visible(model.account_chip_shown);
        self.account_chip.set_label(model.account_name.as_deref().unwrap_or_default());
        self.zoom_chip.set_label(&format!("{}%", model.zoom));
        self.zoom_chip.set_visible(model.current.is_some() && model.zoom != model.zoom_default);
        self.theme_box.set_visible(model.current.is_some() && model.theme_switch_shown);
        self.theme_switch.set_active(model.message_flipped());
        self.reader_box.set_visible(model.current.is_some() && model.reader_switch_shown);
        self.reader_switch.set_active(model.reader_mode);
        self.subject.set_label(&model.current.as_ref().map(|m| model.shown_subject(m)).unwrap_or_default());
        self.header_tags.set_visible(model.header_tags_shown());
        self.body_stack.set_visible_child_name(model.body_page());
        self.root.set_visible_child_name(if model.current.is_some() { "message" } else { "empty" });
    }
}

/// A banner's warning icon and wrapping text, side by side.
fn banner_row(css_class: &str, icon: &str, label: &gtk::Label) -> gtk::Box {
    let row = gtk::Box::builder().css_classes([css_class]).spacing(8).build();
    row.append(&gtk::Image::builder().icon_name(icon).build());
    row.append(label);
    row
}

/// A "caption + switch" pair in the header's account line: an icon for the
/// label, a switch beside it, and the tooltip that says what it does.
fn toggle_box(icon: &str, tooltip: String, switch: &gtk::Switch) -> gtk::Box {
    let toggle = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .css_classes(["reader-toggle"])
        .tooltip_text(tooltip)
        .build();
    toggle.append(&gtk::Image::builder().icon_name(icon).css_classes(["dim-label"]).build());
    toggle.append(switch);
    toggle
}

/// A flat icon button in the find bar that sends one message.
fn find_button(sender: &ComponentSender<MessageView>, icon: &str, tooltip: String, message: fn() -> MessageViewInput) -> gtk::Button {
    let button = gtk::Button::builder().icon_name(icon).tooltip_text(tooltip).css_classes(["flat"]).build();
    let sender = sender.clone();
    button.connect_clicked(move |_| sender.input_sender().emit(message()));
    button
}

fn build_widgets(root: &gtk::Stack, sender: &ComponentSender<MessageView>, model: &MessageView, header_tags: &gtk::Box) -> MessageViewWidgets {
    root.set_transition_type(gtk::StackTransitionType::Crossfade);

    let empty = adw::StatusPage::new();
    root.add_named(&empty, Some("empty"));

    let message = gtk::Box::builder().orientation(gtk::Orientation::Vertical).css_classes(["reader-pane"]).build();
    root.add_named(&message, Some("message"));

    // The sender check's banner.
    let spoof_label = gtk::Label::builder().hexpand(true).halign(gtk::Align::Start).wrap(true).xalign(0.0).build();
    let spoof_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .child(&banner_row("spoof-alert", "dialog-warning-symbolic", &spoof_label))
        .build();
    message.append(&spoof_revealer);

    // Remote content is blocked: Load, or always allow this sender.
    // Ask for the full single-line width and wrap only when the pane really
    // is too narrow: without this a wrapping label requests minimal width
    // and folds to two lines even with room to spare.
    let remote_label = gtk::Label::builder()
        .label(i18n("Remote content (images, trackers) is blocked to protect your privacy."))
        .hexpand(true)
        .halign(gtk::Align::Start)
        .wrap(true)
        .natural_wrap_mode(gtk::NaturalWrapMode::None)
        .xalign(0.0)
        .build();
    let remote_row = banner_row("remote-alert", "security-high-symbolic", &remote_label);
    let load_btn = gtk::Button::builder().label(i18n("Load")).valign(gtk::Align::Center).build();
    {
        let sender = sender.clone();
        load_btn.connect_clicked(move |_| sender.input(MessageViewInput::LoadRemoteOnce));
    }
    remote_row.append(&load_btn);
    let allow_btn = gtk::Button::builder().label(i18n("Always allow sender")).valign(gtk::Align::Center).build();
    {
        let sender = sender.clone();
        allow_btn.connect_clicked(move |_| sender.input(MessageViewInput::AllowSenderAlways));
    }
    remote_row.append(&allow_btn);
    let remote_alert = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .child(&remote_row)
        .build();
    message.append(&remote_alert);

    // In-message find (#103): WebKit's FindController does the matching and
    // highlighting; this bar just drives it.
    let find_entry = gtk::SearchEntry::builder().hexpand(true).placeholder_text(i18n("Find in message")).build();
    {
        let sender = sender.clone();
        find_entry.connect_search_changed(move |entry| {
            sender.input_sender().emit(MessageViewInput::FindChanged(entry.text().to_string()));
        });
    }
    {
        let sender = sender.clone();
        find_entry.connect_activate(move |_| sender.input_sender().emit(MessageViewInput::FindNext));
    }
    {
        let sender = sender.clone();
        find_entry.connect_stop_search(move |_| sender.input_sender().emit(MessageViewInput::CloseFind));
    }
    let focus = gtk::EventControllerFocus::new();
    {
        let sender = sender.clone();
        focus.connect_leave(move |ctl| {
            let Some(entry) = ctl.widget().and_downcast::<gtk::SearchEntry>() else { return };
            let sender = sender.clone();
            crate::ui::close_if_focus_left_row(&entry, move || sender.input_sender().emit(MessageViewInput::CloseFind));
        });
    }
    find_entry.add_controller(focus);
    let find_count = gtk::Label::builder().css_classes(["dim-label"]).build();
    let find_box = gtk::Box::builder().css_classes(["reader-find"]).spacing(6).build();
    find_box.append(&find_entry);
    find_box.append(&find_count);
    find_box.append(&find_button(sender, "pan-up-symbolic", i18n("Previous match"), || MessageViewInput::FindPrev));
    find_box.append(&find_button(sender, "pan-down-symbolic", i18n("Next match"), || MessageViewInput::FindNext));
    find_box.append(&find_button(sender, "window-close-symbolic", i18n("Close find"), || MessageViewInput::CloseFind));
    let find_bar = gtk::Revealer::builder().child(&find_box).build();
    message.append(&find_bar);

    // ---- the header ----
    let header = gtk::Box::builder().css_classes(["reader-header"]).orientation(gtk::Orientation::Vertical).spacing(12).build();
    let account_line = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(12).build();

    // Stays when the chip is hidden in Settings: it is what pushes the
    // switches to the line's right end.
    let account_chip = gtk::Label::builder().css_classes(["account-chip", "hylki-account-chip"]).build();
    let chip_box = gtk::Box::builder().halign(gtk::Align::Start).hexpand(true).valign(gtk::Align::Center).build();
    chip_box.append(&account_chip);
    account_line.append(&chip_box);

    // Message zoom (Ctrl+ / Ctrl-): the percentage, shown only while it is
    // away from the default Settings starts at, so a chosen size says
    // nothing about itself. A click goes back to that default.
    let zoom_chip = gtk::Label::builder()
        .valign(gtk::Align::Center)
        .css_classes(["caption", "hylki-zoom-chip"])
        .tooltip_text(i18n("Message zoom. Ctrl+ and Ctrl-, or Ctrl and the scroll wheel, change it; a click here, or Ctrl+0, puts it back to the default from Settings."))
        .build();
    zoom_chip.set_cursor_from_name(Some("pointer"));
    let zoom_click = gtk::GestureClick::new();
    {
        let sender = sender.clone();
        zoom_click.connect_released(move |_, _, _, _| {
            let _ = sender.output(MessageViewOutput::Zoom(0));
        });
    }
    zoom_chip.add_controller(zoom_click);
    account_line.append(&zoom_chip);

    // Light / Dark Mode: the message on screen in the other scheme, for that
    // message alone. Sits just before the Reader View switch, the highlights
    // icon for a label.
    let theme_switch = gtk::Switch::builder().valign(gtk::Align::Center).build();
    theme_switch.update_property(&[gtk::accessible::Property::Label(&i18n("Light / Dark Mode"))]);
    // Setting it to what is already shown (a new message, a theme change)
    // does nothing.
    {
        let sender = sender.clone();
        theme_switch.connect_active_notify(move |sw| {
            sender.input_sender().emit(MessageViewInput::SetMessageFlipped(sw.is_active()));
        });
    }
    let theme_box = toggle_box(
        "highlights-symbolic",
        i18n("Show this message in light or dark mode. Other messages are not affected."),
        &theme_switch,
    );
    account_line.append(&theme_box);

    // Reader View: the message(s) as content alone. A switch behind a pair of
    // reading glasses on the account chip's line, at its right end: part of
    // the message header, not the action row.
    let reader_switch = gtk::Switch::builder().valign(gtk::Align::Center).build();
    reader_switch.update_property(&[gtk::accessible::Property::Label(&i18n("Reader View"))]);
    // The owner decides what a flip does, so it never renders on its own: the
    // main window saves the choice and hands it back to every reader, a
    // message's own window applies it to itself alone.
    {
        let sender = sender.clone();
        reader_switch.connect_active_notify(move |sw| {
            let _ = sender.output(MessageViewOutput::ReaderMode(sw.is_active()));
        });
    }
    let reader_box = toggle_box(
        "glasses-symbolic",
        i18n("Reader View shows a message as its text alone, in one plain format, without the sender's layout, colors and fonts."),
        &reader_switch,
    );
    account_line.append(&reader_box);
    header.append(&account_line);

    // Break mid-word for unbreakable tokens (e.g. an undecodable subject or
    // a long URL) so an extreme subject can never force the pane, and with
    // it the window controls, wider than the screen.
    let subject = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .selectable(true)
        .css_classes(["reader-subject"])
        .build();
    header.append(&subject);

    // Tag chips (#71) for a lone full-bleed message, whose header is these
    // widgets; cards draw theirs in the document. Filled by post_view.
    header_tags.set_spacing(4);
    header_tags.set_halign(gtk::Align::Start);
    header.append(header_tags);
    message.append(&header);

    // The message's ground, painted on the stack itself so every page sits
    // on it: the spinner box is centred and paints only its own few square
    // inches. A cut: the spinner shows with its message, under the pane's
    // still or as the reveal after it, never dissolved over the message
    // before.
    let body_stack = gtk::Stack::builder().css_classes(["reader-cover"]).vexpand(true).build();
    // Dimming comes from the cover's own foreground, which is picked for the
    // message theme: `dim-label` would fade it against the app's instead.
    let loading_page = gtk::Box::builder()
        .css_classes(["reader-loading"])
        .focusable(true)
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(14)
        .build();
    loading_page.append(&gtk::Spinner::builder().spinning(true).width_request(36).height_request(36).build());
    loading_page.append(&gtk::Label::new(Some(&i18n("Loading…"))));
    body_stack.add_named(&loading_page, Some("loading"));
    message.append(&body_stack);

    let widgets = MessageViewWidgets {
        root: root.clone(),
        empty,
        spoof_revealer,
        spoof_label,
        remote_alert,
        allow_btn,
        find_bar,
        find_box,
        find_entry,
        find_count,
        header,
        account_line,
        chip_box,
        account_chip,
        zoom_chip,
        theme_box,
        theme_switch,
        reader_box,
        reader_switch,
        subject,
        header_tags: header_tags.clone(),
        body_stack,
        loading_page,
    };
    widgets.apply(model);
    widgets
}

impl Component for MessageView {
    /// The freeze of the pane the reader is in (with its attachment drawer).
    type Init = crate::ui::reader_swap::ReaderSwap;
    type Input = MessageViewInput;
    type Output = MessageViewOutput;
    type CommandOutput = ();
    type Root = gtk::Stack;
    type Widgets = MessageViewWidgets;

    fn init_root() -> Self::Root {
        gtk::Stack::new()
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.apply(self);
        self.sync_header_tags(&widgets.header_tags);
        self.hold_focus_while_loading();
    }

    fn init(
        swap: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let webview = new_webview();
        let link_preview = link_preview_plaque(&webview);
        let mut model = MessageView::initial(webview, &link_preview, swap);
        model.faces.input = Some(sender.input_sender().clone());
        model.watch_loads(&sender);
        model.watch_context_menu(&sender);
        model.watch_bridge(&sender);
        model.watch_theme(&sender);

        let header_tags = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let widgets = build_widgets(&root, &sender, &model, &header_tags);
        model.find_entry = Some(widgets.find_entry.clone());
        model.find_bar = Some((
            widgets.find_bar.clone(),
            widgets.remote_alert.clone().upcast(),
        ));
        model.loading_page = Some(widgets.loading_page.clone());
        model.account_chip = Some(widgets.account_chip.clone());
        model.cover = Some(widgets.body_stack.clone());
        model.apply_webview_bg(model.effective_dark());
        let body_overlay = gtk::Overlay::new();
        body_overlay.set_child(Some(&model.webview));
        body_overlay.add_overlay(&link_preview);
        let zoom_sender = sender.clone();
        body_overlay.add_controller(crate::ui::ctrl_scroll_zoom(
            || true,
            move |step| {
                let _ = zoom_sender.output(MessageViewOutput::Zoom(step));
            },
        ));
        widgets.body_stack.add_named(&body_overlay, Some("body"));
        widgets.body_stack.set_visible_child_name("body");
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            MessageViewInput::Show {
                thread,
                allow_remote,
                account_name,
                account_color,
                loading,
                primary,
                folder_labels,
                instant,
                sent,
            } => self.on_show(
                thread,
                allow_remote,
                account_name,
                account_color,
                loading,
                primary,
                folder_labels,
                instant,
                sent,
                &sender,
            ),
            MessageViewInput::LoadRemoteOnce => self.on_load_remote_once(&sender),
            MessageViewInput::AllowSenderAlways => self.on_allow_sender_always(&sender),
            MessageViewInput::Print => self.on_print(),
            MessageViewInput::PrintPreview => self.on_print_preview(),
            MessageViewInput::PrintMessages {
                message,
                thread,
                allow_remote,
                preview,
            } => self.on_print_messages(*message, thread, allow_remote, preview),
            MessageViewInput::FacesChanged => {
                self.render();
            }

            MessageViewInput::ThemeChanged => self.on_theme_changed(),
            MessageViewInput::SetAlwaysShowRecipients(on) => {
                self.always_show_recipients = on;
            }
            MessageViewInput::SetFoldMessages(fold) => self.on_set_fold_messages(fold),
            MessageViewInput::SetFoldLook { preview_lines } => self.on_set_fold_look(preview_lines),
            MessageViewInput::Folded {
                account_id,
                id,
                folded,
            } => self.on_folded(account_id, id, folded),
            MessageViewInput::Mapped => self.on_mapped(),
            MessageViewInput::SetChecked {
                count,
                conversations,
            } => self.checked = (count, conversations),
            MessageViewInput::FoldAll(folded) => self.on_fold_all(folded),
            MessageViewInput::SetSingleMessageCard(on) => {
                self.single_message_card = on;
            }
            MessageViewInput::SetPgpLabels(on) => self.on_set_pgp_labels(on),
            MessageViewInput::SetCardAttachments(map) => self.on_set_card_attachments(map),
            MessageViewInput::SetCardAttachmentsShown(on) => self.on_set_card_attachments_shown(on),
            MessageViewInput::SetAttachmentDrawer(on) => self.on_set_attachment_drawer(on),
            MessageViewInput::ScrollToAttachments { account_id, id } => {
                self.on_scroll_to_attachments(account_id, id)
            }
            MessageViewInput::AttachmentAction {
                account_id,
                id,
                index,
                save,
            } => self.on_attachment_action(account_id, id, index, save, &sender),
            MessageViewInput::SetBannerShown(show) => {
                self.show_banner = show;
            }
            MessageViewInput::SetSpoofBannerShown(show) => {
                self.show_spoof_banner = show;
            }
            MessageViewInput::SetUnderSplit(under) => {
                self.under_split = under;
            }
            MessageViewInput::SetContentTheme(o) => self.on_set_content_theme(o),
            MessageViewInput::SetReaderStyle(style) => self.on_set_reader_style(style),
            MessageViewInput::SetReaderMode(on) => {
                self.set_reader_mode(on);
            }
            MessageViewInput::SetMessageFlipped(on) => self.on_set_message_flipped(on),
            MessageViewInput::SetZoomDefault(percent) => {
                self.zoom_default = percent;
            }
            MessageViewInput::SetZoom(percent) => self.on_set_zoom(percent),
            MessageViewInput::SetReaderSwitchShown(on) => {
                self.reader_switch_shown = on;
            }
            MessageViewInput::SetThemeSwitchShown(on) => {
                self.theme_switch_shown = on;
            }
            MessageViewInput::SetAccountChipShown(on) => {
                self.account_chip_shown = on;
            }
            MessageViewInput::SetReaderDefault(policy) => self.on_set_reader_default(policy),
            MessageViewInput::ToggleSenderStyle { account_id, id } => {
                self.on_toggle_sender_style(account_id, id)
            }
            MessageViewInput::SetSenderCheck(check) => {
                self.sender_check = Some(*check);
            }

            MessageViewInput::SenderCheckFor {
                account_id,
                id,
                check,
            } => self.on_sender_check_for(account_id, id, check),
            MessageViewInput::SetUnsubscribed(lists) => self.on_set_unsubscribed(lists),
            MessageViewInput::SetReceiptMarks(marks) => self.on_set_receipt_marks(marks),
            MessageViewInput::Translate { account_id, id } => {
                self.on_translate(account_id, id, &sender)
            }
            MessageViewInput::Translated {
                account_id,
                id,
                result,
            } => self.on_translated(account_id, id, result),
            MessageViewInput::Unsubscribe { account_id, id } => {
                self.on_unsubscribe(account_id, id, &sender)
            }
            MessageViewInput::ReceiptAnswer {
                account_id,
                id,
                send,
            } => self.on_receipt_answer(account_id, id, send, &sender),
            MessageViewInput::UnsubscribeState {
                account_id,
                id,
                state,
            } => self.on_unsubscribe_state(account_id, id, state),
            MessageViewInput::SetIdentities(identities) => self.on_set_identities(identities),
            MessageViewInput::SetInviteAnswers(answers) => self.on_set_invite_answers(answers),
            MessageViewInput::InviteAction {
                account_id,
                id,
                action,
            } => self.on_invite_action(account_id, id, action, &sender),
            MessageViewInput::SenderInfoAt {
                account_id,
                id,
                rect,
                page_width,
            } => self.on_sender_info_at(account_id, id, rect, page_width, &sender),
            MessageViewInput::PgpFetchKey { account_id, id } => {
                self.on_pgp_fetch_key(account_id, id, &sender)
            }
            MessageViewInput::PgpTrustKey { account_id, id } => {
                self.on_pgp_trust_key(account_id, id, &sender)
            }
            MessageViewInput::PgpTrustConfirmed {
                account_id,
                id,
                fingerprint,
            } => self.on_pgp_trust_confirmed(account_id, id, fingerprint, &sender),
            MessageViewInput::PgpDone {
                account_id,
                id,
                result,
            } => self.on_pgp_done(account_id, id, result, &sender),
            MessageViewInput::Rendered => self.on_rendered(),
            MessageViewInput::FaceFound(answer) => self.on_face_found(*answer),
            MessageViewInput::LoadingDeadline(generation) => self.on_loading_deadline(generation),
            MessageViewInput::SuppressAutoRead { account_id, id } => {
                self.on_suppress_auto_read(account_id, id)
            }
            MessageViewInput::ClearCards => self.on_clear_cards(&sender),
            MessageViewInput::CardClicked {
                account_id,
                id,
                mode,
            } => self.on_card_clicked(account_id, id, mode, &sender),
            MessageViewInput::CopySelection => self.on_copy_selection(),
            MessageViewInput::SelectAllCards => self.on_select_all_cards(&sender),
            MessageViewInput::StepCard(delta) => self.on_step_card(delta, &sender),
            MessageViewInput::FocusBody => self.on_focus_body(),
            MessageViewInput::ComposeTo(addr) => {
                let _ = sender.output(MessageViewOutput::ComposeTo(addr));
            }
            MessageViewInput::AddContact(addr) => {
                let _ = sender.output(MessageViewOutput::AddContactAddr(addr));
            }
            MessageViewInput::SetSelectedCards(keys) => self.on_set_selected_cards(keys),
            MessageViewInput::FrameSized {
                account_id,
                id,
                height,
            } => self.on_frame_sized(account_id, id, height),
            MessageViewInput::MarkSeen { account_id, id } => {
                self.on_mark_seen(account_id, id, &sender)
            }
            MessageViewInput::CardMenuAt { key, x, y, hit } => {
                self.on_card_menu_at(key, x, y, hit, &sender)
            }
            MessageViewInput::CardMenuFrom {
                account_id,
                id,
                x,
                y,
                page_width,
            } => self.on_card_menu_from(account_id, id, x, y, page_width, &sender),
            MessageViewInput::ClearDot { account_id, id } => self.on_clear_dot(account_id, id),
            MessageViewInput::SetReadMark(policy) => self.on_set_read_mark(policy),
            MessageViewInput::OpenFind => self.on_open_find(&sender),
            MessageViewInput::CloseFind => self.on_close_find(),
            MessageViewInput::SetFindBottom(on) => self.on_set_find_bottom(on),
            MessageViewInput::DragCard(key) => self.on_drag_card(key),
            MessageViewInput::FindChanged(text) => self.on_find_changed(text),
            MessageViewInput::FindNext => self.eval_find("hylkiFindStep(1)"),
            MessageViewInput::FindPrev => self.eval_find("hylkiFindStep(-1)"),
            MessageViewInput::FindCounted { current, total } => {
                self.find_matches = Some((current, total));
            }
            MessageViewInput::SetTags(tags) => self.on_set_tags(tags),
            MessageViewInput::SetCardKeywords {
                account_id,
                id,
                keywords,
                labels,
            } => self.on_set_card_keywords(account_id, id, keywords, labels),
            MessageViewInput::SetCardStar {
                account_id,
                id,
                starred,
            } => self.on_set_card_star(account_id, id, starred),
            MessageViewInput::FocusCard { account_id, id } => self.on_focus_card(account_id, id),
            MessageViewInput::BlurCard => self.on_blur_card(),
            MessageViewInput::ScrollAnchor {
                account_id,
                id,
                offset,
            } => self.on_scroll_anchor(account_id, id, offset),
            MessageViewInput::PinArrival { account_id, id } => self.on_pin_arrival(account_id, id),
            MessageViewInput::SetDraftsView(on) => {
                self.drafts_view = on;
            }
            MessageViewInput::SetTemplateFolders(folders) => self.on_set_template_folders(folders),
            MessageViewInput::TemplateAction {
                account_id,
                id,
                edit,
            } => self.on_template_action(account_id, id, edit, &sender),
            MessageViewInput::HoldPlace { account_id, id } => self.on_hold_place(account_id, id),
            MessageViewInput::CardAction {
                action,
                account_id,
                id,
            } => self.on_card_action(action, account_id, id, &sender),
            MessageViewInput::OpenHeader { account_id, id } => {
                self.on_open_header(account_id, id, &sender)
            }
        }
    }
}
