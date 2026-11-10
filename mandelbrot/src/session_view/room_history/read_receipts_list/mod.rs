use gtk::{gdk, gio, glib, glib::clone, prelude::*, subclass::prelude::*};

mod read_receipts_popover;

use self::read_receipts_popover::ReadReceiptsPopover;
use super::member_timestamp::MemberTimestamp;
use crate::{
    components::OverlappingAvatars,
    i18n::{gettext_f, ngettext_f},
    prelude::*,
    session::{Member, MemberList, UserReadReceipt},
    utils::{BoundObjectWeakRef, key_bindings},
};

// Keep in sync with the `max-avatars` property of the `avatar_list` in the
// UI file.
const MAX_RECEIPTS_SHOWN: u32 = 5;

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/read_receipts_list/mod.ui"
    )]
    #[properties(wrapper_type = super::ReadReceiptsList)]
    pub struct ReadReceiptsList {
        #[template_child]
        content: TemplateChild<gtk::Box>,
        #[template_child]
        label: TemplateChild<gtk::Label>,
        #[template_child]
        avatar_list: TemplateChild<OverlappingAvatars>,
        /// Whether this list is active.
        ///
        /// This list is active when the popover is displayed.
        #[property(get)]
        active: Cell<bool>,
        /// The list of room members.
        #[property(get, set = Self::set_members, explicit_notify, nullable)]
        members: RefCell<Option<MemberList>>,
        /// The list of read receipts.
        #[property(get)]
        list: gio::ListStore,
        /// The read receipts used as a source.
        #[property(get, set = Self::set_source, explicit_notify)]
        source: BoundObjectWeakRef<gio::ListModel>,
        /// The displayed member if there is only one receipt.
        receipt_member: BoundObjectWeakRef<Member>,
    }

    impl Default for ReadReceiptsList {
        fn default() -> Self {
            Self {
                content: Default::default(),
                label: Default::default(),
                avatar_list: Default::default(),
                active: Default::default(),
                members: Default::default(),
                list: gio::ListStore::new::<MemberTimestamp>(),
                source: Default::default(),
                receipt_member: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ReadReceiptsList {
        const NAME: &'static str = "ContentReadReceiptsList";
        type Type = super::ReadReceiptsList;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk::BinLayout>();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("read-receipts-list");
            klass.set_accessible_role(gtk::AccessibleRole::ToggleButton);

            klass.install_action("read-receipts-list.activate", None, |obj, _, _| {
                obj.imp().show_popover(1, 0.0, 0.0);
            });

            key_bindings::add_activate_bindings(klass, "read-receipts-list.activate");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for ReadReceiptsList {
        fn constructed(&self) {
            self.parent_constructed();

            self.avatar_list.bind_model(Some(&self.list), |item| {
                item.downcast_ref::<MemberTimestamp>()
                    .and_then(MemberTimestamp::member)
                    .expect("item should be a member timestamp with a member")
                    .avatar_data()
            });

            self.list.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _, _, _| {
                    imp.update_tooltip();
                    imp.update_label();
                }
            ));

            self.set_pressed_state(false);
        }

        fn dispose(&self) {
            self.content.unparent();
        }
    }

    impl WidgetImpl for ReadReceiptsList {}

    impl AccessibleImpl for ReadReceiptsList {
        fn first_accessible_child(&self) -> Option<gtk::Accessible> {
            // Hide the children in the a11y tree.
            None
        }
    }

    #[gtk::template_callbacks]
    impl ReadReceiptsList {
        /// Set the list of room members.
        fn set_members(&self, members: Option<MemberList>) {
            if *self.members.borrow() == members {
                return;
            }

            self.members.replace(members);
            self.obj().notify_members();

            if let Some(source) = self.source.obj() {
                self.items_changed(&source, 0, self.list.n_items(), source.n_items());
            }
        }

        /// Set whether this list is active.
        fn set_active(&self, active: bool) {
            if self.active.get() == active {
                return;
            }

            self.active.set(active);

            self.obj().notify_active();
            self.set_pressed_state(active);
        }

        /// Set the read receipts that are used as a source of data.
        fn set_source(&self, source: &gio::ListModel) {
            if self.source.obj().as_ref() == Some(source) {
                return;
            }

            let items_changed_handler_id = source.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |source, pos, removed, added| {
                    imp.items_changed(source, pos, removed, added);
                }
            ));
            self.items_changed(source, 0, self.list.n_items(), source.n_items());

            self.source.set(source, vec![items_changed_handler_id]);
            self.obj().notify_source();
        }

        /// Set the CSS and a11y states.
        fn set_pressed_state(&self, pressed: bool) {
            let obj = self.obj();

            if pressed {
                obj.set_state_flags(gtk::StateFlags::CHECKED, false);
            } else {
                obj.unset_state_flags(gtk::StateFlags::CHECKED);
            }

            let tristate = if pressed {
                gtk::AccessibleTristate::True
            } else {
                gtk::AccessibleTristate::False
            };
            obj.update_state(&[gtk::accessible::State::Pressed(tristate)]);
        }

        /// Handle when items changed in the source.
        fn items_changed(&self, source: &gio::ListModel, pos: u32, removed: u32, added: u32) {
            let Some(members) = &*self.members.borrow() else {
                return;
            };

            let mut new_receipts = Vec::with_capacity(added as usize);

            for i in pos..pos + added {
                let Some(boxed) = source.item(i).and_downcast::<glib::BoxedAnyObject>() else {
                    break;
                };

                let source_receipt = boxed.borrow::<UserReadReceipt>();
                let member = members.get_or_create(source_receipt.user_id.clone());
                let receipt = MemberTimestamp::new(
                    &member,
                    source_receipt.receipt.ts.map(|ts| ts.as_secs().into()),
                );

                new_receipts.push(receipt);
            }

            self.list.splice(pos, removed, &new_receipts);
        }

        /// Update the tooltip of this list.
        fn update_tooltip(&self) {
            self.receipt_member.disconnect_signals();
            let n_items = self.list.n_items();

            if n_items == 1
                && let Some(member) = self
                    .list
                    .item(0)
                    .and_downcast::<MemberTimestamp>()
                    .and_then(|r| r.member())
            {
                // Listen to changes of the display name.
                let handler_id = member.connect_display_name_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |member| {
                        imp.update_member_tooltip(member);
                    }
                ));

                self.receipt_member.set(&member, vec![handler_id]);
                self.update_member_tooltip(&member);
                return;
            }

            let text = (n_items > 0).then(|| {
                ngettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "Seen by 1 member",
                    "Seen by {n} members",
                    n_items,
                    &[("n", &n_items.to_string())],
                )
            });

            self.obj().set_tooltip_text(text.as_deref());
        }

        /// Update the tooltip of this list for a single member.
        fn update_member_tooltip(&self, member: &Member) {
            // Translators: Do NOT translate the content between '{' and '}', this is a
            // variable name.
            let text = gettext_f("Seen by {name}", &[("name", &member.disambiguated_name())]);

            self.obj().set_tooltip_text(Some(&text));
        }

        /// Update the label of this list.
        fn update_label(&self) {
            let n_items = self.list.n_items();

            if n_items > MAX_RECEIPTS_SHOWN {
                self.label
                    .set_text(&format!("{} +", n_items - MAX_RECEIPTS_SHOWN));
                self.label.set_visible(true);
            } else {
                self.label.set_visible(false);
            }
        }

        /// Handle a click on the container.
        ///
        /// Shows a popover with the list of receipts if there are any.
        #[template_callback]
        fn show_popover(&self, _n_press: i32, x: f64, y: f64) {
            if self.list.n_items() == 0 {
                // No popover.
                return;
            }
            self.set_active(true);

            let popover = ReadReceiptsPopover::new(&self.list);
            popover.set_parent(&*self.obj());
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 0, 0)));
            popover.connect_closed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |popover| {
                    popover.unparent();
                    imp.set_active(false);
                }
            ));

            popover.popup();
        }
    }
}

glib::wrapper! {
    /// A widget displaying the read receipts on a message.
    pub struct ReadReceiptsList(ObjectSubclass<imp::ReadReceiptsList>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ReadReceiptsList {
    pub fn new(members: &MemberList) -> Self {
        glib::Object::builder().property("members", members).build()
    }
}
