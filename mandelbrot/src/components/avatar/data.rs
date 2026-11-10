use gtk::{gdk, glib, prelude::*, subclass::prelude::*};
use tracing::warn;

use super::AvatarImage;
use crate::{
    application::Application,
    utils::notifications::{paintable_as_notification_icon, string_as_notification_icon},
};

mod imp {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::AvatarData)]
    pub struct AvatarData {
        /// The data of the user-defined image.
        #[property(get, set = Self::set_image, explicit_notify, nullable)]
        image: RefCell<Option<AvatarImage>>,
        /// The display name used as a fallback for this avatar.
        #[property(get, set = Self::set_display_name, explicit_notify)]
        display_name: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AvatarData {
        const NAME: &'static str = "AvatarData";
        type Type = super::AvatarData;
    }

    #[glib::derived_properties]
    impl ObjectImpl for AvatarData {}

    impl AvatarData {
        /// Set the data of the user-defined image.
        fn set_image(&self, image: Option<AvatarImage>) {
            if *self.image.borrow() == image {
                return;
            }

            self.image.replace(image);
            self.obj().notify_image();
        }

        /// Set the display name used as a fallback for this avatar.
        fn set_display_name(&self, display_name: String) {
            if *self.display_name.borrow() == display_name {
                return;
            }

            self.display_name.replace(display_name);
            self.obj().notify_display_name();
        }
    }
}

glib::wrapper! {
    /// Data about a User’s or Room’s avatar.
    pub struct AvatarData(ObjectSubclass<imp::AvatarData>);
}

impl AvatarData {
    /// Construct a new empty `AvatarData`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Get this avatar as a notification icon.
    ///
    /// If `inhibit_image` is set, the image of the avatar will not be used.
    ///
    /// Returns `None` if an error occurred while generating the icon.
    pub(crate) async fn as_notification_icon(&self, inhibit_image: bool) -> Option<gdk::Texture> {
        let Some(window) = Application::default().active_window() else {
            warn!("Could not generate icon for notification: no active window");
            return None;
        };
        let Some(renderer) = window.renderer() else {
            warn!("Could not generate icon for notification: no renderer");
            return None;
        };
        let scale_factor = window.scale_factor();

        if !inhibit_image && let Some(image) = self.image() {
            match image.load_small_paintable().await {
                Ok(Some(paintable)) => {
                    let texture = paintable_as_notification_icon(
                        paintable.upcast_ref(),
                        scale_factor,
                        &renderer,
                    );
                    return Some(texture);
                }
                // No paintable, we will try to generate the fallback.
                Ok(None) => {}
                // Could not get the paintable, we will try to generate the fallback.
                Err(error) => {
                    warn!("Could not generate icon for notification: {error}");
                }
            }
        }

        let texture = string_as_notification_icon(
            &self.display_name(),
            scale_factor,
            &window.create_pango_layout(None),
            &renderer,
        );
        Some(texture)
    }
}

impl Default for AvatarData {
    fn default() -> Self {
        Self::new()
    }
}
