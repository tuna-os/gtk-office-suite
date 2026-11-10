use adw::{prelude::*, subclass::prelude::*};
use gtk::{gdk, gio, glib, glib::clone};
use matrix_sdk_ui::timeline::TimelineEventItemId;

use super::{CallRow, EventActionsGroup, MessageRow, RoomHistory, StateRow};
use crate::{
    components::ContextMenuBin,
    prelude::*,
    session::Event,
    session_view::room_history::message_toolbar::ComposerState,
    utils::{BoundObject, BoundObjectWeakRef},
};

mod imp {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::EventRow)]
    pub struct EventRow {
        /// The ancestor room history of this row.
        #[property(get, set = Self::set_room_history, construct_only)]
        room_history: glib::WeakRef<RoomHistory>,
        message_toolbar_handler: RefCell<Option<glib::SignalHandlerId>>,
        composer_state: BoundObjectWeakRef<ComposerState>,
        /// The event presented by this row.
        #[property(get, set = Self::set_event, explicit_notify, nullable)]
        event: BoundObject<Event>,
        /// The event action group of this row.
        action_group: RefCell<Option<gio::SimpleActionGroup>>,
        shortcut_controller: RefCell<Option<gtk::ShortcutController>>,
        permissions_handler: RefCell<Option<glib::SignalHandlerId>>,
        target_user_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EventRow {
        const NAME: &'static str = "RoomHistoryEventRow";
        type Type = super::EventRow;
        type ParentType = ContextMenuBin;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("event-row");
            klass.set_accessible_role(gtk::AccessibleRole::ListItem);
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for EventRow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            obj.connect_parent_notify(|obj| {
                obj.imp().update_highlight();
            });
            obj.add_css_class("room-history-row");
        }

        fn dispose(&self) {
            self.disconnect_event_signals();

            if let Some(handler) = self.message_toolbar_handler.take()
                && let Some(room_history) = self.room_history.upgrade()
            {
                room_history.message_toolbar().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for EventRow {}

    impl ContextMenuBinImpl for EventRow {
        fn menu_opened(&self) {
            let Some(room_history) = self.room_history.upgrade() else {
                return;
            };

            let obj = self.obj();
            let Some(event) = self.event.obj() else {
                obj.set_popover(None);
                return;
            };
            if self.action_group.borrow().is_none() {
                // There are no possible actions.
                obj.set_popover(None);
                return;
            }

            let menu = room_history.event_context_menu();

            // Reset the state when the popover is closed.
            let closed_handler_cell: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::default();
            let closed_handler = menu.popover.connect_closed(clone!(
                #[weak]
                obj,
                #[weak]
                room_history,
                #[strong]
                closed_handler_cell,
                move |popover| {
                    room_history.enable_sticky_mode(true);
                    obj.remove_css_class("has-open-popup");

                    if let Some(handler) = closed_handler_cell.take() {
                        popover.disconnect(handler);
                    }
                }
            ));
            closed_handler_cell.replace(Some(closed_handler));

            if event.can_be_reacted_to() {
                menu.add_quick_reaction_chooser(event.reactions());
            } else {
                menu.remove_quick_reaction_chooser();
            }

            room_history.enable_sticky_mode(false);
            obj.add_css_class("has-open-popup");

            obj.set_popover(Some(menu.popover.clone()));
        }
    }

    impl EventActionsGroup for EventRow {
        fn event(&self) -> Option<Event> {
            self.event.obj()
        }

        fn texture(&self) -> Option<gdk::Texture> {
            self.obj()
                .child()
                .and_downcast::<MessageRow>()
                .and_then(|r| r.texture())
        }

        fn popover(&self) -> Option<gtk::PopoverMenu> {
            self.obj().popover()
        }
    }

    impl EventRow {
        /// Set the ancestor room history of this row.
        fn set_room_history(&self, room_history: &RoomHistory) {
            self.room_history.set(Some(room_history));

            let message_toolbar = room_history.message_toolbar();
            let message_toolbar_handler =
                message_toolbar.connect_current_composer_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |message_toolbar| {
                        imp.watch_related_event(&message_toolbar.current_composer_state());
                    }
                ));
            self.message_toolbar_handler
                .replace(Some(message_toolbar_handler));

            self.watch_related_event(&message_toolbar.current_composer_state());
        }

        /// Watch the related event for given current composer state of the
        /// toolbar.
        fn watch_related_event(&self, composer_state: &ComposerState) {
            self.composer_state.disconnect_signals();

            let composer_state_handler = composer_state.connect_related_to_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |composer_state| {
                    imp.update_for_related_event(
                        composer_state
                            .related_to()
                            .map(|info| TimelineEventItemId::EventId(info.event_id()))
                            .as_ref(),
                    );
                }
            ));
            self.composer_state
                .set(composer_state, vec![composer_state_handler]);

            self.update_for_related_event(
                composer_state
                    .related_to()
                    .map(|info| TimelineEventItemId::EventId(info.event_id()))
                    .as_ref(),
            );
        }

        /// Disconnect the signal handlers.
        fn disconnect_event_signals(&self) {
            if let Some(event) = self.event.obj() {
                self.event.disconnect_signals();

                if let Some(handler) = self.permissions_handler.take() {
                    event.room().permissions().disconnect(handler);
                }

                if let Some(handler) = self.target_user_handler.take()
                    && let Some(target_user) = event.target_user()
                {
                    target_user.disconnect(handler);
                }
            }
        }

        /// Set the event presented by this row.
        fn set_event(&self, event: Option<Event>) {
            // Reinitialize the header.
            self.obj().remove_css_class("has-avatar");
            self.obj().remove_css_class("has-icon");

            self.disconnect_event_signals();

            if let Some(event) = event {
                let permissions_handler = event.room().permissions().connect_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_actions();
                    }
                ));
                self.permissions_handler.replace(Some(permissions_handler));

                if let Some(target_user) = event.target_user() {
                    let target_user_handler = target_user.connect_membership_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_actions();
                        }
                    ));
                    self.target_user_handler.replace(Some(target_user_handler));
                }

                let state_notify_handler = event.connect_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_actions();
                    }
                ));
                let source_notify_handler = event.connect_source_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |event| {
                        imp.build_event_widget(event.clone());
                        imp.update_actions();
                    }
                ));
                let edit_source_notify_handler = event.connect_latest_edit_source_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |event| {
                        imp.build_event_widget(event.clone());
                        imp.update_actions();
                    }
                ));
                let is_highlighted_notify_handler = event.connect_is_highlighted_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_highlight();
                    }
                ));

                self.event.set(
                    event.clone(),
                    vec![
                        state_notify_handler,
                        source_notify_handler,
                        edit_source_notify_handler,
                        is_highlighted_notify_handler,
                    ],
                );

                self.build_event_widget(event);
            }

            self.update_actions();
            self.update_highlight();
        }

        /// Construct the widget for the given event
        fn build_event_widget(&self, event: Event) {
            let obj = self.obj();

            if event.is_call_event() {
                let child = obj.child_or_default::<CallRow>();
                child.set_event(event);
            } else if event.is_state_event() {
                let child = obj.child_or_default::<StateRow>();
                child.set_event(event);
            } else {
                let child = obj.child_or_else::<MessageRow>(|| {
                    let child = MessageRow::default();

                    child.connect_texture_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |child| {
                            let Some(copy_image_action) = imp
                                .action_group
                                .borrow()
                                .as_ref()
                                .and_then(|action_group| action_group.lookup_action("copy-image"))
                                .and_downcast::<gio::SimpleAction>()
                            else {
                                return;
                            };

                            copy_image_action.set_enabled(child.texture().is_some());
                        }
                    ));

                    child
                });

                child.set_event(event);
            }
        }

        /// Update the highlight state of this row.
        fn update_highlight(&self) {
            let obj = self.obj();

            let highlight = self.event.obj().is_some_and(|event| event.is_highlighted());
            if highlight {
                obj.add_css_class("highlight");
            } else {
                obj.remove_css_class("highlight");
            }
        }

        /// Update this row for the related event with the given identifier.
        fn update_for_related_event(&self, related_event_id: Option<&TimelineEventItemId>) {
            let obj = self.obj();

            if related_event_id.is_some_and(|identifier| {
                self.event
                    .obj()
                    .is_some_and(|event| event.matches_identifier(identifier))
            }) {
                obj.add_css_class("selected");
            } else {
                obj.remove_css_class("selected");
            }
        }

        /// Update the actions available for the given event.
        fn update_actions(&self) {
            let obj = self.obj();
            let action_group = self.event_actions_group();
            let has_context_menu = action_group.is_some();

            if let Some(copy_image_action) = action_group
                .as_ref()
                .and_then(|action_group| action_group.lookup_action("copy-image"))
                .and_downcast::<gio::SimpleAction>()
            {
                copy_image_action.set_enabled(self.texture().is_some());
            }

            if action_group
                .as_ref()
                .is_some_and(|action_group| action_group.has_action("properties"))
            {
                if self.shortcut_controller.borrow().is_none() {
                    let shortcut_controller = gtk::ShortcutController::new();
                    shortcut_controller.add_shortcut(gtk::Shortcut::new(
                        Some(
                            gtk::ShortcutTrigger::parse_string("<Alt>Return")
                                .expect("trigger string should be valid"),
                        ),
                        gtk::ShortcutAction::parse_string("action(event.properties)"),
                    ));
                    obj.add_controller(shortcut_controller.clone());
                    self.shortcut_controller.replace(Some(shortcut_controller));
                }
            } else if let Some(shortcut_controller) = self.shortcut_controller.take() {
                obj.remove_controller(&shortcut_controller);
            }

            obj.insert_action_group("event", action_group.as_ref());
            self.action_group.replace(action_group);
            obj.set_has_context_menu(has_context_menu);
        }
    }
}

glib::wrapper! {
    /// A row presenting an event in the room history.
    pub struct EventRow(ObjectSubclass<imp::EventRow>)
        @extends gtk::Widget, ContextMenuBin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl EventRow {
    pub fn new(room_history: &RoomHistory) -> Self {
        glib::Object::builder()
            .property("room-history", room_history)
            .build()
    }
}

impl ChildPropertyExt for EventRow {
    fn child_property(&self) -> Option<gtk::Widget> {
        self.child()
    }

    fn set_child_property(&self, child: Option<&impl IsA<gtk::Widget>>) {
        self.set_child(child);
    }
}
