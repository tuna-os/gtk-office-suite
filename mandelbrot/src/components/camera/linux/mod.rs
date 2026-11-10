use std::time::Duration;

use ashpd::desktop::camera;
use gtk::prelude::*;
use tokio::time::timeout;
use tracing::error;

mod viewfinder;

use self::viewfinder::LinuxCameraViewfinder;
use super::{CameraExt, CameraViewfinder};
use crate::spawn_tokio;

/// Camera API under Linux.
#[derive(Debug)]
pub(crate) struct LinuxCamera;

impl CameraExt for LinuxCamera {
    async fn has_cameras() -> bool {
        let fut = async move {
            let camera = match camera::Camera::new().await {
                Ok(camera) => camera,
                Err(error) => {
                    error!("Could not create instance of camera proxy: {error}");
                    return false;
                }
            };

            match camera.is_present().await {
                Ok(is_present) => is_present,
                Err(error) => {
                    error!("Could not check whether system has cameras: {error}");
                    false
                }
            }
        };
        let handle = spawn_tokio!(async move { timeout(Duration::from_secs(1), fut).await });

        if let Ok(is_present) = handle.await.expect("task was not aborted") {
            is_present
        } else {
            error!("Could not check whether system has cameras: request timed out");
            false
        }
    }

    async fn viewfinder() -> Option<CameraViewfinder> {
        LinuxCameraViewfinder::new().await.and_upcast()
    }
}
