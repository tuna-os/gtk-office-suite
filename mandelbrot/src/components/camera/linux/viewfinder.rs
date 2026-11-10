use ashpd::desktop::camera;
use gtk::{
    glib,
    glib::{clone, subclass::prelude::*},
    prelude::*,
    subclass::prelude::*,
};
use matrix_sdk::{
    authentication::oauth::qrcode::QrCodeData, encryption::verification::QrVerificationData,
};
use tokio::task::AbortHandle;
use tracing::{debug, error};

use crate::{
    components::camera::{
        CameraViewfinder, CameraViewfinderExt, CameraViewfinderImpl, CameraViewfinderState,
        ScannedQrCode,
    },
    spawn_tokio,
};

impl From<aperture::ViewfinderState> for CameraViewfinderState {
    fn from(value: aperture::ViewfinderState) -> Self {
        match value {
            aperture::ViewfinderState::Loading => Self::Loading,
            aperture::ViewfinderState::Ready => Self::Ready,
            aperture::ViewfinderState::NoCameras => Self::NoCameras,
            aperture::ViewfinderState::Error => Self::Error,
        }
    }
}

mod imp {
    use std::cell::RefCell;

    use matrix_sdk::encryption::verification::DecodingError;

    use super::*;

    #[derive(Debug)]
    pub struct LinuxCameraViewfinder {
        /// The child viewfinder.
        child: aperture::Viewfinder,
        /// The device provider for the viewfinder.
        provider: aperture::DeviceProvider,
        abort_handle: RefCell<Option<AbortHandle>>,
    }

    impl Default for LinuxCameraViewfinder {
        fn default() -> Self {
            Self {
                child: Default::default(),
                provider: aperture::DeviceProvider::instance().clone(),
                abort_handle: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LinuxCameraViewfinder {
        const NAME: &'static str = "LinuxCameraViewfinder";
        type Type = super::LinuxCameraViewfinder;
        type ParentType = CameraViewfinder;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for LinuxCameraViewfinder {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.child.set_parent(&*obj);
            self.child.set_detect_codes(true);

            self.child.connect_state_notify(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_state();
                }
            ));
            self.update_state();

            self.child.connect_code_detected(clone!(
                #[weak]
                obj,
                move |_, code| {
                    match QrVerificationData::from_bytes(&code) {
                        Ok(data) => {
                            obj.emit_qrcode_detected(ScannedQrCode::Verification(Box::new(data)));
                        }
                        Err(error) => {
                            // It might be a QR code login QR code instead.
                            if let Ok(data) = QrCodeData::from_bytes(&code) {
                                obj.emit_qrcode_detected(ScannedQrCode::Login(data));
                                return;
                            }

                            let code = String::from_utf8_lossy(&code);

                            if matches!(error, DecodingError::Header) {
                                debug!("Detected non-Matrix QR Code: {code}");
                            } else {
                                error!(
                                    "Could not decode Matrix verification QR code {code}: {error}"
                                );
                            }
                        }
                    }
                }
            ));
        }

        fn dispose(&self) {
            self.child.stop_stream();
            self.child.unparent();

            if let Some(abort_handle) = self.abort_handle.take() {
                abort_handle.abort();
            }
        }
    }

    impl WidgetImpl for LinuxCameraViewfinder {}
    impl CameraViewfinderImpl for LinuxCameraViewfinder {}

    impl LinuxCameraViewfinder {
        /// Initialize the viewfinder.
        pub(super) async fn init(&self) -> Result<(), ()> {
            if self.provider.started() {
                return Ok(());
            }

            let handle = spawn_tokio!(camera::request());
            self.set_abort_handle(Some(handle.abort_handle()));

            let Ok(request_result) = handle.await else {
                debug!("Camera request was aborted");
                self.set_abort_handle(None);
                return Err(());
            };

            self.set_abort_handle(None);

            let fd = match request_result {
                Ok(Some(fd)) => fd,
                Ok(None) => {
                    error!("Could not access camera: no camera present");
                    return Err(());
                }
                Err(error) => {
                    error!("Could not access camera: {error}");
                    return Err(());
                }
            };

            if let Err(error) = self.provider.set_fd(fd) {
                error!("Could not access camera: {error}");
                return Err(());
            }

            if let Err(error) = self.provider.start_with_default(|camera| {
                matches!(camera.location(), aperture::CameraLocation::Back)
            }) {
                error!("Could not access camera: {error}");
                return Err(());
            }

            Ok(())
        }

        /// Update the current state.
        fn update_state(&self) {
            self.obj().set_state(self.child.state().into());
        }

        /// Set the current abort handle.
        fn set_abort_handle(&self, abort_handle: Option<AbortHandle>) {
            self.abort_handle.replace(abort_handle);
        }
    }
}

glib::wrapper! {
    /// A camera viewfinder widget for Linux.
    pub struct LinuxCameraViewfinder(ObjectSubclass<imp::LinuxCameraViewfinder>)
        @extends gtk::Widget, CameraViewfinder,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LinuxCameraViewfinder {
    pub(super) async fn new() -> Option<Self> {
        let obj = glib::Object::new::<Self>();

        obj.imp().init().await.ok()?;

        Some(obj)
    }
}
