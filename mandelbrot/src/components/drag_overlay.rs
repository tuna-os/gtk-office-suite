use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};

use crate::utils::BoundObject;

mod imp {
    use std::marker::PhantomData;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::DragOverlay)]
    pub struct DragOverlay {
        overlay: gtk::Overlay,
        revealer: gtk::Revealer,
        status: adw::StatusPage,
        /// The title of this `DragOverlay`.
        #[property(get = Self::title, set = Self::set_title)]
        title: PhantomData<glib::GString>,
        /// The child of this `DragOverlay`.
        #[property(get = Self::child, set = Self::set_child, nullable)]
        child: PhantomData<Option<gtk::Widget>>,
        /// The [`gtk::DropTarget`] of this `DragOverlay`.
        #[property(get, set = Self::set_drop_target)]
        drop_target: BoundObject<gtk::DropTarget>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DragOverlay {
        const NAME: &'static str = "DragOverlay";
        type Type = super::DragOverlay;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("dragoverlay");
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for DragOverlay {
        fn constructed(&self) {
            let obj = self.obj();

            self.overlay.set_parent(&*obj);
            self.overlay.add_overlay(&self.revealer);

            self.revealer.set_can_target(false);
            self.revealer
                .set_transition_type(gtk::RevealerTransitionType::Crossfade);
            self.revealer.set_reveal_child(false);
            self.revealer.set_visible(false);

            self.status.set_icon_name(Some("attachment-symbolic"));

            self.revealer.set_child(Some(&self.status));

            self.revealer.connect_child_revealed_notify(|revealer| {
                // Hide the revealer when we don't want to show the child and the animation is
                // finished.
                if !revealer.reveals_child() && !revealer.is_child_revealed() {
                    revealer.set_visible(false);
                }
            });
        }

        fn dispose(&self) {
            self.overlay.unparent();
        }
    }

    impl WidgetImpl for DragOverlay {}

    impl DragOverlay {
        /// The title of this `DragOverlay`.
        fn title(&self) -> glib::GString {
            self.status.title()
        }

        /// Set the title of this `DragOverlay`.
        fn set_title(&self, title: &str) {
            self.status.set_title(title);
            self.obj()
                .update_property(&[gtk::accessible::Property::Label(title)]);
        }

        /// The child of this `DragOverlay`.
        fn child(&self) -> Option<gtk::Widget> {
            self.overlay.child()
        }

        /// Set the child of this `DragOverlay`.
        fn set_child(&self, child: Option<&gtk::Widget>) {
            self.overlay.set_child(child);
        }

        /// Set the [`gtk::DropTarget`] of this `DragOverlay`.
        fn set_drop_target(&self, drop_target: gtk::DropTarget) {
            let obj = self.obj();

            if let Some(target) = self.drop_target.obj() {
                obj.remove_controller(&target);
            }
            self.drop_target.disconnect_signals();

            let handler_id = drop_target.connect_current_drop_notify(clone!(
                #[weak(rename_to = revealer)]
                self.revealer,
                move |target| {
                    let reveal = target.current_drop().is_some();

                    if reveal {
                        revealer.set_visible(true);
                    }

                    revealer.set_reveal_child(reveal);
                }
            ));

            obj.add_controller(drop_target.clone());
            self.drop_target.set(drop_target, vec![handler_id]);
            obj.notify_drop_target();
        }
    }
}

glib::wrapper! {
    /// A widget displaying an overlay when something is dragged onto it.
    pub struct DragOverlay(ObjectSubclass<imp::DragOverlay>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl DragOverlay {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for DragOverlay {
    fn default() -> Self {
        Self::new()
    }
}
