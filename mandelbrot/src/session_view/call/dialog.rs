//! Dialog presenting the prescreen and the ongoing call of a room.

use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use super::{
    CallPrescreen, CallView,
    state::{CallConnectionState, CallState},
};

mod imp {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::CallDialog)]
    pub struct CallDialog {
        pub(super) stack: gtk::Stack,
        pub(super) header_bar: adw::HeaderBar,
        pub(super) prescreen: CallPrescreen,
        pub(super) call_view: CallView,
        /// The call state driving this dialog.
        #[property(get, construct_only)]
        pub(super) state: RefCell<Option<CallState>>,
        pub(super) state_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CallDialog {
        const NAME: &'static str = "CallDialog";
        type Type = super::CallDialog;
        type ParentType = adw::Dialog;
    }

    #[glib::derived_properties]
    impl ObjectImpl for CallDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            obj.set_content_width(1000);
            obj.set_content_height(700);
            obj.set_follows_content_size(false);
            obj.set_title(&gettext("Call"));

            let close_button = gtk::Button::builder()
                .icon_name("close-symbolic")
                .tooltip_text(gettext("Close"))
                .css_classes(["flat"])
                .build();
            close_button.connect_clicked(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.close();
                }
            ));
            self.header_bar.pack_end(&close_button);

            self.stack
                .set_transition_type(gtk::StackTransitionType::Crossfade);
            self.stack.add_named(&self.prescreen, Some("prescreen"));
            self.stack.add_named(&self.call_view, Some("call"));

            let toolbar_view = adw::ToolbarView::new();
            toolbar_view.add_top_bar(&self.header_bar);
            toolbar_view.set_content(Some(&self.stack));
            obj.set_child(Some(&toolbar_view));

            let state = self.state.borrow().clone();
            self.prescreen.set_state(state.clone());
            self.call_view.set_state(state.clone());

            // Collapsing the call view keeps the call running: the call bar
            // in the room history is the way back.
            self.call_view.connect_pip_requested(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.close();
                }
            ));

            if let Some(state) = state {
                let mut handlers = Vec::new();
                handlers.push(state.connect_connection_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_page();
                    }
                )));
                handlers.push(state.connect_ended(clone!(
                    #[weak]
                    obj,
                    move |_| {
                        obj.close();
                    }
                )));
                self.state_handlers.replace(handlers);
            }

            self.update_page();
        }

        fn dispose(&self) {
            if let Some(state) = self.state.borrow().as_ref() {
                for handler in self.state_handlers.take() {
                    state.disconnect(handler);
                }
            }
        }
    }

    impl WidgetImpl for CallDialog {}

    impl AdwDialogImpl for CallDialog {
        fn closed(&self) {
            self.parent_closed();

            // Dismissing the prescreen without joining must not leave the
            // camera open. When the call is ongoing, the dialog is only a
            // window onto it and closing it keeps the camera as it is.
            if let Some(state) = self.state.borrow().as_ref()
                && state.connection_state() == CallConnectionState::Disconnected
            {
                state.set_camera_on(false);
            }
        }
    }

    impl CallDialog {
        /// Show the page matching the connection state.
        pub(super) fn update_page(&self) {
            let disconnected =
                self.state.borrow().as_ref().is_none_or(|state| {
                    state.connection_state() == CallConnectionState::Disconnected
                });

            let page = if disconnected { "prescreen" } else { "call" };
            self.stack.set_visible_child_name(page);
        }
    }
}

glib::wrapper! {
    /// A dialog presenting the prescreen and the ongoing call of a room.
    pub struct CallDialog(ObjectSubclass<imp::CallDialog>)
        @extends gtk::Widget, adw::Dialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CallDialog {
    /// Create a new dialog for the given call state.
    pub fn new(state: &CallState) -> Self {
        glib::Object::builder().property("state", state).build()
    }

    /// Connect to the signal emitted when the user wants to join the call
    /// from the prescreen.
    pub fn connect_join<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.imp().prescreen.connect_join(clone!(
            #[weak(rename_to = obj)]
            self,
            #[upgrade_or_default]
            move |_| {
                f(&obj);
            }
        ))
    }
}
