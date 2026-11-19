//! Tuta and Proton sign-in in the welcome wizard's manual form. Fork-only:
//! `manual.rs` keeps only the `uses_helper()` calls that lead here, so an
//! upstream merge has as little fork code as possible to conflict with
//! (fork/MAINTAINING.md, rule 2).

use super::*;

impl Welcome {
    /// Tuta or Proton: sign in through the helper, which opens the session
    /// the account keeps in place of the password. A second factor, a
    /// mailbox password or a human verification brings out its row, and
    /// the next Test & Add sends what it holds.
    pub(super) fn sign_in_with_helper(
        &mut self,
        widgets: &WelcomeWidgets,
        sender: &ComponentSender<Self>,
        protocol: Protocol,
        email: String,
        password: String,
    ) {
        if email.is_empty() || password.is_empty() {
            widgets
                .status_lbl
                .set_css_classes(&["welcome-hint", "error"]);
            widgets
                .status_lbl
                .set_text(&i18n("Enter your email address and password first."));
            return;
        }
        let account = AccountConfig {
            name: widgets.name_row.text().trim().to_string(),
            email: email.clone(),
            protocol,
            imap_port: 0,
            smtp_port: 0,
            username: email.clone(),
            ..blank_account()
        };
        let (hv_token, hv_type) = self.human_check.solved();
        let mut login = worker::HelperLogin {
            email,
            password,
            totp: totp_code(&widgets.totp_row),
            mailbox_password: mailbox_password(&widgets.mailbox_pass_row),
            hv_token,
            hv_type,
            pending: None,
        };
        login.pending = self.second_factor.begin(&login);
        self.test_seq += 1;
        let seq = self.test_seq;
        widgets.test_spinner.set_visible(true);
        widgets.test_spinner.start();
        widgets.status_lbl.set_css_classes(&["welcome-hint"]);
        widgets.status_lbl.set_text(&i18n("Signing in…"));
        sender.oneshot_command(async move {
            let result = tokio::task::spawn_blocking(move || {
                worker::helper_login_blocking(protocol, &login)
            })
            .await
            .unwrap_or_else(|_| Err(worker::HelperError::new("helper_failed")));
            WelcomeCmd::HelperSignedIn {
                account: Box::new(account),
                result,
                seq,
            }
        });
    }

    /// Show Proton's challenge for the form's sign-in: when a sign-in
    /// comes back asking for it, and from the Verify button after.
    pub(super) fn on_verify_human(
        &mut self,
        widgets: &WelcomeWidgets,
        sender: &ComponentSender<Self>,
    ) {
        let Some(challenge) = self.human_check.challenge() else {
            return;
        };
        let input = sender.input_sender().clone();
        proton_verify::present(&widgets.verify_row, challenge, move |verified| {
            if let Some(verified) = verified {
                input.emit(WelcomeInput::HumanVerified(verified));
            }
        });
    }

    pub(super) fn on_helper_signed_in(
        &mut self,
        mut account: Box<AccountConfig>,
        result: Result<String, worker::HelperError>,
        seq: u32,
        widgets: &WelcomeWidgets,
        sender: ComponentSender<Self>,
    ) {
        if seq != self.test_seq {
            return;
        }
        self.second_factor.settle(result.as_ref().err());
        widgets.test_spinner.stop();
        widgets.test_spinner.set_visible(false);
        let session = match result {
            Ok(session) => session,
            Err(e) => {
                match self.human_check.stopped(&e) {
                    proton_verify::HumanStep::Retry => {
                        return self.on_test_and_add(widgets, sender);
                    }
                    proton_verify::HumanStep::Show => self.on_verify_human(widgets, &sender),
                    proton_verify::HumanStep::Nothing => {}
                }
                return show_sign_in_failure(widgets, &e);
            }
        };
        // The session is what the account keeps; the passwords it bought
        // are not stored, and are cleared from the form with the code.
        account.helper_session = session;
        self.human_check.clear();
        let email = account.email.clone();
        self.added.push(email.clone());
        widgets
            .status_lbl
            .set_css_classes(&["welcome-hint", "success"]);
        widgets.status_lbl.set_text(&i18n_f(
            "✓ {email} — signed in and added",
            &[("email", &email)],
        ));
        widgets.pass_row.set_text("");
        hide_sign_in_rows(&sign_in_rows(widgets));
        widgets.email_row.set_text("");
        let _ = sender.output(WelcomeOutput::AddAccount(account));
        self.manual_added = true;
        self.refresh_footer(widgets);
    }
}

