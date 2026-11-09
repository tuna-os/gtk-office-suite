use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};
use tracing::error;

use super::{HistoryViewerEvent, HistoryViewerEventType, HistoryViewerTimeline, VisualMediaItem};
use crate::{
    components::LoadingRow,
    prelude::*,
    session_view::MediaViewer,
    spawn,
    utils::{BoundConstructOnlyObject, LoadingState},
};

/// The minimum number of items that should be loaded.
const MIN_N_ITEMS: u32 = 50;
/// The minimum size requested by an item.
const SIZE_REQUEST: i32 = 150;

mod imp {
    use std::ops::ControlFlow;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/history_viewer/visual_media.ui"
    )]
    #[properties(wrapper_type = super::VisualMediaHistoryViewer)]
    pub struct VisualMediaHistoryViewer {
        #[template_child]
        media_viewer: TemplateChild<MediaViewer>,
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        grid_view: TemplateChild<gtk::GridView>,
        /// The timeline containing the media events.
        #[property(get, set = Self::set_timeline, construct_only)]
        timeline: BoundConstructOnlyObject<HistoryViewerTimeline>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VisualMediaHistoryViewer {
        const NAME: &'static str = "ContentVisualMediaHistoryViewer";
        type Type = super::VisualMediaHistoryViewer;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("visual-media-history-viewer");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for VisualMediaHistoryViewer {
        fn constructed(&self) {
            self.parent_constructed();

            let factory = gtk::SignalListItemFactory::new();

            factory.connect_bind(move |_, list_item| {
                let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
                    error!("List item factory did not receive a list item: {list_item:?}");
                    return;
                };

                list_item.set_activatable(false);
                list_item.set_selectable(false);
            });
            factory.connect_bind(move |_, list_item| {
                let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
                    error!("List item factory did not receive a list item: {list_item:?}");
                    return;
                };

                let item = list_item.item();

                if let Some(loading_row) = item
                    .and_downcast_ref::<LoadingRow>()
                    .filter(|_| !list_item.child().is_some_and(|c| c.is::<LoadingRow>()))
                {
                    loading_row.unparent();
                    loading_row.set_width_request(SIZE_REQUEST);
                    loading_row.set_height_request(SIZE_REQUEST);

                    list_item.set_child(Some(loading_row));
                } else if let Some(event) = item.and_downcast::<HistoryViewerEvent>() {
                    let media_item = list_item.child_or_default::<VisualMediaItem>();
                    media_item.set_event(Some(event));
                }
            });

            self.grid_view.set_factory(Some(&factory));
        }
    }

    impl WidgetImpl for VisualMediaHistoryViewer {}
    impl NavigationPageImpl for VisualMediaHistoryViewer {}

    #[gtk::template_callbacks]
    impl VisualMediaHistoryViewer {
        /// Set the timeline containing the media events.
        fn set_timeline(&self, timeline: HistoryViewerTimeline) {
            let filter = gtk::CustomFilter::new(|obj| {
                obj.downcast_ref::<HistoryViewerEvent>()
                    .is_some_and(|e| e.event_type() == HistoryViewerEventType::Media)
                    || obj.is::<LoadingRow>()
            });
            let filter_model =
                gtk::FilterListModel::new(Some(timeline.with_loading_item().clone()), Some(filter));

            let model = gtk::NoSelection::new(Some(filter_model));
            model.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _, _, _| {
                    imp.update_state();
                }
            ));
            self.grid_view.set_model(Some(&model));

            let timeline_state_handler = timeline.connect_state_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_state();
                }
            ));
            self.timeline.set(timeline, vec![timeline_state_handler]);
            self.update_state();

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.init_timeline().await;
                }
            ));
        }

        /// Initialize the timeline
        async fn init_timeline(&self) {
            self.load_more_items().await;

            let adj = self
                .grid_view
                .vadjustment()
                .expect("GtkGridView has a vadjustment");
            adj.connect_value_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    if imp.needs_more_items() {
                        spawn!(async move {
                            imp.load_more_items().await;
                        });
                    }
                }
            ));
        }

        /// Load more items in this viewer.
        #[template_callback]
        async fn load_more_items(&self) {
            self.timeline
                .obj()
                .load(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    #[upgrade_or]
                    ControlFlow::Break(()),
                    move || {
                        if imp.needs_more_items() {
                            ControlFlow::Continue(())
                        } else {
                            ControlFlow::Break(())
                        }
                    }
                ))
                .await;
        }

        /// Whether this viewer needs more items.
        fn needs_more_items(&self) -> bool {
            let Some(model) = self.grid_view.model() else {
                return false;
            };

            // Make sure there is an initial number of items.
            if model.n_items() < MIN_N_ITEMS {
                return true;
            }

            let adj = self
                .grid_view
                .vadjustment()
                .expect("GtkGridView has a vadjustment");
            adj.value() + adj.page_size() * 2.0 >= adj.upper()
        }

        /// Update this viewer for the current state.
        fn update_state(&self) {
            let Some(model) = self.grid_view.model() else {
                return;
            };
            let timeline = self.timeline.obj();

            let visible_child_name = match timeline.state() {
                LoadingState::Initial => "loading",
                LoadingState::Error => "error",
                LoadingState::Ready if model.n_items() == 0 => "empty",
                LoadingState::Loading => {
                    if model.n_items() == 0
                        || (model.n_items() == 1
                            && model.item(0).is_some_and(|item| item.is::<LoadingRow>()))
                    {
                        "loading"
                    } else {
                        "content"
                    }
                }
                LoadingState::Ready => "content",
            };
            self.stack.set_visible_child_name(visible_child_name);
        }

        /// Show the given media item in the media viewer.
        pub(super) fn show_media_viewer(&self, item: &VisualMediaItem) {
            let Some(event) = item.event() else {
                return;
            };
            let Some(room) = event.room() else {
                return;
            };

            let media_message = event
                .visual_media_message()
                .expect("visual media items should contain only visual message content");
            self.media_viewer
                .set_message(&room, media_message, Some(event.event_id()));
            self.media_viewer.reveal(item);
        }
    }
}

glib::wrapper! {
    /// A view presenting the list of visual media (image or video) events in a room.
    pub struct VisualMediaHistoryViewer(ObjectSubclass<imp::VisualMediaHistoryViewer>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl VisualMediaHistoryViewer {
    pub fn new(timeline: &HistoryViewerTimeline) -> Self {
        glib::Object::builder()
            .property("timeline", timeline)
            .build()
    }

    /// Show the given media item in the media viewer.
    pub(crate) fn show_media_viewer(&self, item: &VisualMediaItem) {
        self.imp().show_media_viewer(item);
    }
}
