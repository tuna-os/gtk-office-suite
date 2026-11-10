use adw::{prelude::*, subclass::prelude::*};
use as_variant::as_variant;
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use matrix_sdk_ui::timeline::TimelineItemContent;
use ruma::events::rtc::notification::CallIntent;

use super::{EventTimestamp, ReadReceiptsList};
use crate::{
    gettext_f,
    prelude::*,
    session::{Event, Member},
    utils::{BoundObject, BoundObjectWeakRef},
};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/call_row.ui")]
    #[properties(wrapper_type = super::CallRow)]
    pub struct CallRow {
        /// The call icon shown to the user.
        #[template_child]
        icon: TemplateChild<gtk::Image>,
        /// The text shown to the user.
        #[template_child]
        inner_label: TemplateChild<gtk::Label>,
        /// The `RtcNotification` event displayed by this widget.
        #[property(get, set = Self::set_event, explicit_notify)]
        event: BoundObject<Event>,
        /// The sender of the event that is presented.
        #[property(get = Self::sender, explicit_notify)]
        sender: BoundObjectWeakRef<Member>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CallRow {
        const NAME: &'static str = "CallRow";
        type Type = super::CallRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            EventTimestamp::ensure_type();
            ReadReceiptsList::ensure_type();

            Self::bind_template(klass);
            klass.set_css_name("call-row");
            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for CallRow {}

    impl WidgetImpl for CallRow {}
    impl BinImpl for CallRow {}

    impl CallRow {
        /// Set the event presented by this row.
        fn set_event(&self, event: Event) {
            let obj = self.obj();

            // Update CSS classes of the parent
            if let Some(row) = self.obj().parent() {
                row.add_css_class("has-icon");
            }

            // Listen for redactions, as they're allowed by the protocol, to
            // make sure we're always rendering reasonable state.
            self.event.disconnect_signals();
            let item_changed_handler = event.connect_item_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_content();
                }
            ));
            self.event.set(event, vec![item_changed_handler]);

            self.sender.disconnect_signals();

            if let Some(sender) = self.sender() {
                let handler = sender.connect_disambiguated_name_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_content();
                    }
                ));

                self.sender.set(&sender, vec![handler]);
            }
            obj.notify_event();
            obj.notify_sender();

            self.update_content();
        }

        /// The sender of the event that is presented.
        fn sender(&self) -> Option<Member> {
            self.event.obj().map(|event| event.sender())
        }

        /// Update the content for the current state.
        fn update_content(&self) {
            let Some(sender) = self.sender() else {
                return;
            };

            let call_intent = self.event.obj().and_then(|event| {
                as_variant!(event.content(), TimelineItemContent::RtcNotification { call_intent, .. } => call_intent)?
            });
            if let Some(CallIntent::Video) = call_intent {
                let text = if sender.is_own_user() {
                    gettext("Outgoing video call.")
                } else {
                    gettext_f(
                        // Translators: Do NOT translate the content between '{' and '}', this
                        // is a variable name.
                        "Incoming video call from {user}. Use another client to answer.",
                        &[("user", &sender.disambiguated_name())],
                    )
                };

                self.inner_label.set_text(&text);
                self.icon.set_icon_name(Some("video-symbolic"));
            } else {
                let text = if sender.is_own_user() {
                    gettext("Outgoing call.")
                } else {
                    gettext_f(
                        // Translators: Do NOT translate the content between '{' and '}', this
                        // is a variable name.
                        "Incoming call from {user}. Use another client to answer.",
                        &[("user", &sender.disambiguated_name())],
                    )
                };

                self.inner_label.set_text(&text);
                self.icon.set_icon_name(Some("phone-right-facing-symbolic"));
            }
        }
    }
}

glib::wrapper! {
    /// A row showing an `m.rtc.notification` event ([MSC4075]) in the timeline.
    ///
    /// [MSC4075]: https://github.com/matrix-org/matrix-spec-proposals/pull/4075
    pub struct CallRow(ObjectSubclass<imp::CallRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CallRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for CallRow {
    fn default() -> Self {
        Self::new()
    }
}
