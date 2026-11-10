use adw::{prelude::*, subclass::prelude::*};
use gtk::{
    glib,
    glib::{clone, closure_local},
};

use crate::components::{AvatarImageSafetySetting, Pill, PillSource};

mod imp {
    use std::{cell::RefCell, collections::HashMap, marker::PhantomData, sync::LazyLock};

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/pill/search_entry.ui")]
    #[properties(wrapper_type = super::PillSearchEntry)]
    pub struct PillSearchEntry {
        #[template_child]
        text_view: TemplateChild<gtk::TextView>,
        #[template_child]
        text_buffer: TemplateChild<gtk::TextBuffer>,
        /// The text of the entry.
        #[property(get = Self::text)]
        text: PhantomData<glib::GString>,
        /// Whether the entry is editable.
        #[property(get = Self::editable, set = Self::set_editable, explicit_notify)]
        editable: PhantomData<bool>,
        /// The pills in the text view.
        ///
        /// A map of pill identifier to anchor of the pill in the text view.
        pills: RefCell<HashMap<String, gtk::TextChildAnchor>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PillSearchEntry {
        const NAME: &'static str = "PillSearchEntry";
        type Type = super::PillSearchEntry;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for PillSearchEntry {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    Signal::builder("pill-removed")
                        .param_types([PillSource::static_type()])
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.text_buffer.connect_delete_range(clone!(
                #[weak]
                obj,
                move |_, start, end| {
                    if start == end {
                        // Nothing to do.
                        return;
                    }

                    // If a pill was removed, emit the corresponding signal.
                    let mut current = *start;
                    loop {
                        if let Some(source) = current
                            .child_anchor()
                            .and_then(|a| a.widgets().first().cloned())
                            .and_downcast_ref::<Pill>()
                            .and_then(Pill::source)
                        {
                            let removed = obj
                                .imp()
                                .pills
                                .borrow_mut()
                                .remove(&source.identifier())
                                .is_some();

                            if removed {
                                obj.emit_by_name::<()>("pill-removed", &[&source]);
                            }
                        }

                        current.forward_char();

                        if &current == end {
                            break;
                        }
                    }
                }
            ));

            self.text_buffer
                .connect_insert_text(|text_buffer, location, text| {
                    let mut changed = false;

                    // We do not allow adding chars before and between pills.
                    loop {
                        if location.child_anchor().is_some() {
                            changed = true;
                            if !location.forward_char() {
                                break;
                            }
                        } else {
                            break;
                        }
                    }

                    if changed {
                        text_buffer.place_cursor(location);
                        text_buffer.stop_signal_emission_by_name("insert-text");
                        text_buffer.insert(location, text);
                    }
                });

            self.text_buffer.connect_text_notify(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.notify_text();
                }
            ));
        }
    }

    impl WidgetImpl for PillSearchEntry {
        fn grab_focus(&self) -> bool {
            self.text_view.grab_focus()
        }
    }

    impl BinImpl for PillSearchEntry {}

    impl PillSearchEntry {
        /// The text of the entry.
        fn text(&self) -> glib::GString {
            let (start, end) = self.text_buffer.bounds();
            self.text_buffer.text(&start, &end, false)
        }

        /// Whether the entry is editable.
        fn editable(&self) -> bool {
            self.text_view.is_editable()
        }

        /// Set whether the entry is editable.
        fn set_editable(&self, editable: bool) {
            if self.editable() == editable {
                return;
            }

            self.text_view.set_editable(editable);
            self.obj().notify_editable();
        }

        /// Add a pill for the given source to the entry.
        pub(super) fn add_pill(&self, source: &PillSource) {
            let identifier = source.identifier();

            // If the pill already exists, do not insert it again.
            if self.pills.borrow().contains_key(&identifier) {
                return;
            }

            // We do not need to watch the safety setting as this entry should only be used
            // with search results.
            let pill = Pill::new(source, AvatarImageSafetySetting::None, None);
            pill.set_margin_start(3);
            pill.set_margin_end(3);

            let (mut start_iter, mut end_iter) = self.text_buffer.bounds();

            // We don't allow adding chars before and between pills
            loop {
                if start_iter.child_anchor().is_some() {
                    start_iter.forward_char();
                } else {
                    break;
                }
            }

            self.text_buffer.delete(&mut start_iter, &mut end_iter);
            let anchor = self.text_buffer.create_child_anchor(&mut start_iter);
            self.text_view.add_child_at_anchor(&pill, &anchor);
            self.pills.borrow_mut().insert(identifier, anchor);

            self.text_view.grab_focus();
        }

        /// Remove the pill with the given identifier.
        pub(super) fn remove_pill(&self, identifier: &str) {
            let Some(anchor) = self.pills.borrow_mut().remove(identifier) else {
                return;
            };

            if anchor.is_deleted() {
                // Nothing to do.
                return;
            }

            let mut start_iter = self.text_buffer.iter_at_child_anchor(&anchor);
            let mut end_iter = start_iter;
            end_iter.forward_char();
            self.text_buffer.delete(&mut start_iter, &mut end_iter);
        }

        /// Clear this entry.
        pub(super) fn clear(&self) {
            let (mut start, mut end) = self.text_buffer.bounds();
            self.text_buffer.delete(&mut start, &mut end);
        }
    }
}

glib::wrapper! {
    /// Search entry where selected results can be added as [`Pill`]s.
    pub struct PillSearchEntry(ObjectSubclass<imp::PillSearchEntry>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PillSearchEntry {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Add a pill for the given source to the entry.
    pub(crate) fn add_pill(&self, source: &impl IsA<PillSource>) {
        self.imp().add_pill(source.upcast_ref());
    }

    /// Remove the pill with the given identifier.
    pub(crate) fn remove_pill(&self, identifier: &str) {
        self.imp().remove_pill(identifier);
    }

    /// Clear this entry.
    pub(crate) fn clear(&self) {
        self.imp().clear();
    }

    /// Connect to the signal emitted when a pill is removed from the entry.
    ///
    /// The second parameter is the source of the pill.
    pub fn connect_pill_removed<F: Fn(&Self, PillSource) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "pill-removed",
            true,
            closure_local!(|obj: Self, source: PillSource| {
                f(&obj, source);
            }),
        )
    }
}
