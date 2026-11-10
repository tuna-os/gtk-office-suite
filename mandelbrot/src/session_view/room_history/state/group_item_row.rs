use gtk::{gdk, gio, glib, glib::clone, prelude::*, subclass::prelude::*};
use tracing::error;

use super::StateContent;
use crate::{
    prelude::*,
    session::Event,
    session_view::room_history::{EventActionsGroup, RoomHistory},
    utils::{BoundObject, BoundObjectWeakRef, key_bindings},
};

mod imp {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::StateGroupItemRow)]
    pub struct StateGroupItemRow {
        content: StateContent,
        /// The state event presented by this row.
        #[property(get, set = Self::set_event, explicit_notify)]
        event: BoundObjectWeakRef<Event>,
        /// The event action group of this row.
        action_group: RefCell<Option<gio::SimpleActionGroup>>,
        /// The popover of this row.
        popover: BoundObject<gtk::PopoverMenu>,
        permissions_handler: RefCell<Option<glib::SignalHandlerId>>,
        target_user_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StateGroupItemRow {
        const NAME: &'static str = "ContentStateGroupItemRow";
        type Type = super::StateGroupItemRow;
        type ParentType = gtk::ListBoxRow;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("state-group-item-row");

            klass.install_action("context-menu.activate", None, |obj, _, _| {
                obj.imp().open_context_menu(0, 0);
            });
            key_bindings::add_context_menu_bindings(klass, "context-menu.activate");

            klass.install_action("context-menu.close", None, |obj, _, _| {
                if let Some(popover) = obj.imp().popover.obj() {
                    popover.popdown();
                }
            });
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for StateGroupItemRow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            obj.set_child(Some(&self.content));

            // Open context menu on long tap.
            let long_press_gesture = gtk::GestureLongPress::builder()
                .touch_only(true)
                .exclusive(true)
                .build();
            long_press_gesture.connect_pressed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |gesture, x, y| {
                    if imp.action_group.borrow().is_some() {
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                        gesture.reset();
                        imp.open_context_menu(x as i32, y as i32);
                    }
                }
            ));
            obj.add_controller(long_press_gesture);

            // Open context menu on right click.
            let right_click_gesture = gtk::GestureClick::builder()
                .button(3)
                .exclusive(true)
                .build();
            right_click_gesture.connect_released(clone!(
                #[weak(rename_to = imp)]
                self,
                move |gesture, n_press, x, y| {
                    if n_press > 1 {
                        return;
                    }

                    if imp.action_group.borrow().is_some() {
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                        imp.open_context_menu(x as i32, y as i32);
                    }
                }
            ));
            obj.add_controller(right_click_gesture);
        }

        fn dispose(&self) {
            self.disconnect_event_signals();

            if let Some(popover) = self.popover.obj() {
                popover.unparent();
            }
        }
    }

    impl WidgetImpl for StateGroupItemRow {}
    impl ListBoxRowImpl for StateGroupItemRow {}

    impl EventActionsGroup for StateGroupItemRow {
        fn event(&self) -> Option<Event> {
            self.event.obj()
        }

        fn texture(&self) -> Option<gdk::Texture> {
            None
        }

        fn popover(&self) -> Option<gtk::PopoverMenu> {
            self.popover.obj()
        }
    }

    impl StateGroupItemRow {
        /// Set the state event presented by this row.
        fn set_event(&self, event: Option<&Event>) {
            if self.event.obj().as_ref() == event {
                return;
            }

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
                    move |_| {
                        imp.update_actions();
                    }
                ));
                let edit_source_notify_handler = event.connect_latest_edit_source_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_actions();
                    }
                ));

                self.event.set(
                    event,
                    vec![
                        state_notify_handler,
                        source_notify_handler,
                        edit_source_notify_handler,
                    ],
                );
            }

            self.content.set_event(event);
            self.update_actions();

            self.obj().notify_event();
        }

        /// Update the actions available for the given event.
        fn update_actions(&self) {
            let obj = self.obj();
            let action_group = self.event_actions_group();
            let has_context_menu = action_group.is_some();

            obj.insert_action_group("event", action_group.as_ref());
            self.action_group.replace(action_group);

            obj.update_property(&[gtk::accessible::Property::HasPopup(has_context_menu)]);
            obj.action_set_enabled("context-menu.activate", has_context_menu);
            obj.action_set_enabled("context-menu.close", has_context_menu);
        }

        /// Set the popover of this row.
        fn set_popover(&self, popover: Option<gtk::PopoverMenu>) {
            let prev_popover = self.popover.obj();

            if prev_popover == popover {
                return;
            }
            let obj = self.obj();

            if let Some(popover) = prev_popover
                && popover.parent().is_some_and(|w| w == *obj)
            {
                popover.unparent();
            }
            self.popover.disconnect_signals();

            if let Some(popover) = popover {
                popover.unparent();
                popover.set_parent(&*obj);

                let parent_handler = popover.connect_parent_notify(clone!(
                    #[weak]
                    obj,
                    move |popover| {
                        if popover.parent().is_none_or(|w| w != obj) {
                            obj.imp().popover.disconnect_signals();
                        }
                    }
                ));

                self.popover.set(popover, vec![parent_handler]);
            }
        }

        /// Open the context menu of the row at the given coordinates.
        fn open_context_menu(&self, x: i32, y: i32) {
            let obj = self.obj();

            if self.action_group.borrow().is_none() {
                // There are no possible actions.
                self.set_popover(None);
                return;
            }

            let Some(room_history) = obj
                .ancestor(RoomHistory::static_type())
                .and_downcast::<RoomHistory>()
            else {
                error!("Could not find RoomHistory ancestor");
                self.set_popover(None);
                return;
            };

            let menu = room_history.event_context_menu();
            menu.remove_quick_reaction_chooser();

            room_history.enable_sticky_mode(false);
            obj.add_css_class("has-open-popup");

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

            self.set_popover(Some(menu.popover.clone()));

            menu.popover
                .set_pointing_to(Some(&gdk::Rectangle::new(x, y, 0, 0)));
            menu.popover.popup();
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
    }
}

glib::wrapper! {
    /// A row presenting a state event that is part of a group.
    pub struct StateGroupItemRow(ObjectSubclass<imp::StateGroupItemRow>)
        @extends gtk::Widget, gtk::ListBoxRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl StateGroupItemRow {
    pub fn new(event: &Event) -> Self {
        glib::Object::builder().property("event", event).build()
    }
}
