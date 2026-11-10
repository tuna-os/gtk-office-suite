use adw::prelude::*;
use gtk::{glib, subclass::prelude::*};

use crate::utils::ChildPropertyExt;

mod imp {
    use std::marker::PhantomData;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/loading/bin.ui")]
    #[properties(wrapper_type = super::LoadingBin)]
    pub struct LoadingBin {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        child_bin: TemplateChild<adw::Bin>,
        /// The child widget.
        #[property(get = Self::child, set = Self::set_child, explicit_notify, nullable)]
        child: PhantomData<Option<gtk::Widget>>,
        /// Whether this is showing the spinner.
        #[property(get = Self::is_loading, set = Self::set_is_loading, explicit_notify)]
        is_loading: PhantomData<bool>,
        /// Whether this should keep the same height when showing the spinner or
        /// the content.
        #[property(get = Self::vhomogeneous, set = Self::set_vhomogeneous)]
        vhomogeneous: PhantomData<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LoadingBin {
        const NAME: &'static str = "LoadingBin";
        type Type = super::LoadingBin;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_layout_manager_type::<gtk::BinLayout>();
            klass.set_css_name("loading-bin");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for LoadingBin {
        fn dispose(&self) {
            self.stack.unparent();
        }
    }

    impl WidgetImpl for LoadingBin {}

    impl LoadingBin {
        /// Whether this row is showing the spinner.
        fn is_loading(&self) -> bool {
            self.stack.visible_child_name().as_deref() == Some("loading")
        }

        /// Set whether this row is showing the spinner.
        fn set_is_loading(&self, loading: bool) {
            if self.is_loading() == loading {
                return;
            }

            let child_name = if loading { "loading" } else { "child" };
            self.stack.set_visible_child_name(child_name);
            self.obj().notify_is_loading();
        }

        /// Whether this should keep the same height when showing the spinner or
        /// the content.
        fn vhomogeneous(&self) -> bool {
            self.stack.is_vhomogeneous()
        }

        /// Set whether this should keep the same height when showing the
        /// spinner or the content.
        fn set_vhomogeneous(&self, homogeneous: bool) {
            self.stack.set_vhomogeneous(homogeneous);
        }

        /// The child widget.
        fn child(&self) -> Option<gtk::Widget> {
            self.child_bin.child()
        }

        /// Set the child widget.
        fn set_child(&self, child: Option<&gtk::Widget>) {
            if self.child().as_ref() == child {
                return;
            }

            self.child_bin.set_child(child);
            self.obj().notify_child();
        }
    }
}

glib::wrapper! {
    /// A Bin that shows either its child or a loading spinner.
    pub struct LoadingBin(ObjectSubclass<imp::LoadingBin>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LoadingBin {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for LoadingBin {
    fn default() -> Self {
        Self::new()
    }
}

impl ChildPropertyExt for LoadingBin {
    fn child_property(&self) -> Option<gtk::Widget> {
        self.child()
    }

    fn set_child_property(&self, child: Option<&impl IsA<gtk::Widget>>) {
        self.set_child(child.map(Cast::upcast_ref));
    }
}
