//! Tuta and Proton sign-in in the account editor. Fork-only: the upstream
//! files (`form.rs`, `fields.rs`) keep only the match arms and the
//! `uses_helper()` calls that lead here, so an upstream merge has as little
//! fork code as possible to conflict with (fork/MAINTAINING.md, rule 2).

use super::*;

/// Clear what a Tuta or Proton account has no use for. The helper reaches
/// the provider's own servers and signs in with the address, so the
/// server, SMTP and push rows are hidden for it, and would only carry what
/// another provider picked before it left in them.
pub(super) fn drop_server_fields(account: &mut AccountConfig) {
    account.imap_host.clear();
    account.imap_port = 0;
    account.smtp_host.clear();
    account.smtp_port = 0;
    account.username = account.email.clone();
    account.smtp_separate = false;
    account.smtp_username.clear();
    account.smtp_password.clear();
    account.tls_accept_hostname_mismatch = false;
    account.jmap_token = false;
    account.push = None;
}

/// The second-factor code to sign in with: the code row's text without the
/// spaces authenticator apps show in it ("123 456"), or none while the row
/// is hidden or empty.
pub(crate) fn totp_code(row: &adw::EntryRow) -> Option<String> {
    if !row.get_visible() {
        return None;
    }
    code_without_spaces(&row.text())
}

fn code_without_spaces(text: &str) -> Option<String> {
    let code: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    (!code.is_empty()).then_some(code)
}

/// The rows a Tuta or Proton sign-in can ask for after the password, each
/// hidden until it does. Shared with the welcome wizard.
pub(crate) struct SignInRows<'a> {
    pub(crate) code: &'a adw::EntryRow,
    pub(crate) mailbox_password: &'a adw::PasswordEntryRow,
    pub(crate) verify: &'a adw::ActionRow,
}

/// The editor's own.
pub(super) fn sign_in_rows(widgets: &AccountsWindowWidgets) -> SignInRows<'_> {
    SignInRows {
        code: &widgets.form.totp_row,
        mailbox_password: &widgets.form.mailbox_pass_row,
        verify: &widgets.form.verify_row,
    }
}

/// Put the rows away, and what was typed in them: a code is good for one
/// sign-in, and a mailbox password is not kept once it opened a session.
pub(crate) fn hide_sign_in_rows(rows: &SignInRows) {
    rows.code.set_text("");
    rows.code.set_visible(false);
    rows.mailbox_password.set_text("");
    rows.mailbox_password.set_visible(false);
    rows.verify.set_visible(false);
}

/// The mailbox password to sign in with, or none while its row is hidden
/// or empty. Taken as typed: a password may hold spaces.
pub(crate) fn mailbox_password(row: &adw::PasswordEntryRow) -> Option<String> {
    if !row.get_visible() {
        return None;
    }
    let text = row.text();
    (!text.is_empty()).then(|| text.to_string())
}

/// Whether a sign-in stopped for something the form can give it (a code,
/// a mailbox password, a solved challenge) rather than failing. A demand
/// for verification without a challenge to show cannot be met from here.
pub(crate) fn is_sign_in_step(error: &crate::worker::HelperError) -> bool {
    error.needs_second_factor()
        || error.needs_mailbox_password()
        || (error.needs_human_verification() && error.hv_challenge().is_some())
}

/// What to say about a Tuta or Proton sign-in that did not finish, with
/// the row it asks for brought out: a step is not a failure, and the next
/// sign-in sends what the row holds. The password stays in its row for
/// that sign-in. Shared with the welcome wizard.
pub(crate) fn reveal_sign_in_row(rows: &SignInRows, error: &crate::worker::HelperError) -> String {
    if error.needs_second_factor() {
        rows.code.set_text("");
        rows.code.set_visible(true);
        rows.code.grab_focus();
        return i18n("Enter the code from your authenticator app, then sign in again.");
    }
    if error.needs_mailbox_password() {
        rows.mailbox_password.set_text("");
        rows.mailbox_password.set_visible(true);
        rows.mailbox_password.grab_focus();
        return i18n("This account has a separate mailbox password. Enter it, then sign in again.");
    }
    if error.needs_human_verification() && error.hv_challenge().is_some() {
        rows.verify.set_visible(true);
        return i18n(
            "Proton wants to confirm a person is signing in. Solve the challenge to continue.",
        );
    }
    error.message()
}

