use gtk::{glib, glib::closure_local, prelude::*, subclass::prelude::*};
use matrix_sdk_ui::timeline::VirtualTimelineItem;

use super::{Timeline, TimelineItem, TimelineItemImpl};
use crate::utils::matrix::timestamp_to_date;

/// The kind of virtual item.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub(crate) enum VirtualItemKind {
    /// A spinner, when the timeline is loading.
    #[default]
    Spinner,
    /// The typing status.
    Typing,
    /// The start of the timeline.
    TimelineStart,
    /// A day separator.
    ///
    /// The date is in UTC.
    DayDivider(glib::DateTime),
    /// A separator for the read marker.
    NewMessages,
}

impl VirtualItemKind {
    /// Construct a `VirtualItemKind` from the given item.
    fn with_item(item: &VirtualTimelineItem) -> Self {
        match item {
            VirtualTimelineItem::DateDivider(ts) => Self::DayDivider(timestamp_to_date(*ts)),
            VirtualTimelineItem::ReadMarker => Self::NewMessages,
            VirtualTimelineItem::TimelineStart => Self::TimelineStart,
        }
    }
}

mod imp {
    use std::{cell::RefCell, sync::LazyLock};

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, Default)]
    pub struct VirtualItem {
        /// The kind of virtual item.
        kind: RefCell<VirtualItemKind>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VirtualItem {
        const NAME: &'static str = "TimelineVirtualItem";
        type Type = super::VirtualItem;
        type ParentType = TimelineItem;
    }

    impl ObjectImpl for VirtualItem {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("kind-changed").build()]);
            SIGNALS.as_ref()
        }
    }

    impl TimelineItemImpl for VirtualItem {}

    impl VirtualItem {
        /// Set the kind of virtual item.
        pub(super) fn set_kind(&self, kind: VirtualItemKind) {
            self.kind.replace(kind);
            self.obj().emit_by_name::<()>("kind-changed", &[]);
        }

        /// The kind of virtual item.
        pub(super) fn kind(&self) -> VirtualItemKind {
            self.kind.borrow().clone()
        }
    }
}

glib::wrapper! {
    /// A virtual item in the timeline.
    ///
    /// A virtual item is an item not based on a timeline event.
    pub struct VirtualItem(ObjectSubclass<imp::VirtualItem>) @extends TimelineItem;
}

impl VirtualItem {
    /// Create a new `VirtualItem`.
    fn new(timeline: &Timeline, kind: VirtualItemKind, timeline_id: &str) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("timeline", timeline)
            .property("timeline-id", timeline_id)
            .build();
        obj.imp().set_kind(kind);
        obj
    }

    /// Create a new `VirtualItem` from a virtual timeline item.
    pub(crate) fn with_item(
        timeline: &Timeline,
        item: &VirtualTimelineItem,
        timeline_id: &str,
    ) -> Self {
        let kind = VirtualItemKind::with_item(item);
        Self::new(timeline, kind, timeline_id)
    }

    /// The kind of virtual item.
    pub(crate) fn kind(&self) -> VirtualItemKind {
        self.imp().kind()
    }

    /// Update this `VirtualItem` with the given virtual timeline item.
    pub(crate) fn update_with_item(&self, item: &VirtualTimelineItem) {
        let kind = VirtualItemKind::with_item(item);
        self.imp().set_kind(kind);
    }

    /// Create a spinner virtual item.
    pub(crate) fn spinner(timeline: &Timeline) -> Self {
        Self::new(
            timeline,
            VirtualItemKind::Spinner,
            "VirtualItemKind::Spinner",
        )
    }

    /// Create a typing virtual item.
    pub(crate) fn typing(timeline: &Timeline) -> Self {
        Self::new(timeline, VirtualItemKind::Typing, "VirtualItemKind::Typing")
    }

    /// Connect to the signal emitted when the kind changed.
    pub fn connect_kind_changed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "kind-changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}
