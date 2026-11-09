//! Camera viewfinder API.

use gettextrs::gettext;
use gtk::{glib, glib::closure_local, prelude::*, subclass::prelude::*};

use super::ScannedQrCode;

/// The possible states of a [`CameraViewfinder`].
#[derive(Default, Debug, Copy, Clone, glib::Enum, PartialEq)]
#[enum_type(name = "CameraViewfinderState")]
pub enum CameraViewfinderState {
    /// The viewfinder is still loading.
    #[default]
    Loading,
    /// The viewfinder is ready for use.
    Ready,
    /// The viewfinder could not find any cameras to use.
    NoCameras,
    /// The viewfinder had an error and is not usable.
    Error,
}

mod imp {
    use std::{cell::Cell, sync::LazyLock};

    use glib::subclass::Signal;

    use super::*;

    #[repr(C)]
    pub struct CameraViewfinderClass {
        parent_class: glib::object::Class<gtk::Widget>,
    }

    unsafe impl ClassStruct for CameraViewfinderClass {
        type Type = CameraViewfinder;
    }

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::CameraViewfinder)]
    pub struct CameraViewfinder {
        /// The state of this viewfinder.
        #[property(get, set = Self::set_state, explicit_notify, builder(CameraViewfinderState::default()))]
        state: Cell<CameraViewfinderState>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CameraViewfinder {
        const NAME: &'static str = "CameraViewfinder";
        type Type = super::CameraViewfinder;
        type ParentType = gtk::Widget;
        type Class = CameraViewfinderClass;
    }

    #[glib::derived_properties]
    impl ObjectImpl for CameraViewfinder {
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

        fn constructed(&self) {
            self.parent_constructed();

            self.obj()
                .update_property(&[gtk::accessible::Property::Label(&gettext("Viewfinder"))]);
        }
    }

    impl WidgetImpl for CameraViewfinder {}

    impl CameraViewfinder {
        /// Set the state of this viewfinder.
        fn set_state(&self, state: CameraViewfinderState) {
            if self.state.get() == state {
                return;
            }

            self.state.set(state);
            self.obj().notify_state();
        }
    }
}

glib::wrapper! {
    /// Subclassable camera viewfinder widget.
    ///
    /// The widget presents the output of the camera and detects QR codes.
    ///
    /// To construct this, use `Camera::viewfinder()`.
    pub struct CameraViewfinder(ObjectSubclass<imp::CameraViewfinder>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// Trait implemented by types that subclass [`CameraViewfinder`].
#[allow(dead_code)]
pub(super) trait CameraViewfinderExt: 'static {
    /// The state of this viewfinder.
    fn state(&self) -> CameraViewfinderState;

    /// Set the state of this viewfinder.
    fn set_state(&self, state: CameraViewfinderState);

    /// Connect to the signal emitted when a QR code is detected.
    fn connect_qrcode_detected<F: Fn(&Self, ScannedQrCode) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId;

    /// Emit the signal that a QR code was detected.
    fn emit_qrcode_detected(&self, data: ScannedQrCode);
}

impl<O: IsA<CameraViewfinder>> CameraViewfinderExt for O {
    fn state(&self) -> CameraViewfinderState {
        self.upcast_ref().state()
    }

    /// Set the state of this viewfinder.
    fn set_state(&self, state: CameraViewfinderState) {
        self.upcast_ref().set_state(state);
    }

    fn connect_qrcode_detected<F: Fn(&Self, ScannedQrCode) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "qrcode-detected",
            true,
            closure_local!(|obj: Self, data: ScannedQrCode| {
                f(&obj, data);
            }),
        )
    }

    fn emit_qrcode_detected(&self, data: ScannedQrCode) {
        self.emit_by_name::<()>("qrcode-detected", &[&data]);
    }
}

/// Trait that must be implemented for types that subclass `CameraViewfinder`.
///
/// Overriding a method from this Trait overrides also its behavior in
/// [`CameraViewfinderExt`].
pub(super) trait CameraViewfinderImpl: ObjectImpl {}

unsafe impl<T> IsSubclassable<T> for CameraViewfinder
where
    T: CameraViewfinderImpl + WidgetImpl,
    T::Type: IsA<CameraViewfinder>,
{
    fn class_init(class: &mut glib::Class<Self>) {
        Self::parent_class_init::<T>(class.upcast_ref_mut());
    }
}