/// The rows a Tuta or Proton sign-in can ask for, in the wizard's form.
pub(super) fn sign_in_rows(widgets: &WelcomeWidgets) -> SignInRows<'_> {
    SignInRows {
        code: &widgets.totp_row,
        mailbox_password: &widgets.mailbox_pass_row,
        verify: &widgets.verify_row,
    }
}

/// Why a Tuta or Proton sign-in did not finish, under the button. A second
/// factor, a mailbox password or a challenge is a step rather than a
/// failure: its row comes out, and the password stays so the next Test &
/// Add can send both. The helper's text already says what failed, so it
/// stands alone.
pub(super) fn show_sign_in_failure(widgets: &WelcomeWidgets, error: &worker::HelperError) {
    let text = reveal_sign_in_row(&sign_in_rows(widgets), error);
    let classes: &[&str] = if is_sign_in_step(error) {
        &["welcome-hint"]
    } else {
        &["welcome-hint", "error"]
    };
    widgets.status_lbl.set_css_classes(classes);
    widgets.status_lbl.set_text(&text);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wizard's form for Tuta: no servers, the code row hidden until a
    /// sign-in asks for a second factor, and hidden again for another
    /// provider. Ignored by default: it needs a display. Run it on its own
    /// (GTK is initialised once per process, from one thread):
    ///
    /// ```text
    /// cargo test -- --ignored the_wizard_asks_for_a_tuta_code_only_when_needed
    /// ```
    #[test]
    #[ignore = "needs a display"]
    fn the_wizard_asks_for_a_tuta_code_only_when_needed() {
        gtk::init().expect("a display");
        adw::init().expect("libadwaita");
        let settle = || while gtk::glib::MainContext::default().iteration(false) {};
        let wizard = Welcome::builder().launch(()).detach();
        let tuta = wizard_providers()
            .position(|p| p.wizard_protocol() == Protocol::Tuta)
            .expect("Tuta in the wizard") as u32;
        wizard.widgets().provider_row.set_selected(tuta);
        settle();
        {
            let w = wizard.widgets();
            // The rows' own visibility: the window itself is never shown.
            assert!(w.email_row.get_visible());
            assert!(w.pass_row.get_visible());
            assert!(!w.server_exp.get_visible());
            assert!(!w.totp_row.get_visible());
            assert_eq!(w.add_btn.label().as_deref(), Some("Test & Add"));

            // Tuta wants a second factor: the code row comes out, with a
            // hint rather than an error.
            w.email_row.set_text("me@tuta.com");
            w.pass_row.set_text("secret");
            show_sign_in_failure(&w, &worker::HelperError::new("second_factor_required"));
            assert!(w.totp_row.get_visible());
            assert!(!w.status_lbl.has_css_class("error"));
            assert_eq!(w.pass_row.text(), "secret");
            w.totp_row.set_text("123 456");
            assert_eq!(totp_code(&w.totp_row).as_deref(), Some("123456"));

            // Any other answer is an error, and leaves the row as it was.
            show_sign_in_failure(&w, &worker::HelperError::new("invalid_totp"));
            assert!(w.status_lbl.has_css_class("error"));
            assert!(w.totp_row.get_visible());
        }
        // Another provider puts the code row away.
        let manual = wizard_providers()
            .position(|p| p.wizard_is_manual())
            .expect("the manual entry") as u32;
        wizard.widgets().provider_row.set_selected(manual);
        settle();
        let w = wizard.widgets();
        assert!(!w.totp_row.get_visible());
        assert!(w.server_exp.get_visible());
    }

    /// The wizard's form for native Proton: no servers, and the code,
    /// mailbox password and human verification rows each hidden until a
    /// sign-in asks for it, then hidden again for another provider. The
    /// Bridge preset beside it keeps its servers. Ignored by default: it
    /// needs a display. Run it on its own:
    ///
    /// ```text
    /// cargo test -- --ignored the_wizard_reveals_protons_rows_only_when_needed
    /// ```
    #[test]
    #[ignore = "needs a display"]
    fn the_wizard_reveals_protons_rows_only_when_needed() {
        gtk::init().expect("a display");
        adw::init().expect("libadwaita");
        let settle = || while gtk::glib::MainContext::default().iteration(false) {};
        let wizard = Welcome::builder().launch(()).detach();
        let proton = wizard_providers()
            .position(|p| p.wizard_protocol() == Protocol::Proton)
            .expect("Proton in the wizard") as u32;
        wizard.widgets().provider_row.set_selected(proton);
        settle();
        {
            let w = wizard.widgets();
            assert!(w.email_row.get_visible());
            assert!(w.pass_row.get_visible());
            assert!(!w.server_exp.get_visible());
            assert!(!w.totp_row.get_visible());
            assert!(!w.mailbox_pass_row.get_visible());
            assert!(!w.verify_row.get_visible());
            assert_eq!(w.add_btn.label().as_deref(), Some("Test & Add"));
            assert!(w.hint_lbl.get_visible());
            assert!(w.hint_lbl.text().contains("without Proton Mail Bridge"));

            w.email_row.set_text("me@proton.me");
            w.pass_row.set_text("secret");

            // Each step brings out its own row, as a hint, and keeps the
            // password for the next Test & Add.
            show_sign_in_failure(&w, &worker::HelperError::new("second_factor_required"));
            assert!(w.totp_row.get_visible());
            assert!(!w.mailbox_pass_row.get_visible());
            assert!(!w.status_lbl.has_css_class("error"));

            show_sign_in_failure(&w, &worker::HelperError::new("mailbox_password_required"));
            assert!(w.mailbox_pass_row.get_visible());
            assert!(!w.status_lbl.has_css_class("error"));
            assert_eq!(w.pass_row.text(), "secret");
            w.mailbox_pass_row.set_text("mailbox secret");
            assert_eq!(
                mailbox_password(&w.mailbox_pass_row).as_deref(),
                Some("mailbox secret")
            );
            show_sign_in_failure(
                &w,
                &worker::HelperError::with_challenge(worker::HvChallenge {
                    token: "t".into(),
                    methods: vec!["captcha".into()],
                }),
            );
            assert!(w.verify_row.get_visible());
            assert!(!w.status_lbl.has_css_class("error"));

            // A wrong mailbox password is an error.
            show_sign_in_failure(&w, &worker::HelperError::new("mailbox_password_failed"));
            assert!(w.status_lbl.has_css_class("error"));
        }
        // Bridge is the IMAP preset it always was: its servers, none of
        // the native rows, and what was typed in them gone.
        let bridge = wizard_providers()
            .position(|p| p.wizard_label() == "Proton Mail (Bridge)")
            .expect("the Bridge preset") as u32;
        wizard.widgets().provider_row.set_selected(bridge);
        settle();
        let w = wizard.widgets();
        assert!(w.server_exp.get_visible());
        assert_eq!(w.host_row.text(), "127.0.0.1");
        assert_eq!(w.port_row.text(), "1143");
        assert!(!w.totp_row.get_visible());
        assert!(!w.mailbox_pass_row.get_visible());
        assert!(!w.verify_row.get_visible());
        assert_eq!(w.mailbox_pass_row.text(), "");
    }
}
