use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};
use matrix_sdk_ui::timeline::TimelineEventItemId;
use ruma::{EventId, events::room::message::MessageType};
use tracing::{error, warn};

use super::{
    RoomHistory, event_row::EventRow, message_toolbar::MessageToolbar, set_virtual_item_child,
};
use crate::{
    prelude::*,
    session::{Event, Timeline, VirtualItem},
    spawn,
    utils::{BoundObject, LoadingState},
};

mod imp {
    use std::{cell::Cell, ops::ControlFlow};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/thread_panel.ui")]
    #[properties(wrapper_type = super::ThreadPanel)]
    pub struct ThreadPanel {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        listview: TemplateChild<gtk::ListView>,
        #[template_child]
        scrolled_window: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub(super) message_toolbar: TemplateChild<MessageToolbar>,
        /// The ancestor room history of this panel.
        #[property(get, set = Self::set_room_history, nullable)]
        room_history: glib::WeakRef<RoomHistory>,
        /// The thread timeline currently displayed.
        #[property(get, set = Self::set_timeline, explicit_notify, nullable)]
        timeline: BoundObject<Timeline>,
        /// Whether the panel should stick to the newest message in the thread.
        is_sticky: Cell<bool>,
        /// Whether the current scrolling is automatic.
        is_auto_scrolling: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ThreadPanel {
        const NAME: &'static str = "RoomHistoryThreadPanel";
        type Type = super::ThreadPanel;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_accessible_role(gtk::AccessibleRole::Group);

            // Shadow the room history actions so that events in the thread
            // panel interact with the thread timeline and composer.
            klass.install_action(
                "room-history.reply",
                Some(&String::static_variant_type()),
                |obj, _, v| {
                    let Some(event) = obj.imp().event_for_action_target(v) else {
                        return;
                    };

                    obj.imp().message_toolbar.set_reply_to(event);
                },
            );

            klass.install_action(
                "room-history.edit",
                Some(&String::static_variant_type()),
                |obj, _, v| {
                    let Some(event) = obj.imp().event_for_action_target(v) else {
                        return;
                    };

                    obj.imp().message_toolbar.set_edit(&event);
                },
            );

            klass.install_action(
                "room-history.scroll-to-event",
                Some(&TimelineEventItemId::static_variant_type()),
                |obj, _, v| {
                    let Some(event_key) = v.and_then(TimelineEventItemId::from_variant) else {
                        error!("Could not parse event identifier to scroll to");
                        return;
                    };

                    obj.imp().scroll_to_event(&event_key);
                },
            );

            klass.install_action("room-history.edit-latest-message", None, |obj, _, _| {
                obj.imp().edit_latest_message();
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for ThreadPanel {
        fn constructed(&self) {
            self.parent_constructed();

            self.init_listview();

            self.message_toolbar.connect_message_sent(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.refresh_root_thread_indicator();
                }
            ));
        }
    }

    impl WidgetImpl for ThreadPanel {
        fn grab_focus(&self) -> bool {
            self.message_toolbar.grab_focus()
        }
    }

    impl BinImpl for ThreadPanel {}

    #[gtk::template_callbacks]
    impl ThreadPanel {
        /// Set the ancestor room history of this panel.
        fn set_room_history(&self, room_history: Option<&RoomHistory>) {
            self.room_history.set(room_history);
        }

        /// Initialize the list view.
        fn init_listview(&self) {
            let factory = gtk::SignalListItemFactory::new();
            factory.connect_setup(move |_, list_item| {
                let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
                    error!("List item factory did not receive a list item: {list_item:?}");
                    return;
                };

                list_item.set_activatable(false);
                list_item.set_selectable(false);
            });
            factory.connect_bind(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, list_item| {
                    let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
                        error!("List item factory did not receive a list item: {list_item:?}");
                        return;
                    };

                    imp.bind_list_item_to_item(list_item);
                }
            ));
            self.listview.set_factory(Some(&factory));

            // Needed to use the natural height of GtkPictures.
            self.listview
                .set_vscroll_policy(gtk::ScrollablePolicy::Natural);

