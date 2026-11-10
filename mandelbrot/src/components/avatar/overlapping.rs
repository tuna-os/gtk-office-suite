use adw::{prelude::*, subclass::prelude::*};
use gtk::{gdk, gio, glib, glib::clone};
use tracing::error;

use super::{Avatar, AvatarData, crop_circle::CropCircle};

/// Function to extract the avatar data from a supported `GObject`.
type ExtractAvatarDataFn = dyn Fn(&glib::Object) -> AvatarData + 'static;

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
    };

    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::OverlappingAvatars)]
    pub struct OverlappingAvatars {
        /// The children containing the avatars.
        children: RefCell<Vec<CropCircle>>,
        /// The size of the avatars.
        #[property(get, set = Self::set_avatar_size, explicit_notify)]
        avatar_size: Cell<u32>,
        /// The spacing between the avatars.
        #[property(get, set = Self::set_spacing, explicit_notify)]
        spacing: Cell<u32>,
        /// The maximum number of avatars to display.
        #[property(get = Self::max_avatars, set = Self::set_max_avatars)]
        max_avatars: PhantomData<u32>,
        slice_model: gtk::SliceListModel,
        /// The method used to extract `AvatarData` from the items of the list
        /// model, if any.
        extract_avatar_data_fn: RefCell<Option<Box<ExtractAvatarDataFn>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OverlappingAvatars {
        const NAME: &'static str = "OverlappingAvatars";
        type Type = super::OverlappingAvatars;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for OverlappingAvatars {
        fn constructed(&self) {
            self.parent_constructed();

            self.slice_model.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, position, removed, added| {
                    imp.handle_items_changed(position, removed, added);
                }
            ));
        }

        fn dispose(&self) {
            for child in self.children.take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for OverlappingAvatars {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            if self.children.borrow().is_empty() {
                return (0, 0, -1, -1);
            }

            let avatar_size = self.avatar_size.get();

            if orientation == gtk::Orientation::Vertical {
                let size = avatar_size.try_into().unwrap_or(i32::MAX);
                return (size, size, -1, -1);
            }

            let n_children = u32::try_from(self.children.borrow().len())
                .expect("count of children fits into u32");

            // The last avatar has no overlap.
            let mut size = n_children.saturating_sub(1) * self.distance_between_centers();
            size += avatar_size;

            let size = size.try_into().unwrap_or(i32::MAX);
            (size, size, -1, -1)
        }

        fn size_allocate(&self, _width: i32, _height: i32, _baseline: i32) {
            let avatar_size = i32::try_from(self.avatar_size.get()).unwrap_or(i32::MAX);
            let distance_between_centers = i32::try_from(self.distance_between_centers())
                .expect("distance between centers fits into i32");

            let mut x = 0;
            for child in self.children.borrow().iter() {
                let allocation = gdk::Rectangle::new(x, 0, avatar_size, avatar_size);
                child.size_allocate(&allocation, -1);

                x = x.saturating_add(distance_between_centers);
            }
        }
    }

    impl AccessibleImpl for OverlappingAvatars {
        fn first_accessible_child(&self) -> Option<gtk::Accessible> {
            // Hide the children in the a11y tree.
            None
        }
    }

    impl OverlappingAvatars {
        /// Set the size of the avatars.
        fn set_avatar_size(&self, size: u32) {
            if self.avatar_size.get() == size {
                return;
            }
            let obj = self.obj();

            self.avatar_size.set(size);

            // Update the sizes of the avatars.
            let size = i32::try_from(size).unwrap_or(i32::MAX);
            let overlap = self.overlap();
            for child in self.children.borrow().iter() {
                child.set_cropped_width(overlap);

                if let Some(avatar) = child.child().and_downcast::<Avatar>() {
                    avatar.set_size(size);
                }
            }
            obj.queue_resize();

            obj.notify_avatar_size();
        }

        /// Compute the avatars overlap according to their size.
        #[allow(clippy::cast_sign_loss)] // The result can only be positive.
        fn overlap(&self) -> u32 {
            let avatar_size = self.avatar_size.get();
            // Make the overlap a little less than half the size of the avatar.
            (f64::from(avatar_size) / 2.5) as u32
        }

        /// Compute the distance between the center of two avatars.
        fn distance_between_centers(&self) -> u32 {
            self.avatar_size
                .get()
                .saturating_sub(self.overlap())
                .saturating_add(self.spacing.get())
        }

        /// Set the spacing between the avatars.
        fn set_spacing(&self, spacing: u32) {
            if self.spacing.get() == spacing {
                return;
            }

            self.spacing.set(spacing);

            let obj = self.obj();
            obj.queue_resize();
            obj.notify_avatar_size();
        }

        /// The maximum number of avatars to display.
        fn max_avatars(&self) -> u32 {
            self.slice_model.size()
        }

        /// Set the maximum number of avatars to display.
        fn set_max_avatars(&self, max_avatars: u32) {
            self.slice_model.set_size(max_avatars);
        }

        /// Bind a `GListModel` to this list.
        pub(super) fn bind_model<P: Fn(&glib::Object) -> AvatarData + 'static>(
            &self,
            model: Option<&gio::ListModel>,
            extract_avatar_data_fn: P,
        ) {
            self.extract_avatar_data_fn
                .replace(Some(Box::new(extract_avatar_data_fn)));
            self.slice_model.set_model(model);
        }

        /// Handle when the items of the model changed.
        fn handle_items_changed(&self, position: u32, removed: u32, added: u32) {
            let mut children = self.children.borrow_mut();
            let prev_count = children.len();

            let extract_avatar_data_fn_borrow = self.extract_avatar_data_fn.borrow();
            let extract_avatar_data_fn = extract_avatar_data_fn_borrow
                .as_ref()
                .expect("extract avatar data fn should be set if model is set");

            let avatar_size = i32::try_from(self.avatar_size.get()).unwrap_or(i32::MAX);
            let cropped_width = self.overlap();
            let obj = self.obj();

            let added = (position..(position + added)).filter_map(|position| {
                let Some(item) = self.slice_model.item(position) else {
                    error!("Could not get item in slice model at position {position}");
                    return None;
                };

                let avatar_data = extract_avatar_data_fn(&item);

                let avatar = Avatar::new();
                avatar.set_data(Some(avatar_data));
                avatar.set_size(avatar_size);

                let child = CropCircle::new();
                child.set_child(Some(avatar));
                child.set_cropped_width(cropped_width);
                child.set_parent(&*obj);

                Some(child)
            });

            for child in children.splice(position as usize..(position + removed) as usize, added) {
                child.unparent();
            }

            // Make sure that only the last avatar is not cropped.
            let mut peekable_children = children.iter().peekable();
            while let Some(child) = peekable_children.next() {
                child.set_is_cropped(peekable_children.peek().is_some());
            }

            if prev_count != children.len() {
                obj.queue_resize();
            }
        }
    }
}

glib::wrapper! {
    /// A horizontal list of overlapping avatars.
    pub struct OverlappingAvatars(ObjectSubclass<imp::OverlappingAvatars>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl OverlappingAvatars {
    /// Create an empty `OverlappingAvatars`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Bind a `GListModel` to this list.
    pub(crate) fn bind_model<P: Fn(&glib::Object) -> AvatarData + 'static>(
        &self,
        model: Option<&impl IsA<gio::ListModel>>,
        extract_avatar_data_fn: P,
    ) {
        self.imp()
            .bind_model(model.map(Cast::upcast_ref), extract_avatar_data_fn);
    }
}
