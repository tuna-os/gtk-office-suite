use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gio, glib};
use tracing::error;

use super::HistoryViewerEvent;
use crate::{gettext_f, prelude::*, toast, utils::matrix::MediaMessage};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/history_viewer/file_row.ui"
    )]
    #[properties(wrapper_type = super::FileRow)]
    pub struct FileRow {
        #[template_child]
        button: TemplateChild<gtk::Button>,
        #[template_child]
        title_label: TemplateChild<gtk::Label>,
        #[template_child]
        size_label: TemplateChild<gtk::Label>,
        /// The file event.
        #[property(get, set = Self::set_event, explicit_notify, nullable)]
        event: RefCell<Option<HistoryViewerEvent>>,
        file: RefCell<Option<gio::File>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FileRow {
        const NAME: &'static str = "ContentFileHistoryViewerRow";
        type Type = super::FileRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for FileRow {}

    impl WidgetImpl for FileRow {}
    impl BinImpl for FileRow {}

    #[gtk::template_callbacks]
    impl FileRow {
        /// Set the file event.
        fn set_event(&self, event: Option<HistoryViewerEvent>) {
            if *self.event.borrow() == event {
                return;
            }

            if let Some(event) = &event {
                let media_message = event.media_message();
                if let MediaMessage::File(file) = &media_message {
                    let filename = media_message.filename(&event.timestamp());

                    self.title_label.set_label(&filename);
                    self.button
                        .update_property(&[gtk::accessible::Property::Label(&gettext_f(
                            // Translators: Do NOT translate the content between '{' and '}',
                            // this is a variable name.
                            "Save {filename}",
                            &[("filename", &filename)],
                        ))]);

                    if let Some(size) = file.info.as_ref().and_then(|i| i.size) {
                        let size = glib::format_size(size.into());
                        self.size_label.set_label(&size);
                    } else {
                        self.size_label.set_label(&gettext("Unknown size"));
                    }
                }
            }

            self.event.replace(event);
            self.file.take();
            self.update_button();

            self.obj().notify_event();
        }

        /// Update the button for the current state.
        pub(super) fn update_button(&self) {
            if self.file.borrow().is_some() {
                self.button.set_icon_name("document-symbolic");
                self.button.set_tooltip_text(Some(&gettext("Open File")));
            } else {
                self.button.set_icon_name("save-symbolic");
                self.button.set_tooltip_text(Some(&gettext("Save File")));
            }
        }

        /// Handle when the row's button was clicked.
        #[template_callback]
        async fn button_clicked(&self) {
            let file = self.file.borrow().clone();

            // If there is a file, open it.
            if let Some(file) = file {
                if let Err(error) =
                    gio::AppInfo::launch_default_for_uri(&file.uri(), gio::AppLaunchContext::NONE)
                {
                    error!("Could not open file: {error}");
                }
            } else {
                // Otherwise save the file.
                self.save_file().await;
            }
        }

        /// Save the file of this row.
        async fn save_file(&self) {
            let Some(event) = self.event.borrow().clone() else {
                return;
            };
            let obj = self.obj();

            let data = match event.get_file_content().await {
                Ok(res) => res,
                Err(error) => {
                    error!("Could not get file: {error}");
                    toast!(obj, error.to_user_facing());

                    return;
                }
            };
            let filename = event.media_message().filename(&event.timestamp());

            let parent_window = obj.root().and_downcast::<gtk::Window>();
            let dialog = gtk::FileDialog::builder()
                .title(gettext("Save File"))
                .accept_label(gettext("Save"))
                .initial_name(filename)
                .build();

            if let Ok(file) = dialog.save_future(parent_window.as_ref()).await {
                if let Err(error) = file.replace_contents(
                    &data,
                    None,
                    false,
                    gio::FileCreateFlags::REPLACE_DESTINATION,
                    gio::Cancellable::NONE,
                ) {
                    error!("Could not write file content: {error}");
                    toast!(obj, gettext("Could not save file"));
                    return;
                }

                self.file.replace(Some(file));
                self.update_button();
            }
        }
    }
}

glib::wrapper! {
    /// A row presenting a file event.
    pub struct FileRow(ObjectSubclass<imp::FileRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FileRow {
    /// Construct an empty `FileRow`.
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for FileRow {
    fn default() -> Self {
        Self::new()
    }
}
