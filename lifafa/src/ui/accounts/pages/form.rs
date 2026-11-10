//! The account editor's form: the account and its servers, appearance,
//! aliases, signatures, syncing, Microsoft 365, folders and OpenPGP.
//! Filled and read by `form`, wired by `setup`.

use adw::prelude::*;
use relm4::prelude::*;

use super::super::form::{REPLY_COPIES_HINT, SENT_COPY_HINT};
use super::super::{AccountsInput, AccountsWindow};
use crate::i18n::i18n;

/// The page, and the widgets on it that the panel reaches later.
#[derive(Debug)]
pub(in crate::ui::accounts) struct EditorForm {
    pub(in crate::ui::accounts) page: adw::PreferencesPage,
    pub(in crate::ui::accounts) provider_mark: gtk::Box,
    pub(in crate::ui::accounts) goa_banner: adw::PreferencesGroup,
    pub(in crate::ui::accounts) goa_enabled_row: adw::SwitchRow,
    pub(in crate::ui::accounts) quota_group: adw::PreferencesGroup,
    pub(in crate::ui::accounts) quota_row: adw::ActionRow,
    pub(in crate::ui::accounts) quota_bar: gtk::LevelBar,
    pub(in crate::ui::accounts) label_row: adw::EntryRow,
    pub(in crate::ui::accounts) provider_row: adw::ComboRow,
    pub(in crate::ui::accounts) name_row: adw::EntryRow,
    pub(in crate::ui::accounts) email_row: adw::EntryRow,
    pub(in crate::ui::accounts) protocol_row: adw::ComboRow,
    pub(in crate::ui::accounts) host_row: adw::EntryRow,
    pub(in crate::ui::accounts) port_row: adw::EntryRow,
    pub(in crate::ui::accounts) smtp_row: adw::EntryRow,
    pub(in crate::ui::accounts) smtp_port_row: adw::EntryRow,
    pub(in crate::ui::accounts) user_row: adw::EntryRow,
    pub(in crate::ui::accounts) pass_row: adw::PasswordEntryRow,
    pub(in crate::ui::accounts) totp_row: adw::EntryRow,
    pub(in crate::ui::accounts) mailbox_pass_row: adw::PasswordEntryRow,
    pub(in crate::ui::accounts) verify_row: adw::ActionRow,
    pub(in crate::ui::accounts) jmap_token_row: adw::SwitchRow,
    pub(in crate::ui::accounts) oauth_client_id_row: adw::EntryRow,
    pub(in crate::ui::accounts) oauth_secret_row: adw::PasswordEntryRow,
    pub(in crate::ui::accounts) oauth_auth_url_row: adw::EntryRow,
    pub(in crate::ui::accounts) oauth_token_url_row: adw::EntryRow,
    pub(in crate::ui::accounts) oauth_scope_row: adw::EntryRow,
    pub(in crate::ui::accounts) ms_advanced: adw::ExpanderRow,
    pub(in crate::ui::accounts) ms_client_row: adw::EntryRow,
    pub(in crate::ui::accounts) ms_tenant_row: adw::EntryRow,
    pub(in crate::ui::accounts) ms_scopes_row: adw::EntryRow,
    pub(in crate::ui::accounts) ms_redirect_row: adw::EntryRow,
    pub(in crate::ui::accounts) oauth_signin_btn: gtk::Button,
    pub(in crate::ui::accounts) oauth_status: gtk::Label,
    pub(in crate::ui::accounts) goa_hint: gtk::Box,
    pub(in crate::ui::accounts) smtp_separate_row: adw::SwitchRow,
    pub(in crate::ui::accounts) smtp_user_row: adw::EntryRow,
    pub(in crate::ui::accounts) smtp_pass_row: adw::PasswordEntryRow,
    pub(in crate::ui::accounts) tls_mismatch_row: adw::SwitchRow,
    pub(in crate::ui::accounts) in_unified_row: adw::SwitchRow,
    pub(in crate::ui::accounts) test_btn: gtk::Button,
    pub(in crate::ui::accounts) test_result: gtk::Label,
    pub(in crate::ui::accounts) preview_disc: gtk::Box,
    pub(in crate::ui::accounts) color_btn: gtk::ColorDialogButton,
    pub(in crate::ui::accounts) gravatar_row: adw::SwitchRow,
    pub(in crate::ui::accounts) mode_glyph_btn: gtk::ToggleButton,
    pub(in crate::ui::accounts) mode_picture_btn: gtk::ToggleButton,
    pub(in crate::ui::accounts) emoji_row: adw::ActionRow,
    pub(in crate::ui::accounts) emoji_clear_btn: gtk::Button,
    pub(in crate::ui::accounts) picture_row: adw::ActionRow,
    pub(in crate::ui::accounts) avatar_clear_btn: gtk::Button,
    pub(in crate::ui::accounts) aliases_group: adw::PreferencesGroup,
    pub(in crate::ui::accounts) aliases_list: gtk::ListBox,
    pub(in crate::ui::accounts) signatures_group: adw::PreferencesGroup,
    pub(in crate::ui::accounts) syncing_group: adw::PreferencesGroup,
    pub(in crate::ui::accounts) openpgp_group: adw::PreferencesGroup,
    pub(in crate::ui::accounts) sig_choice_row: adw::ComboRow,
    pub(in crate::ui::accounts) sig_name_row: adw::EntryRow,
    pub(in crate::ui::accounts) sig_default_row: adw::SwitchRow,
    pub(in crate::ui::accounts) sig_holder: gtk::Box,
    pub(in crate::ui::accounts) push_row: adw::ComboRow,
    pub(in crate::ui::accounts) empty_junk_row: adw::ComboRow,
    pub(in crate::ui::accounts) empty_trash_row: adw::ComboRow,
    pub(in crate::ui::accounts) clean_lists_row: adw::ComboRow,
    pub(in crate::ui::accounts) offline_mail_row: adw::ComboRow,
    pub(in crate::ui::accounts) offline_attachments_row: adw::SwitchRow,
    pub(in crate::ui::accounts) copy_local_row: adw::SwitchRow,
    pub(in crate::ui::accounts) import_group: adw::PreferencesGroup,
    pub(in crate::ui::accounts) graph_group: adw::PreferencesGroup,
    pub(in crate::ui::accounts) graph_directory_row: adw::SwitchRow,
    pub(in crate::ui::accounts) directory_only_row: adw::SwitchRow,
    pub(in crate::ui::accounts) graph_labels_row: adw::SwitchRow,
    pub(in crate::ui::accounts) graph_status: gtk::Label,
    pub(in crate::ui::accounts) folder_sort_row: adw::ComboRow,
    pub(in crate::ui::accounts) folder_sent_row: adw::ComboRow,
    pub(in crate::ui::accounts) folder_sent_copy_row: adw::ComboRow,
    pub(in crate::ui::accounts) reply_copies_row: adw::ComboRow,
    pub(in crate::ui::accounts) server_saves_row: adw::SwitchRow,
    pub(in crate::ui::accounts) folder_drafts_row: adw::ComboRow,
    pub(in crate::ui::accounts) folder_templates_row: adw::ComboRow,
    pub(in crate::ui::accounts) folder_trash_row: adw::ComboRow,
    pub(in crate::ui::accounts) folder_junk_row: adw::ComboRow,
    pub(in crate::ui::accounts) folder_archive_row: adw::ComboRow,
    pub(in crate::ui::accounts) folder_snoozed_row: adw::ComboRow,
    pub(in crate::ui::accounts) hidden_list: gtk::ListBox,
    pub(in crate::ui::accounts) hidden_empty: gtk::Label,
    pub(in crate::ui::accounts) pgp_key_row: adw::ComboRow,
    pub(in crate::ui::accounts) sign_default_row: adw::SwitchRow,
    pub(in crate::ui::accounts) hide_subject_row: adw::SwitchRow,
    pub(in crate::ui::accounts) tuta_group: adw::PreferencesGroup,
    pub(in crate::ui::accounts) confidential_default_row: adw::SwitchRow,
    pub(in crate::ui::accounts) keyring_note: gtk::Label,
}

