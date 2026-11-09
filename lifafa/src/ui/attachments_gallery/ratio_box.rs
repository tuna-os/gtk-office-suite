//! A container that keeps its child at a 4:3 aspect ratio, for the gallery's
//! thumbnails.

use gtk::glib;
use gtk::prelude::*;

glib::wrapper! {
    /// A single-child container that forces a fixed 4:3 (width:height) aspect
    /// ratio via true height-for-width sizing, so the thumbnail fills the
    /// (responsive) column width and its height follows — no centred gaps and
    /// no continuous frame-clock ticking.
    pub(crate) struct RatioBox(ObjectSubclass<imp::RatioBox>) @extends gtk::Widget;
}

impl RatioBox {
    pub(super) fn new(child: &impl IsA<gtk::Widget>) -> Self {
        let obj: Self = glib::Object::new();
        child.set_parent(&obj);
        obj
    }
}

mod imp {
    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    /// Height as a fraction of width — 3/4 gives a 4:3 landscape thumbnail.
    const HEIGHT_OVER_WIDTH_NUM: i32 = 3;
    const HEIGHT_OVER_WIDTH_DEN: i32 = 4;

    #[derive(Default)]
    pub(crate) struct RatioBox;

    #[glib::object_subclass]
    impl ObjectSubclass for RatioBox {
        const NAME: &'static str = "HylkiRatioBox";
        type Type = super::RatioBox;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for RatioBox {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for RatioBox {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Vertical => {
                    // Height follows the allocated width. Until the width is
                    // known (for_size < 0) request nothing and let the parent
                    // stretch us horizontally first.
                    let h = if for_size > 0 {
                        for_size * HEIGHT_OVER_WIDTH_NUM / HEIGHT_OVER_WIDTH_DEN
                    } else {
                        0
                    };
                    (h, h, -1, -1)
                }
                // Width is driven by the parent (hexpand + width-request); ask
                // for nothing intrinsic so we fill whatever the column offers.
                _ => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.obj().first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}
