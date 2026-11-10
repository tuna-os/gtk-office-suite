use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};

use crate::utils::BoundObject;

/// A function that returns `true` if two `GObject`s are considered equivalent.
pub(crate) type EquivalentObjectFn = dyn Fn(&glib::Object, &glib::Object) -> bool;

mod imp {
    use std::{
        cell::{Cell, RefCell},
        fmt,
        marker::PhantomData,
    };

    use super::*;

    #[derive(glib::Properties)]
    #[properties(wrapper_type = super::FixedSelection)]
    pub struct FixedSelection {
        /// The underlying model.
        #[property(get, set = Self::set_model, explicit_notify, nullable)]
        model: BoundObject<gio::ListModel>,
        /// The function to use to test for equivalence of two items.
        ///
        /// It is used when checking if an object still present when the
        /// underlying model changes. Which means that if there are two
        /// equivalent objects at the same time in the underlying model, the
        /// selected item might change unexpectedly between those two objects.
        ///
        /// If this is not set, the `Eq` implementation is used, meaning that
        /// they must be the same object.
        pub(super) item_equivalence_fn: RefCell<Option<Box<EquivalentObjectFn>>>,
        /// The position of the selected item.
        #[property(get, set = Self::set_selected, explicit_notify, default = gtk::INVALID_LIST_POSITION)]
        selected: Cell<u32>,
        /// The selected item.
        #[property(get, set = Self::set_selected_item, explicit_notify, nullable)]
        selected_item: RefCell<Option<glib::Object>>,
        /// Whether the model is empty.
        #[property(get = Self::is_empty)]
        is_empty: PhantomData<bool>,
    }

