use crate::relm::{ContainerChild, RelmSetChildExt};
use gtk::prelude::*;

/// Widget types which can have widgets removed from them.
pub trait RelmRemoveExt: ContainerChild {
    /// Removes the widget from the container
    /// if it is a child of the container.
    fn container_remove(&self, child: &impl AsRef<Self::Child>);
}

impl<T> RelmRemoveExt for T
where
    T: RelmSetChildExt,
    T::Child: AsRef<T::Child>,
{
    fn container_remove(&self, child: &impl AsRef<Self::Child>) {
        if let Some(current_child) = self.container_get_child() {
            let remove_child = child.as_ref().upcast_ref();
            if remove_child == &current_child {
                self.container_set_child(None::<&T::Child>);
            }
        }
    }
}

impl RelmRemoveExt for adw::PreferencesGroup {
    fn container_remove(&self, widget: &impl AsRef<Self::Child>) {
        use adw::prelude::PreferencesGroupExt;
        self.remove(widget.as_ref());
    }
}

impl RelmRemoveExt for adw::ExpanderRow {
    fn container_remove(&self, widget: &impl AsRef<Self::Child>) {
        use adw::prelude::ExpanderRowExt;

        let child = widget.as_ref();
        self.remove(child);
    }
}

macro_rules! remove_impl {
    ($($type:ty),+) => {
        $(
            impl $crate::relm::extensions::RelmRemoveExt for $type {
                fn container_remove(&self, widget: &impl AsRef<Self::Child>) {
                    self.remove(widget.as_ref());
                }
            }
        )+
    }
}

macro_rules! remove_child_impl {
    ($($type:ty),+) => {
        $(
            #[allow(deprecated)]
            impl $crate::relm::extensions::RelmRemoveExt for $type {
                fn container_remove(&self, widget: &impl AsRef<Self::Child>) {
                    self.remove_child(widget.as_ref());
                }
            }
        )+
    }
}

remove_impl!(
    gtk::Box,
    gtk::Fixed,
    gtk::Grid,
    gtk::ActionBar,
    gtk::Stack,
    gtk::HeaderBar,
    gtk::ListBox,
    gtk::FlowBox
);
remove_child_impl!(gtk::InfoBar);



/// Widget types that allow removal of all their children.
pub trait RelmRemoveAllExt {
    /// Remove all children from the container.
    fn remove_all(&self);
}

impl<T> RelmRemoveAllExt for T
where
    T: RelmSetChildExt,
    T::Child: AsRef<T::Child>,
{
    fn remove_all(&self) {
        self.container_set_child(None::<&T::Child>);
    }
}

macro_rules! remove_all_impl {
    ($($type:ty),+) => {
        $(
            impl RelmRemoveAllExt for $type {
                fn remove_all(&self) {
                    while let Some(child) = self.last_child() {
                        self.remove(&child);
                    }
                }
            }
        )+
    }
}

remove_all_impl!(gtk::Box, gtk::FlowBox, gtk::Stack, gtk::Grid);

impl RelmRemoveAllExt for gtk::ListBox {
    fn remove_all(&self) {
        while let Some(child) = self.last_child() {
            let row = child
                .downcast::<gtk::ListBoxRow>()
                .expect("The child of `ListBox` is not a `ListBoxRow`.");
            row.set_child(None::<&gtk::Widget>);
            self.remove(&row);
        }
    }
}

impl RelmRemoveAllExt for adw::ExpanderRow {
    fn remove_all(&self) {
        use adw::prelude::ExpanderRowExt;

        while let Some(child) = self.last_child() {
            let row = child
                .downcast::<gtk::ListBoxRow>()
                .expect("The child of `ExpanderRow` is not a `ListBoxRow`.");
            row.set_child(None::<&gtk::Widget>);
            self.remove(&row);
        }
    }
}
