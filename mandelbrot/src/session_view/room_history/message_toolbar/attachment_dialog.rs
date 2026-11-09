use adw::{prelude::*, subclass::prelude::*};
use gtk::{gdk, gio, glib, glib::clone};

use crate::{components::MediaContentViewer, spawn, utils::OneshotNotifier};

mod imp {
    use std::cell::OnceCell;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_toolbar/attachment_dialog.ui"
    )]
    pub struct AttachmentDialog {
        #[template_child]
        cancel_button: TemplateChild<gtk::Button>,
        #[template_child]
        send_button: TemplateChild<gtk::Button>,
        #[template_child]
        media: TemplateChild<MediaContentViewer>,
        notifier: OnceCell<OneshotNotifier<Option<()>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AttachmentDialog {
        const NAME: &'static str = "AttachmentDialog";
        type Type = super::AttachmentDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for AttachmentDialog {
        fn constructed(&self) {
            self.parent_constructed();

            self.set_loading(true);
        }
    }

    impl WidgetImpl for AttachmentDialog {
        fn grab_focus(&self) -> bool {
            let loading = !self.send_button.is_sensitive();

            if loading {
                self.cancel_button.grab_focus()
            } else {
                self.send_button.grab_focus()
            }
        }
    }

    impl AdwDialogImpl for AttachmentDialog {
        fn closed(&self) {
            self.notifier().notify();
        }
    }

    #[gtk::template_callbacks]
    impl AttachmentDialog {
        /// Set whether this dialog is loading.
        fn set_loading(&self, loading: bool) {
            self.send_button.set_sensitive(!loading);
            self.grab_focus();
        }

        /// The notifier to send the response.
        fn notifier(&self) -> &OneshotNotifier<Option<()>> {
            self.notifier
                .get_or_init(|| OneshotNotifier::new("AttachmentDialog"))
        }

        /// Set the image to preview.
        pub(super) fn set_image(&self, image: &gdk::Texture) {
            self.media.view_image(image);
            self.set_loading(false);
        }

        /// Set the file to preview.
        pub(super) async fn set_file(&self, file: gio::File) {
            self.media.view_file(file.into(), None).await;
            self.set_loading(false);
        }

        /// Set the location to preview.
        pub(super) fn set_location(&self, geo_uri: &geo_uri::GeoUri) {
            self.media.view_location(geo_uri);
            self.set_loading(false);
        }

        /// Emit the signal that the user wants to send the attachment.
        #[template_callback]
        fn send(&self) {
            self.notifier().notify_value(Some(()));
            self.obj().close();
        }

        /// Present the dialog and wait for the user to select a response.
        ///
        /// The response is [`gtk::ResponseType::Ok`] if the user clicked on
        /// send, otherwise it is [`gtk::ResponseType::Cancel`].
        pub(super) async fn response_future(&self, parent: &gtk::Widget) -> gtk::ResponseType {
            let receiver = self.notifier().listen();

            self.obj().present(Some(parent));

            if receiver.await.is_some() {
                gtk::ResponseType::Ok
            } else {
                gtk::ResponseType::Cancel
            }
        }
    }
}

glib::wrapper! {
    /// A dialog to preview an attachment before sending it.
    pub struct AttachmentDialog(ObjectSubclass<imp::AttachmentDialog>)
        @extends gtk::Widget, adw::Dialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl AttachmentDialog {
    /// Create an attachment dialog with the given title.
    ///
    /// Its initial state is loading.
    pub fn new(title: &str) -> Self {
        glib::Object::builder().property("title", title).build()
    }

    /// Set the image to preview.
    pub(crate) fn set_image(&self, image: &gdk::Texture) {
        self.imp().set_image(image);
    }

    /// Set the file to preview.
    pub(crate) fn set_file(&self, file: gio::File) {
        let imp = self.imp();

        spawn!(clone!(
            #[weak]
            imp,
            async move {
                imp.set_file(file).await;
            }
        ));
    }

    /// Create an attachment dialog to preview and send a location.
    pub(crate) fn set_location(&self, geo_uri: &geo_uri::GeoUri) {
        self.imp().set_location(geo_uri);
    }

    /// Present the dialog and wait for the user to select a response.
    ///
    /// The response is [`gtk::ResponseType::Ok`] if the user clicked on send,
    /// otherwise it is [`gtk::ResponseType::Cancel`].
    pub(crate) async fn response_future(
        &self,
        parent: &impl IsA<gtk::Widget>,
    ) -> gtk::ResponseType {
        self.imp().response_future(parent.upcast_ref()).await
    }
}