    impl fmt::Debug for FixedSelection {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("FixedSelection")
                .field("model", &self.model)
                .field("selected", &self.selected)
                .field("selected_item", &self.selected_item)
                .finish_non_exhaustive()
        }
    }

    impl Default for FixedSelection {
        fn default() -> Self {
            Self {
                model: Default::default(),
                item_equivalence_fn: Default::default(),
                selected: Cell::new(gtk::INVALID_LIST_POSITION),
                selected_item: Default::default(),
                is_empty: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FixedSelection {
        const NAME: &'static str = "FixedSelection";
        type Type = super::FixedSelection;
        type Interfaces = (gio::ListModel, gtk::SelectionModel);
    }

    #[glib::derived_properties]
    impl ObjectImpl for FixedSelection {}

    impl ListModelImpl for FixedSelection {
        fn item_type(&self) -> glib::Type {
            glib::Object::static_type()
        }

        fn n_items(&self) -> u32 {
            self.model.obj().map(|m| m.n_items()).unwrap_or_default()
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.model.obj()?.item(position)
        }
    }

    impl SelectionModelImpl for FixedSelection {
        fn selection_in_range(&self, _position: u32, _n_items: u32) -> gtk::Bitset {
            let bitset = gtk::Bitset::new_empty();
            let selected = self.selected.get();

            if selected != gtk::INVALID_LIST_POSITION {
                bitset.add(selected);
            }

            bitset
        }

        fn is_selected(&self, position: u32) -> bool {
            self.selected.get() == position
        }
    }

    impl FixedSelection {
        /// Set the underlying model.
        fn set_model(&self, model: Option<gio::ListModel>) {
            let prev_model = self.model.obj();

            if prev_model == model {
                return;
            }

            let prev_n_items = prev_model
                .as_ref()
                .map(ListModelExt::n_items)
                .unwrap_or_default();
            let n_items = model
                .as_ref()
                .map(ListModelExt::n_items)
                .unwrap_or_default();

            self.model.disconnect_signals();

            let obj = self.obj();
            let _guard = obj.freeze_notify();

            if let Some(model) = model {
                let items_changed_handler = model.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |m, p, r, a| {
                        imp.items_changed_cb(m, p, r, a);
                    }
                ));

                self.model.set(model, vec![items_changed_handler]);
            }

            if self.selected.get() != gtk::INVALID_LIST_POSITION {
                self.selected.replace(gtk::INVALID_LIST_POSITION);
                obj.notify_selected();
            }
            if self.selected_item.borrow().is_some() {
                self.selected_item.replace(None);
                obj.notify_selected_item();
            }

            if prev_n_items > 0 || n_items > 0 {
                obj.items_changed(0, prev_n_items, n_items);
            }
            if (prev_n_items > 0 && n_items == 0) || (prev_n_items == 0 && n_items > 0) {
                obj.notify_is_empty();
            }

            obj.notify_model();
        }

        /// Set the selected item by its position.
        fn set_selected(&self, position: u32) {
            let prev_selected = self.selected.get();
            if prev_selected == position {
                return;
            }

            let selected_item = self.model.obj().and_then(|m| m.item(position));

            let selected = if selected_item.is_none() {
                gtk::INVALID_LIST_POSITION
            } else {
                position
            };

            if prev_selected == selected {
                return;
            }
            let obj = self.obj();

            self.selected.replace(selected);
            self.selected_item.replace(selected_item);

            if prev_selected == gtk::INVALID_LIST_POSITION {
                obj.selection_changed(selected, 1);
            } else if selected == gtk::INVALID_LIST_POSITION {
                obj.selection_changed(prev_selected, 1);
            } else if selected < prev_selected {
                obj.selection_changed(selected, prev_selected - selected + 1);
            } else {
                obj.selection_changed(prev_selected, selected - prev_selected + 1);
            }

            obj.notify_selected();
            obj.notify_selected_item();
        }

        /// Set the selected item.
        fn set_selected_item(&self, item: Option<glib::Object>) {
            if *self.selected_item.borrow() == item {
                return;
            }
            let obj = self.obj();

            let prev_selected = self.selected.get();
            let mut selected = gtk::INVALID_LIST_POSITION;

            if item.is_some()
                && let Some(model) = self.model.obj()
            {
                for i in 0..model.n_items() {
                    let current_item = model.item(i);
                    if current_item == item {
                        selected = i;
                        break;
                    }
                }
            }

            self.selected_item.replace(item);

            if prev_selected != selected {
                self.selected.replace(selected);

                if prev_selected == gtk::INVALID_LIST_POSITION {
                    obj.selection_changed(selected, 1);
                } else if selected == gtk::INVALID_LIST_POSITION {
                    obj.selection_changed(prev_selected, 1);
                } else if selected < prev_selected {
                    obj.selection_changed(selected, prev_selected - selected + 1);
                } else {
                    obj.selection_changed(prev_selected, selected - prev_selected + 1);
                }
                obj.notify_selected();
            }

            obj.notify_selected_item();
        }

        /// Whether the model is empty.
        fn is_empty(&self) -> bool {
            self.model.obj().is_none_or(|model| model.n_items() == 0)
        }

        /// Handle when items changed in the underlying model.
        fn items_changed_cb(
            &self,
            model: &gio::ListModel,
            position: u32,
            removed: u32,
            added: u32,
        ) {
            let obj = self.obj();
            let _guard = obj.freeze_notify();

            let selected = self.selected.get();
            let selected_item = self.selected_item.borrow().clone();

            if selected_item.is_none() || selected < position {
                // unchanged
            } else if selected != gtk::INVALID_LIST_POSITION && selected >= position + removed {
                self.selected.set(selected + added - removed);
                obj.notify_selected();
            } else {
                let mut found = false;
                let item_equivalence_fn = self.item_equivalence_fn.borrow();

                for i in position..(position + added) {
                    let item = model.item(i);

                    if item.as_ref().zip(selected_item.as_ref()).is_some_and(
                        |(item, selected_item)| {
                            if let Some(item_equivalence_fn) = &*item_equivalence_fn {
                                item_equivalence_fn(item, selected_item)
                            } else {
                                item == selected_item
                            }
                        },
                    ) {
                        if selected != i {
                            // The item moved.
                            self.selected.set(i);
                            obj.notify_selected();
                        }

                        if item != selected_item {
                            // The item changed.
                            self.selected_item.replace(item);
                            obj.notify_selected_item();
                        }

                        found = true;
                        break;
                    }
                }

                if !found {
                    // The item is no longer in the model.
                    self.selected.set(gtk::INVALID_LIST_POSITION);
                    obj.notify_selected();
                }
            }

            obj.items_changed(position, removed, added);

            let n_items = model.n_items();
            if n_items == 0 || (removed == 0 && n_items == added) {
                obj.notify_is_empty();
            }
        }
    }
}

glib::wrapper! {
    /// A `GtkSelectionModel` that keeps track of the selected item even if its
    /// position changes or it is removed from the list.
    pub struct FixedSelection(ObjectSubclass<imp::FixedSelection>)
        @implements gio::ListModel, gtk::SelectionModel;
}

impl FixedSelection {
    /// Construct a new `FixedSelection` with the given model.
    pub fn new(model: Option<&impl IsA<gio::ListModel>>) -> Self {
        glib::Object::builder().property("model", model).build()
    }

    /// Set the function to use to test for equivalence of two items.
    ///
    /// It is used when checking if an object still present when the underlying
    /// model changes. Which means that if there are two equivalent objects at
    /// the same time in the underlying model, the selected item might change
    /// unexpectedly between those two objects.
    ///
    /// If this is not set, the `Eq` implementation is used, meaning that they
    /// must be the same object.
    pub(crate) fn set_item_equivalence_fn(
        &self,
        equivalence_fn: impl Fn(&glib::Object, &glib::Object) -> bool + 'static,
    ) {
        self.imp()
            .item_equivalence_fn
            .replace(Some(Box::new(equivalence_fn)));
    }
}

impl Default for FixedSelection {
    fn default() -> Self {
        Self::new(None::<&gio::ListModel>)
    }
}