impl AccountsWindow {
    /// Sign in to Tuta or Proton through the helper with the form's address
    /// and password and whatever else the provider has asked for by now: a
    /// code, a mailbox password, a solved challenge. The answer comes back
    /// as [`AccountsCmd::HelperSignIn`]. Save starts one as well, with
    /// `then_save`, when a password was typed but not yet signed in with.
    pub(super) fn start_helper_sign_in(
        &mut self,
        widgets: &AccountsWindowWidgets,
        sender: &ComponentSender<Self>,
        then_save: bool,
    ) {
        let protocol = form_protocol(widgets);
        let email = trimmed(&widgets.form.email_row);
        let password = widgets.form.pass_row.text().to_string();
        widgets.form.test_result.set_visible(true);
        if email.is_empty() || password.is_empty() {
            widgets.form.test_result.set_css_classes(&["error"]);
            widgets
                .form
                .test_result
                .set_label(&i18n("Enter your email address and password first."));
            return;
        }
        let (hv_token, hv_type) = self.human_check.solved();
        let mut login = worker::HelperLogin {
            email: email.clone(),
            password,
            totp: totp_code(&widgets.form.totp_row),
            mailbox_password: mailbox_password(&widgets.form.mailbox_pass_row),
            hv_token,
            hv_type,
            pending: None,
        };
        login.pending = self.second_factor.begin(&login);
        widgets.form.test_btn.set_sensitive(false);
        widgets.form.test_result.set_css_classes(&["dim-label"]);
        widgets.form.test_result.set_label(&i18n("Signing in…"));
        let editor = self.editor_session;
        sender.oneshot_command(async move {
            let result = tokio::task::spawn_blocking(move || {
                worker::helper_login_blocking(protocol, &login)
            })
            .await
            .unwrap_or_else(|_| Err(worker::HelperError::new("helper_failed")));
            AccountsCmd::HelperSignIn {
                editor,
                then_save,
                result: result.map(|session| (protocol, email, session)),
            }
        });
    }

    pub(super) fn on_helper_signed_in(
        &mut self,
        editor: u64,
        then_save: bool,
        result: Result<(Protocol, String, String), worker::HelperError>,
        widgets: &AccountsWindowWidgets,
        sender: ComponentSender<Self>,
    ) {
        // Started in an editor since closed or replaced.
        if editor != self.editor_session {
            return;
        }
        self.second_factor.settle(result.as_ref().err());
        widgets.form.test_btn.set_sensitive(true);
        widgets.form.test_result.set_visible(true);
        let e = match result {
            Ok(signed_in) => {
                let name = helper_name(signed_in.0);
                self.pending_helper_session = Some(signed_in);
                self.human_check.clear();
                // The session stands in for the password from here on, so
                // neither password is kept even in the form.
                widgets.form.pass_row.set_text("");
                hide_sign_in_rows(&sign_in_rows(widgets));
                widgets.form.test_result.set_css_classes(&["success"]);
                widgets
                    .form
                    .test_result
                    .set_label(&i18n_f("✓ {provider}: signed in", &[("provider", name)]));
                if then_save {
                    sender.input_sender().emit(AccountsInput::Save);
                }
                return;
            }
            Err(e) => e,
        };
        match self.human_check.stopped(&e) {
            proton_verify::HumanStep::Retry => {
                self.start_helper_sign_in(widgets, &sender, then_save);
                return;
            }
            proton_verify::HumanStep::Show => {
                self.human_check_then_save = then_save;
                self.on_verify_human(widgets, &sender);
            }
            proton_verify::HumanStep::Nothing => {}
        }
        let step = is_sign_in_step(&e);
        let text = reveal_sign_in_row(&sign_in_rows(widgets), &e);
        widgets
            .form
            .test_result
            .set_css_classes(&[if step { "dim-label" } else { "error" }]);
        widgets.form.test_result.set_label(&if step {
            text
        } else {
            let name = helper_name(form_protocol(widgets));
            i18n_f("✗ {provider}: {e}", &[("provider", name), ("e", &text)])
        });
    }

    /// Show Proton's challenge for the editor's sign-in: when a sign-in
    /// comes back asking for it, and from the Verify button after.
    pub(super) fn on_verify_human(
        &mut self,
        widgets: &AccountsWindowWidgets,
        sender: &ComponentSender<Self>,
    ) {
        let Some(challenge) = self.human_check.challenge() else {
            return;
        };
        let input = sender.input_sender().clone();
        proton_verify::present(&widgets.form.verify_row, challenge, move |verified| {
            if let Some(verified) = verified {
                input.emit(AccountsInput::HumanVerified(verified));
            }
        });
    }

