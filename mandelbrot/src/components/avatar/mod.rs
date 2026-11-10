use adw::{prelude::*, subclass::prelude::*};
use gtk::{gdk, glib, glib::clone};

mod crop_circle;
mod data;
mod editable;
mod image;
mod overlapping;

use self::image::AvatarPaintableSize;
pub use self::{
    data::AvatarData,
    editable::EditableAvatar,
    image::{AvatarImage, AvatarUriSource},
    overlapping::OverlappingAvatars,
};
use crate::{
    components::AnimatedImagePaintable,
    session::Room,
    utils::{BoundObject, BoundObjectWeakRef, CountedRef},
};

/// The safety setting to watch to decide whether the image of the avatar should
/// be displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, glib::Enum, Default)]
#[enum_type(name = "AvatarImageSafetySetting")]
pub enum AvatarImageSafetySetting {
    /// No setting needs to be watched, the image is always shown when
    /// available.
    #[default]
    None,

    /// The media previews safety setting should be watched, with the image only
    /// shown when allowed.
    ///
    /// This setting also requires the [`Room`] where the avatar is presented.
    MediaPreviews,

    /// The invite avatars safety setting should be watched, with the image only
    /// shown when allowed.
    ///
    /// This setting also requires the [`Room`] where the avatar is presented.
    InviteAvatars,
}

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
    };

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/avatar/mod.ui")]
    #[properties(wrapper_type = super::Avatar)]
    pub struct Avatar {
        #[template_child]
        avatar: TemplateChild<adw::Avatar>,
        /// The [`AvatarData`] displayed by this widget.
        #[property(get, set = Self::set_data, explicit_notify, nullable)]
        data: BoundObject<AvatarData>,
        /// The [`AvatarImage`] watched by this widget.
        #[property(get)]
        image: BoundObjectWeakRef<AvatarImage>,
        /// The size of the Avatar.
        #[property(get = Self::size, set = Self::set_size, explicit_notify, builder().default_value(-1).minimum(-1))]
        size: PhantomData<i32>,
        /// The safety setting to watch to decide whether the image of the
        /// avatar should be displayed.
        #[property(get, set = Self::set_watched_safety_setting, explicit_notify, builder(AvatarImageSafetySetting::default()))]
        watched_safety_setting: Cell<AvatarImageSafetySetting>,
        /// The room to watch to apply the current safety settings.
        ///
        /// This is required if `watched_safety_setting` is not `None`.
        #[property(get, set = Self::set_watched_room, explicit_notify, nullable)]
        watched_room: RefCell<Option<Room>>,
        paintable_ref: RefCell<Option<CountedRef>>,
        paintable_animation_ref: RefCell<Option<CountedRef>>,
        watched_room_handler: RefCell<Option<glib::SignalHandlerId>>,
        watched_global_account_data_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Avatar {
        const NAME: &'static str = "Avatar";
        type Type = super::Avatar;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            AvatarImage::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Avatar {
        fn dispose(&self) {
            self.disconnect_safety_setting_signals();
        }
    }

    impl WidgetImpl for Avatar {
        fn map(&self) {
            self.parent_map();
            self.update_paintable();
        }

        fn unmap(&self) {
            self.parent_unmap();
            self.update_animated_paintable_state();
        }
    }

    impl BinImpl for Avatar {}

    impl AccessibleImpl for Avatar {
        fn first_accessible_child(&self) -> Option<gtk::Accessible> {
            // Hide the children in the a11y tree.
            None
        }
    }

    #[gtk::template_callbacks]
    impl Avatar {
        /// The size of the Avatar.
        fn size(&self) -> i32 {
            self.avatar.size()
        }

        /// Set the size of the Avatar.
        fn set_size(&self, size: i32) {
            if self.size() == size {
                return;
            }

            self.avatar.set_size(size);

            self.update_paintable();
            self.obj().notify_size();
        }

        /// Set the safety setting to watch to decide whether the image of the
        /// avatar should be displayed.
        fn set_watched_safety_setting(&self, setting: AvatarImageSafetySetting) {
            if self.watched_safety_setting.get() == setting {
                return;
            }

            self.disconnect_safety_setting_signals();

            self.watched_safety_setting.set(setting);

            self.connect_safety_setting_signals();
            self.obj().notify_watched_safety_setting();
        }

        /// Set the room to watch to apply the current safety settings.
        fn set_watched_room(&self, room: Option<Room>) {
            if *self.watched_room.borrow() == room {
                return;
            }

            self.disconnect_safety_setting_signals();

            self.watched_room.replace(room);

            self.connect_safety_setting_signals();
            self.obj().notify_watched_room();
        }

        /// Connect to the proper signals for the current safety setting.
        fn connect_safety_setting_signals(&self) {
            let Some(room) = self.watched_room.borrow().clone() else {
                return;
            };
            let Some(session) = room.session() else {
                return;
            };

            match self.watched_safety_setting.get() {
                AvatarImageSafetySetting::None => {}
                AvatarImageSafetySetting::MediaPreviews => {
                    let room_handler = room.connect_join_rule_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_paintable();
                        }
                    ));
                    self.watched_room_handler.replace(Some(room_handler));

                    let global_account_data_handler = session
                        .global_account_data()
                        .connect_media_previews_enabled_changed(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            move |_| {
                                imp.update_paintable();
                            }
                        ));
                    self.watched_global_account_data_handler
                        .replace(Some(global_account_data_handler));
                }
                AvatarImageSafetySetting::InviteAvatars => {
                    let room_handler = room.connect_is_invite_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_paintable();
                        }
                    ));
                    self.watched_room_handler.replace(Some(room_handler));

                    let global_account_data_handler = session
                        .global_account_data()
                        .connect_invite_avatars_enabled_notify(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            move |_| {
                                imp.update_paintable();
                            }
                        ));
                    self.watched_global_account_data_handler
                        .replace(Some(global_account_data_handler));
                }
            }

            self.update_paintable();
        }

        /// Disconnect the handlers for the signals of the safety setting.
        fn disconnect_safety_setting_signals(&self) {
            if let Some(room) = self.watched_room.borrow().as_ref() {
                if let Some(handler) = self.watched_room_handler.take() {
                    room.disconnect(handler);
                }

                if let Some(handler) = self.watched_global_account_data_handler.take() {
                    room.session()
                        .inspect(|session| session.global_account_data().disconnect(handler));
                }
            }
        }

        /// Whether we can display the image of the avatar with the current
        /// state.
        fn can_show_image(&self) -> bool {
            let watched_safety_setting = self.watched_safety_setting.get();

            if watched_safety_setting == AvatarImageSafetySetting::None {
                return true;
            }

            let Some(room) = self.watched_room.borrow().clone() else {
                return false;
            };
            let Some(session) = room.session() else {
                return false;
            };

            match watched_safety_setting {
                AvatarImageSafetySetting::None => unreachable!(),
                AvatarImageSafetySetting::MediaPreviews => session
                    .global_account_data()
                    .should_room_show_media_previews(&room),
                AvatarImageSafetySetting::InviteAvatars => {
                    !room.is_invite() || session.global_account_data().invite_avatars_enabled()
                }
            }
        }

        /// Set the [`AvatarData`] displayed by this widget.
        fn set_data(&self, data: Option<AvatarData>) {
            if self.data.obj() == data {
                return;
            }

            self.data.disconnect_signals();

            if let Some(data) = data {
                let image_handler = data.connect_image_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_image();
                    }
                ));

                self.data.set(data, vec![image_handler]);
            }

            self.update_image();
            self.obj().notify_data();
        }

        /// Set the [`AvatarImage`] watched by this widget.
        fn update_image(&self) {
            let image = self.data.obj().and_then(|data| data.image());

            if self.image.obj() == image {
                return;
            }

            self.image.disconnect_signals();

            if let Some(image) = &image {
                let small_paintable_handler = image.connect_small_paintable_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_paintable();
                    }
                ));
                let big_paintable_handler = image.connect_big_paintable_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_paintable();
                    }
                ));

                self.image
                    .set(image, vec![small_paintable_handler, big_paintable_handler]);
            }

            self.update_scale_factor();
            self.update_paintable();

            self.obj().notify_image();
        }

        /// Whether this avatar needs a small paintable.
        fn needs_small_paintable(&self) -> bool {
            AvatarPaintableSize::from(self.size()) == AvatarPaintableSize::Small
        }

        /// Update the scale factor used to load the paintable.
        #[template_callback]
        fn update_scale_factor(&self) {
            let Some(image) = self.image.obj() else {
                return;
            };

            let scale_factor = self.obj().scale_factor().try_into().unwrap_or(1);
            image.set_scale_factor(scale_factor);
        }

        /// Update the paintable for this avatar.
        fn update_paintable(&self) {
            let _old_paintable_ref = self.paintable_ref.take();

            if !self.can_show_image() {
                // We need to unset the paintable.
                self.avatar.set_custom_image(None::<&gdk::Paintable>);
                self.update_animated_paintable_state();
                return;
            }

            if !self.obj().is_mapped() {
                // We do not need a paintable.
                self.update_animated_paintable_state();
                return;
            }

            let Some(image) = self.image.obj() else {
                self.update_animated_paintable_state();
                return;
            };

            let (paintable, paintable_ref) = if self.needs_small_paintable() {
                (image.small_paintable(), image.small_paintable_ref())
            } else {
                (
                    // Fallback to small paintable while the big paintable is loading.
                    image.big_paintable().or_else(|| image.small_paintable()),
                    image.big_paintable_ref(),
                )
            };
            self.avatar.set_custom_image(paintable.as_ref());
            self.paintable_ref.replace(Some(paintable_ref));

            self.update_animated_paintable_state();
        }

        /// Update the state of the animated paintable for this avatar.
        fn update_animated_paintable_state(&self) {
            let _old_paintable_animation_ref = self.paintable_animation_ref.take();

            if !self.can_show_image() || !self.obj().is_mapped() {
                // We do not need to animate the paintable.
                return;
            }

            let Some(image) = self.image.obj() else {
                return;
            };

            let paintable = if self.needs_small_paintable() {
                image.small_paintable()
            } else {
                image.big_paintable()
            };

            let Some(paintable) = paintable.and_downcast::<AnimatedImagePaintable>() else {
                return;
            };

            self.paintable_animation_ref
                .replace(Some(paintable.animation_ref()));
        }
    }
}

glib::wrapper! {
    /// A widget displaying an `Avatar` for a `Room` or `User`.
    pub struct Avatar(ObjectSubclass<imp::Avatar>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Avatar {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
