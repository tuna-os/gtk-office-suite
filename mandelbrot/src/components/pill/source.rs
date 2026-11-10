use gtk::{glib, prelude::*, subclass::prelude::*};

use super::Pill;
use crate::{
    components::{AvatarData, AvatarImageSafetySetting},
    session::Room,
};

mod imp {
    use std::{cell::Cell, marker::PhantomData};

    use super::*;

    #[repr(C)]
    pub struct PillSourceClass {
        parent_class: glib::object::ObjectClass,
        pub(super) identifier: fn(&super::PillSource) -> String,
    }

    unsafe impl ClassStruct for PillSourceClass {
        type Type = PillSource;
    }

    pub(super) fn pill_source_identifier(this: &super::PillSource) -> String {
        let klass = this.class();
        (klass.as_ref().identifier)(this)
    }

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::PillSource)]
    pub struct PillSource {
        /// A unique identifier for this source.
        #[property(get = Self::identifier)]
        identifier: PhantomData<String>,
        /// The display name of this source.
        #[property(get = Self::display_name, set = Self::set_display_name, explicit_notify)]
        display_name: PhantomData<String>,
        /// Whether the display name of this source is ambiguous.
        #[property(get, set = Self::set_name_ambiguous, explicit_notify)]
        is_name_ambiguous: Cell<bool>,
        /// The disambiguated display name of this source.
        ///
        /// This is the name to display in case the identifier does not appear
        /// next to it.
        #[property(get = Self::disambiguated_name)]
        disambiguated_name: PhantomData<String>,
        /// The avatar data of this source.
        #[property(get)]
        avatar_data: AvatarData,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PillSource {
        const NAME: &'static str = "PillSource";
        const ABSTRACT: bool = true;
        type Type = super::PillSource;
        type Class = PillSourceClass;
    }

    #[glib::derived_properties]
    impl ObjectImpl for PillSource {}

    impl PillSource {
        /// A unique identifier for this source.
        fn identifier(&self) -> String {
            imp::pill_source_identifier(&self.obj())
        }

        /// The display name of this source.
        fn display_name(&self) -> String {
            self.avatar_data.display_name()
        }

        /// Set the display name of this source.
        fn set_display_name(&self, display_name: String) {
            if self.display_name() == display_name {
                return;
            }

            self.avatar_data.set_display_name(display_name);

            let obj = self.obj();
            obj.notify_display_name();
            obj.notify_disambiguated_name();
        }

        /// Set whether the display name of this source is ambiguous.
        fn set_name_ambiguous(&self, is_ambiguous: bool) {
            if self.is_name_ambiguous.get() == is_ambiguous {
                return;
            }

            self.is_name_ambiguous.set(is_ambiguous);

            let obj = self.obj();
            obj.notify_is_name_ambiguous();
            obj.notify_disambiguated_name();
        }

        /// The disambiguated display name of this source.
        fn disambiguated_name(&self) -> String {
            let display_name = self.display_name();

            if self.is_name_ambiguous.get() {
                format!("{display_name} ({})", self.identifier())
            } else {
                display_name
            }
        }
    }
}

glib::wrapper! {
    /// Parent class of objects that can be represented as a `Pill`.
    pub struct PillSource(ObjectSubclass<imp::PillSource>);
}

/// Public trait containing implemented methods for everything that derives from
/// `PillSource`.
///
/// To override the behavior of these methods, override the corresponding method
/// of `PillSourceImpl`.
pub trait PillSourceExt: 'static {
    /// A unique identifier for this source.
    #[allow(dead_code)]
    fn identifier(&self) -> String;

    /// The display name of this source.
    fn display_name(&self) -> String;

    /// Set the display name of this source.
    fn set_display_name(&self, display_name: String);

    /// Whether the display name of this source is ambiguous.
    #[allow(dead_code)]
    fn is_name_ambiguous(&self) -> bool;

    /// Set whether the display name of this source is ambiguous.
    fn set_is_name_ambiguous(&self, is_ambiguous: bool);

    /// The disambiguated display name of this source.
    ///
    /// This is the name to display in case the identifier does not appear next
    /// to it.
    fn disambiguated_name(&self) -> String;

    /// The avatar data of this source.
    fn avatar_data(&self) -> AvatarData;

    /// Connect to the signal emitted when the display name changes.
    fn connect_display_name_notify<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId;

    /// Connect to the signal emitted when the disambiguated name changes.
    fn connect_disambiguated_name_notify<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId;

    /// Get a `Pill` representing this source, watching the given safety
    /// setting.
    fn to_pill(
        &self,
        watched_safety_setting: AvatarImageSafetySetting,
        watched_room: Option<Room>,
    ) -> Pill;
}

impl<O: IsA<PillSource>> PillSourceExt for O {
    /// A unique identifier for this source.
    fn identifier(&self) -> String {
        self.upcast_ref().identifier()
    }

    /// The display name of this source.
    fn display_name(&self) -> String {
        self.upcast_ref().display_name()
    }

    /// Set the display name of this source.
    fn set_display_name(&self, display_name: String) {
        self.upcast_ref().set_display_name(display_name);
    }

    /// Whether the display name of this source is ambiguous.
    fn is_name_ambiguous(&self) -> bool {
        self.upcast_ref().is_name_ambiguous()
    }

    /// Set whether the display name of this source is ambiguous.
    fn set_is_name_ambiguous(&self, is_ambiguous: bool) {
        self.upcast_ref().set_is_name_ambiguous(is_ambiguous);
    }

    /// The disambiguated display name of this source.
    fn disambiguated_name(&self) -> String {
        self.upcast_ref().disambiguated_name()
    }

    /// The avatar data of this source.
    fn avatar_data(&self) -> AvatarData {
        self.upcast_ref().avatar_data()
    }

    /// Connect to the signal emitted when the display name changes.
    fn connect_display_name_notify<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.upcast_ref()
            .connect_display_name_notify(move |source| f(source.downcast_ref().unwrap()))
    }

    /// Connect to the signal emitted when the disambiguated name changes.
    fn connect_disambiguated_name_notify<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.upcast_ref()
            .connect_disambiguated_name_notify(move |source| f(source.downcast_ref().unwrap()))
    }

    /// Get a `Pill` representing this source.
    fn to_pill(
        &self,
        watched_safety_setting: AvatarImageSafetySetting,
        watched_room: Option<Room>,
    ) -> Pill {
        Pill::new(self, watched_safety_setting, watched_room)
    }
}

/// Public trait that must be implemented for everything that derives from
/// `PillSource`.
///
/// Overriding a method from this Trait overrides also its behavior in
/// `PillSourceExt`.
pub trait PillSourceImpl: ObjectImpl {
    /// A unique identifier for this source.
    fn identifier(&self) -> String;
}

// Make `PillSource` subclassable.
unsafe impl<T> IsSubclassable<T> for PillSource
where
    T: PillSourceImpl,
    T::Type: IsA<PillSource>,
{
    fn class_init(class: &mut glib::Class<Self>) {
        Self::parent_class_init::<T>(class.upcast_ref_mut());

        let klass = class.as_mut();

        klass.identifier = identifier_trampoline::<T>;
    }
}

// Virtual method implementation trampolines.
fn identifier_trampoline<T>(this: &PillSource) -> String
where
    T: ObjectSubclass + PillSourceImpl,
    T::Type: IsA<PillSource>,
{
    let this = this.downcast_ref::<T::Type>().unwrap();
    this.imp().identifier()
}
