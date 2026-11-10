use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

mod account_row;

use self::account_row::AccountRow;
use crate::{
    session_list::{SessionInfo, SessionList},
    utils::OneshotNotifier,
};

mod imp {
    use std::cell::OnceCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/account_chooser_dialog/mod.ui")]
    #[properties(wrapper_type = super::AccountChooserDialog)]
    pub struct AccountChooserDialog {
        #[template_child]
        pub accounts: TemplateChild<gtk::ListBox>,
        /// The list of logged-in sessions.
        #[property(get, set = Self::set_session_list, construct)]
        pub session_list: glib::WeakRef<SessionList>,
        notifier: OnceCell<OneshotNotifier<Option<String>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AccountChooserDialog {
        const NAME: &'static str = "AccountChooserDialog";
        type Type = super::AccountChooserDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::Type::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AccountChooserDialog {}

    impl WidgetImpl for AccountChooserDialog {}

    impl AdwDialogImpl for AccountChooserDialog {
        fn closed(&self) {
            if let Some(notifier) = self.notifier.get() {
                notifier.notify();
            }
        }
    }

    impl AccountChooserDialog {
        /// The notifier for sending the response.
        pub(super) fn notifier(&self) -> &OneshotNotifier<Option<String>> {
            self.notifier
                .get_or_init(|| OneshotNotifier::new("AccountChooserDialog"))
        }

        /// Set the list of logged-in sessions.
        fn set_session_list(&self, session_list: &SessionList) {
            self.accounts.bind_model(Some(session_list), |session| {
                let row = AccountRow::new(session.downcast_ref().unwrap());
                row.upcast()
            });

            self.session_list.set(Some(session_list));
        }
    }
}

glib::wrapper! {
    /// A dialog to choose an account among the ones that are connected.
    ///
    /// Should be used by calling [`Self::choose_account()`].
    pub struct AccountChooserDialog(ObjectSubclass<imp::AccountChooserDialog>)
        @extends gtk::Widget, adw::Dialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

#[gtk::template_callbacks]
impl AccountChooserDialog {
    pub fn new(session_list: &SessionList) -> Self {
        glib::Object::builder()
            .property("session-list", session_list)
            .build()
    }

    /// Open this dialog to choose an account.
    pub async fn choose_account(&self, parent: &impl IsA<gtk::Widget>) -> Option<String> {
        let receiver = self.imp().notifier().listen();

        self.present(Some(parent));

        receiver.await
    }

    /// Select the given row in the session list.
    #[template_callback]
    fn select_row(&self, row: &gtk::ListBoxRow) {
        let index = row
            .index()
            .try_into()
            .expect("selected row should have an index");

        let session_id = self
            .session_list()
            .and_then(|l| l.item(index))
            .and_downcast::<SessionInfo>()
            .map(|s| s.session_id());

        self.imp().notifier().notify_value(session_id);
        self.close();
    }
}
