use adw::{prelude::*, subclass::prelude::*};
use gtk::{gio, glib, glib::clone};

use super::StateGroupItemRow;
use crate::{
    ngettext_f,
    prelude::*,
    session::{Event, Room},
    session_view::room_history::ReadReceiptsList,
    utils::{BoundObject, GroupingListGroup, key_bindings},
};

mod imp {
    use std::{
        cell::{Cell, OnceCell},
        marker::PhantomData,
    };

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/state/group_row.ui")]
    #[properties(wrapper_type = super::StateGroupRow)]
    pub struct StateGroupRow {
        #[template_child]
        label: TemplateChild<gtk::Label>,
        #[template_child]
        list_box: TemplateChild<gtk::ListBox>,
        /// The group displayed by this widget.
        #[property(get, set = Self::set_group, explicit_notify, nullable)]
        group: BoundObject<GroupingListGroup>,
        /// The room containing the events of the current group.
        #[property(get = Self::room)]
        room: PhantomData<Option<Room>>,
        /// The list model containing the read receipts lists of the children.
        read_receipts_lists: OnceCell<gio::ListStore>,
        /// The list model containing all the read receiptsof the children.
        #[property(get = Self::read_receipts_list_model_owned)]
        read_receipts_list_model: OnceCell<gtk::FlattenListModel>,
        /// Whether this group contains read receipts.
        #[property(get = Self::has_read_receipts)]
        has_read_receipts: PhantomData<bool>,
        /// Whether this group is expanded.
        #[property(get, set = Self::set_is_expanded, construct)]
        is_expanded: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StateGroupRow {
        const NAME: &'static str = "ContentStateGroupRow";
        type Type = super::StateGroupRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            ReadReceiptsList::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("state-group-row");
            klass.set_accessible_role(gtk::AccessibleRole::ListItem);

            klass.install_action("state-group-row.toggle-expanded", None, |obj, _, _| {
                obj.imp().toggle_expanded();
            });
            key_bindings::add_activate_bindings(klass, "state-group-row.toggle-expanded");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for StateGroupRow {}

    impl WidgetImpl for StateGroupRow {}
    impl BinImpl for StateGroupRow {}

    #[gtk::template_callbacks]
    impl StateGroupRow {
        /// Set the group presented by this row.
        fn set_group(&self, group: Option<GroupingListGroup>) {
            let prev_group = self.group.obj();

            if prev_group == group {
                return;
            }

            self.group.disconnect_signals();

            let removed = prev_group.map(|group| group.n_items()).unwrap_or_default();
            let added = group
                .as_ref()
                .map(GroupingListGroup::n_items)
                .unwrap_or_default();

            if let Some(group) = group {
                let items_changed_handler = group.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, position, removed, added| {
                        imp.items_changed(position, removed, added);
                    }
                ));

                self.list_box.bind_model(Some(&group), |item| {
                    let event = item
                        .downcast_ref::<Event>()
                        .expect("group item should be an event");

                    StateGroupItemRow::new(event).upcast()
                });

                self.group.set(group, vec![items_changed_handler]);
            }

            self.items_changed(0, removed, added);

            let obj = self.obj();
            obj.notify_group();
            obj.notify_room();
        }

        /// The room containing the events of this group.
        fn room(&self) -> Option<Room> {
            // Get the room of the first event, since they are all in the same room.
            self.group
                .obj()
                .and_then(|group| group.item(0))
                .and_downcast::<Event>()
                .map(|event| event.room())
        }

        /// The list model containing the read receipts lists of the children.
        fn read_receipts_lists(&self) -> &gio::ListStore {
            self.read_receipts_lists
                .get_or_init(gio::ListStore::new::<gio::ListStore>)
        }

        /// The list model containing all the read receipts of the children.
        fn read_receipts_list_model(&self) -> &gtk::FlattenListModel {
            self.read_receipts_list_model.get_or_init(|| {
                gtk::FlattenListModel::new(Some(self.read_receipts_lists().clone()))
            })
        }
        /// The owned list model containing all the read receipts of the
        /// children.
        fn read_receipts_list_model_owned(&self) -> gtk::FlattenListModel {
            self.read_receipts_list_model().clone()
        }

        /// Whether this group contains read receipts.
        fn has_read_receipts(&self) -> bool {
            self.read_receipts_list_model().n_items() > 0
        }

        /// Set whether this row is expanded.
        fn set_is_expanded(&self, is_expanded: bool) {
            let obj = self.obj();

            if is_expanded {
                obj.set_state_flags(gtk::StateFlags::CHECKED, false);
            } else {
                obj.unset_state_flags(gtk::StateFlags::CHECKED);
            }

            self.is_expanded.set(is_expanded);

            obj.notify_is_expanded();
            obj.update_state(&[gtk::accessible::State::Expanded(Some(is_expanded))]);
        }

        /// Toggle whether this group is expanded.
        #[template_callback]
        fn toggle_expanded(&self) {
            self.set_is_expanded(!self.is_expanded.get());
        }

        /// Handle when items changed in the underlying group.
        fn items_changed(&self, position: u32, removed: u32, added: u32) {
            let Some(group) = self.group.obj() else {
                self.read_receipts_lists().remove_all();
                self.update_label();
                self.obj().notify_has_read_receipts();
                return;
            };

            let had_read_receipts = self.has_read_receipts();

            let added_read_receipts = (position..position + added)
                .map(|pos| {
                    group
                        .item(pos)
                        .and_downcast::<Event>()
                        .expect("state group item should be an event")
                        .read_receipts()
                })
                .collect::<Vec<_>>();
            self.read_receipts_lists()
                .splice(position, removed, &added_read_receipts);

            self.update_label();

            if had_read_receipts != self.has_read_receipts() {
                self.obj().notify_has_read_receipts();
            }
        }

        /// Update the label of this row for the current state.
        fn update_label(&self) {
            let n = self
                .group
                .obj()
                .map(|group| group.n_items())
                .unwrap_or_default();

            let label = ngettext_f(
                // Translators: This is a change in the room, not a change between
                // rooms. Do NOT translate the content between '{' and '}', this
                // is a variable name.
                "1 room change",
                "{n} room changes",
                n,
                &[("n", &n.to_string())],
            );
            self.label.set_label(&label);
        }
    }
}

glib::wrapper! {
    /// A row presenting a group of state events.
    pub struct StateGroupRow(ObjectSubclass<imp::StateGroupRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl StateGroupRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for StateGroupRow {
    fn default() -> Self {
        Self::new()
    }
}
