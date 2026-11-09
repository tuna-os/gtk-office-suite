use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{glib, prelude::*};

use super::ContentFormat;
use crate::gettext_f;

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_row/file.ui"
    )]
    #[properties(wrapper_type = super::MessageFile)]
    pub struct MessageFile {
        /// The filename of the file.
        #[property(get, set = Self::set_filename, explicit_notify, nullable)]
        filename: RefCell<Option<String>>,
        /// Whether this file should be displayed in a compact format.
        #[property(get, set = Self::set_compact, explicit_notify)]
        compact: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageFile {
        const NAME: &'static str = "ContentMessageFile";
        type Type = super::MessageFile;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MessageFile {}

    impl WidgetImpl for MessageFile {}
    impl BinImpl for MessageFile {}

    impl MessageFile {
        /// Set the filename of the file.
        fn set_filename(&self, filename: Option<String>) {
            let filename = filename.filter(|s| !s.is_empty());

            if filename == *self.filename.borrow() {
                return;
            }

            let obj = self.obj();
            let accessible_label = if let Some(filename) = &filename {
                gettext_f("File: {filename}", &[("filename", filename)])
            } else {
                gettext("File")
            };
            obj.update_property(&[gtk::accessible::Property::Label(&accessible_label)]);

            self.filename.replace(filename);
            obj.notify_filename();
        }

        /// Set whether this file should be displayed in a compact format.
        fn set_compact(&self, compact: bool) {
            if self.compact.get() == compact {
                return;
            }

            self.compact.set(compact);
            self.obj().notify_compact();
        }
    }
}

glib::wrapper! {
    /// A widget displaying an interface to download the content of a file message.
    pub struct MessageFile(ObjectSubclass<imp::MessageFile>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MessageFile {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the format of the content to present.
    pub(crate) fn set_format(&self, format: ContentFormat) {
        self.set_compact(matches!(
            format,
            ContentFormat::Compact | ContentFormat::Ellipsized
        ));
    }
}

impl Default for MessageFile {
    fn default() -> Self {
        Self::new()
    }
}