            self.is_sticky.set(true);
            let adj = self
                .listview
                .vadjustment()
                .expect("GtkListView has a vadjustment");

            adj.connect_value_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.scroll_value_changed();
                }
            ));
            adj.connect_upper_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.scroll_max_value_changed();
                }
            ));
            adj.connect_page_size_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.scroll_max_value_changed();
                }
            ));
        }

        /// Bind the given `GtkListItem` to its item.
        fn bind_list_item_to_item(&self, list_item: &gtk::ListItem) {
            let Some(item) = list_item.item() else {
                error!("List item does not have an item");
                list_item.set_child(None::<&gtk::Widget>);
                return;
            };

            if let Some(event) = item.downcast_ref::<Event>() {
                let Some(room_history) = self.room_history.upgrade() else {
                    warn!("Thread panel does not have an ancestor room history");
                    return;
                };

                let child = list_item.child_or_else::<EventRow>(|| EventRow::new(&room_history));
                child.set_event(Some(event.clone()));
            } else if let Some(virtual_item) = item.downcast_ref::<VirtualItem>() {
                set_virtual_item_child(list_item, virtual_item);
            } else {
                error!("Could not build widget for unsupported thread panel item: {item:?}");
            }
        }

        /// Set the thread timeline currently displayed.
        fn set_timeline(&self, timeline: Option<Timeline>) {
            if self.timeline.obj() == timeline {
                return;
            }

            self.timeline.disconnect_signals();

            if let Some(timeline) = timeline {
                let empty_handler = timeline.connect_is_empty_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_view();
                    }
                ));
                let state_handler = timeline.connect_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |timeline| {
                        imp.update_view();

                        if timeline.state() == LoadingState::Ready {
                            imp.load_more_events_if_needed();
                        }
                    }
                ));

                self.timeline
                    .set(timeline.clone(), vec![empty_handler, state_handler]);

                let selection_model = gtk::NoSelection::new(Some(timeline.items()));
                self.listview.set_model(Some(&selection_model));

                self.is_sticky.set(true);
                self.scroll_down();
            } else {
                self.listview.set_model(None::<&gtk::SelectionModel>);
            }

            self.update_view();
            self.load_more_events_if_needed();

            self.obj().notify_timeline();
        }

        /// Update the visible page for the current state.
        fn update_view(&self) {
            let Some(timeline) = self.timeline.obj() else {
                self.stack.set_visible_child_name("loading");
                return;
            };

            let visible_child_name = if timeline.is_empty() {
                "loading"
            } else {
                "content"
            };
            self.stack.set_visible_child_name(visible_child_name);
        }

        /// Handle when the scroll value changed.
        fn scroll_value_changed(&self) {
            if self.is_auto_scrolling.get() {
                if self.is_at_bottom() {
                    self.is_auto_scrolling.set(false);
                }

                return;
            }

            self.is_sticky.set(self.is_at_bottom());
            self.load_more_events_if_needed();
        }

        /// Handle when the maximum scroll value changed.
        fn scroll_max_value_changed(&self) {
            if self.is_sticky.get() {
                self.scroll_down();
            }

            self.load_more_events_if_needed();
        }

        /// Whether the list view is scrolled at the bottom.
        fn is_at_bottom(&self) -> bool {
            let adj = self
                .listview
                .vadjustment()
                .expect("GtkListView has a vadjustment");
            (adj.value() + adj.page_size() - adj.upper()).abs() < 0.0001
        }

        /// Scroll to the bottom of the thread.
        fn scroll_down(&self) {
            if self.is_at_bottom() {
                return;
            }

            self.is_auto_scrolling.set(true);

            self.scrolled_window
                .emit_scroll_child(gtk::ScrollType::End, false);
        }

        /// Whether we need to load more events at the start of the thread.
        fn needs_more_events_at_the_start(&self) -> bool {
            let Some(timeline) = self.timeline.obj() else {
                return false;
            };

            if timeline.is_empty() {
                return true;
            }

            let adj = self
                .listview
                .vadjustment()
                .expect("GtkListView has a vadjustment");
            adj.value() < adj.page_size() * 2.0
        }

        /// Load more events at the start of the thread, if needed.
        fn load_more_events_if_needed(&self) {
            if !self.needs_more_events_at_the_start() {
                return;
            }

            let Some(timeline) = self.timeline.obj() else {
                return;
            };

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    timeline
                        .paginate_backwards(clone!(
                            #[weak]
                            imp,
                            #[upgrade_or]
                            ControlFlow::Break(()),
                            move || {
                                if imp.needs_more_events_at_the_start() {
                                    ControlFlow::Continue(())
                                } else {
                                    ControlFlow::Break(())
                                }
                            }
                        ))
                        .await;
                }
            ));
        }

        /// Scroll to the event with the given identifier.
        fn scroll_to_event(&self, key: &TimelineEventItemId) {
            let Some(timeline) = self.timeline.obj() else {
                return;
            };

            if let Some(pos) = timeline.find_event_position(key) {
                let pos = pos as u32;
                self.listview
                    .scroll_to(pos, gtk::ListScrollFlags::FOCUS, None);
            }
        }

        /// Get the event of the current timeline matching the given action
        /// target.
        fn event_for_action_target(&self, v: Option<&glib::Variant>) -> Option<Event> {
            let Some(event_id) = v
                .and_then(String::from_variant)
                .and_then(|s| EventId::parse(s).ok())
            else {
                error!("Could not parse event ID of thread panel action");
                return None;
            };

            let event = self.timeline.obj().and_then(|timeline| {
                timeline.event_by_identifier(&TimelineEventItemId::EventId(event_id))
            });

            if event.is_none() {
                warn!("Could not find event in thread timeline");
            }

            event
        }

        /// Edit the latest editable message sent by our own user in this
        /// thread.
        fn edit_latest_message(&self) {
            let Some(timeline) = self.timeline.obj() else {
                return;
            };

            let own_member = timeline.room().own_member();
            let own_user_id = own_member.user_id();

            let Some(event) = timeline
                .items()
                .iter::<glib::Object>()
                .rev()
                .find_map(|item| {
                    item.ok().and_downcast::<Event>().filter(|event| {
                        event.sender_id() == *own_user_id
                            && event.event_id().is_some()
                            && event.message().is_some_and(|message| {
                                matches!(
                                    message.msgtype(),
                                    MessageType::Text(_) | MessageType::Emote(_)
                                )
                            })
                    })
                })
            else {
                warn!("Could not find latest event to edit in thread");
                return;
            };

            self.message_toolbar.set_edit(&event);
        }

        /// Close the thread panel.
        #[template_callback]
        fn close(&self) {
            if self
                .obj()
                .activate_action("room-history.close-thread", None)
                .is_err()
            {
                error!("Could not activate `room-history.close-thread` action");
            }
        }

        /// Refresh the thread indicator on the root event in the main
        /// timeline after a message was sent in this thread.
        fn refresh_root_thread_indicator(&self) {
            let Some(root_event_id) = self
                .timeline
                .obj()
                .and_then(|timeline| timeline.thread_root_id())
            else {
                return;
            };

            let Some(room_history) = self.room_history.upgrade() else {
                return;
            };

            let Some(main_timeline) = room_history.timeline() else {
                return;
            };

            let root_event =
                main_timeline.event_by_identifier(&TimelineEventItemId::EventId(root_event_id));

            if let Some(root_event) = root_event {
                spawn!(async move {
                    if let Err(error) = root_event.fetch_missing_details().await {
                        error!("Could not fetch details for root event: {error}");
                    }
                });
            }
        }
    }
}

glib::wrapper! {
    /// A panel presenting a thread: its timeline and a composer to post in the
    /// thread.
    pub struct ThreadPanel(ObjectSubclass<imp::ThreadPanel>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ThreadPanel {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for ThreadPanel {
    fn default() -> Self {
        Self::new()
    }
}
