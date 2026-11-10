use gtk::{gdk, glib, glib::clone, prelude::*, subclass::prelude::*};

use crate::utils::{BoundObject, key_bindings};

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[repr(C)]
    pub struct ContextMenuBinClass {
        parent_class: glib::object::Class<gtk::Widget>,
        pub(super) menu_opened: fn(&super::ContextMenuBin),
    }

    unsafe impl ClassStruct for ContextMenuBinClass {
        type Type = ContextMenuBin;
    }

    // Hand-written class structs do not get the `Deref` that
    // `glib::subclass::basic::ClassStruct` provides, and since gtk4-rs 0.11.4
    // `add_binding_action()` lives on `WidgetClassManualExt`, which is
    // implemented for `glib::Class<W>` — reached through exactly that `Deref`.
    // Without this, `key_bindings::add_context_menu_bindings()` cannot be
    // called with this class.
    impl std::ops::Deref for ContextMenuBinClass {
        type Target = glib::object::Class<gtk::Widget>;

        fn deref(&self) -> &Self::Target {
            &self.parent_class
        }
    }

    pub(super) fn context_menu_bin_menu_opened(this: &super::ContextMenuBin) {
        let klass = this.class();
        (klass.as_ref().menu_opened)(this);
    }

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/context_menu_bin.ui")]
    #[properties(wrapper_type = super::ContextMenuBin)]
    pub struct ContextMenuBin {
        #[template_child]
        click_gesture: TemplateChild<gtk::GestureClick>,
        #[template_child]
        long_press_gesture: TemplateChild<gtk::GestureLongPress>,
        /// Whether this widget has a context menu.
        ///
        /// If this is set to `false`, all the actions will be disabled.
        #[property(get, set = Self::set_has_context_menu, explicit_notify)]
        has_context_menu: Cell<bool>,
        /// The popover displaying the context menu.
        #[property(get, set = Self::set_popover, explicit_notify, nullable)]
        popover: BoundObject<gtk::PopoverMenu>,
        /// The child widget.
        #[property(get, set = Self::set_child, explicit_notify, nullable)]
        child: RefCell<Option<gtk::Widget>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ContextMenuBin {
        const NAME: &'static str = "ContextMenuBin";
        const ABSTRACT: bool = true;
        type Type = super::ContextMenuBin;
        type ParentType = gtk::Widget;
        type Class = ContextMenuBinClass;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_layout_manager_type::<gtk::BinLayout>();

            klass.install_action("context-menu.activate", None, |obj, _, _| {
                obj.open_menu_at(0, 0);
            });
            key_bindings::add_context_menu_bindings(klass, "context-menu.activate");

            klass.install_action("context-menu.close", None, |obj, _, _| {
                if let Some(popover) = obj.popover() {
                    popover.popdown();
                }
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for ContextMenuBin {
        fn constructed(&self) {
            let obj = self.obj();

            self.long_press_gesture.connect_pressed(clone!(
                #[weak]
                obj,
                move |gesture, x, y| {
                    if obj.has_context_menu() {
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                        gesture.reset();
                        obj.open_menu_at(x as i32, y as i32);
                    }
                }
            ));

            self.click_gesture.connect_released(clone!(
                #[weak]
                obj,
                move |gesture, n_press, x, y| {
                    if n_press > 1 {
                        return;
                    }

                    if obj.has_context_menu() {
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                        obj.open_menu_at(x as i32, y as i32);
                    }
                }
            ));
            self.parent_constructed();
        }

        fn dispose(&self) {
            if let Some(popover) = self.popover.obj() {
                popover.unparent();
            }

            if let Some(child) = self.child.take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for ContextMenuBin {}

    impl ContextMenuBin {
        /// Set whether this widget has a context menu.
        fn set_has_context_menu(&self, has_context_menu: bool) {
            if self.has_context_menu.get() == has_context_menu {
                return;
            }

            self.has_context_menu.set(has_context_menu);

            let obj = self.obj();
            obj.update_property(&[gtk::accessible::Property::HasPopup(has_context_menu)]);
            obj.action_set_enabled("context-menu.activate", has_context_menu);
            obj.action_set_enabled("context-menu.close", has_context_menu);

            obj.notify_has_context_menu();
        }

        /// Set the popover displaying the context menu.
        fn set_popover(&self, popover: Option<gtk::PopoverMenu>) {
            let prev_popover = self.popover.obj();

            if prev_popover == popover {
                return;
            }
            let obj = self.obj();

            if let Some(popover) = prev_popover
                && popover.parent().is_some_and(|w| w == *obj)
            {
                popover.unparent();
            }
            self.popover.disconnect_signals();

            if let Some(popover) = popover {
                popover.unparent();
                popover.set_parent(&*obj);

                let parent_handler = popover.connect_parent_notify(clone!(
                    #[weak]
                    obj,
                    move |popover| {
                        if popover.parent().is_none_or(|w| w != obj) {
                            obj.imp().popover.disconnect_signals();
                        }
                    }
                ));

                self.popover.set(popover, vec![parent_handler]);
            }

            obj.notify_popover();
        }

        /// The child widget.
        fn child(&self) -> Option<gtk::Widget> {
            self.child.borrow().clone()
        }

        /// Set the child widget.
        fn set_child(&self, child: Option<gtk::Widget>) {
            if self.child() == child {
                return;
            }

            if let Some(child) = &child {
                child.set_parent(&*self.obj());
            }

            if let Some(old_child) = self.child.replace(child) {
                old_child.unparent();
            }

            self.obj().notify_child();
        }
    }
}

glib::wrapper! {
    /// A Bin widget that can have a context menu.
    pub struct ContextMenuBin(ObjectSubclass<imp::ContextMenuBin>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ContextMenuBin {
    fn open_menu_at(&self, x: i32, y: i32) {
        if !self.has_context_menu() {
            return;
        }

        self.menu_opened();

        if let Some(popover) = self.popover() {
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x, y, 0, 0)));
            popover.popup();
        }
    }
}

pub trait ContextMenuBinExt: 'static {
    /// Whether this widget has a context menu.
    #[allow(dead_code)]
    fn has_context_menu(&self) -> bool;

    /// Set whether this widget has a context menu.
    fn set_has_context_menu(&self, has_context_menu: bool);

    /// Get the `PopoverMenu` used in the context menu.
    #[allow(dead_code)]
    fn popover(&self) -> Option<gtk::PopoverMenu>;

    /// Set the `PopoverMenu` used in the context menu.
    fn set_popover(&self, popover: Option<gtk::PopoverMenu>);

    /// Get the child widget.
    #[allow(dead_code)]
    fn child(&self) -> Option<gtk::Widget>;

    /// Set the child widget.
    fn set_child(&self, child: Option<&impl IsA<gtk::Widget>>);

    /// Called when the menu was requested to open but before the menu is shown.
    fn menu_opened(&self);
}

impl<O: IsA<ContextMenuBin>> ContextMenuBinExt for O {
    fn has_context_menu(&self) -> bool {
        self.upcast_ref().has_context_menu()
    }

    fn set_has_context_menu(&self, has_context_menu: bool) {
        self.upcast_ref().set_has_context_menu(has_context_menu);
    }

    fn popover(&self) -> Option<gtk::PopoverMenu> {
        self.upcast_ref().popover()
    }

    fn set_popover(&self, popover: Option<gtk::PopoverMenu>) {
        self.upcast_ref().set_popover(popover);
    }

    fn child(&self) -> Option<gtk::Widget> {
        self.upcast_ref().child()
    }

    fn set_child(&self, child: Option<&impl IsA<gtk::Widget>>) {
        self.upcast_ref()
            .set_child(child.map(|w| w.clone().upcast()));
    }

    fn menu_opened(&self) {
        imp::context_menu_bin_menu_opened(self.upcast_ref());
    }
}

/// Public trait that must be implemented for everything that derives from
/// `ContextMenuBin`.
///
/// Overriding a method from this Trait overrides also its behavior in
/// `ContextMenuBinExt`.
pub trait ContextMenuBinImpl: WidgetImpl {
    /// Called when the menu was requested to open but before the menu is shown.
    ///
    /// This method should be used to set the popover dynamically.
    fn menu_opened(&self) {}
}

unsafe impl<T> IsSubclassable<T> for ContextMenuBin
where
    T: ContextMenuBinImpl,
    T::Type: IsA<ContextMenuBin>,
{
    fn class_init(class: &mut glib::Class<Self>) {
        Self::parent_class_init::<T>(class.upcast_ref_mut());

        let klass = class.as_mut();

        klass.menu_opened = menu_opened_trampoline::<T>;
    }
}

// Virtual method implementation trampolines.
fn menu_opened_trampoline<T>(this: &ContextMenuBin)
where
    T: ObjectSubclass + ContextMenuBinImpl,
    T::Type: IsA<ContextMenuBin>,
{
    let this = this.downcast_ref::<T::Type>().unwrap();
    this.imp().menu_opened();
}
