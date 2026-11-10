use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{
    glib,
    glib::{clone, closure_local},
};
use matrix_sdk::{
    authentication::oauth::qrcode::QrCodeData, encryption::verification::QrVerificationData,
};

use super::{Camera, CameraExt, CameraViewfinder, CameraViewfinderExt, CameraViewfinderState};
use crate::utils::BoundConstructOnlyObject;

/// A Matrix QR code that was scanned with the camera.
#[derive(Clone, Debug, PartialEq, glib::Boxed)]
#[boxed_type(name = "ScannedQrCode")]
pub enum ScannedQrCode {
    /// A QR code for an interactive identity verification.
    Verification(Box<QrVerificationData>),
    /// A QR code to log in a device, as defined in MSC4108.
    Login(QrCodeData),
}

mod imp {
    use std::sync::LazyLock;

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, gtk::CompositeTemplate, Default, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/camera/qrcode_scanner.ui")]
    #[properties(wrapper_type = super::QrCodeScanner)]
    pub struct QrCodeScanner {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        /// The viewfinder to use to scan the QR code.
        #[property(get, set = Self::set_viewfinder, construct_only)]
        viewfinder: BoundConstructOnlyObject<CameraViewfinder>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for QrCodeScanner {
        const NAME: &'static str = "QrCodeScanner";
        type Type = super::QrCodeScanner;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for QrCodeScanner {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    Signal::builder("qrcode-detected")
                        .param_types([ScannedQrCode::static_type()])
                        .run_first()
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }
    }

    impl WidgetImpl for QrCodeScanner {}
    impl BinImpl for QrCodeScanner {}

    impl QrCodeScanner {
        /// Set the viewfinder to use to scan the QR code.
        fn set_viewfinder(&self, viewfinder: CameraViewfinder) {
            let obj = self.obj();

            let state_handler = viewfinder.connect_state_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_visible_page();
                }
            ));
            let qrcode_detected_handler = viewfinder.connect_qrcode_detected(clone!(
                #[weak]
                obj,
                move |_, data| {
                    obj.emit_by_name::<()>("qrcode-detected", &[&data]);
                }
            ));

            viewfinder.set_overflow(gtk::Overflow::Hidden);
            viewfinder.add_css_class("card");

            self.stack
                .add_titled(&viewfinder, Some("camera"), &gettext("Camera"));

            self.viewfinder
                .set(viewfinder, vec![state_handler, qrcode_detected_handler]);

            self.update_visible_page();
        }

        /// Update the visible page according to the current state.
        fn update_visible_page(&self) {
            let name = match self.viewfinder.obj().state() {
                CameraViewfinderState::Loading => "loading",
                CameraViewfinderState::Ready => "camera",
                CameraViewfinderState::NoCameras | CameraViewfinderState::Error => "no-camera",
            };

            self.stack.set_visible_child_name(name);
        }
    }
}

glib::wrapper! {
    /// A widget to show the output of the camera and detect QR codes with it.
    pub struct QrCodeScanner(ObjectSubclass<imp::QrCodeScanner>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl QrCodeScanner {
    /// Try to construct a new `QrCodeScanner`.
    ///
    /// Returns `None` if we could not get a [`CameraViewfinder`].
    pub async fn new() -> Option<Self> {
        let viewfinder = Camera::viewfinder().await?;

        let obj = glib::Object::builder()
            .property("viewfinder", viewfinder)
            .build();

        Some(obj)
    }

    /// Connect to the signal emitted when a QR code is detected.
    pub fn connect_qrcode_detected<F: Fn(&Self, ScannedQrCode) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "qrcode-detected",
            true,
            closure_local!(move |obj: Self, data: ScannedQrCode| {
                f(&obj, data);
            }),
        )
    }
}
