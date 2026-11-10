use gtk::prelude::*;

use crate::relm::{ContainerChild, RelmSetChildExt};

/// Widget types which can have widgets attached to them.
pub trait RelmContainerExt: ContainerChild {
    /// Add widget as child to container.
    fn container_add(&self, widget: &impl AsRef<Self::Child>);
}

impl<T: RelmSetChildExt> RelmContainerExt for T {
    fn container_add(&self, widget: &impl AsRef<T::Child>) {
        self.container_set_child(Some(widget));
    }
}

#[allow(deprecated)]
impl RelmContainerExt for gtk::Dialog {
    fn container_add(&self, widget: &impl AsRef<gtk::Widget>) {
        self.content_area().append(widget.as_ref());
    }
}

macro_rules! append_impl {
    ($($type:ty),+) => {
        $(
            impl RelmContainerExt for $type {
                #[allow(unused_qualifications)]
                fn container_add(&self, widget: &impl AsRef<<$type as crate::relm::extensions::ContainerChild>::Child>) {
                    self.append(widget.as_ref());
                }
            }
        )+
    }
}

macro_rules! add_child_impl {
    ($($type:ty),+) => {
        $(
            #[allow(deprecated)]
            impl RelmContainerExt for $type {
                fn container_add(&self, widget: &impl AsRef<gtk::Widget>) {
                    self.add_child(widget.as_ref());
                }
            }
        )+
    }
}

macro_rules! add_impl {
    ($($type:ty: $child:ty), +) => {
        $(
            impl RelmContainerExt for $type {
                fn container_add(&self, child: &impl AsRef<$child>) {
                    self.add(child.as_ref());
                }
            }
        )+
    };
    ($($type:ty), +) => {
        $(
            impl RelmContainerExt for $type {
                fn container_add(&self, widget: &impl AsRef<gtk::Widget>) {
                    self.add(widget.as_ref());
                }
            }
        )+
    };
}

append_impl!(gtk::Box, gtk::ListBox);
add_child_impl!(gtk::InfoBar, gtk::Stack);


mod libadwaita {
    use super::RelmContainerExt;
    use adw::prelude::{PreferencesGroupExt, PreferencesPageExt};
    append_impl!(adw::Leaflet, adw::Carousel, adw::TabView);
    add_impl! {
        adw::PreferencesPage: adw::PreferencesGroup
    }
    add_impl! {
        adw::PreferencesGroup,
        adw::Squeezer
    }


}