    /// The challenge was solved: sign in again at once with its token, and
    /// with the code the account may also want, so the token is not spent
    /// on a sign-in that stops there.
    pub(super) fn on_human_verified(
        &mut self,
        verified: proton_verify::Verified,
        widgets: &AccountsWindowWidgets,
        sender: &ComponentSender<Self>,
    ) {
        if !self.human_check.solve(verified) {
            return;
        }
        let then_save = std::mem::take(&mut self.human_check_then_save);
        self.start_helper_sign_in(widgets, sender, then_save);
    }
}

/// The provider a helper signs in to, as its result lines name it. A brand,
/// so not translated.
pub(super) fn helper_name(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Proton => "Proton Mail",
        _ => "Tuta",
    }
}

#[cfg(test)]
mod tests {
    use super::super::providers::{manual_index, ProviderKind};
    use super::*;

    #[test]
    fn a_tuta_account_keeps_no_servers() {
        let mut acc = AccountConfig {
            email: "me@tuta.com".into(),
            protocol: Protocol::Tuta,
            // What an IMAP preset picked first leaves in the hidden rows.
            imap_host: "imap.mail.me.com".into(),
            imap_port: 993,
            smtp_host: "smtp.mail.me.com".into(),
            smtp_port: 587,
            username: "someone-else".into(),
            smtp_separate: true,
            smtp_username: "smtp-user".into(),
            smtp_password: "smtp-pw".into(),
            tls_accept_hostname_mismatch: true,
            push: Some(true),
            ..crate::config::blank_account()
        };
        drop_server_fields(&mut acc);
        assert_eq!((acc.imap_host.as_str(), acc.imap_port), ("", 0));
        assert_eq!((acc.smtp_host.as_str(), acc.smtp_port), ("", 0));
        assert_eq!(acc.username, "me@tuta.com");
        assert!(!acc.smtp_separate);
        assert!(acc.smtp_username.is_empty() && acc.smtp_password.is_empty());
        assert!(!acc.tls_accept_hostname_mismatch);
        assert_eq!(acc.push, None);
    }

    #[test]
    fn codes_lose_the_spaces_apps_show() {
        assert_eq!(code_without_spaces("123 456").as_deref(), Some("123456"));
        assert_eq!(code_without_spaces(" 123456\n").as_deref(), Some("123456"));
        assert_eq!(code_without_spaces("   "), None);
        assert_eq!(code_without_spaces(""), None);
    }

    /// A Tuta account through the real editor: filled in, read back, the
    /// rows the form shows for it, and the code row a second factor brings
    /// out. Ignored by default: it needs a display. Run it on its own (GTK
    /// is initialised once per process, from one thread):
    ///
    /// ```text
    /// cargo test -- --ignored tuta_account_round_trips_through_the_editor
    /// ```
    #[test]
    #[ignore = "needs a display"]
    fn tuta_account_round_trips_through_the_editor() {
        gtk::init().expect("a display");
        adw::init().expect("libadwaita");
        let panel = AccountsWindow::builder()
            .launch(AccountsInit {
                accounts: Vec::new(),
                allowed_senders: Vec::new(),
                blacklist: Vec::new(),
                filters: Vec::new(),
                tags: Vec::new(),
                smart_folders: Vec::new(),
            })
            .detach();
        let w = panel.widgets();
        let acc = AccountConfig {
            name: "Me".into(),
            email: "me@tuta.com".into(),
            protocol: Protocol::Tuta,
            username: "me@tuta.com".into(),
            imap_port: 0,
            smtp_port: 0,
            ..crate::config::blank_account()
        };
        fill_editor(&w, &acc);
        panel.model().apply_provider(&w);
        assert_eq!(form_protocol(&w), Protocol::Tuta);

        let back = read_account(&w, None, None);
        assert_eq!(back.protocol, Protocol::Tuta);
        assert_eq!(back.name, "Me");
        assert_eq!(back.email, "me@tuta.com");
        assert_eq!(back.username, "me@tuta.com");
        assert_eq!((back.imap_host.as_str(), back.imap_port), ("", 0));
        assert_eq!((back.smtp_host.as_str(), back.smtp_port), ("", 0));
        assert!(back.password.is_empty());
        assert!(back.helper_session.is_empty());
        assert!(!back.oauth);

        // The address and password, and nothing about servers.
        assert!(w.form.email_row.get_visible());
        assert!(w.form.pass_row.get_visible());
        for row in [
            w.form.protocol_row.upcast_ref::<gtk::Widget>(),
            w.form.host_row.upcast_ref(),
            w.form.port_row.upcast_ref(),
            w.form.smtp_row.upcast_ref(),
            w.form.smtp_port_row.upcast_ref(),
            w.form.user_row.upcast_ref(),
            w.form.smtp_separate_row.upcast_ref(),
            w.form.tls_mismatch_row.upcast_ref(),
            w.form.jmap_token_row.upcast_ref(),
        ] {
            assert!(!row.get_visible(), "{}", row.type_().name());
        }
        assert!(!w.form.totp_row.get_visible());
        assert!(!w.form.mailbox_pass_row.get_visible());
        assert!(!w.form.verify_row.get_visible());

        // A second factor brings the code row out, and the code is sent
        // without the space the app showed in it.
        let second_factor = crate::worker::HelperError::new("second_factor_required");
        let hint = reveal_sign_in_row(&sign_in_rows(&w), &second_factor);
        assert!(!hint.is_empty());
        assert!(is_sign_in_step(&second_factor));
        assert!(w.form.totp_row.get_visible());
        w.form.totp_row.set_text("123 456");
        assert_eq!(totp_code(&w.form.totp_row).as_deref(), Some("123456"));

        // Another provider puts it away again.
        w.form.provider_row.set_selected(manual_index());
        panel.model().apply_provider(&w);
        assert!(!w.form.totp_row.get_visible());
        assert!(w.form.host_row.get_visible());
        assert_eq!(totp_code(&w.form.totp_row), None);
    }

