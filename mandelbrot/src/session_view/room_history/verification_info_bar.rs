use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use crate::{
    Window,
    components::LoadingButton,
    gettext_f,
    prelude::*,
    session::{IdentityVerification, VerificationState},
    toast,
    utils::BoundObjectWeakRef,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/verification_info_bar.ui"
    )]
    #[properties(wrapper_type = super::VerificationInfoBar)]
    pub struct VerificationInfoBar {
        #[template_child]
        revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        label: TemplateChild<gtk::Label>,
        #[template_child]
        accept_btn: TemplateChild<LoadingButton>,
        #[template_child]
        cancel_btn: TemplateChild<LoadingButton>,
        /// The identity verification presented by this info bar.
        #[property(get, set = Self::set_verification, explicit_notify)]
        verification: BoundObjectWeakRef<IdentityVerification>,
        user_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VerificationInfoBar {
        const NAME: &'static str = "ContentVerificationInfoBar";
        type Type = super::VerificationInfoBar;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_css_name("infobar");
            klass.set_accessible_role(gtk::AccessibleRole::Group);

            klass.install_action_async("verification.accept", None, |obj, _, _| async move {
                let Some(window) = obj.root().and_downcast::<Window>() else {
                    return;
                };
                let Some(verification) = obj.verification() else {
                    return;
                };
                let imp = obj.imp();

                if verification.state() == VerificationState::Requested {
                    imp.accept_btn.set_is_loading(true);

                    if verification.accept().await.is_err() {
                        toast!(obj, gettext("Could not accept verification"));
                        imp.accept_btn.set_is_loading(false);
                        return;
                    }
                }

                window
                    .session_view()
                    .select_identity_verification(verification);
                imp.accept_btn.set_is_loading(false);
            });

            klass.install_action_async("verification.decline", None, |obj, _, _| async move {
                let Some(verification) = obj.verification() else {
                    return;
                };
                let imp = obj.imp();

                imp.cancel_btn.set_is_loading(true);

                if verification.cancel().await.is_err() {
                    toast!(obj, gettext("Could not decline verification"));
                }

                imp.cancel_btn.set_is_loading(false);
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for VerificationInfoBar {
        fn dispose(&self) {
            if let Some(verification) = self.verification.obj()
                && let Some(handler) = self.user_handler.take()
            {
                verification.user().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for VerificationInfoBar {}
    impl BinImpl for VerificationInfoBar {}

    impl VerificationInfoBar {
        /// Set the identity verification presented by this info bar.
        fn set_verification(&self, verification: Option<&IdentityVerification>) {
            let prev_verification = self.verification.obj();

            if prev_verification.as_ref() == verification {
                return;
            }

            if let Some(verification) = prev_verification
                && let Some(handler) = self.user_handler.take()
            {
                verification.user().disconnect(handler);
            }
            self.verification.disconnect_signals();

            if let Some(verification) = verification {
                let user_handler = verification.user().connect_display_name_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_bar();
                    }
                ));
                self.user_handler.replace(Some(user_handler));

                let state_handler = verification.connect_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_bar();
                    }
                ));

                self.verification.set(verification, vec![state_handler]);
            }

            self.update_bar();
            self.obj().notify_verification();
        }

        /// Update the bar for the current verification state.
        fn update_bar(&self) {
            let Some(verification) = self.verification.obj().filter(|v| !v.is_finished()) else {
                self.revealer.set_reveal_child(false);
                return;
            };

            if matches!(verification.state(), VerificationState::Requested) {
                self.label.set_markup(&gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "{user_name} wants to be verified",
                    &[(
                        "user_name",
                        &format!("<b>{}</b>", verification.user().display_name()),
                    )],
                ));
                self.accept_btn.set_label(&gettext("Verify"));
                self.cancel_btn.set_label(&gettext("Decline"));
            } else {
                self.label.set_label(&gettext("Verification in progress"));
                self.accept_btn.set_label(&gettext("Continue"));
                self.cancel_btn.set_label(&gettext("Cancel"));
            }

            self.revealer.set_reveal_child(true);
        }
    }
}

glib::wrapper! {
    /// An info bar presenting an ongoing identity verification.
    pub struct VerificationInfoBar(ObjectSubclass<imp::VerificationInfoBar>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl VerificationInfoBar {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
