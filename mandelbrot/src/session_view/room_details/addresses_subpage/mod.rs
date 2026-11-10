use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gio, glib, glib::clone, pango};
use ruma::RoomAliasId;
use tracing::error;

mod completion_popover;
mod public_address;

use self::{completion_popover::CompletionPopover, public_address::PublicAddress};
use crate::{
    components::{EntryAddRow, LoadingButton, RemovableRow, SubstringEntryRow},
    gettext_f,
    prelude::*,
    session::{AddAltAliasError, RegisterLocalAliasError, Room},
    spawn, toast,
    utils::{PlaceholderObject, SingleItemListModel},
};

mod imp {
    use std::{
        cell::{OnceCell, RefCell},
        collections::HashSet,
    };

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/addresses_subpage/mod.ui"
    )]
    #[properties(wrapper_type = super::AddressesSubpage)]
    pub struct AddressesSubpage {
        #[template_child]
        public_addresses_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        public_addresses_error_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        public_addresses_error: TemplateChild<gtk::Label>,
        #[template_child]
        local_addresses_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        local_addresses_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        local_addresses_error_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        local_addresses_error: TemplateChild<gtk::Label>,
        #[template_child]
        public_addresses_add_row: TemplateChild<EntryAddRow>,
        #[template_child]
        local_addresses_add_row: TemplateChild<SubstringEntryRow>,
        /// The room users will be invited to.
        #[property(get, set = Self::set_room, construct_only)]
        room: glib::WeakRef<Room>,
        /// The full list of public addresses.
        public_addresses: OnceCell<gio::ListStore>,
        /// The full list of local addresses.
        local_addresses: gtk::StringList,
        aliases_changed_handler: RefCell<Option<glib::SignalHandlerId>>,
        public_addresses_completion: CompletionPopover,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AddressesSubpage {
        const NAME: &'static str = "RoomDetailsAddressesSubpage";
        type Type = super::AddressesSubpage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AddressesSubpage {
        fn constructed(&self) {
            self.parent_constructed();

            let add_item = SingleItemListModel::new(Some(&PlaceholderObject::new("add")));

            // Public addresses.
            let public_items = gio::ListStore::new::<glib::Object>();
            public_items.append(self.public_addresses());
            public_items.append(&add_item);

            let flattened_public_list = gtk::FlattenListModel::new(Some(public_items));
            self.public_addresses_list.bind_model(
                Some(&flattened_public_list),
                clone!(
                    #[weak(rename_to = imp)]
                    self,
                    #[upgrade_or_else]
                    || { adw::ActionRow::new().upcast() },
                    move |item| imp.create_public_address_row(item)
                ),
            );

            self.public_addresses_add_row.connect_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_public_addresses_add_row();
                }
            ));

            // Filter addresses already in the list.
            let new_addresses_filter = gtk::CustomFilter::new(clone!(
                #[weak(rename_to = imp)]
                self,
                #[upgrade_or]
                false,
                move |item: &glib::Object| {
                    let Some(item) = item.downcast_ref::<gtk::StringObject>() else {
                        return false;
                    };

                    let address = item.string();

                    for public_address in imp.public_addresses().iter::<PublicAddress>() {
                        let Ok(public_address) = public_address else {
                            // The iterator is broken.
                            break;
                        };

                        if public_address.alias().as_str() == address {
                            return false;
                        }
                    }

                    true
                }
            ));

            // Update the filtered list everytime an item changes.
            self.public_addresses().connect_items_changed(clone!(
                #[weak]
                new_addresses_filter,
                move |_, _, _, _| {
                    new_addresses_filter.changed(gtk::FilterChange::Different);
                }
            ));

            let new_local_addresses = gtk::FilterListModel::new(
                Some(self.local_addresses.clone()),
                Some(new_addresses_filter),
            );

            self.public_addresses_completion
                .set_model(Some(new_local_addresses));
            self.public_addresses_completion.set_entry(Some(
                self.public_addresses_add_row.upcast_ref::<gtk::Editable>(),
            ));

            // Local addresses.
            let local_items = gio::ListStore::new::<glib::Object>();
            local_items.append(&self.local_addresses);
            local_items.append(&add_item);

            let flattened_local_list = gtk::FlattenListModel::new(Some(local_items));
            self.local_addresses_list.bind_model(
                Some(&flattened_local_list),
                clone!(
                    #[weak(rename_to = imp)]
                    self,
                    #[upgrade_or_else]
                    || { adw::ActionRow::new().upcast() },
                    move |item| imp.create_local_address_row(item)
                ),
            );

            self.local_addresses_add_row.connect_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_local_addresses_add_row();
                }
            ));
        }

        fn dispose(&self) {
            if let Some(room) = self.room.upgrade()
                && let Some(handler) = self.aliases_changed_handler.take()
            {
                room.aliases().disconnect(handler);
            }

            self.public_addresses_completion.unparent();
        }
    }

    impl WidgetImpl for AddressesSubpage {}
    impl NavigationPageImpl for AddressesSubpage {}

    #[gtk::template_callbacks]
    impl AddressesSubpage {
        fn public_addresses(&self) -> &gio::ListStore {
            self.public_addresses
                .get_or_init(gio::ListStore::new::<PublicAddress>)
        }

        /// Set the room users will be invited to.
        fn set_room(&self, room: &Room) {
            let aliases = room.aliases();

            let aliases_changed_handler = aliases.connect_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_public_addresses();
                }
            ));
            self.aliases_changed_handler
                .replace(Some(aliases_changed_handler));

            self.room.set(Some(room));

            self.obj().notify_room();
            self.update_public_addresses();
            self.update_local_addresses_server();

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.update_local_addresses().await;
                }
            ));
        }

        /// Update the list of public addresses.
        fn update_public_addresses(&self) {
            let Some(room) = self.room.upgrade() else {
                return;
            };

            let aliases = room.aliases();
            let canonical_alias = aliases.canonical_alias();
            let alt_aliases = aliases.alt_aliases();

            // Map of `(alias, is_main)`.
            let mut public_aliases = canonical_alias
                .into_iter()
                .map(|a| (a, true))
                .chain(alt_aliases.into_iter().map(|a| (a, false)))
                .collect::<Vec<_>>();

            let public_addresses = self.public_addresses();

            // Remove aliases that are not in the list anymore and update the main alias.
            let mut i = 0;
            while i < public_addresses.n_items() {
                let Some(item) = public_addresses.item(i).and_downcast::<PublicAddress>() else {
                    break;
                };

                let position = public_aliases
                    .iter()
                    .position(|(alias, _)| item.alias() == alias);

                if let Some(position) = position {
                    // It is in the list, update whether it is the main alias.
                    let (_, is_main) = public_aliases.remove(position);
                    item.set_is_main(is_main);

                    i += 1;
                } else {
                    // It is not in the list, remove.
                    public_addresses.remove(i);
                }
            }

            // If there are new aliases in the list, append them.
            if !public_aliases.is_empty() {
                let new_aliases = public_aliases
                    .into_iter()
                    .map(|(alias, is_main)| PublicAddress::new(alias, is_main))
                    .collect::<Vec<_>>();
                public_addresses.splice(public_addresses.n_items(), 0, &new_aliases);
            }

            self.reset_public_addresses_state();
        }

        /// Reset the public addresses section UI state.
        fn reset_public_addresses_state(&self) {
            // Reset the list.
            self.public_addresses_list.set_sensitive(true);

            // Reset the rows loading state.
            let n_items = i32::try_from(self.public_addresses().n_items()).unwrap_or(i32::MAX);
            for i in 0..n_items {
                let Some(row) = self
                    .public_addresses_list
                    .row_at_index(i)
                    .and_downcast::<RemovableRow>()
                else {
                    break;
                };

                row.set_is_loading(false);

                if let Some(button) = row.extra_suffix().and_downcast::<LoadingButton>() {
                    button.set_is_loading(false);
                }
            }

            self.public_addresses_add_row.set_is_loading(false);
        }

        /// Update the server of the local addresses.
        fn update_local_addresses_server(&self) {
            let Some(room) = self.room.upgrade() else {
                return;
            };
            let own_member = room.own_member();
            let server_name = own_member.user_id().server_name();

            self.local_addresses_group.set_title(&gettext_f(
                // Translators: Do NOT translate the content between '{' and '}',
                // this is a variable name.
                "Local Addresses on {homeserver}",
                &[("homeserver", server_name.as_str())],
            ));
            self.local_addresses_add_row
                .set_suffix_text(format!(":{server_name}"));
        }

        /// Update the list of local addresses.
        async fn update_local_addresses(&self) {
            let Some(room) = self.room.upgrade() else {
                return;
            };

            let aliases = room.aliases();

            let Ok(local_aliases) = aliases.local_aliases().await else {
                return;
            };

            let mut local_aliases = local_aliases
                .into_iter()
                .map(String::from)
                .collect::<HashSet<_>>();

            // Remove aliases that are not in the list anymore.
            let mut i = 0;
            while i < self.local_addresses.n_items() {
                let Some(item) = self
                    .local_addresses
                    .item(i)
                    .and_downcast::<gtk::StringObject>()
                else {
                    break;
                };

                let address = String::from(item.string());

                if local_aliases.remove(&address) {
                    i += 1;
                } else {
                    self.local_addresses.remove(i);
                }
            }

            // If there are new aliases in the list, append them.
            if !local_aliases.is_empty() {
                let new_aliases = local_aliases.iter().map(String::as_str).collect::<Vec<_>>();
                self.local_addresses
                    .splice(self.local_addresses.n_items(), 0, &new_aliases);
            }
        }

        /// Create a row for the given item in the public addresses section.
        fn create_public_address_row(&self, item: &glib::Object) -> gtk::Widget {
            let Some(address) = item.downcast_ref::<PublicAddress>() else {
                // It can only be the dummy item to add a new alias.
                return self.public_addresses_add_row.clone().upcast();
            };

            let alias = address.alias();
            let row = RemovableRow::new();
            row.set_title(alias.as_str());
            row.set_remove_button_tooltip_text(Some(gettext("Remove address")));
            row.set_remove_button_accessible_label(Some(gettext_f(
                // Translators: Do NOT translate the content between '{' and '}',
                // this is a variable name.
                "Remove “{address}”",
                &[("address", alias.as_str())],
            )));

            address.connect_is_main_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                #[weak]
                row,
                move |address| {
                    imp.update_public_row_is_main(&row, address.is_main());
                }
            ));
            self.update_public_row_is_main(&row, address.is_main());

            row.connect_remove(clone!(
                #[weak(rename_to = imp)]
                self,
                move |row| {
                    spawn!(clone!(
                        #[weak]
                        row,
                        async move {
                            imp.remove_public_address(&row).await;
                        }
                    ));
                }
            ));

            row.upcast()
        }

        /// Update the given row for whether the address it presents is the main
        /// address or not.
        fn update_public_row_is_main(&self, row: &RemovableRow, is_main: bool) {
            if is_main && !public_row_is_main(row) {
                let label = gtk::Label::builder()
                    .label(gettext("Main Address"))
                    .ellipsize(pango::EllipsizeMode::End)
                    .build();
                let image = gtk::Image::builder()
                    .icon_name("checkmark-symbolic")
                    .accessible_role(gtk::AccessibleRole::Presentation)
                    .build();
                let main_box = gtk::Box::builder()
                    .spacing(6)
                    .css_classes(["public-address-tag"])
                    .valign(gtk::Align::Center)
                    .build();

                main_box.append(&image);
                main_box.append(&label);

                row.update_relation(&[gtk::accessible::Relation::DescribedBy(&[
                    label.upcast_ref()
                ])]);
                row.set_extra_suffix(Some(main_box));
            } else if !is_main && !row.extra_suffix().is_some_and(|w| w.is::<LoadingButton>()) {
                let button = LoadingButton::new();
                button.set_content_icon_name("checkmark-symbolic");
                button.add_css_class("flat");
                button.set_tooltip_text(Some(&gettext("Set as main address")));
                button.set_valign(gtk::Align::Center);

                let accessible_label = gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}',
                    // this is a variable name.
                    "Set “{address}” as main address",
                    &[("address", &row.title())],
                );
                button.update_property(&[gtk::accessible::Property::Label(&accessible_label)]);

                button.connect_clicked(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    #[weak]
                    row,
                    move |_| {
                        spawn!(async move {
                            imp.set_main_public_address(&row).await;
                        });
                    }
                ));

                row.set_extra_suffix(Some(button));
            }
        }

        /// Remove the public address from the given row.
        async fn remove_public_address(&self, row: &RemovableRow) {
            let Some(room) = self.room.upgrade() else {
                return;
            };
            let Ok(alias) = RoomAliasId::parse(row.title()) else {
                error!("Cannot remove address with invalid alias");
                return;
            };

            let aliases = room.aliases();

            self.public_addresses_list.set_sensitive(false);
            row.set_is_loading(true);

            let result = if public_row_is_main(row) {
                aliases.remove_canonical_alias(&alias).await
            } else {
                aliases.remove_alt_alias(&alias).await
            };

            if result.is_err() {
                toast!(self.obj(), gettext("Could not remove public address"));
                self.public_addresses_list.set_sensitive(true);
                row.set_is_loading(false);
            }
        }

        /// Set the address from the given row as the main public address.
        async fn set_main_public_address(&self, row: &RemovableRow) {
            let Some(room) = self.room.upgrade() else {
                return;
            };
            let Some(button) = row.extra_suffix().and_downcast::<LoadingButton>() else {
                return;
            };
            let Ok(alias) = RoomAliasId::parse(row.title()) else {
                error!("Cannot set main public address with invalid alias");
                return;
            };

            let aliases = room.aliases();

            self.public_addresses_list.set_sensitive(false);
            button.set_is_loading(true);

            if aliases.set_canonical_alias(alias).await.is_err() {
                toast!(self.obj(), gettext("Could not set main public address"));
                self.public_addresses_list.set_sensitive(true);
                button.set_is_loading(false);
            }
        }

        /// Update the public addresses add row for the current state.
        fn update_public_addresses_add_row(&self) {
            self.public_addresses_add_row
                .set_inhibit_add(!self.can_add_public_address());
        }

        /// Activate the auto-completion of the public addresses add row.
        #[template_callback]
        async fn handle_public_addresses_add_row_activated(&self) {
            if !self.public_addresses_completion.activate_selected_row() {
                self.add_public_address().await;
            }
        }

        /// Add a an address to the public list.
        #[template_callback]
        async fn add_public_address(&self) {
            if !self.can_add_public_address() {
                return;
            }

            let Some(room) = self.room.upgrade() else {
                return;
            };

            let row = &self.public_addresses_add_row;

            let Ok(alias) = RoomAliasId::parse(row.text()) else {
                error!("Cannot add public address with invalid alias");
                return;
            };

            self.public_addresses_list.set_sensitive(false);
            row.set_is_loading(true);
            self.public_addresses_error_revealer.set_reveal_child(false);

            let aliases = room.aliases();
            match aliases.add_alt_alias(alias).await {
                Ok(()) => {
                    row.set_text("");
                }
                Err(error) => {
                    toast!(self.obj(), gettext("Could not add public address"));

                    let label = match error {
                        AddAltAliasError::NotRegistered => {
                            Some(gettext("This address is not registered as a local address"))
                        }
                        AddAltAliasError::InvalidRoomId => {
                            Some(gettext("This address does not belong to this room"))
                        }
                        AddAltAliasError::Other => None,
                    };

                    if let Some(label) = label {
                        self.public_addresses_error.set_label(&label);
                        self.public_addresses_error_revealer.set_reveal_child(true);
                    }

                    self.public_addresses_list.set_sensitive(true);
                    row.set_is_loading(false);
                }
            }
        }

        /// Whether the user can add the current address to the public list.
        fn can_add_public_address(&self) -> bool {
            let new_address = self.public_addresses_add_row.text();

            // Cannot add an empty address.
            if new_address.is_empty() {
                return false;
            }

            // Cannot add an invalid alias.
            let Ok(new_alias) = RoomAliasId::parse(new_address) else {
                return false;
            };

            // Cannot add a duplicate address.
            for public_address in self.public_addresses().iter::<PublicAddress>() {
                let Ok(public_address) = public_address else {
                    // The iterator is broken.
                    return false;
                };

                if *public_address.alias() == new_alias {
                    return false;
                }
            }

            true
        }

        /// Create a row for the given item in the public addresses section.
        fn create_local_address_row(&self, item: &glib::Object) -> gtk::Widget {
            let Some(string_obj) = item.downcast_ref::<gtk::StringObject>() else {
                // It can only be the dummy item to add a new alias.
                return self.local_addresses_add_row.clone().upcast();
            };

            let alias = string_obj.string();
            let row = RemovableRow::new();
            row.set_title(&alias);
            row.set_remove_button_tooltip_text(Some(gettext("Unregister local address")));
            row.set_remove_button_accessible_label(Some(gettext_f(
                // Translators: Do NOT translate the content between '{' and '}',
                // this is a variable name.
                "Unregister “{address}”",
                &[("address", &alias)],
            )));

            row.connect_remove(clone!(
                #[weak(rename_to = imp)]
                self,
                move |row| {
                    spawn!(clone!(
                        #[weak]
                        row,
                        async move {
                            imp.unregister_local_address(&row).await;
                        }
                    ));
                }
            ));

            row.upcast()
        }

        /// Unregister the local address from the given row.
        async fn unregister_local_address(&self, row: &RemovableRow) {
            let Some(room) = self.room.upgrade() else {
                return;
            };
            let Ok(alias) = RoomAliasId::parse(row.title()) else {
                error!("Cannot unregister local address with invalid alias");
                return;
            };

            let aliases = room.aliases();

            row.set_is_loading(true);

            if aliases.unregister_local_alias(alias).await.is_err() {
                toast!(self.obj(), gettext("Could not unregister local address"));
            }

            self.update_local_addresses().await;

            row.set_is_loading(false);
        }

        /// The full new address in the public addresses add row.
        ///
        /// Returns `None` if the localpart is empty.
        fn new_local_address(&self) -> Option<String> {
            let row = &self.local_addresses_add_row;
            let localpart = row.text();

            if localpart.is_empty() {
                return None;
            }

            let server_name = row.suffix_text();
            Some(format!("#{localpart}{server_name}"))
        }

        /// Update the public addresses add row for the current state.
        fn update_local_addresses_add_row(&self) {
            let row = &self.local_addresses_add_row;

            row.set_inhibit_add(!self.can_register_local_address());

            let accessible_label = self.new_local_address().map(|address| {
                gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}',
                    // this is a variable name.
                    "Register “{address}”",
                    &[("address", &address)],
                )
            });
            row.set_add_button_accessible_label(accessible_label);
        }

        /// Register a local address.
        #[template_callback]
        async fn register_local_address(&self) {
            if !self.can_register_local_address() {
                return;
            }

            let Some(room) = self.room.upgrade() else {
                return;
            };

            let Some(new_address) = self.new_local_address() else {
                return;
            };
            let Ok(alias) = RoomAliasId::parse(new_address) else {
                error!("Cannot register local address with invalid alias");
                return;
            };

            let row = &self.local_addresses_add_row;
            row.set_is_loading(true);
            self.local_addresses_error_revealer.set_reveal_child(false);

            let aliases = room.aliases();

            match aliases.register_local_alias(alias).await {
                Ok(()) => {
                    row.set_text("");
                }
                Err(error) => {
                    toast!(self.obj(), gettext("Could not register local address"));

                    if let RegisterLocalAliasError::AlreadyInUse = error {
                        self.local_addresses_error
                            .set_label(&gettext("This address is already registered"));
                        self.local_addresses_error_revealer.set_reveal_child(true);
                    }
                }
            }

            self.update_local_addresses().await;

            row.set_is_loading(false);
        }

        /// Whether the user can add the current address to the local list.
        fn can_register_local_address(&self) -> bool {
            // Cannot add an empty address.
            let Some(new_address) = self.new_local_address() else {
                return false;
            };

            // Cannot add an invalid alias.
            let Ok(new_alias) = RoomAliasId::parse(new_address) else {
                return false;
            };

            // Cannot add a duplicate address.
            for local_address in self.public_addresses().iter::<glib::Object>() {
                let Some(local_address) = local_address.ok().and_downcast::<gtk::StringObject>()
                else {
                    // The iterator is broken.
                    return true;
                };

                if local_address.string() == new_alias.as_str() {
                    return false;
                }
            }

            true
        }
    }
}

glib::wrapper! {
    /// Subpage to manage the public addresses of a room.
    pub struct AddressesSubpage(ObjectSubclass<imp::AddressesSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AddressesSubpage {
    pub fn new(room: &Room) -> Self {
        glib::Object::builder().property("room", room).build()
    }
}

/// Whether the given public row contains the main address.
fn public_row_is_main(row: &RemovableRow) -> bool {
    row.extra_suffix().is_some_and(|w| w.is::<gtk::Box>())
}
