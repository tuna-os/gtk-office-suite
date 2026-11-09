use adw::{prelude::*, subclass::prelude::*};
use gtk::{gdk, glib, glib::clone};
use tracing::error;

mod audio;
mod caption;
mod content;
mod file;
mod info;
mod location;
mod message_state_stack;
mod poll;
mod reaction;
mod reaction_list;
mod reply;
mod sender_name;
mod text;
mod visual_media;

pub use self::content::{ContentFormat, MessageContent};
use self::{
    message_state_stack::MessageStateStack, reaction_list::MessageReactionList,
    sender_name::MessageSenderName,
};
use super::{EventTimestamp, ReadReceiptsList};
use crate::{
    components::UserProfileDialog,
    ngettext_f,
    prelude::*,
    session::{Event, EventHeaderState, Member},
    utils::BoundObject,
};

mod imp {
    use std::{cell::RefCell, marker::PhantomData};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_row/mod.ui")]
    #[properties(wrapper_type = super::MessageRow)]
    pub struct MessageRow {
        #[template_child]
        avatar_button: TemplateChild<gtk::Button>,
        #[template_child]
        header: TemplateChild<gtk::Box>,
        #[template_child]
        display_name: TemplateChild<MessageSenderName>,
        #[template_child]
        content: TemplateChild<MessageContent>,
        #[template_child]
        message_state: TemplateChild<MessageStateStack>,
        #[template_child]
        reactions: TemplateChild<MessageReactionList>,
        #[template_child]
        thread_indicator: TemplateChild<gtk::Button>,
        #[template_child]
        thread_indicator_label: TemplateChild<gtk::Label>,
        binding: RefCell<Option<glib::Binding>>,
        /// The event that is presented.
        #[property(get, set = Self::set_event, explicit_notify)]
        event: BoundObject<Event>,
        /// The sender of the event that is presented.
        #[property(get = Self::sender)]
        sender: PhantomData<Option<Member>>,
        /// The texture of the image preview displayed by the descendant of this
        /// widget, if any.
        #[property(get = Self::texture)]
        texture: PhantomData<Option<gdk::Texture>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageRow {
        const NAME: &'static str = "ContentMessageRow";
        type Type = super::MessageRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            EventTimestamp::ensure_type();
            ReadReceiptsList::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
            klass.set_css_name("message-row");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MessageRow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.content.connect_format_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |content| {
                    imp.reactions.set_visible(!matches!(
                        content.format(),
                        ContentFormat::Compact | ContentFormat::Ellipsized
                    ));
                }
            ));
            self.content.connect_texture_notify(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.notify_texture();
                }
            ));
        }

        fn dispose(&self) {
            if let Some(binding) = self.binding.take() {
                binding.unbind();
            }
        }
    }

    impl WidgetImpl for MessageRow {}
    impl BinImpl for MessageRow {}

    #[gtk::template_callbacks]
    impl MessageRow {
        /// Set the event that is presented.
        fn set_event(&self, event: Event) {
            let obj = self.obj();

            // Remove signals and bindings from the previous event.
            self.event.disconnect_signals();
            if let Some(binding) = self.binding.take() {
                binding.unbind();
            }

            let sender = event.sender();
            self.display_name.set_sender(Some(sender));

            let state_binding = event
                .bind_property("state", &*self.message_state, "state")
                .sync_create()
                .build();

            self.binding.replace(Some(state_binding));

            let header_state_handler = event.connect_header_state_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_header();
                }
            ));

            // Listening to changes in the source might not be enough, there are changes
            // that we display that do not affect the source, like related events.
            let item_changed_handler = event.connect_item_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_content();
                    imp.update_thread_indicator();
                }
            ));

            self.reactions
                .set_reaction_list(&event.room().get_or_create_members(), &event.reactions());
            self.event
                .set(event, vec![header_state_handler, item_changed_handler]);

            obj.notify_event();
            obj.notify_sender();

            self.update_content();
            self.update_header();
            self.update_thread_indicator();
        }

        /// The sender of the event that is presented.
        fn sender(&self) -> Option<Member> {
            self.event.obj().map(|event| event.sender())
        }

        /// Update the header for the current event.
        fn update_header(&self) {
            let Some(event) = self.event.obj() else {
                return;
            };

            let header_state = event.header_state();
            let avatar_name_visible = header_state == EventHeaderState::Full;
            let header_visible = header_state != EventHeaderState::Hidden;

            self.avatar_button.set_visible(avatar_name_visible);
            self.display_name.set_visible(avatar_name_visible);
            self.header.set_visible(header_visible);

            if let Some(row) = self.obj().parent() {
                if avatar_name_visible {
                    row.add_css_class("has-avatar");
                } else {
                    row.remove_css_class("has-avatar");
                }
            }
        }

        /// Update the content for the current event.
        fn update_content(&self) {
            let Some(event) = self.event.obj() else {
                return;
            };

            self.content.update_for_event(&event);
        }

        /// Get the texture displayed by this widget, if any.
        pub(super) fn texture(&self) -> Option<gdk::Texture> {
            self.content.texture()
        }

        /// Update the thread indicator for the current event.
        fn update_thread_indicator(&self) {
            let count = self
                .event
                .obj()
                .filter(|event| {
                    // Thread timelines do not need an indicator on the thread
                    // root.
                    !event.timeline().is_thread()
                })
                .and_then(|event| event.thread_replies_count())
                .filter(|count| *count > 0);

            let Some(count) = count else {
                self.thread_indicator.set_visible(false);
                return;
            };

            self.thread_indicator_label.set_label(&ngettext_f(
                // Translators: Do NOT translate the content between '{' and '}',
                // this is a variable name.
                "1 reply in thread",
                "{n} replies in thread",
                count,
                &[("n", &count.to_string())],
            ));
            self.thread_indicator.set_visible(true);
        }

        /// Open the thread panel for the current event.
        #[template_callback]
        fn open_thread(&self) {
            let Some(event_id) = self.event.obj().and_then(|event| event.event_id()) else {
                error!("Could not open thread for event without an event ID");
                return;
            };

            if self
                .obj()
                .activate_action(
                    "room-history.show-thread",
                    Some(&event_id.as_str().to_variant()),
                )
                .is_err()
            {
                error!("Could not activate `room-history.show-thread` action");
            }
        }

        /// View the profile of the sender.
        #[template_callback]
        fn view_sender_profile(&self) {
            let Some(sender) = self.sender() else {
                error!("Could not open profile for missing sender");
                return;
            };

            let dialog = UserProfileDialog::new();
            dialog.set_room_member(sender);
            dialog.present(Some(&*self.obj()));
        }
    }
}

glib::wrapper! {
    /// A row displaying a message in the timeline.
    pub struct MessageRow(ObjectSubclass<imp::MessageRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MessageRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for MessageRow {
    fn default() -> Self {
        Self::new()
    }
}
