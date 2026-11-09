use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

mod imp {
    use std::marker::PhantomData;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/dialogs/toastable.ui")]
    #[properties(wrapper_type = super::ToastableDialog)]
    pub struct ToastableDialog {
        #[template_child]
        toast_overlay: TemplateChild<adw::ToastOverlay>,
        /// The child widget containing the content of this dialog.
        #[property(get = Self::child_content, set = Self::set_child_content, nullable)]
        child_content: PhantomData<Option<gtk::Widget>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ToastableDialog {
        const NAME: &'static str = "ToastableDialog";
        const ABSTRACT: bool = true;
        type Type = super::ToastableDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for ToastableDialog {}

    impl WidgetImpl for ToastableDialog {}
    impl AdwDialogImpl for ToastableDialog {}

    impl ToastableDialog {
        /// The child widget containing the content of this dialog.
        fn child_content(&self) -> Option<gtk::Widget> {
            self.toast_overlay.child()
        }

        /// Set the child widget containing the content of this dialog.
        fn set_child_content(&self, content: Option<&gtk::Widget>) {
            self.toast_overlay.set_child(content);
        }

        /// Present the given toast in this dialog.
        pub(super) fn add_toast(&self, toast: adw::Toast) {
            self.toast_overlay.add_toast(toast);
        }
    }
}

glib::wrapper! {
    /// A dialog that can display toasts.
    pub struct ToastableDialog(ObjectSubclass<imp::ToastableDialog>)
        @extends gtk::Widget, adw::Dialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

pub trait ToastableDialogExt: 'static {
    /// Get the content of this dialog.
    #[allow(dead_code)]
    fn child_content(&self) -> Option<gtk::Widget>;

    /// Set the content of this dialog.
    ///
    /// Use this instead of `set_child` or `set_content`, otherwise it will
    /// panic.
    #[allow(dead_code)]
    fn set_child_content(&self, content: Option<&gtk::Widget>);

    /// Add a toast.
    fn add_toast(&self, toast: adw::Toast);
}

impl<O: IsA<ToastableDialog>> ToastableDialogExt for O {
    fn child_content(&self) -> Option<gtk::Widget> {
        self.upcast_ref().child_content()
    }

    fn set_child_content(&self, content: Option<&gtk::Widget>) {
        self.upcast_ref().set_child_content(content);
    }

    fn add_toast(&self, toast: adw::Toast) {
        self.upcast_ref().imp().add_toast(toast);
    }
}

/// Public trait that must be implemented for everything that derives from
/// `ToastableDialog`.
pub trait ToastableDialogImpl: AdwDialogImpl {}

unsafe impl<T> IsSubclassable<T> for ToastableDialog where T: ToastableDialogImpl {}
