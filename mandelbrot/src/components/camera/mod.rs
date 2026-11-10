//! Camera API.

#[cfg(target_os = "linux")]
mod linux;
mod qrcode_scanner;
mod viewfinder;

pub(crate) use self::qrcode_scanner::{QrCodeScanner, ScannedQrCode};
use self::viewfinder::{
    CameraViewfinder, CameraViewfinderExt, CameraViewfinderImpl, CameraViewfinderState,
};

cfg_if::cfg_if! {
    if #[cfg(target_os = "linux")] {
        /// The camera API.
        pub(crate) type Camera = linux::LinuxCamera;
    } else {
        /// The camera API.
        pub(crate) type Camera = unimplemented::UnimplementedCamera;
    }
}

/// Trait implemented by camera backends.
pub trait CameraExt {
    /// Whether any cameras are available.
    async fn has_cameras() -> bool;

    /// Get a viewfinder displaying the output of the camera.
    ///
    /// This method should try to get the permission to access cameras, and
    /// return `None` when it fails.
    async fn viewfinder() -> Option<CameraViewfinder>;
}

/// The fallback `Camera` API, to use on platforms where it is unimplemented.
#[cfg(not(target_os = "linux"))]
mod unimplemented {
    use super::*;

    #[derive(Debug)]
    pub(crate) struct UnimplementedCamera;

    impl CameraExt for UnimplementedCamera {
        async fn has_cameras() -> bool {
            false
        }

        async fn viewfinder() -> Option<CameraViewfinder> {
            tracing::error!("The camera API is not supported on this platform");
            None
        }
    }
}