impl EditorForm {
    pub(in crate::ui::accounts) fn build(sender: &ComponentSender<AccountsWindow>) -> Self {
        let page = adw::PreferencesPage::default();
        // GNOME Online Accounts owns this account's servers and
        // credentials; Hylki only mirrors them. Saying so where
        // the greyed-out fields are is worth more than leaving
        // the user to work out why they can't type.
        // GNOME Online Accounts owns this account's servers and
        // credentials; Hylki only mirrors them, and can hide it
        // locally. Both facts belong together, above the fields
        // they explain.
        // The provider's mark over the form, following
        // the Provider picker (a protocol tile for manual
        // IMAP and custom OAuth). Filled in apply_provider.
        let preferences_group_1 = adw::PreferencesGroup::default();
        let provider_mark = gtk::Box::default();
        provider_mark.set_halign(gtk::Align::Center);
        provider_mark.set_height_request(56);
        provider_mark.set_margin_bottom(6);
        preferences_group_1.add(&provider_mark);
        page.add(&preferences_group_1);
        let goa_banner = adw::PreferencesGroup::default();
        goa_banner.set_visible(false);
        goa_banner.set_title(&i18n("GNOME Online Account"));
        let goa_enabled_row = adw::SwitchRow::default();
        goa_enabled_row.set_title(&i18n("Show in Hylki"));
        goa_enabled_row.set_subtitle(&i18n(
            "Off returns it to the import list. It stays in GNOME Online Accounts.",
        ));
        {
            let sender = sender.clone();
            goa_enabled_row.connect_active_notify(move |row| {
                sender
                    .input_sender()
                    .emit(AccountsInput::ToggleCurrentEnabled(row.is_active()));
            });
        }
        goa_banner.add(&goa_enabled_row);
        let box_2 = gtk::Box::default();
        box_2.set_orientation(gtk::Orientation::Vertical);
        box_2.set_spacing(12);
        box_2.set_halign(gtk::Align::Start);
        box_2.set_margin_top(12);
        let label_3 = gtk::Label::default();
        label_3.set_label(&i18n("Managed by GNOME Online Accounts. Its address, servers and password are changed in Settings \u{2192} Online Accounts."));
        label_3.set_xalign(0.0);
        label_3.set_halign(gtk::Align::Start);
        label_3.set_wrap(true);
        label_3.add_css_class("dim-label");
        box_2.append(&label_3);
        let button_4 = gtk::Button::default();
        button_4.set_label(&i18n("Open Online Accounts\u{2026}"));
        button_4.set_halign(gtk::Align::Start);
        {
            let sender = sender.clone();
            button_4.connect_clicked(move |_| sender.input(AccountsInput::OpenOnlineAccounts));
        }
        box_2.append(&button_4);
        goa_banner.add(&box_2);
        page.add(&goa_banner);
        // The mailbox's storage (#298): shown once the
        // server has said how much is in use, never for
        // one that keeps no limit or cannot say.
        let quota_group = adw::PreferencesGroup::default();
        quota_group.set_visible(false);
        let quota_row = adw::ActionRow::default();
        quota_row.set_title(&i18n("Storage"));
        quota_row.add_css_class("property");
        let quota_bar = gtk::LevelBar::default();
        quota_bar.set_valign(gtk::Align::Center);
        quota_bar.set_width_request(160);
        quota_bar.set_min_value(0.0);
        quota_bar.set_max_value(1.0);
        quota_row.add_suffix(&quota_bar);
        quota_group.add(&quota_row);
        page.add(&quota_group);
        let preferences_group_5 = adw::PreferencesGroup::default();
        preferences_group_5.set_title(&i18n("Mail Account"));
        // The nickname first: it is the one thing
        // most edits are for, and it names the
        // account wherever it is listed, the address
        // standing in while there is none. Where it
        // shows up is said in the title, the way the
        // other rows carry their hints.
        let label_row = adw::EntryRow::default();
        label_row.set_title(&i18n("Nickname"));
        preferences_group_5.add(&label_row);
        // Then the provider; the rest of the form
        // adapts (server fields vs. OAuth sign-in).
        let provider_row = adw::ComboRow::default();
        provider_row.set_title(&i18n("Provider"));
        {
            let sender = sender.clone();
            provider_row
                .connect_selected_notify(move |_| sender.input(AccountsInput::ProviderChanged));
        }
        preferences_group_5.add(&provider_row);
        let name_row = adw::EntryRow::default();
        name_row.set_title(&i18n("Display Name"));
        preferences_group_5.add(&name_row);
        let email_row = adw::EntryRow::default();
        email_row.set_title(&i18n("Email Address"));
        email_row.set_input_purpose(gtk::InputPurpose::Email);
        preferences_group_5.add(&email_row);
        let protocol_row = adw::ComboRow::default();
        protocol_row.set_title(&i18n("Incoming Protocol"));
        {
            let sender = sender.clone();
            protocol_row
                .connect_selected_notify(move |_| sender.input(AccountsInput::ProtocolChanged));
        }
        preferences_group_5.add(&protocol_row);
        let host_row = adw::EntryRow::default();
        host_row.set_title(&i18n("Incoming Server"));
        preferences_group_5.add(&host_row);
        let port_row = adw::EntryRow::default();
        port_row.set_title(&i18n("Port (IMAP 993 / POP3 995)"));
        port_row.set_input_purpose(gtk::InputPurpose::Digits);
        preferences_group_5.add(&port_row);
        let smtp_row = adw::EntryRow::default();
        smtp_row.set_title(&i18n("SMTP Server (optional)"));
        preferences_group_5.add(&smtp_row);
        let smtp_port_row = adw::EntryRow::default();
        smtp_port_row.set_title(&i18n("SMTP Port (default 587)"));
        smtp_port_row.set_input_purpose(gtk::InputPurpose::Digits);
        preferences_group_5.add(&smtp_port_row);
        let user_row = adw::EntryRow::default();
        user_row.set_title(&i18n("Username"));
        preferences_group_5.add(&user_row);
        let pass_row = adw::PasswordEntryRow::default();
        pass_row.set_title(&i18n("Password"));
        preferences_group_5.add(&pass_row);
        // JMAP with a bearer token in place of the
        // username and password (#356).
        let jmap_token_row = adw::SwitchRow::default();
        jmap_token_row.set_title(&i18n("Sign in with an API token"));
        jmap_token_row.set_subtitle(&i18n(
            "For servers, like Fastmail, that take a token instead of a password.",
        ));
        jmap_token_row.set_visible(false);
        {
            let sender = sender.clone();
            jmap_token_row
                .connect_active_notify(move |_| sender.input(AccountsInput::ProtocolChanged));
        }
        preferences_group_5.add(&jmap_token_row);
        // Tuta's and Proton's second factor: hidden until a sign-in
        // comes back asking for it, since most accounts
        // have none and an empty code row reads as one
        // more thing to fill in.
        let totp_row = adw::EntryRow::default();
        totp_row.set_title(&i18n("Authenticator Code"));
        totp_row.set_input_purpose(gtk::InputPurpose::Digits);
        totp_row.set_visible(false);
        preferences_group_5.add(&totp_row);
        // Proton's second password, for accounts that keep
        // their mail behind one: hidden until a sign-in
        // asks for it, as most have a single password.
        let mailbox_pass_row = adw::PasswordEntryRow::default();
        mailbox_pass_row.set_title(&i18n("Mailbox Password"));
        mailbox_pass_row.set_visible(false);
        preferences_group_5.add(&mailbox_pass_row);
        // Proton's human verification, shown once a
        // sign-in comes back asking for it. The challenge
        // opens on its own then; the button brings it back
        // after it was closed.
        let verify_row = adw::ActionRow::default();
        verify_row.set_title(&i18n("Human Verification"));
        verify_row.set_subtitle(&i18n("Proton wants to confirm a person is signing in."));
        verify_row.set_visible(false);
        let button_6 = gtk::Button::default();
        button_6.set_valign(gtk::Align::Center);
        button_6.set_label(&i18n("Verify…"));
        {
            let sender = sender.clone();
            button_6.connect_clicked(move |_| sender.input(AccountsInput::VerifyHuman));
        }
        verify_row.add_suffix(&button_6);
        preferences_group_5.add(&verify_row);
        // ---- OAuth fields (shown when Authentication is an OAuth option) ----
        let oauth_client_id_row = adw::EntryRow::default();
        oauth_client_id_row.set_title(&i18n("OAuth Client ID"));
        oauth_client_id_row.set_visible(false);
        preferences_group_5.add(&oauth_client_id_row);
        let oauth_secret_row = adw::PasswordEntryRow::default();
        oauth_secret_row.set_title(&i18n("OAuth Client Secret (optional)"));
        oauth_secret_row.set_visible(false);
        preferences_group_5.add(&oauth_secret_row);
        let oauth_auth_url_row = adw::EntryRow::default();
        oauth_auth_url_row.set_title(&i18n("Authorization URL"));
        oauth_auth_url_row.set_visible(false);
        preferences_group_5.add(&oauth_auth_url_row);
        let oauth_token_url_row = adw::EntryRow::default();
        oauth_token_url_row.set_title(&i18n("Token URL"));
        oauth_token_url_row.set_visible(false);
        preferences_group_5.add(&oauth_token_url_row);
        let oauth_scope_row = adw::EntryRow::default();
        oauth_scope_row.set_title(&i18n("Scopes (space-separated)"));
        oauth_scope_row.set_visible(false);
        preferences_group_5.add(&oauth_scope_row);
        // Microsoft's own sign-in, made with another app
        // registration (#329): one an organization has
        // approved, say. Empty rows keep Hylki's.
        let ms_advanced = adw::ExpanderRow::default();
        ms_advanced.set_title(&i18n("Advanced"));
        ms_advanced.set_subtitle(&i18n("Sign in with another app registration"));
        ms_advanced.set_visible(false);
        let ms_client_row = adw::EntryRow::default();
        ms_client_row.set_title(&i18n("Client ID"));
        ms_advanced.add_row(&ms_client_row);
        let ms_tenant_row = adw::EntryRow::default();
        ms_tenant_row.set_title(&i18n("Tenant (common, organizations, consumers or an ID)"));
        ms_advanced.add_row(&ms_tenant_row);
        let ms_scopes_row = adw::EntryRow::default();
        ms_scopes_row.set_title(&i18n("Scopes (space-separated)"));
        ms_advanced.add_row(&ms_scopes_row);
        let oauth_signin_btn = gtk::Button::default();
        let ms_redirect_row = adw::EntryRow::default();
        ms_redirect_row.set_title(&i18n("Redirect URI"));
        // A redirect of its own signs in in
        // Hylki's window, not the browser.
        {
            let signin = oauth_signin_btn.clone();
            ms_redirect_row.connect_changed(move |row| {
                signin.set_label(&if row.text().trim().is_empty() {
                    i18n("Sign In with Browser")
                } else {
                    i18n("Sign In")
                });
            });
        }
        ms_advanced.add_row(&ms_redirect_row);
        preferences_group_5.add(&ms_advanced);
        oauth_signin_btn.set_label(&i18n("Sign In with Browser"));
        oauth_signin_btn.set_halign(gtk::Align::Start);
        oauth_signin_btn.set_margin_top(16);
        oauth_signin_btn.set_visible(false);
        oauth_signin_btn.add_css_class("suggested-action");
        {
            let sender = sender.clone();
            oauth_signin_btn.connect_clicked(move |_| sender.input(AccountsInput::OAuthSignIn));
        }
        preferences_group_5.add(&oauth_signin_btn);
        let oauth_status = gtk::Label::default();
        oauth_status.set_visible(false);
        oauth_status.set_halign(gtk::Align::Start);
        oauth_status.set_xalign(0.0);
        oauth_status.set_wrap(true);
        preferences_group_5.add(&oauth_status);
        // Shown for Google/Microsoft when no built-in/own OAuth
        // client is available: point the user at GNOME Online
        // Accounts (the only sign-in path for these providers).
        let goa_hint = gtk::Box::default();
        goa_hint.set_orientation(gtk::Orientation::Vertical);
        goa_hint.set_spacing(12);
        goa_hint.set_margin_top(8);
        goa_hint.set_visible(false);
        let label_7 = gtk::Label::default();
        label_7.set_wrap(true);
        label_7.set_xalign(0.0);
        label_7.add_css_class("dim-label");
        label_7.set_label(&i18n("Google and Microsoft sign in through GNOME Online Accounts. Sign in there, then reopen this window and switch the account on under GNOME Online Accounts."));
        goa_hint.append(&label_7);
        let button_8 = gtk::Button::default();
        button_8.set_label(&i18n("Open Online Accounts…"));
        button_8.set_halign(gtk::Align::Start);
        button_8.add_css_class("suggested-action");
        {
            let sender = sender.clone();
            button_8.connect_clicked(move |_| sender.input(AccountsInput::OpenOnlineAccounts));
        }
        goa_hint.append(&button_8);
        preferences_group_5.add(&goa_hint);
        let smtp_separate_row = adw::SwitchRow::default();
        smtp_separate_row.set_title(&i18n("Separate SMTP credentials"));
        smtp_separate_row.set_subtitle(&i18n("Off uses the credentials above."));
        preferences_group_5.add(&smtp_separate_row);
        let smtp_user_row = adw::EntryRow::default();
        smtp_user_row.set_title(&i18n("SMTP Username"));
        preferences_group_5.add(&smtp_user_row);
        let smtp_pass_row = adw::PasswordEntryRow::default();
        smtp_pass_row.set_title(&i18n("SMTP Password"));
        preferences_group_5.add(&smtp_pass_row);
        let tls_mismatch_row = adw::SwitchRow::default();
        tls_mismatch_row.set_title(&i18n("Accept a certificate for another name"));
        tls_mismatch_row.set_subtitle(&i18n(
            "For shared hosting. Turn on only when the connection test says the names differ.",
        ));
        preferences_group_5.add(&tls_mismatch_row);
        // Off keeps a mailbox read on its own out of
        // the merged lists (#267); its own section,
        // the tray and notifications are unchanged.
        let in_unified_row = adw::SwitchRow::default();
        in_unified_row.set_title(&i18n("Show in All Inboxes"));
        in_unified_row.set_subtitle(&i18n(
            "Also in the unified Starred, Sent, Drafts, Archive, Filters and Tags.",
        ));
        in_unified_row.set_active(true);
        preferences_group_5.add(&in_unified_row);
        let box_9 = gtk::Box::default();
        box_9.set_orientation(gtk::Orientation::Vertical);
        box_9.set_spacing(6);
        box_9.set_margin_top(16);
        let test_btn = gtk::Button::default();
        test_btn.set_label(&i18n("Test Connection"));
        test_btn.set_halign(gtk::Align::Start);
        {
            let sender = sender.clone();
            test_btn.connect_clicked(move |_| sender.input(AccountsInput::TestConnection));
        }
        box_9.append(&test_btn);
        let test_result = gtk::Label::default();
        test_result.set_visible(false);
        test_result.set_halign(gtk::Align::Start);
        test_result.set_xalign(0.0);
        test_result.set_wrap(true);
        // The text can be copied out (#246):
        // an error is often something to
        // paste into a bug report.
        test_result.set_selectable(true);
        box_9.append(&test_result);
        preferences_group_5.add(&box_9);
        page.add(&preferences_group_5);
        let preferences_group_10 = adw::PreferencesGroup::default();
        preferences_group_10.set_title(&i18n("Appearance"));
        // The sidebar circle as it will look, as the
        // group's first row (a non-row child would
        // land below the rows): the accent color,
        // and the picture, emoji or initials it
        // shows. Not a button — it neither activates
        // nor selects.
        let preferences_row_11 = adw::PreferencesRow::default();
        preferences_row_11.set_activatable(false);
        preferences_row_11.set_selectable(false);
        preferences_row_11.set_can_focus(false);
        let box_12 = gtk::Box::default();
        box_12.set_orientation(gtk::Orientation::Vertical);
        box_12.set_spacing(2);
        box_12.set_halign(gtk::Align::Center);
        box_12.set_margin_top(10);
        box_12.set_margin_bottom(10);
        box_12.set_margin_start(10);
        box_12.set_margin_end(10);
        let preview_disc = gtk::Box::default();
        preview_disc.add_css_class("account-circle");
        preview_disc.add_css_class("account-preview-disc");
        preview_disc.set_size_request(72, 72);
        preview_disc.set_halign(gtk::Align::Center);
        preview_disc.set_hexpand(false);
        preview_disc.set_overflow(gtk::Overflow::Hidden);
        box_12.append(&preview_disc);
        preferences_row_11.set_child(Some(&box_12));
        preferences_group_10.add(&preferences_row_11);
        // The accent stands on its own: it colors
        // the circle, and marks the account's
        // folders and mail elsewhere.
        let action_row_13 = adw::ActionRow::default();
        action_row_13.set_title(&i18n("Account accent color"));
        let color_btn = gtk::ColorDialogButton::default();
        color_btn.set_valign(gtk::Align::Center);
        color_btn.set_dialog(&gtk::ColorDialog::new());
        {
            let sender = sender.clone();
            color_btn.connect_rgba_notify(move |_| {
                sender.input_sender().emit(AccountsInput::RefreshPreview);
            });
        }
        action_row_13.add_suffix(&color_btn);
        preferences_group_10.add(&action_row_13);
        // This mailbox's own Gravatar (#189), ahead
        // of everything below — when the address has
        // one. Off by default: the lookup tells a
        // third party the address is in use here.
        let gravatar_row = adw::SwitchRow::default();
        gravatar_row.set_title(&i18n("Use my Gravatar"));
        gravatar_row.set_subtitle(&i18n("Sends gravatar.com a hash of the address."));
        {
            let sender = sender.clone();
            gravatar_row.connect_active_notify(move |row| {
                sender
                    .input_sender()
                    .emit(AccountsInput::SetOwnGravatar(row.is_active()));
            });
        }
        preferences_group_10.add(&gravatar_row);
        // What the circle shows (#162): initials or
        // an emoji, or a picture. The row below
        // follows the choice.
        let action_row_14 = adw::ActionRow::default();
        action_row_14.set_title(&i18n("Circle shows"));
        let box_15 = gtk::Box::default();
        box_15.add_css_class("linked");
        box_15.set_valign(gtk::Align::Center);
        let mode_glyph_btn = gtk::ToggleButton::default();
        mode_glyph_btn.set_label(&i18n("Initials or emoji"));
        mode_glyph_btn.set_active(true);
        box_15.append(&mode_glyph_btn);
        let mode_picture_btn = gtk::ToggleButton::default();
        mode_picture_btn.set_label(&i18n("Picture"));
        mode_picture_btn.set_group(Some(&mode_glyph_btn));
        {
            let sender = sender.clone();
            mode_picture_btn.connect_toggled(move |b| {
                sender
                    .input_sender()
                    .emit(AccountsInput::SetPictureMode(b.is_active()));
            });
        }
        box_15.append(&mode_picture_btn);
        action_row_14.add_suffix(&box_15);
        preferences_group_10.add(&action_row_14);
        let emoji_row = adw::ActionRow::default();
        emoji_row.set_title(&i18n("Emoji"));
        emoji_row.set_subtitle(&i18n("Instead of initials."));
        let emoji_btn = gtk::MenuButton::default();
        emoji_btn.set_valign(gtk::Align::Center);
        emoji_btn.set_label(&i18n("Choose…"));
        let emoji_chooser_16 = gtk::EmojiChooser::default();
        {
            let sender = sender.clone();
            emoji_chooser_16.connect_emoji_picked(move |_, text| {
                sender
                    .input_sender()
                    .emit(AccountsInput::SetEmoji(text.to_string()));
            });
        }
        emoji_btn.set_popover(Some(&emoji_chooser_16));
        emoji_row.add_suffix(&emoji_btn);
        let emoji_clear_btn = gtk::Button::default();
        emoji_clear_btn.set_valign(gtk::Align::Center);
        emoji_clear_btn.set_label(&i18n("Use initials"));
        emoji_clear_btn.set_visible(false);
        {
            let sender = sender.clone();
            emoji_clear_btn.connect_clicked(move |_| sender.input(AccountsInput::ClearGlyph));
        }
        emoji_row.add_suffix(&emoji_clear_btn);
        preferences_group_10.add(&emoji_row);
        let picture_row = adw::ActionRow::default();
        picture_row.set_title(&i18n("Picture"));
        picture_row.set_visible(false);
        let button_17 = gtk::Button::default();
        button_17.set_valign(gtk::Align::Center);
        button_17.set_label(&i18n("Choose…"));
        {
            let sender = sender.clone();
            button_17.connect_clicked(move |_| sender.input(AccountsInput::PickAvatar));
        }
        picture_row.add_suffix(&button_17);
        let avatar_clear_btn = gtk::Button::default();
        avatar_clear_btn.set_valign(gtk::Align::Center);
        avatar_clear_btn.set_label(&i18n("Remove"));
        avatar_clear_btn.set_visible(false);
        {
            let sender = sender.clone();
            avatar_clear_btn.connect_clicked(move |_| sender.input(AccountsInput::ClearGlyph));
        }
        picture_row.add_suffix(&avatar_clear_btn);
        preferences_group_10.add(&picture_row);
        page.add(&preferences_group_10);
        // Send-as aliases (#34): extra From identities the
        // composer offers, and replies to an alias answer
        // from it. Each alias sends through this account's
        // SMTP, or — for a forwarded mailbox whose provider
        // would rewrite the sender — through its own.
        let aliases_group = adw::PreferencesGroup::default();
        aliases_group.set_title(&i18n("Send-as aliases"));
        aliases_group.set_description(Some(
            i18n("Extra addresses this account can send as.").as_str(),
        ));
        // At the header's end, styled like the Add
        // Filter…, Add Tag… and cloud Add Account…
        // buttons.
        let box_18 = gtk::Box::default();
        box_18.set_orientation(gtk::Orientation::Vertical);
        box_18.set_valign(gtk::Align::Start);
        box_18.set_halign(gtk::Align::End);
        box_18.set_margin_start(24);
        let button_19 = gtk::Button::default();
        button_19.set_label(&i18n("Add Alias…"));
        button_19.set_size_request(130, -1);
        {
            let sender = sender.clone();
            button_19.connect_clicked(move |_| sender.input(AccountsInput::AliasAdd));
        }
        box_18.append(&button_19);
        aliases_group.set_header_suffix(Some(&box_18));
        let aliases_list = gtk::ListBox::default();
        aliases_list.add_css_class("boxed-list");
        aliases_list.set_selection_mode(gtk::SelectionMode::None);
        {
            let sender = sender.clone();
            aliases_list.connect_row_activated(move |_, row| {
                sender
                    .input_sender()
                    .emit(AccountsInput::AliasEdit(row.index() as usize));
            });
        }
        aliases_group.add(&aliases_list);
        page.add(&aliases_group);
        let signatures_group = adw::PreferencesGroup::default();
        signatures_group.set_title(&i18n("Signatures"));
        signatures_group.set_description(Some(
                        i18n("With more than one, the composer's signature button allows you to pick between them.")
                            .as_str()
                    ));
        // A designed signature usually exists as HTML
        // already (#120): bring it in from its file.
        // Its source is edited in place, from the
        // editor's own toolbar. Stacked and 130px
        // wide, like Add Alias… and the other
        // groups' buttons.
        let box_20 = gtk::Box::default();
        box_20.set_orientation(gtk::Orientation::Vertical);
        box_20.set_spacing(6);
        box_20.set_valign(gtk::Align::Start);
        box_20.set_halign(gtk::Align::End);
        box_20.set_margin_start(24);
        let button_21 = gtk::Button::default();
        button_21.set_label(&i18n("Add Signature"));
        button_21.set_size_request(130, -1);
        {
            let sender = sender.clone();
            button_21.connect_clicked(move |_| sender.input(AccountsInput::SigAdd));
        }
        box_20.append(&button_21);
        let button_22 = gtk::Button::default();
        button_22.set_label(&i18n("Import File…"));
        button_22.set_size_request(130, -1);
        {
            let sender = sender.clone();
            button_22.connect_clicked(move |_| sender.input(AccountsInput::SignatureImport));
        }
        box_20.append(&button_22);
        signatures_group.set_header_suffix(Some(&box_20));
        // Which signature the editor below holds,
        // and its name: shown once there are two.
        let sig_choice_row = adw::ComboRow::default();
        sig_choice_row.set_title(&i18n("Signature"));
        sig_choice_row.set_visible(false);
        {
            let sender = sender.clone();
            sig_choice_row.connect_selected_notify(move |_| sender.input(AccountsInput::SigShow));
        }
        signatures_group.add(&sig_choice_row);
        let sig_name_row = adw::EntryRow::default();
        sig_name_row.set_title(&i18n("Name"));
        sig_name_row.set_visible(false);
        {
            let sender = sender.clone();
            sig_name_row.connect_changed(move |_| sender.input(AccountsInput::SigRename));
        }
        let button_23 = gtk::Button::default();
        button_23.set_icon_name("user-trash-symbolic");
        button_23.set_valign(gtk::Align::Center);
        button_23.set_tooltip_text(Some(&i18n("Delete Signature")));
        button_23.add_css_class("flat");
        {
            let sender = sender.clone();
            button_23.connect_clicked(move |_| sender.input(AccountsInput::SigDelete));
        }
        sig_name_row.add_suffix(&button_23);
        signatures_group.add(&sig_name_row);
        let sig_default_row = adw::SwitchRow::default();
        sig_default_row.set_title(&i18n("Start messages with this signature"));
        {
            let sender = sender.clone();
            sig_default_row.connect_active_notify(move |_| sender.input(AccountsInput::SigDefault));
        }
        signatures_group.add(&sig_default_row);
        let sig_holder = gtk::Box::default();
        sig_holder.set_orientation(gtk::Orientation::Vertical);
        sig_holder.set_margin_top(6);
        signatures_group.add(&sig_holder);
        page.add(&signatures_group);
        // Per-account push override (#91): some servers
        // mishandle IDLE, and one bad account shouldn't
        // cost the good ones their instant delivery.
        let syncing_group = adw::PreferencesGroup::default();
        syncing_group.set_title(&i18n("Syncing"));
        let push_row = adw::ComboRow::default();
        push_row.set_title(&i18n("Instant new mail (push)"));
        push_row.set_subtitle(&i18n(
            "Turn off for servers that stall on push connections.",
        ));
        syncing_group.add(&push_row);
        // Auto-empty (#140): mail past the chosen age in
        // Junk / Trash is deleted for good at each sync.
        let empty_junk_row = adw::ComboRow::default();
        empty_junk_row.set_title(&i18n("Empty Junk automatically"));
        syncing_group.add(&empty_junk_row);
        let empty_trash_row = adw::ComboRow::default();
        empty_trash_row.set_title(&i18n("Empty Trash automatically"));
        syncing_group.add(&empty_trash_row);
        // Mailing-list mail past the chosen age goes
        // to the Trash (#407).
        let clean_lists_row = adw::ComboRow::default();
        clean_lists_row.set_title(&i18n("Move list mail to Trash"));
        clean_lists_row.set_subtitle(&i18n(
            "Inbox mail with an unsubscribe link older than this gets trashed unless it's starred.",
        ));
        syncing_group.add(&clean_lists_row);
        // Mail downloaded ahead of being opened (#536).
        let offline_mail_row = adw::ComboRow::default();
        offline_mail_row.set_title(&i18n("Keep mail cached"));
        offline_mail_row.set_subtitle(&i18n("Leaves out Trash, Junk and Drafts."));
        syncing_group.add(&offline_mail_row);
        let offline_attachments_row = adw::SwitchRow::default();
        offline_attachments_row.set_title(&i18n("Include attachments"));
        offline_attachments_row.set_subtitle(&i18n("Can consume a lot of hard drive space."));
        syncing_group.add(&offline_attachments_row);
        // Every message copied to On This Computer (#552).
        let copy_local_row = adw::SwitchRow::default();
        copy_local_row.set_title(&i18n("Copy mail to On This Computer"));
        copy_local_row.set_subtitle(&i18n("Downloads copies of every message from every folder of this account to this computer (except Trash, Junk and Drafts) and stores them in On This Computer."));
        syncing_group.add(&copy_local_row);
        page.add(&syncing_group);
        // On This Computer only (#552): mail from another
        // program's folders.
        let import_group = adw::PreferencesGroup::default();
        import_group.set_title(&i18n("Import"));
        import_group.set_description(Some(
            i18n("Mail already here is not imported again.").as_str(),
        ));
        import_group.set_visible(false);
        let action_row_24 = adw::ActionRow::default();
        action_row_24.set_title(&i18n("Maildir folders"));
        action_row_24.set_subtitle(&i18n(
            "Evolution's On This Computer, Dovecot and other Maildir trees.",
        ));
        let button_25 = gtk::Button::default();
        button_25.set_label(&i18n("Choose…"));
        button_25.set_valign(gtk::Align::Center);
        {
            let sender = sender.clone();
            button_25.connect_clicked(move |_| sender.input(AccountsInput::ImportLocal(false)));
        }
        action_row_24.add_suffix(&button_25);
        import_group.add(&action_row_24);
        let action_row_26 = adw::ActionRow::default();
        action_row_26.set_title(&i18n("Mbox files"));
        action_row_26.set_subtitle(&i18n("Thunderbird's folders and other mbox files."));
        let button_27 = gtk::Button::default();
        button_27.set_label(&i18n("Choose…"));
        button_27.set_valign(gtk::Align::Center);
        {
            let sender = sender.clone();
            button_27.connect_clicked(move |_| sender.input(AccountsInput::ImportLocal(true)));
        }
        action_row_26.add_suffix(&button_27);
        import_group.add(&action_row_26);
        page.add(&import_group);
        // What a Microsoft 365 account can use from
        // Microsoft Graph, in one place: the
        // directory's suggestions (#410) and names for
        // sensitivity labels (#411). Each asks for its
        // own access when turned on.
        let graph_group = adw::PreferencesGroup::default();
        graph_group.set_visible(false);
        graph_group.set_title(&i18n("Microsoft 365"));
        let graph_directory_row = adw::SwitchRow::default();
        graph_directory_row.set_title(&i18n("Search the organization directory"));
        graph_directory_row
            .set_subtitle(&i18n("Suggests people from your organization as you type."));
        {
            let sender = sender.clone();
            graph_directory_row.connect_active_notify(move |row| {
                sender
                    .input_sender()
                    .emit(AccountsInput::SetGraphDirectory(row.is_active()));
            });
        }
        graph_group.add(&graph_directory_row);
        let directory_only_row = adw::SwitchRow::default();
        directory_only_row.set_title(&i18n("Suggest only from this account"));
        directory_only_row.set_subtitle(&i18n(
            "Suggestions come from the directory and this account's mail only.",
        ));
        graph_group.add(&directory_only_row);
        let graph_labels_row = adw::SwitchRow::default();
        graph_labels_row.set_title(&i18n("Show sensitivity label names"));
        graph_labels_row.set_subtitle(&i18n(
            "Shows a label's name when a message gives only its ID.",
        ));
        {
            let sender = sender.clone();
            graph_labels_row.connect_active_notify(move |row| {
                sender
                    .input_sender()
                    .emit(AccountsInput::SetGraphLabels(row.is_active()));
            });
        }
        graph_group.add(&graph_labels_row);
        // How the last request for access went.
        let graph_status = gtk::Label::default();
        graph_status.set_visible(false);
        graph_status.set_halign(gtk::Align::Start);
        graph_status.set_xalign(0.0);
        graph_status.set_wrap(true);
        graph_status.set_selectable(true);
        graph_status.set_margin_top(8);
        graph_group.add(&graph_status);
        page.add(&graph_group);
        // Its own folder order, or Settings → Sidebar's.
        let preferences_group_28 = adw::PreferencesGroup::default();
        preferences_group_28.set_title(&i18n("Folder Order"));
        preferences_group_28.set_description(Some(
            i18n("Also in the right-click menu of its Folders heading.").as_str(),
        ));
        let folder_sort_row = adw::ComboRow::default();
        folder_sort_row.set_title(&i18n("Sort by"));
        preferences_group_28.add(&folder_sort_row);
        page.add(&preferences_group_28);
        // Manual special-folder mapping (#82): for servers
        // whose Sent/Trash/… aren't detected, pin each role
        // to one of the account's real folders. "Save
        // copies in" (#199) rides with them because it is
        // read as a refinement of Sent, but it is a
        // different kind of thing: a role says what a
        // folder *is*, and relabelling the Inbox costs the
        // account its inbox (#136), which is why the Inbox
        // is offered in that row and in none of the others.
        let preferences_group_29 = adw::PreferencesGroup::default();
        preferences_group_29.set_title(&i18n("Special Folders"));
        preferences_group_29.set_description(Some(
                        i18n("Follows the server's own markings unless you pick a folder. Nothing already on the server moves.").as_str()
                    ));
        let folder_sent_row = adw::ComboRow::default();
        folder_sent_row.set_title(&i18n("Sent"));
        preferences_group_29.add(&folder_sent_row);
        let folder_sent_copy_row = adw::ComboRow::default();
        folder_sent_copy_row.set_title(&i18n("Save a copy of sent mail in"));
        folder_sent_copy_row.set_subtitle(&i18n(SENT_COPY_HINT));
        preferences_group_29.add(&folder_sent_copy_row);
        // Replies beside what they answer (#468), as
        // Evolution offers. Microsoft 365 files its
        // own copy and POP3 has no folders, so the
        // row is for IMAP and JMAP only.
        let reply_copies_row = adw::ComboRow::default();
        reply_copies_row.set_title(&i18n("Save a copy of replies in"));
        reply_copies_row.set_subtitle(&i18n(REPLY_COPIES_HINT));
        preferences_group_29.add(&reply_copies_row);
        // Gmail files a copy of anything sent through
        // its SMTP, so Hylki's append makes a second
        // one. Off by default: a server that does not
        // do it, paired with a Hylki that has stopped
        // appending, keeps no sent mail at all.
        let server_saves_row = adw::SwitchRow::default();
        server_saves_row.set_title(&i18n("Server saves its own copy of sent mail"));
        server_saves_row.set_subtitle(&i18n(
            "For Gmail and others that file sent mail themselves.",
        ));
        {
            let sender = sender.clone();
            server_saves_row.connect_active_notify(move |row| {
                sender
                    .input_sender()
                    .emit(AccountsInput::SetServerSavesSent(row.is_active()));
            });
        }
        preferences_group_29.add(&server_saves_row);
        let folder_drafts_row = adw::ComboRow::default();
        folder_drafts_row.set_title(&i18n("Drafts"));
        preferences_group_29.add(&folder_drafts_row);
        // Nothing on a server marks a Templates folder
        // (#360): one called Templates is found, any
        // other is pointed at here.
        let folder_templates_row = adw::ComboRow::default();
        folder_templates_row.set_title(&i18n("Templates"));
        preferences_group_29.add(&folder_templates_row);
        let folder_trash_row = adw::ComboRow::default();
        folder_trash_row.set_title(&i18n("Trash"));
        preferences_group_29.add(&folder_trash_row);
        let folder_junk_row = adw::ComboRow::default();
        folder_junk_row.set_title(&i18n("Junk"));
        preferences_group_29.add(&folder_junk_row);
        let folder_archive_row = adw::ComboRow::default();
        folder_archive_row.set_title(&i18n("Archive"));
        preferences_group_29.add(&folder_archive_row);
        // Snoozed mail waits here (#527), found by name
        // like Templates.
        let folder_snoozed_row = adw::ComboRow::default();
        folder_snoozed_row.set_title(&i18n("Snoozed"));
        preferences_group_29.add(&folder_snoozed_row);
        page.add(&preferences_group_29);
        // Hidden folders (#239): what "Hide Folder" in the
        // sidebar took out of sight, each with the way back.
        let hidden_group = adw::PreferencesGroup::default();
        hidden_group.set_title(&i18n("Hidden Folders"));
        hidden_group.set_description(Some(
            i18n(
                "Hidden folders are not synced. Hide one from its right-click menu in the sidebar.",
            )
            .as_str(),
        ));
        let hidden_list = gtk::ListBox::default();
        hidden_list.add_css_class("boxed-list");
        hidden_list.set_selection_mode(gtk::SelectionMode::None);
        hidden_group.add(&hidden_list);
        let hidden_empty = gtk::Label::default();
        hidden_empty.set_label(&i18n("No hidden folders."));
        hidden_empty.add_css_class("dim-label");
        hidden_empty.set_xalign(0.0);
        hidden_group.add(&hidden_empty);
        page.add(&hidden_group);
        // OpenPGP (#133): which of the user's keys this
        // account signs and decrypts with.
        let openpgp_group = adw::PreferencesGroup::default();
        openpgp_group.set_title(&i18n("OpenPGP"));
        openpgp_group.set_description(Some(
            i18n("Keys are managed under OpenPGP in the sidebar.").as_str(),
        ));
        let pgp_key_row = adw::ComboRow::default();
        pgp_key_row.set_title(&i18n("Key"));
        pgp_key_row.set_subtitle(&i18n("Automatic uses the key whose address matches."));
        openpgp_group.add(&pgp_key_row);
        // For an account whose mail is always signed
        // (#267): the composer's toggle starts on,
        // and still turns it off for one message.
        let sign_default_row = adw::SwitchRow::default();
        sign_default_row.set_title(&i18n("Sign messages by default"));
        openpgp_group.add(&sign_default_row);
        // Protected headers (#493): on, an encrypted
        // message says "..." where its subject would be.
        let hide_subject_row = adw::SwitchRow::default();
        hide_subject_row.set_title(&i18n("Hide the subject of encrypted mail"));
        hide_subject_row.set_subtitle(&i18n(
            "Only the recipients see the subject. Others see \"...\".",
        ));
        openpgp_group.add(&hide_subject_row);
        page.add(&openpgp_group);
        // Tuta's confidential mail: end-to-end encrypted, with a
        // password for each recipient outside Tuta. Shown for a
        // Tuta account only (see `apply_protocol`).
        let tuta_group = adw::PreferencesGroup::default();
        tuta_group.set_title(&i18n("Tuta"));
        tuta_group.set_visible(false);
        let confidential_default_row = adw::SwitchRow::default();
        confidential_default_row.set_title(&i18n("Send confidential by default"));
        confidential_default_row.set_subtitle(&i18n("New messages start end-to-end encrypted. Recipients outside Tuta then need a password."));
        confidential_default_row.set_active(true);
        tuta_group.add(&confidential_default_row);
        page.add(&tuta_group);
        // For a GOA-imported account this removes it from
        // Hylki only — it stays in GNOME Online Accounts and
        // returns to the import list.
        let preferences_group_30 = adw::PreferencesGroup::default();
        // Not for a GNOME Online Account: GNOME holds its
        // sign-in, and Google and Microsoft have no password.
        let keyring_note = gtk::Label::default();
        keyring_note.set_wrap(true);
        keyring_note.set_xalign(0.0);
        keyring_note.add_css_class("dim-label");
        keyring_note.add_css_class("caption");
        keyring_note.set_label(&i18n(
            "Your password is kept in the system keyring, never in plain text.",
        ));
        preferences_group_30.add(&keyring_note);
        page.add(&preferences_group_30);
        Self {
            page,
            provider_mark,
            goa_banner,
            goa_enabled_row,
            quota_group,
            quota_row,
            quota_bar,
            label_row,
            provider_row,
            name_row,
            email_row,
            protocol_row,
            host_row,
            port_row,
            smtp_row,
            smtp_port_row,
            user_row,
            pass_row,
            totp_row,
            mailbox_pass_row,
            verify_row,
            jmap_token_row,
            oauth_client_id_row,
            oauth_secret_row,
            oauth_auth_url_row,
            oauth_token_url_row,
            oauth_scope_row,
            ms_advanced,
            ms_client_row,
            ms_tenant_row,
            ms_scopes_row,
            ms_redirect_row,
            oauth_signin_btn,
            oauth_status,
            goa_hint,
            smtp_separate_row,
            smtp_user_row,
            smtp_pass_row,
            tls_mismatch_row,
            in_unified_row,
            test_btn,
            test_result,
            preview_disc,
            color_btn,
            gravatar_row,
            mode_glyph_btn,
            mode_picture_btn,
            emoji_row,
            emoji_clear_btn,
            picture_row,
            avatar_clear_btn,
            aliases_group,
            aliases_list,
            signatures_group,
            syncing_group,
            openpgp_group,
            sig_choice_row,
            sig_name_row,
            sig_default_row,
            sig_holder,
            push_row,
            empty_junk_row,
            empty_trash_row,
            clean_lists_row,
            offline_mail_row,
            offline_attachments_row,
            copy_local_row,
            import_group,
            graph_group,
            graph_directory_row,
            directory_only_row,
            graph_labels_row,
            graph_status,
            folder_sort_row,
            folder_sent_row,
            folder_sent_copy_row,
            reply_copies_row,
            server_saves_row,
            folder_drafts_row,
            folder_templates_row,
            folder_trash_row,
            folder_junk_row,
            folder_archive_row,
            folder_snoozed_row,
            hidden_list,
            hidden_empty,
            pgp_key_row,
            sign_default_row,
            hide_subject_row,
            tuta_group,
            confidential_default_row,
            keyring_note,
        }
    }
}
