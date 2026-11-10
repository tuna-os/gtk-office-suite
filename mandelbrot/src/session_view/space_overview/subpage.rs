use adw::{prelude::*, subclass::prelude::*};
use gtk::{gio, glib, glib::clone};
use tracing::error;

use super::child_row::SpaceChildRow;
use crate::{
    components::{Avatar, AvatarData, LoadingRow},
    prelude::*,
    session::{SpaceHierarchy, SpaceHierarchyChild},
    utils::{BoundObject, LoadingState, SingleItemListModel},
};

mod imp {
    use std::cell::{OnceCell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/space_overview/subpage.ui")]
    #[properties(wrapper_type = super::SpaceOverviewSubpage)]
    pub struct SpaceOverviewSubpage {
        #[template_child]
        pub(super) header_bar: TemplateChild<adw::HeaderBar>,
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        scrolled_window: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        listview: TemplateChild<gtk::ListView>,
        #[template_child]
        space_avatar: TemplateChild<Avatar>,
        #[template_child]
        space_name: TemplateChild<gtk::Label>,
        #[template_child]
        space_topic: TemplateChild<gtk::Label>,
        /// The space hierarchy presented by this page.
        #[property(get, set = Self::set_hierarchy, explicit_notify, nullable)]
        hierarchy: BoundObject<SpaceHierarchy>,
        /// The bindings to the room of the space.
        space_bindings: RefCell<Vec<glib::Binding>>,
        /// The items added at the end of the list.
        end_items: OnceCell<SingleItemListModel>,
        /// The full list model.
        full_model: OnceCell<gio::ListStore>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SpaceOverviewSubpage {
        const NAME: &'static str = "SpaceOverviewSubpage";
        type Type = super::SpaceOverviewSubpage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            Avatar::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SpaceOverviewSubpage {
        fn constructed(&self) {
            self.parent_constructed();

            // Load more items when scrolling, if needed.
            let adj = self.scrolled_window.vadjustment();
            adj.connect_value_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |adj| {
                    if adj.upper() - adj.value() < adj.page_size() * 2.0
                        && let Some(hierarchy) = imp.hierarchy.obj()
                    {
                        hierarchy.load_more();
                    }
                }
            ));

            // Set up the item factory of the list view.
            let factory = gtk::SignalListItemFactory::new();
            factory.connect_bind(move |_, list_item| {
                let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
                    error!("List item factory did not receive a list item: {list_item:?}");
                    return;
                };
                list_item.set_activatable(false);
                list_item.set_selectable(false);

                let Some(item) = list_item.item() else {
                    return;
                };

                if let Some(child) = item.downcast_ref::<SpaceHierarchyChild>() {
                    let row = list_item.child_or_default::<SpaceChildRow>();
                    row.set_room(Some(child.clone()));
                } else if let Some(loading_row) = item.downcast_ref::<LoadingRow>() {
                    list_item.set_child(Some(loading_row));
                }
            });
            self.listview.set_factory(Some(&factory));

            let flattened_model = gtk::FlattenListModel::new(Some(self.full_model().clone()));
            self.listview
                .set_model(Some(&gtk::NoSelection::new(Some(flattened_model))));
        }
    }

    impl WidgetImpl for SpaceOverviewSubpage {}
    impl NavigationPageImpl for SpaceOverviewSubpage {}

