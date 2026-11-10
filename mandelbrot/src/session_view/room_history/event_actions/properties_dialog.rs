use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::glib;
use sourceview::prelude::*;

use crate::{
    components::{CopyableRow, ToastableDialog, UserProfileDialog},
    prelude::*,
    session::Event,
    toast, utils,
    utils::TemplateCallbacks,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/event_actions/properties_dialog.ui"
    )]
    #[properties(wrapper_type = super::EventPropertiesDialog)]
    pub struct EventPropertiesDialog {
        /// The event that is displayed in the dialog.
        #[property(get, construct_only)]
        event: RefCell<Option<Event>>,
        #[template_child]
        navigation_view: TemplateChild<adw::NavigationView>,
        #[template_child]
        source_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        source_view: TemplateChild<sourceview::View>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EventPropertiesDialog {
        const NAME: &'static str = "EventPropertiesDialog";
        type Type = super::EventPropertiesDialog;
        type ParentType = ToastableDialog;

        fn class_init(klass: &mut Self::Class) {
            CopyableRow::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
            TemplateCallbacks::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for EventPropertiesDialog {
        fn constructed(&self) {
            self.parent_constructed();

            let json_lang = sourceview::LanguageManager::default().language("json");

            let buffer = self
                .source_view
                .buffer()
                .downcast::<sourceview::Buffer>()
                .unwrap();
            buffer.set_language(json_lang.as_ref());
            utils::sourceview::setup_style_scheme(&buffer);
        }
    }

    impl WidgetImpl for EventPropertiesDialog {}
    impl AdwDialogImpl for EventPropertiesDialog {}
    impl ToastableDialogImpl for EventPropertiesDialog {}

    #[gtk::template_callbacks]
    impl EventPropertiesDialog {
        /// View the given source.
        fn show_source(&self, title: &str, source: &str) {
            self.source_view.buffer().set_text(source);
            self.source_page.set_title(title);
            self.navigation_view.push_by_tag("source");
        }

        /// Open the profile of the sender.
        #[template_callback]
        fn open_sender_profile(&self) {
            let Some(sender) = self.event.borrow().as_ref().map(Event::sender) else {
                return;
            };

            let dialog = UserProfileDialog::new();
            dialog.set_room_member(sender);
            dialog.present(Some(&*self.obj()));
        }

        /// View the original source.
        #[template_callback]
        fn show_original_source(&self) {
            let Some(event) = self.event.borrow().clone() else {
                return;
            };

            if let Some(source) = event.source() {
                let title = if event.is_edited() {
                    gettext("Original Event Source")
                } else {
                    gettext("Event Source")
                };
                self.show_source(&title, &source);
            }
        }

        /// View the source of the latest edit.
        #[template_callback]
        fn show_edit_source(&self) {
            let Some(event) = self.event.borrow().clone() else {
                return;
            };

            let source = event.latest_edit_source();
            let title = gettext("Latest Edit Source");
            self.show_source(&title, &source);
        }

        /// Copy the source that is currently shown.
        #[template_callback]
        fn copy_source(&self) {
            let obj = self.obj();

            let buffer = self.source_view.buffer();
            let (start_iter, end_iter) = buffer.bounds();
            obj.clipboard()
                .set_text(&buffer.text(&start_iter, &end_iter, true));

            toast!(obj, gettext("Source copied to clipboard"));
        }
    }
}

glib::wrapper! {
    /// A dialog showing the properties of an event.
    pub struct EventPropertiesDialog(ObjectSubclass<imp::EventPropertiesDialog>)
        @extends gtk::Widget, adw::Dialog, ToastableDialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl EventPropertiesDialog {
    pub fn new(event: &Event) -> Self {
        glib::Object::builder().property("event", event).build()
    }
}
