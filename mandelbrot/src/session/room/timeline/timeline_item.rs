use gtk::{glib, prelude::*, subclass::prelude::*};
use matrix_sdk_ui::timeline::{TimelineItem as SdkTimelineItem, TimelineItemKind};
use tracing::error;

use super::{Event, Timeline, VirtualItem};
use crate::session::Room;

mod imp {
    use std::cell::{OnceCell, RefCell};

    use super::*;

    #[repr(C)]
    pub struct TimelineItemClass {
        parent_class: glib::object::ObjectClass,
    }

    unsafe impl ClassStruct for TimelineItemClass {
        type Type = TimelineItem;
    }

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::TimelineItem)]
    pub struct TimelineItem {
        /// The timeline containing this `TimelineItem`.
        #[property(get, construct_only)]
        timeline: OnceCell<Timeline>,
        /// A unique ID for this `TimelineItem` in the local timeline.
        #[property(get, construct_only)]
        timeline_id: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TimelineItem {
        const NAME: &'static str = "TimelineItem";
        const ABSTRACT: bool = true;
        type Type = super::TimelineItem;
        type Class = TimelineItemClass;
    }

    #[glib::derived_properties]
    impl ObjectImpl for TimelineItem {}
}

glib::wrapper! {
    /// Interface implemented by items inside the `Timeline`.
    pub struct TimelineItem(ObjectSubclass<imp::TimelineItem>);
}

impl TimelineItem {
    /// Create a new `TimelineItem` with the given SDK timeline item.
    ///
    /// Constructs the proper child type.
    pub fn new(item: &SdkTimelineItem, timeline: &Timeline) -> Self {
        let timeline_id = &item.unique_id().0;

        match item.kind() {
            TimelineItemKind::Event(event_item) => {
                Event::new(timeline, event_item.clone(), timeline_id).upcast()
            }
            TimelineItemKind::Virtual(virtual_item) => {
                VirtualItem::with_item(timeline, virtual_item, timeline_id).upcast()
            }
        }
    }

    /// Update this `TimelineItem` with the given SDK timeline item.
    ///
    /// A `TimelineItem` should not be updated with a SDK item that has a
    /// different timeline ID.
    pub(crate) fn update_with(&self, item: &SdkTimelineItem) {
        if self.timeline_id() != item.unique_id().0 {
            error!("Should not update an item with a different timeline ID");
        }

        match item.kind() {
            TimelineItemKind::Event(new_event) => {
                if let Some(event) = self.downcast_ref::<Event>() {
                    event.update_with(new_event.clone());
                } else {
                    error!(
                        "Could not update a TimelineItem that is not an Event with an event SDK item"
                    );
                }
            }
            TimelineItemKind::Virtual(new_item) => {
                if let Some(virtual_item) = self.downcast_ref::<VirtualItem>() {
                    virtual_item.update_with_item(new_item);
                } else {
                    error!(
                        "Could not update a TimelineItem that is not a VirtualItem with a virtual SDK item"
                    );
                }
            }
        }
    }
}

/// Public trait containing implemented methods for everything that derives from
/// `TimelineItem`.
///
/// To override the behavior of these methods, override the corresponding method
/// of `TimelineItemImpl`.
#[allow(dead_code)]
pub(crate) trait TimelineItemExt: 'static {
    /// The timeline containing this `TimelineItem`.
    fn timeline(&self) -> Timeline;

    /// The room containing this `TimelineItem`.
    fn room(&self) -> Room {
        self.timeline().room()
    }

    /// A unique ID for this `TimelineItem` in the local timeline.
    fn timeline_id(&self) -> String;
}

impl<O: IsA<TimelineItem>> TimelineItemExt for O {
    fn timeline(&self) -> Timeline {
        self.upcast_ref().timeline()
    }

    fn timeline_id(&self) -> String {
        self.upcast_ref().timeline_id()
    }
}

/// Public trait that must be implemented for everything that derives from
/// `TimelineItem`.
///
/// Overriding a method from this Trait overrides also its behavior in
/// `TimelineItemExt`.
pub(crate) trait TimelineItemImpl: ObjectImpl {}

// Make `TimelineItem` subclassable.
unsafe impl<T> IsSubclassable<T> for TimelineItem
where
    T: TimelineItemImpl,
    T::Type: IsA<TimelineItem>,
{
    fn class_init(class: &mut glib::Class<Self>) {
        Self::parent_class_init::<T>(class.upcast_ref_mut());
    }
}