    #[gtk::template_callbacks]
    impl SpaceOverviewSubpage {
        /// Set the space hierarchy presented by this page.
        fn set_hierarchy(&self, hierarchy: Option<SpaceHierarchy>) {
            if self.hierarchy.obj() == hierarchy {
                return;
            }

            self.hierarchy.disconnect_signals();
            for binding in self.space_bindings.take() {
                binding.unbind();
            }

            let model = self.full_model();
            model.remove_all();

            if let Some(hierarchy) = hierarchy {
                let loading_state_handler = hierarchy.connect_loading_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_visible_child();
                    }
                ));
                let items_changed_handler = hierarchy.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, _, _, _| {
                        imp.update_visible_child();
                    }
                ));
                let space_handler = hierarchy.connect_space_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_space();
                    }
                ));

                model.append(&hierarchy);
                model.append(self.end_items());

                self.hierarchy.set(
                    hierarchy,
                    vec![loading_state_handler, items_changed_handler, space_handler],
                );
            }

            self.update_space();
            self.update_visible_child();
            self.obj().notify_hierarchy();
        }

        /// The items added at the end of the list.
        fn end_items(&self) -> &SingleItemListModel {
            self.end_items.get_or_init(|| {
                let model = SingleItemListModel::new(Some(&LoadingRow::new()));
                model.set_is_hidden(true);
                model
            })
        }

        /// The full list model.
        fn full_model(&self) -> &gio::ListStore {
            self.full_model
                .get_or_init(gio::ListStore::new::<gio::ListModel>)
        }

        /// Update the header presenting the room of the space.
        fn update_space(&self) {
            for binding in self.space_bindings.take() {
                binding.unbind();
            }

            let Some(space) = self.hierarchy.obj().and_then(|h| h.space()) else {
                self.space_avatar.set_data(None::<AvatarData>);
                return;
            };
            let obj = self.obj();

            self.space_avatar.set_data(Some(space.avatar_data()));

            let title_binding = space
                .bind_property("display-name", &*obj, "title")
                .sync_create()
                .build();
            let name_binding = space
                .bind_property("display-name", &*self.space_name, "label")
                .sync_create()
                .build();
            let topic_binding = space
                .bind_property("topic-linkified", &*self.space_topic, "label")
                .sync_create()
                .transform_to(|_, topic: Option<String>| Some(topic.unwrap_or_default()))
                .build();
            let topic_visible_binding = space
                .bind_property("topic-linkified", &*self.space_topic, "visible")
                .sync_create()
                .transform_to(|_, topic: Option<String>| {
                    Some(topic.is_some_and(|topic| !topic.is_empty()))
                })
                .build();

            self.space_bindings.replace(vec![
                title_binding,
                name_binding,
                topic_binding,
                topic_visible_binding,
            ]);
        }

        /// Update the visible child according to the current state.
        fn update_visible_child(&self) {
            let Some(hierarchy) = self.hierarchy.obj() else {
                self.stack.set_visible_child_name("loading");
                return;
            };

            let loading_state = hierarchy.loading_state();
            let is_empty = hierarchy.is_empty();

            // Create or remove the loading row, as needed.
            let show_loading_row = matches!(loading_state, LoadingState::Loading) && !is_empty;
            self.end_items().set_is_hidden(!show_loading_row);

            // Update the visible page.
            let page_name = match loading_state {
                LoadingState::Initial | LoadingState::Loading => {
                    if is_empty {
                        "loading"
                    } else {
                        "content"
                    }
                }
                LoadingState::Ready => {
                    if is_empty {
                        "empty"
                    } else {
                        "content"
                    }
                }
                LoadingState::Error => "error",
            };
            self.stack.set_visible_child_name(page_name);
        }

        /// Reload the hierarchy.
        #[template_callback]
        fn refresh(&self) {
            if let Some(hierarchy) = self.hierarchy.obj() {
                hierarchy.reload();
            }
        }
    }
}

glib::wrapper! {
    /// A page presenting the hierarchy of a space.
    pub struct SpaceOverviewSubpage(ObjectSubclass<imp::SpaceOverviewSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SpaceOverviewSubpage {
    /// Construct a new `SpaceOverviewSubpage` presenting the given hierarchy.
    pub(super) fn new(hierarchy: &SpaceHierarchy) -> Self {
        glib::Object::builder()
            .property("hierarchy", hierarchy)
            .build()
    }

    /// The header bar of this page.
    pub(super) fn header_bar(&self) -> &adw::HeaderBar {
        &self.imp().header_bar
    }
}
