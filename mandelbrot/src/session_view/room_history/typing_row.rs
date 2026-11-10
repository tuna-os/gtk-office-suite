use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};

use crate::{
    components::OverlappingAvatars,
    i18n::{gettext_f, ngettext_f},
    prelude::*,
    session::{Member, TypingList},
    utils::BoundObjectWeakRef,
};

mod imp {
    use std::marker::PhantomData;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/typing_row.ui")]
    #[properties(wrapper_type = super::TypingRow)]
    pub struct TypingRow {
        #[template_child]
        avatar_list: TemplateChild<OverlappingAvatars>,
        #[template_child]
        label: TemplateChild<gtk::Label>,
        /// The list of members that are currently typing.
        #[property(get, set = Self::set_list, explicit_notify, nullable)]
        list: BoundObjectWeakRef<TypingList>,
        /// Whether the list is empty.
        #[property(get = Self::is_empty, default = true)]
        is_empty: PhantomData<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TypingRow {
        const NAME: &'static str = "ContentTypingRow";
        type Type = super::TypingRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_css_name("typing-row");
            klass.set_accessible_role(gtk::AccessibleRole::ListItem);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for TypingRow {}

    impl WidgetImpl for TypingRow {}
    impl BinImpl for TypingRow {}

    impl TypingRow {
        /// Set the list of members that are currently typing.
        fn set_list(&self, list: Option<&TypingList>) {
            if self.list.obj().as_ref() == list {
                return;
            }
            let obj = self.obj();

            let prev_is_empty = self.is_empty();

            self.list.disconnect_signals();

            if let Some(list) = list {
                let items_changed_handler_id = list.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |list, _pos, removed, added| {
                        if removed != 0 || added != 0 {
                            imp.update_label(list);
                        }
                    }
                ));
                let is_empty_notify_handler_id = list.connect_is_empty_notify(clone!(
                    #[weak]
                    obj,
                    move |_| obj.notify_is_empty()
                ));

                self.avatar_list.bind_model(Some(list), |item| {
                    item.downcast_ref::<Member>()
                        .expect("typing list item should be a member")
                        .avatar_data()
                });

                self.list.set(
                    list,
                    vec![items_changed_handler_id, is_empty_notify_handler_id],
                );
                self.update_label(list);
            }

            if prev_is_empty != self.is_empty() {
                obj.notify_is_empty();
            }

            obj.notify_list();
        }

        /// Whether the list is empty.
        fn is_empty(&self) -> bool {
            let Some(list) = self.list.obj() else {
                return true;
            };

            list.is_empty()
        }

        /// Update the label for the current state of the given typing list.
        fn update_label(&self, list: &TypingList) {
            let n = list.n_items();
            if n == 0 {
                // Do not update anything, the `is-empty` property should trigger a revealer
                // animation.
                return;
            }

            let label = if n == 1 {
                let user = list
                    .item(0)
                    .and_downcast::<Member>()
                    .expect("typing list has a member")
                    .disambiguated_name();

                gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', these are
                    // variable names.
                    "{user} is typing…",
                    &[("user", &format!("<b>{user}</b>"))],
                )
            } else {
                ngettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', these are
                    // variable names.
                    "{n} member is typing…",
                    "{n} members are typing…",
                    n,
                    &[("n", &n.to_string())],
                )
            };
            self.label.set_label(&label);
        }
    }
}

glib::wrapper! {
    /// A widget row used to display typing members.
    pub struct TypingRow(ObjectSubclass<imp::TypingRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl TypingRow {
    /// Construct a new `TypingRow`.
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for TypingRow {
    fn default() -> Self {
        Self::new()
    }
}
