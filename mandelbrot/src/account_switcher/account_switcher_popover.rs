use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};

use super::session_item::SessionItemRow;
use crate::utils::{BoundObjectWeakRef, FixedSelection};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/account_switcher/account_switcher_popover.ui")]
    #[properties(wrapper_type = super::AccountSwitcherPopover)]
    pub struct AccountSwitcherPopover {
        #[template_child]
        sessions: TemplateChild<gtk::ListBox>,
        /// The model containing the logged-in sessions selection.
        #[property(get, set = Self::set_session_selection, explicit_notify, nullable)]
        session_selection: BoundObjectWeakRef<FixedSelection>,
        /// The selected row.
        selected_row: glib::WeakRef<SessionItemRow>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AccountSwitcherPopover {
        const NAME: &'static str = "AccountSwitcherPopover";
        type Type = super::AccountSwitcherPopover;
        type ParentType = gtk::Popover;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.install_action("account-switcher.close", None, |obj, _, _| {
                obj.popdown();
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AccountSwitcherPopover {}

    impl WidgetImpl for AccountSwitcherPopover {}
    impl PopoverImpl for AccountSwitcherPopover {}

    #[gtk::template_callbacks]
    impl AccountSwitcherPopover {
        /// Set the model containing the logged-in sessions selection.
        fn set_session_selection(&self, selection: Option<&FixedSelection>) {
            if selection == self.session_selection.obj().as_ref() {
                return;
            }

            self.session_selection.disconnect_signals();

            self.sessions.bind_model(selection, |session| {
                let row = SessionItemRow::new(
                    session
                        .downcast_ref()
                        .expect("sessions list box item should be a Session"),
                );
                row.upcast()
            });

            if let Some(selection) = selection {
                let selected_handler = selection.connect_selected_item_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |selection| {
                        imp.update_selected_item(selection.selected());
                    }
                ));
                self.update_selected_item(selection.selected());

                self.session_selection
                    .set(selection, vec![selected_handler]);
            }

            self.obj().notify_session_selection();
        }

        /// Select the given row in the session list.
        #[template_callback]
        fn select_row(&self, row: &gtk::ListBoxRow) {
            self.obj().popdown();

            let Some(selection) = self.session_selection.obj() else {
                return;
            };

            let index = u32::try_from(row.index()).expect("selected row has an index");
            selection.set_selected(index);
        }

        /// Update the selected item in the session list.
        fn update_selected_item(&self, selected: u32) {
            let old_selected = self.selected_row.upgrade();
            let new_selected = if selected == gtk::INVALID_LIST_POSITION {
                None
            } else {
                let index = selected.try_into().expect("item index should fit into i32");
                self.sessions
                    .row_at_index(index)
                    .and_downcast::<SessionItemRow>()
            };

            if old_selected == new_selected {
                return;
            }

            if let Some(row) = &old_selected {
                row.set_selected(false);
            }
            if let Some(row) = &new_selected {
                row.set_selected(true);
            }

            self.selected_row.set(new_selected.as_ref());
        }
    }
}

glib::wrapper! {
    /// A popover allowing to switch between the available sessions, to open their
    /// account settings, or to log into a new account.
    pub struct AccountSwitcherPopover(ObjectSubclass<imp::AccountSwitcherPopover>)
        @extends gtk::Widget, gtk::Popover,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native, gtk::ShortcutManager;
}

impl AccountSwitcherPopover {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for AccountSwitcherPopover {
    fn default() -> Self {
        Self::new()
    }
}
