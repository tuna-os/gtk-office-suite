use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};
use indexmap::IndexMap;
use tracing::error;

use crate::utils::BoundObject;

mod imp {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::ExpressionListModel)]
    pub struct ExpressionListModel {
        #[property(get, set = Self::set_model, explicit_notify, nullable)]
        model: BoundObject<gio::ListModel>,
        expressions: RefCell<Vec<gtk::Expression>>,
        /// Tracked items with their expression watches, kept in sync with
        /// the underlying model's positions. Supports O(1) lookup by item.
        watches: RefCell<IndexMap<glib::Object, Vec<gtk::ExpressionWatch>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ExpressionListModel {
        const NAME: &'static str = "ExpressionListModel";
        type Type = super::ExpressionListModel;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for ExpressionListModel {
        fn dispose(&self) {
            for watch in self.watches.take().into_values().flatten() {
                watch.unwatch();
            }
        }
    }

    impl ListModelImpl for ExpressionListModel {
        fn item_type(&self) -> glib::Type {
            self.model
                .obj()
                .map_or_else(glib::Object::static_type, |m| m.item_type())
        }

        fn n_items(&self) -> u32 {
            self.model.obj().map(|m| m.n_items()).unwrap_or_default()
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.model.obj().and_then(|m| m.item(position))
        }
    }

    impl ExpressionListModel {
        /// Set the underlying model.
        fn set_model(&self, model: Option<gio::ListModel>) {
            if self.model.obj() == model {
                return;
            }

            let obj = self.obj();
            let removed = self.n_items();

            self.model.disconnect_signals();
            for watch in self.watches.take().into_values().flatten() {
                watch.unwatch();
            }

            let added = if let Some(model) = model {
                let items_changed_handler = model.connect_items_changed(clone!(
                    #[strong]
                    obj,
                    move |_, pos, removed, added| {
                        obj.imp().watch_items(pos, removed, added);
                        obj.items_changed(pos, removed, added);
                    }
                ));

                let added = model.n_items();
                self.model.set(model, vec![items_changed_handler]);

                self.watch_items(0, removed, added);
                added
            } else {
                0
            };

            let obj = self.obj();
            obj.items_changed(0, removed, added);
            obj.notify_model();
        }

        /// Set the expressions to watch.
        pub(super) fn set_expressions(&self, expressions: Vec<gtk::Expression>) {
            for watch in self.watches.take().into_values().flatten() {
                watch.unwatch();
            }

            self.expressions.replace(expressions);

            let n_items = self.n_items();
            self.watch_items(0, n_items, n_items);
        }

        /// Watch and unwatch items according to changes in the underlying
        /// model.
        fn watch_items(&self, pos: u32, removed: u32, added: u32) {
            let Some(model) = self.model.obj() else {
                return;
            };

            let expressions = self.expressions.borrow().clone();
            if expressions.is_empty() {
                return;
            }

            let mut new_entries = Vec::with_capacity(added as usize);
            for item_pos in pos..pos + added {
                let Some(item) = model.item(item_pos) else {
                    error!("Out of bounds item");
                    break;
                };

                let obj = self.obj();
                let mut item_watches = Vec::with_capacity(expressions.len());
                for expression in &expressions {
                    item_watches.push(expression.watch(
                        Some(&item),
                        clone!(
                            #[strong]
                            obj,
                            #[weak]
                            item,
                            move || {
                                obj.imp().item_expr_changed(&item);
                            }
                        ),
                    ));
                }

                new_entries.push((item, item_watches));
            }

            let mut watches = self.watches.borrow_mut();
            let removed_range = (pos as usize)..((pos + removed) as usize);
            for watch in watches
                .splice(removed_range, new_entries)
                .flat_map(|(_, w)| w)
            {
                watch.unwatch();
            }
        }

        fn item_expr_changed(&self, item: &glib::Object) {
            // O(1) lookup via the IndexMap.
            let pos = self.watches.borrow().get_index_of(item);
            if let Some(pos) = pos {
                self.obj().items_changed(pos as u32, 1, 1);
            }
        }
    }
}

glib::wrapper! {
    /// A list model that signals an item as changed when the expression's value changes.
    pub struct ExpressionListModel(ObjectSubclass<imp::ExpressionListModel>)
        @implements gio::ListModel;
}

impl ExpressionListModel {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the expressions to watch.
    pub(crate) fn set_expressions(&self, expressions: Vec<gtk::Expression>) {
        self.imp().set_expressions(expressions);
    }
}

impl Default for ExpressionListModel {
    fn default() -> Self {
        Self::new()
    }
}