    #[test]
    fn a_proton_account_keeps_no_servers() {
        let mut acc = AccountConfig {
            email: "me@proton.me".into(),
            protocol: Protocol::Proton,
            // What the Bridge preset picked first leaves in the hidden rows.
            imap_host: "127.0.0.1".into(),
            imap_port: 1143,
            smtp_host: "127.0.0.1".into(),
            smtp_port: 1025,
            username: "bridge-user".into(),
            ..crate::config::blank_account()
        };
        drop_server_fields(&mut acc);
        assert_eq!((acc.imap_host.as_str(), acc.imap_port), ("", 0));
        assert_eq!((acc.smtp_host.as_str(), acc.smtp_port), ("", 0));
        assert_eq!(acc.username, "me@proton.me");
    }

    /// A native Proton account through the real editor: filled in, read
    /// back, the rows the form shows for it, and the three rows a sign-in
    /// can ask for (code, mailbox password, human verification), each
    /// hidden until it does. Ignored by default: it needs a display. Run it
    /// on its own (GTK is initialised once per process, from one thread):
    ///
    /// ```text
    /// cargo test -- --ignored proton_account_round_trips_through_the_editor
    /// ```
    #[test]
    #[ignore = "needs a display"]
    fn proton_account_round_trips_through_the_editor() {
        use crate::worker::{HelperError, HvChallenge};
        gtk::init().expect("a display");
        adw::init().expect("libadwaita");
        let panel = AccountsWindow::builder()
            .launch(AccountsInit {
                accounts: Vec::new(),
                allowed_senders: Vec::new(),
                blacklist: Vec::new(),
                filters: Vec::new(),
                tags: Vec::new(),
                smart_folders: Vec::new(),
            })
            .detach();
        let w = panel.widgets();
        let acc = AccountConfig {
            name: "Me".into(),
            email: "me@proton.me".into(),
            protocol: Protocol::Proton,
            username: "me@proton.me".into(),
            imap_port: 0,
            smtp_port: 0,
            ..crate::config::blank_account()
        };
        fill_editor(&w, &acc);
        panel.model().apply_provider(&w);
        assert_eq!(form_protocol(&w), Protocol::Proton);
        assert_eq!(
            provider_at(w.form.provider_row.selected()).label,
            "Proton Mail"
        );
        assert_eq!(w.form.test_btn.label().as_deref(), Some("Sign In"));

        let back = read_account(&w, None, None);
        assert_eq!(back.protocol, Protocol::Proton);
        assert_eq!(back.name, "Me");
        assert_eq!(back.email, "me@proton.me");
        assert_eq!(back.username, "me@proton.me");
        assert_eq!((back.imap_host.as_str(), back.imap_port), ("", 0));
        assert_eq!((back.smtp_host.as_str(), back.smtp_port), ("", 0));
        assert!(back.password.is_empty());
        assert!(back.helper_session.is_empty());
        assert!(!back.oauth);

        // The address and password, nothing about servers, and none of
        // the rows a sign-in asks for until it does.
        assert!(w.form.email_row.get_visible());
        assert!(w.form.pass_row.get_visible());
        for row in [
            w.form.protocol_row.upcast_ref::<gtk::Widget>(),
            w.form.host_row.upcast_ref(),
            w.form.port_row.upcast_ref(),
            w.form.smtp_row.upcast_ref(),
            w.form.smtp_port_row.upcast_ref(),
            w.form.user_row.upcast_ref(),
            w.form.smtp_separate_row.upcast_ref(),
            w.form.tls_mismatch_row.upcast_ref(),
            w.form.jmap_token_row.upcast_ref(),
            w.form.totp_row.upcast_ref(),
            w.form.mailbox_pass_row.upcast_ref(),
            w.form.verify_row.upcast_ref(),
        ] {
            assert!(!row.get_visible(), "{}", row.type_().name());
        }
        let rows = sign_in_rows(&w);

        // A second factor: the code row.
        let second_factor = HelperError::new("second_factor_required");
        assert!(is_sign_in_step(&second_factor));
        reveal_sign_in_row(&rows, &second_factor);
        assert!(w.form.totp_row.get_visible());
        assert!(!w.form.mailbox_pass_row.get_visible());
        assert!(!w.form.verify_row.get_visible());
        hide_sign_in_rows(&rows);

        // A two-password account: the mailbox password row, sent as typed
        // (spaces and all) and only while it shows.
        let mailbox = HelperError::new("mailbox_password_required");
        assert!(is_sign_in_step(&mailbox));
        let hint = reveal_sign_in_row(&rows, &mailbox);
        assert!(!hint.is_empty());
        assert!(w.form.mailbox_pass_row.get_visible());
        assert!(!w.form.totp_row.get_visible());
        assert_eq!(mailbox_password(&w.form.mailbox_pass_row), None);
        w.form.mailbox_pass_row.set_text(" two words ");
        assert_eq!(
            mailbox_password(&w.form.mailbox_pass_row).as_deref(),
            Some(" two words ")
        );
        // Never part of the account read back.
        let back = read_account(&w, None, None);
        assert!(!format!("{back:?}").contains("two words"));
        hide_sign_in_rows(&rows);
        assert_eq!(w.form.mailbox_pass_row.text(), "");
        assert_eq!(mailbox_password(&w.form.mailbox_pass_row), None);

        // Human verification: its row, when the refusal carries a
        // challenge to show.
        let challenge = HelperError::with_challenge(HvChallenge {
            token: "t".into(),
            methods: vec!["captcha".into()],
        });
        assert!(is_sign_in_step(&challenge));
        reveal_sign_in_row(&rows, &challenge);
        assert!(w.form.verify_row.get_visible());
        assert!(!w.form.totp_row.get_visible());
        assert!(!w.form.mailbox_pass_row.get_visible());
        hide_sign_in_rows(&rows);
        // Without one there is nothing to solve: an error, no row.
        let bare = HelperError::new("human_verification_required");
        assert!(!is_sign_in_step(&bare));
        assert_eq!(reveal_sign_in_row(&rows, &bare), bare.message());
        assert!(!w.form.verify_row.get_visible());

        // Tuta puts Proton's own rows away, and keeps the code row.
        reveal_sign_in_row(&rows, &mailbox);
        reveal_sign_in_row(&rows, &challenge);
        w.form.provider_row.set_selected(
            PROVIDERS
                .iter()
                .position(|p| p.kind == ProviderKind::Tuta)
                .expect("Tuta") as u32,
        );
        panel.model().apply_provider(&w);
        assert!(!w.form.mailbox_pass_row.get_visible());
        assert!(!w.form.verify_row.get_visible());
        // The Bridge preset is an IMAP account with its servers shown.
        w.form.provider_row.set_selected(
            PROVIDERS
                .iter()
                .position(|p| p.label == "Proton Mail (Bridge)")
                .expect("Bridge") as u32,
        );
        panel.model().apply_provider(&w);
        assert_eq!(form_protocol(&w), Protocol::Imap);
        assert!(w.form.host_row.get_visible());
        assert_eq!(w.form.host_row.text(), "127.0.0.1");
        assert!(!w.form.mailbox_pass_row.get_visible());
        assert!(!w.form.verify_row.get_visible());
    }
}
