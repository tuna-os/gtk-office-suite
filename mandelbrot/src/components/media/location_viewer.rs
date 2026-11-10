use adw::{prelude::*, subclass::prelude::*};
use geo_uri::GeoUri;
use gtk::{gdk, gio, glib};
use shumate::prelude::*;

use crate::i18n::gettext_f;

mod imp {
    use std::cell::Cell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/media/location_viewer.ui")]
    #[properties(wrapper_type = super::LocationViewer)]
    pub struct LocationViewer {
        #[template_child]
        map: TemplateChild<shumate::SimpleMap>,
        #[template_child]
        marker_img: TemplateChild<gtk::Image>,
        marker: shumate::Marker,
        /// Whether to display this location in a compact format.
        #[property(get, set = Self::set_compact, explicit_notify)]
        compact: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LocationViewer {
        const NAME: &'static str = "LocationViewer";
        type Type = super::LocationViewer;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_css_name("location-viewer");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for LocationViewer {
        fn constructed(&self) {
            self.marker.set_child(Some(&*self.marker_img));

            let style = gio::resources_lookup_data(
                "/org/tunaos/mandelbrot/mapstyle/osm-liberty/style.json",
                gio::ResourceLookupFlags::NONE,
            )
            .expect("should be able to load map style");
            let renderer =
                shumate::VectorRenderer::new("vector-tiles", &String::from_utf8_lossy(&style))
                    .expect("should be able to read map style");
            renderer.set_license("© OpenMapTiles © OpenStreetMap");
            renderer.set_license_uri("https://www.openstreetmap.org/copyright");

            let sprite_sheet = renderer
                .sprite_sheet()
                .expect("renderer should have sprite sheet");

            let sprites_texture = gdk::Texture::from_resource(
                "/org/tunaos/mandelbrot/mapstyle/osm-liberty/sprites.png",
            );
            let sprites_json = gio::resources_lookup_data(
                "/org/tunaos/mandelbrot/mapstyle/osm-liberty/sprites.json",
                gio::ResourceLookupFlags::NONE,
            )
            .expect("should be able to load map sprite sheet");
            sprite_sheet
                .add_page(
                    &sprites_texture,
                    &String::from_utf8_lossy(&sprites_json),
                    1.0,
                )
                .expect("should be able to add map sprite sheet page");

            let sprites_2x_texture = gdk::Texture::from_resource(
                "/org/tunaos/mandelbrot/mapstyle/osm-liberty/sprites@2x.png",
            );
            let sprites_2x_json = gio::resources_lookup_data(
                "/org/tunaos/mandelbrot/mapstyle/osm-liberty/sprites@2x.json",
                gio::ResourceLookupFlags::NONE,
            )
            .expect("should be able to load map 2x sprite sheet");
            sprite_sheet
                .add_page(
                    &sprites_2x_texture,
                    &String::from_utf8_lossy(&sprites_2x_json),
                    2.0,
                )
                .expect("should be able to add map 2x sprite sheet page");

            self.map.set_map_source(Some(&renderer));

            let viewport = self.map.viewport().expect("map has a viewport");
            viewport.set_zoom_level(12.0);
            let marker_layer = shumate::MarkerLayer::new(&viewport);
            marker_layer.add_marker(&self.marker);
            self.map.add_overlay_layer(&marker_layer);

            // Hide the scale.
            self.map
                .scale()
                .expect("map has a scale")
                .set_visible(false);
            self.parent_constructed();
        }
    }

    impl WidgetImpl for LocationViewer {}
    impl BinImpl for LocationViewer {}

    impl LocationViewer {
        /// Set the compact format of this location.
        fn set_compact(&self, compact: bool) {
            if self.compact.get() == compact {
                return;
            }

            self.map.set_show_zoom_buttons(!compact);
            if let Some(license) = self.map.license() {
                license.set_visible(!compact);
            }

            self.compact.set(compact);
            self.obj().notify_compact();
        }

        // Move the map viewport to the provided coordinates and draw a marker.
        pub(super) fn set_location(&self, geo_uri: &GeoUri) {
            let latitude = geo_uri.latitude();
            let longitude = geo_uri.longitude();

            self.map
                .viewport()
                .expect("map has a viewport")
                .set_location(latitude, longitude);
            self.marker.set_location(latitude, longitude);

            self.obj()
                .update_property(&[gtk::accessible::Property::Description(&gettext_f(
                    "Location at latitude {latitude} and longitude {longitude}",
                    &[
                        ("latitude", &latitude.to_string()),
                        ("longitude", &longitude.to_string()),
                    ],
                ))]);
        }
    }
}

glib::wrapper! {
    /// A widget displaying a location.
    pub struct LocationViewer(ObjectSubclass<imp::LocationViewer>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LocationViewer {
    /// Create a new location message.
    pub fn new() -> Self {
        glib::Object::new()
    }

    // Move the map viewport to the provided coordinates and draw a marker.
    pub(crate) fn set_location(&self, geo_uri: &GeoUri) {
        self.imp().set_location(geo_uri);
    }
}

impl Default for LocationViewer {
    fn default() -> Self {
        Self::new()
    }
}
