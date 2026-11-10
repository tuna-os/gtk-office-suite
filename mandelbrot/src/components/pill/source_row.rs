use gtk::{glib, prelude::*, subclass::prelude::*};

use super::{AtRoom, Avatar, AvatarImageSafetySetting, PillSource};
use crate::session::Room;

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/pill/source_row.ui")]
    #[properties(wrapper_type = super::PillSourceRow)]
    pub struct PillSourceRow {
        #[template_child]
        avatar: TemplateChild<Avatar>,
        #[template_child]
        display_name: TemplateChild<gtk::Label>,
        #[template_child]
        id: TemplateChild<gtk::Label>,
        /// The source of the data displayed by this row.
        #[property(get, set = Self::set_source, explicit_notify, nullable)]
        source: RefCell<Option<PillSource>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PillSourceRow {
        const NAME: &'static str = "PillSourceRow";
        type Type = super::PillSourceRow;
        type ParentType = gtk::ListBoxRow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for PillSourceRow {}

    impl WidgetImpl for PillSourceRow {}
    impl ListBoxRowImpl for PillSourceRow {}

    impl PillSourceRow {
        /// Set the source of the data displayed by this row.
        fn set_source(&self, source: Option<PillSource>) {
            if *self.source.borrow() == source {
                return;
            }

            let (watched_safety_setting, watched_room) = if let Some(room) = source
                .and_downcast_ref::<Room>()
                .cloned()
                .or_else(|| source.and_downcast_ref::<AtRoom>().map(AtRoom::room))
            {
                // We must always watch the invite avatars setting for local rooms.
                (AvatarImageSafetySetting::InviteAvatars, Some(room))
            } else {
                (AvatarImageSafetySetting::None, None)
            };
            self.avatar
                .set_watched_safety_setting(watched_safety_setting);
            self.avatar.set_watched_room(watched_room);

            self.source.replace(source);
            self.obj().notify_source();
        }
    }
}

glib::wrapper! {
    /// A list row to display a [`PillSource`].
    pub struct PillSourceRow(ObjectSubclass<imp::PillSourceRow>)
        @extends gtk::Widget, gtk::ListBoxRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl PillSourceRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for PillSourceRow {
    fn default() -> Self {
        Self::new()
    }
}
