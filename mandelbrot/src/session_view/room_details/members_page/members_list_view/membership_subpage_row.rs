use gettextrs::{gettext, pgettext};
use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};

use crate::{
    session::MembershipListKind,
    session_view::room_details::membership_subpage_item::MembershipSubpageItem,
};

mod imp {
    use std::{cell::RefCell, marker::PhantomData};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/members_page/members_list_view/membership_subpage_row.ui"
    )]
    #[properties(wrapper_type = super::MembershipSubpageRow)]
    pub struct MembershipSubpageRow {
        /// The item presented by this row.
        #[property(get, set = Self::set_item, explicit_notify, nullable)]
        item: RefCell<Option<MembershipSubpageItem>>,
        items_changed_handler: RefCell<Option<glib::SignalHandlerId>>,
        /// The name of the icon of this row.
        #[property(get = Self::icon_name)]
        icon_name: PhantomData<Option<String>>,
        /// The label of this row.
        #[property(get = Self::label)]
        label: PhantomData<Option<String>>,
        #[template_child]
        members_count: TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MembershipSubpageRow {
        const NAME: &'static str = "MembersPageMembershipSubpageRow";
        type Type = super::MembershipSubpageRow;
        type ParentType = gtk::ListBoxRow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MembershipSubpageRow {
        fn dispose(&self) {
            if let Some(item) = &*self.item.borrow()
                && let Some(handler) = self.items_changed_handler.take()
            {
                item.model().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for MembershipSubpageRow {}
    impl ListBoxRowImpl for MembershipSubpageRow {}

    impl MembershipSubpageRow {
        /// Set the item presented by this row.
        fn set_item(&self, item: Option<MembershipSubpageItem>) {
            if *self.item.borrow() == item {
                return;
            }
            let obj = self.obj();

            if let Some(item) = &*self.item.borrow()
                && let Some(handler) = self.items_changed_handler.take()
            {
                item.model().disconnect(handler);
            }

            if let Some(item) = &item {
                let model = item.model();

                let items_changed_handler = model.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |model, _, _, _| {
                        imp.member_count_changed(model.n_items());
                        imp.obj().notify_label();
                    }
                ));
                self.items_changed_handler
                    .replace(Some(items_changed_handler));

                self.member_count_changed(model.n_items());
            }

            self.item.replace(item);

            obj.notify_item();
            obj.notify_icon_name();
            obj.notify_label();
        }

        /// The name of the icon of this row.
        fn icon_name(&self) -> Option<String> {
            Some(self.item.borrow().as_ref()?.kind().icon_name().to_owned())
        }

        /// The label of this row.
        fn label(&self) -> Option<String> {
            let item = self.item.borrow().clone()?;
            let count = item.model().n_items();

            // We don't use the count in the strings so we use separate pgettext calls for
            // singular and plural rather than using npgettext.
            let label = match item.kind() {
                MembershipListKind::Join => return None,
                // Translators: As in 'Invited Room Member(s)'.
                MembershipListKind::Invite => {
                    if count == 1 {
                        // Translators: This is singular, as in 'Invited Room Member'.
                        pgettext("member", "Invited")
                    } else {
                        // Translators: This is plural, as in 'Invited Room Members'.
                        pgettext("members", "Invited")
                    }
                }
                MembershipListKind::Ban => {
                    if count == 1 {
                        // Translators: This is singular, as in 'Banned Room Member'.
                        pgettext("member", "Banned")
                    } else {
                        // Translators: This is plural, as in 'Banned Room Members'.
                        pgettext("members", "Banned")
                    }
                }
                MembershipListKind::Knock => {
                    if count == 1 {
                        gettext("Invite Request")
                    } else {
                        gettext("Invite Requests")
                    }
                }
            };

            Some(label)
        }

        fn member_count_changed(&self, n: u32) {
            self.members_count.set_text(&format!("{n}"));
        }
    }
}

glib::wrapper! {
    /// A row presenting a `MembershipSubpageItem`.
    pub struct MembershipSubpageRow(ObjectSubclass<imp::MembershipSubpageRow>)
        @extends gtk::Widget, gtk::ListBoxRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl MembershipSubpageRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
