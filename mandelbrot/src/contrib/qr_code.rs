// Taken from https://gitlab.gnome.org/msandova/trinket/-/blob/master/src/qr_code.rs
// All credit goes to Maximiliano

use gettextrs::gettext;
use gtk::{glib, prelude::*, subclass::prelude::*};

pub(crate) mod imp {
    use std::cell::{Cell, RefCell};

    use gtk::{gdk, graphene};

    use super::*;

    #[derive(Debug, glib::Properties)]
    #[properties(wrapper_type = super::QRCode)]
    pub struct QRCode {
        pub data: RefCell<QRCodeData>,
        /// The block size of this QR Code.
        ///
        /// Determines the size of the widget.
        #[property(get, set = Self::set_block_size)]
        pub block_size: Cell<u32>,
    }

    impl Default for QRCode {
        fn default() -> Self {
            Self {
                data: Default::default(),
                block_size: Cell::new(6),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for QRCode {
        const NAME: &'static str = "TriQRCode";
        type Type = super::QRCode;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("qrcode");
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for QRCode {
        fn constructed(&self) {
            self.parent_constructed();

            self.obj()
                .update_property(&[gtk::accessible::Property::Label(&gettext("QR Code"))]);
        }
    }

    impl WidgetImpl for QRCode {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let square_width = obj.width() as f32 / self.data.borrow().width as f32;
            let square_height = obj.height() as f32 / self.data.borrow().height as f32;

            self.data
                .borrow()
                .items
                .iter()
                .enumerate()
                .for_each(|(y, line)| {
                    line.iter().enumerate().for_each(|(x, is_dark)| {
                        let color = if *is_dark {
                            gdk::RGBA::BLACK
                        } else {
                            gdk::RGBA::WHITE
                        };
                        let position = graphene::Rect::new(
                            (x as f32) * square_width,
                            (y as f32) * square_height,
                            square_width,
                            square_height,
                        );

                        snapshot.append_color(&color, &position);
                    });
                });
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let stride = i32::try_from(self.obj().block_size()).expect("block size fits into i32");

            let minimum = match orientation {
                gtk::Orientation::Horizontal => self.data.borrow().width * stride,
                gtk::Orientation::Vertical => self.data.borrow().height * stride,
                _ => unreachable!(),
            };
            let natural = std::cmp::max(for_size, minimum);
            (minimum, natural, -1, -1)
        }
    }

    impl QRCode {
        /// Sets the block size of this QR Code.
        fn set_block_size(&self, block_size: u32) {
            self.block_size.set(std::cmp::max(block_size, 1));

            let obj = self.obj();
            obj.queue_draw();
            obj.queue_resize();
        }
    }
}

glib::wrapper! {
    /// A widget that display a QR Code.
    ///
    /// The QR code of [`QRCode`] is set with the [QRCode::set_bytes()]
    /// method. It is recommended for a QR Code to have a quiet zone, in most
    /// contexts, widgets already count with such a margin.
    ///
    /// The code can be themed via css, where a recommended quiet-zone
    /// can be as a padding:
    ///
    /// ```css
    /// qrcode {
    ///     color: black;
    ///     background: white;
    ///     padding: 24px;  /* 4 ⨉ block-size */
    /// }
    /// ```
    pub struct QRCode(ObjectSubclass<imp::QRCode>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl QRCode {
    /// Creates a new [`QRCode`].
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Creates a new [`QRCode`] with a QR code generated from `bytes`.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let qrcode = Self::new();
        qrcode.set_bytes(bytes);

        qrcode
    }

    /// Sets the displayed code of `self` to a QR code generated from `bytes`.
    pub fn set_bytes(&self, bytes: &[u8]) {
        let data = QRCodeData::try_from(bytes).unwrap_or_else(|_| {
            glib::g_warning!(None, "Could not load QRCode from bytes");
            Default::default()
        });
        self.imp().data.replace(data);

        self.queue_draw();
        self.queue_resize();
    }

    /// Set the `QrCode` to be displayed.
    pub fn set_qrcode(&self, qrcode: qrcode::QrCode) {
        self.imp().data.replace(QRCodeData::from(qrcode));

        self.queue_draw();
        self.queue_resize();
    }
}

impl Default for QRCodeData {
    fn default() -> Self {
        Self::try_from("".as_bytes()).unwrap()
    }
}

#[derive(Debug, Clone)]
pub struct QRCodeData {
    pub width: i32,
    pub height: i32,
    pub items: Vec<Vec<bool>>,
}

impl TryFrom<&[u8]> for QRCodeData {
    type Error = qrcode::types::QrError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        Ok(qrcode::QrCode::new(data)?.into())
    }
}

impl From<qrcode::QrCode> for QRCodeData {
    fn from(code: qrcode::QrCode) -> Self {
        let items = code
            .render::<char>()
            .quiet_zone(false)
            .module_dimensions(1, 1)
            .build()
            .split('\n')
            .map(|line| {
                line.chars()
                    .map(|c| !c.is_whitespace())
                    .collect::<Vec<bool>>()
            })
            .collect::<Vec<Vec<bool>>>();

        let size = items
            .len()
            .try_into()
            .expect("count of items fits into i32");
        Self {
            width: size,
            height: size,
            items,
        }
    }
}
