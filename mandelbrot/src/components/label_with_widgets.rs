use gtk::{glib, pango, prelude::*, subclass::prelude::*};

const OBJECT_REPLACEMENT_CHARACTER: &str = "\u{FFFC}";

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
    };

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::LabelWithWidgets)]
    pub struct LabelWithWidgets {
        /// The child `GtkLabel`.
        child: gtk::Label,
        /// The widgets to display in the label.
        widgets: RefCell<Vec<gtk::Widget>>,
        widgets_sizes: RefCell<Vec<(i32, i32)>>,
        /// The text of the label.
        #[property(get)]
        label: RefCell<Option<String>>,
        /// Whether the label includes Pango markup.
        #[property(get = Self::uses_markup, set = Self::set_use_markup, explicit_notify)]
        use_markup: PhantomData<bool>,
        /// Whether the label should be ellipsized.
        #[property(get, set = Self::set_ellipsize, explicit_notify)]
        ellipsize: Cell<bool>,
        /// The alignment of the lines in the text of the label, relative to
        /// each other.
        #[property(get = Self::justify, set = Self::set_justify, builder(gtk::Justification::Left))]
        justify: PhantomData<gtk::Justification>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LabelWithWidgets {
        const NAME: &'static str = "LabelWithWidgets";
        type Type = super::LabelWithWidgets;
        type ParentType = gtk::Widget;
    }

    #[glib::derived_properties]
    impl ObjectImpl for LabelWithWidgets {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            let child = &self.child;
            child.set_parent(&*obj);
            child.set_wrap(true);
            child.set_wrap_mode(pango::WrapMode::WordChar);
            child.set_xalign(0.0);
            child.set_valign(gtk::Align::Start);
        }

        fn dispose(&self) {
            self.child.unparent();

            for widget in self.widgets.borrow().iter() {
                widget.unparent();
            }
        }
    }

    impl WidgetImpl for LabelWithWidgets {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            self.allocate_shapes();
            self.child.measure(orientation, for_size)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.child.allocate(width, height, baseline, None);
            self.allocate_widgets();
        }

        fn request_mode(&self) -> gtk::SizeRequestMode {
            self.child.request_mode()
        }
    }

    impl LabelWithWidgets {
        /// Set the label and widgets to display.
        pub(super) fn set_label_and_widgets<P: IsA<gtk::Widget>>(
            &self,
            label: String,
            widgets: Vec<P>,
        ) {
            self.set_label(Some(label));
            self.set_widgets(widgets);

            self.update();
        }

        /// Set the widgets to display.
        fn set_widgets<P: IsA<gtk::Widget>>(&self, widgets: Vec<P>) {
            for widget in self.widgets.borrow_mut().drain(..) {
                widget.unparent();
            }

            self.widgets
                .borrow_mut()
                .extend(widgets.into_iter().map(Cast::upcast));

            let obj = self.obj();
            for child in self.widgets.borrow().iter() {
                child.set_parent(&*obj);
            }
        }

        /// Set the text of the label.
        fn set_label(&self, label: Option<String>) {
            if *self.label.borrow() == label {
                return;
            }

            self.label.replace(label);
            self.obj().notify_label();
        }

        /// Whether the label includes Pango markup.
        fn uses_markup(&self) -> bool {
            self.child.uses_markup()
        }

        /// Set whether the label includes Pango markup.
        fn set_use_markup(&self, use_markup: bool) {
            if self.uses_markup() == use_markup {
                return;
            }

            self.child.set_use_markup(use_markup);

            self.invalidate_widgets();
            self.obj().notify_use_markup();
        }

        /// Sets whether the text of the label should be ellipsized.
        fn set_ellipsize(&self, ellipsize: bool) {
            if self.ellipsize.get() == ellipsize {
                return;
            }

            self.ellipsize.set(true);

            self.update();
            self.obj().notify_ellipsize();
        }

        /// The alignment of the lines in the text of the label, relative to
        /// each other.
        fn justify(&self) -> gtk::Justification {
            self.child.justify()
        }

        /// Set the alignment of the lines in the text of the label, relative to
        /// each other.
        fn set_justify(&self, justify: gtk::Justification) {
            self.child.set_justify(justify);
        }

        /// Re-compute the child widgets allocations in the Pango layout.
        fn invalidate_widgets(&self) {
            self.widgets_sizes.borrow_mut().clear();
            self.allocate_shapes();
            self.obj().queue_resize();
        }

        /// Allocate shapes in the Pango layout for the child widgets.
        fn allocate_shapes(&self) {
            if self.label.borrow().as_ref().is_none_or(String::is_empty) {
                // No need to compute shapes if the label is empty.
                return;
            }

            if self.widgets.borrow().is_empty() {
                // There should be no attributes if there are no widgets.
                self.child.set_attributes(None);
                return;
            }

            let mut widgets_sizes = self.widgets_sizes.borrow_mut();

            let mut child_size_changed = false;
            for (i, child) in self.widgets.borrow().iter().enumerate() {
                let (_, natural_size) = child.preferred_size();
                let width = natural_size.width();
                let height = natural_size.height();
                if let Some((old_width, old_height)) = widgets_sizes.get(i) {
                    if old_width != &width || old_height != &height {
                        let _ = std::mem::replace(&mut widgets_sizes[i], (width, height));
                        child_size_changed = true;
                    }
                } else {
                    widgets_sizes.insert(i, (width, height));
                    child_size_changed = true;
                }
            }

            if !child_size_changed {
                return;
            }

            let attrs = pango::AttrList::new();

            for (i, (start_index, _)) in self
                .child
                .text()
                .as_str()
                .match_indices(OBJECT_REPLACEMENT_CHARACTER)
                .enumerate()
            {
                if let Some((width, height)) = widgets_sizes.get(i) {
                    let logical_rect = pango::Rectangle::new(
                        0,
                        -(height - (height / 4)) * pango::SCALE,
                        width * pango::SCALE,
                        height * pango::SCALE,
                    );

                    let mut shape = pango::AttrShape::new(&logical_rect, &logical_rect);
                    shape.set_start_index(start_index as u32);
                    shape.set_end_index((start_index + OBJECT_REPLACEMENT_CHARACTER.len()) as u32);
                    attrs.insert(shape);
                } else {
                    break;
                }
            }

            self.child.set_attributes(Some(&attrs));
        }

        /// Allocate the child widgets.
        fn allocate_widgets(&self) {
            let widgets = self.widgets.borrow();
            let widgets_sizes = self.widgets_sizes.borrow();

            let mut run_iter = self.child.layout().iter();
            let mut i = 0;
            loop {
                if let Some(run) = run_iter.run_readonly()
                    && run
                        .item()
                        .analysis()
                        .extra_attrs()
                        .iter()
                        .any(|attr| attr.type_() == pango::AttrType::Shape)
                {
                    if let Some(widget) = widgets.get(i) {
                        let (width, height) = widgets_sizes[i];
                        let (_, mut extents) = run_iter.run_extents();
                        pango::extents_to_pixels(Some(&mut extents), None);

                        let (offset_x, offset_y) = self.child.layout_offsets();
                        let allocation = gtk::Allocation::new(
                            extents.x() + offset_x,
                            extents.y() + offset_y,
                            width,
                            height,
                        );
                        widget.size_allocate(&allocation, -1);
                        i += 1;
                    } else {
                        break;
                    }
                }

                if !run_iter.next_run() {
                    // We are at the end of the Pango layout.
                    break;
                }
            }
        }

        /// Update this label for the current text and child widgets.
        fn update(&self) {
            let old_label = self.child.text();
            let old_ellipsize = self.child.ellipsize() == pango::EllipsizeMode::End;
            let new_ellipsize = self.ellipsize.get();

            let new_label = if let Some(label) = self.label.borrow().as_ref() {
                let placeholder = <Self as ObjectSubclass>::Type::PLACEHOLDER;
                let label = label.replace(placeholder, OBJECT_REPLACEMENT_CHARACTER);

                if new_ellipsize {
                    if let Some(pos) = label.find('\n') {
                        format!("{}…", &label[0..pos])
                    } else {
                        label
                    }
                } else {
                    label
                }
            } else {
                String::new()
            };

            if old_ellipsize != new_ellipsize || old_label != new_label {
                if new_ellipsize {
                    // Workaround: if both wrap and ellipsize are set, and there are
                    // widgets inserted, GtkLabel reports an erroneous minimum width.
                    self.child.set_wrap(false);
                    self.child.set_ellipsize(pango::EllipsizeMode::End);
                } else {
                    self.child.set_wrap(true);
                    self.child.set_ellipsize(pango::EllipsizeMode::None);
                }

                self.child.set_label(&new_label);
                self.invalidate_widgets();
            }
        }
    }
}

glib::wrapper! {
    /// A Label that can have multiple widgets placed inside the text.
    pub struct LabelWithWidgets(ObjectSubclass<imp::LabelWithWidgets>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LabelWithWidgets {
    /// The placeholder used to mark the locations of widgets in the label.
    pub(crate) const PLACEHOLDER: &'static str = "<widget>";

    /// Create an empty `LabelWithWidget`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the label and widgets to display.
    pub(crate) fn set_label_and_widgets<P: IsA<gtk::Widget>>(
        &self,
        label: String,
        widgets: Vec<P>,
    ) {
        self.imp().set_label_and_widgets(label, widgets);
    }
}

impl Default for LabelWithWidgets {
    fn default() -> Self {
        Self::new()
    }
}
