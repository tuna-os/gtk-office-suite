use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};

mod reaction_popover;

use self::reaction_popover::ReactionPopover;
use crate::{
    gettext_f, ngettext_f,
    prelude::*,
    session::{Member, MemberList, ReactionData, ReactionGroup},
    session_view::room_history::member_timestamp::MemberTimestamp,
    utils::{BoundObjectWeakRef, EMOJI_REGEX, key_bindings},
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_row/reaction/mod.ui"
    )]
    #[properties(wrapper_type = super::MessageReaction)]
    pub struct MessageReaction {
        #[template_child]
        button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        reaction_key: TemplateChild<gtk::Label>,
        #[template_child]
        reaction_count: TemplateChild<gtk::Label>,
        /// The reaction senders group to display.
        #[property(get, set = Self::set_group, construct_only)]
        group: BoundObjectWeakRef<ReactionGroup>,
        /// The list of reaction senders as room members.
        #[property(get)]
        list: gio::ListStore,
        /// The member list of the room of the reaction.
        #[property(get, set = Self::set_members, explicit_notify, nullable)]
        members: RefCell<Option<MemberList>>,
        /// The displayed member if there is only one reaction sender.
        reaction_member: BoundObjectWeakRef<Member>,
    }

    impl Default for MessageReaction {
        fn default() -> Self {
            Self {
                button: Default::default(),
                reaction_key: Default::default(),
                reaction_count: Default::default(),
                group: Default::default(),
                list: gio::ListStore::new::<MemberTimestamp>(),
                members: Default::default(),
                reaction_member: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageReaction {
        const NAME: &'static str = "ContentMessageReaction";
        type Type = super::MessageReaction;
        type ParentType = gtk::FlowBoxChild;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.install_action("reaction.show-popover", None, |obj, _, _| {
                obj.imp().show_popover();
            });
            key_bindings::add_context_menu_bindings(klass, "reaction.show-popover");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MessageReaction {}

    impl WidgetImpl for MessageReaction {}
    impl FlowBoxChildImpl for MessageReaction {}

    #[gtk::template_callbacks]
    impl MessageReaction {
        /// Set the reaction group to display.
        fn set_group(&self, group: &ReactionGroup) {
            let key = group.key();
            self.reaction_key.set_label(&key);

            if EMOJI_REGEX.is_match(&key) {
                self.reaction_key.add_css_class("reaction-key-emoji");
                self.reaction_key.remove_css_class("reaction-key-text");
            } else {
                self.reaction_key.remove_css_class("reaction-key-emoji");
                self.reaction_key.add_css_class("reaction-key-text");
            }

            self.button.set_action_target_value(Some(&key.to_variant()));
            group
                .bind_property("has-own-user", &*self.button, "active")
                .sync_create()
                .build();
            group
                .bind_property("count", &*self.reaction_count, "label")
                .sync_create()
                .build();

            group
                .bind_property("count", &*self.reaction_count, "visible")
                .sync_create()
                .transform_to(|_, count: u32| Some(count > 1))
                .build();

            let items_changed_handler_id = group.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |group, pos, removed, added| imp.items_changed(group, pos, removed, added)
            ));
            self.items_changed(group, 0, self.list.n_items(), group.n_items());

            self.group.set(group, vec![items_changed_handler_id]);
        }

        /// Set the members list of the room of the reaction.
        fn set_members(&self, members: Option<MemberList>) {
            if *self.members.borrow() == members {
                return;
            }

            self.members.replace(members);
            self.obj().notify_members();

            if let Some(group) = self.group.obj() {
                self.items_changed(&group, 0, self.list.n_items(), group.n_items());
            }
        }

        /// Handle when the items changed.
        fn items_changed(&self, group: &ReactionGroup, pos: u32, removed: u32, added: u32) {
            let Some(members) = &*self.members.borrow() else {
                return;
            };

            let mut new_senders = Vec::with_capacity(added as usize);
            for i in pos..pos + added {
                let Some(boxed) = group.item(i).and_downcast::<glib::BoxedAnyObject>() else {
                    break;
                };

                let reaction_data = boxed.borrow::<ReactionData>();
                let member = members.get_or_create(reaction_data.sender_id.clone());
                let timestamp = reaction_data.timestamp.as_secs().into();
                let sender = MemberTimestamp::new(&member, Some(timestamp));

                new_senders.push(sender);
            }

            self.list.splice(pos, removed, &new_senders);
            self.update_tooltip();
        }

        /// Update the text of the tooltip.
        fn update_tooltip(&self) {
            let Some(group) = self.group.obj() else {
                return;
            };

            self.reaction_member.disconnect_signals();
            let n_items = self.list.n_items();

            if n_items == 1
                && let Some(member) = self
                    .list
                    .item(0)
                    .and_downcast::<MemberTimestamp>()
                    .and_then(|r| r.member())
            {
                // Listen to changes of the display name.
                let handler_id = member.connect_display_name_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |member| {
                        imp.update_member_tooltip(member);
                    }
                ));

                self.reaction_member.set(&member, vec![handler_id]);
                self.update_member_tooltip(&member);
                return;
            }

            let text = (n_items > 0).then(|| {
                ngettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "1 member reacted with {reaction_key}",
                    "{n} members reacted with {reaction_key}",
                    n_items,
                    &[("n", &n_items.to_string()), ("reaction_key", &group.key())],
                )
            });

            self.button.set_tooltip_text(text.as_deref());
        }

        /// Update the text of the tooltip when there is a single sender in the
        /// group.
        fn update_member_tooltip(&self, member: &Member) {
            let Some(group) = self.group.obj() else {
                return;
            };

            // Translators: Do NOT translate the content between '{' and '}', this is a
            // variable name.
            let text = gettext_f(
                "{user} reacted with {reaction_key}",
                &[
                    ("user", &member.disambiguated_name()),
                    ("reaction_key", &group.key()),
                ],
            );

            self.button.set_tooltip_text(Some(&text));
        }

        /// Handle a right click/long press on the reaction button.
        ///
        /// Shows a popover with the senders of that reaction, if there are any.
        #[template_callback]
        fn show_popover(&self) {
            if self.list.n_items() == 0 {
                // No popover.
                return;
            }

            let popover = ReactionPopover::new(&self.list);
            popover.set_parent(&*self.button);
            popover.connect_closed(|popover| {
                popover.unparent();
            });
            popover.popup();
        }
    }
}

glib::wrapper! {
    /// A widget displaying a reaction of a message.
    pub struct MessageReaction(ObjectSubclass<imp::MessageReaction>)
        @extends gtk::Widget, gtk::FlowBoxChild,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MessageReaction {
    pub fn new(members: MemberList, reaction_group: ReactionGroup) -> Self {
        glib::Object::builder()
            .property("group", reaction_group)
            .property("members", members)
            .build()
    }
}
