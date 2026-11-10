use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use ruma::events::poll::{
    start::PollKind,
    unstable_start::{
        NewUnstablePollStartEventContent, UnstablePollAnswer, UnstablePollAnswers,
        UnstablePollStartContentBlock,
    },
};
use tracing::error;

use crate::utils::OneshotNotifier;

/// The minimum number of answers of a poll.
const MIN_ANSWERS: usize = 2;
/// The maximum number of answers of a poll, as defined in MSC3381.
const MAX_ANSWERS: usize = 20;

mod imp {
    use std::cell::{OnceCell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_toolbar/create_poll_dialog.ui"
    )]
    pub struct CreatePollDialog {
        #[template_child]
        create_button: TemplateChild<gtk::Button>,
        #[template_child]
        question_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        answers_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        add_answer_row: TemplateChild<adw::ButtonRow>,
        #[template_child]
        disclosed_row: TemplateChild<adw::SwitchRow>,
        /// The rows to enter the answers, with their remove buttons.
        answer_rows: RefCell<Vec<(adw::EntryRow, gtk::Button)>>,
        /// The notifier to send the response.
        notifier: OnceCell<OneshotNotifier<Option<NewUnstablePollStartEventContent>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CreatePollDialog {
        const NAME: &'static str = "CreatePollDialog";
        type Type = super::CreatePollDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for CreatePollDialog {
        fn constructed(&self) {
            self.parent_constructed();

            for _ in 0..MIN_ANSWERS {
                self.add_answer();
            }
        }
    }

    impl WidgetImpl for CreatePollDialog {
        fn grab_focus(&self) -> bool {
            self.question_entry.grab_focus()
        }
    }

    impl AdwDialogImpl for CreatePollDialog {
        fn closed(&self) {
            self.notifier().notify();
        }
    }

    #[gtk::template_callbacks]
    impl CreatePollDialog {
        /// The notifier to send the response.
        fn notifier(&self) -> &OneshotNotifier<Option<NewUnstablePollStartEventContent>> {
            self.notifier
                .get_or_init(|| OneshotNotifier::new("CreatePollDialog"))
        }

        /// Add a row to enter an answer.
        #[template_callback]
        fn add_answer(&self) {
            let n_answers = self.answer_rows.borrow().len();
            if n_answers >= MAX_ANSWERS {
                return;
            }

            let entry = adw::EntryRow::builder().title(gettext("Answer")).build();
            entry.connect_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.validate();
                }
            ));

            let remove_button = gtk::Button::builder()
                .icon_name("remove-symbolic")
                .tooltip_text(gettext("Remove Answer"))
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            remove_button.connect_clicked(clone!(
                #[weak(rename_to = imp)]
                self,
                #[weak]
                entry,
                move |_| {
                    imp.remove_answer(&entry);
                }
            ));
            entry.add_suffix(&remove_button);

            // Insert before the "Add Answer" row.
            self.answers_list
                .insert(&entry, n_answers.try_into().unwrap_or(i32::MAX));
            self.answer_rows.borrow_mut().push((entry, remove_button));

            self.update_answer_rows();
        }

        /// Remove the given answer row.
        fn remove_answer(&self, entry: &adw::EntryRow) {
            {
                let mut answer_rows = self.answer_rows.borrow_mut();

                if answer_rows.len() <= MIN_ANSWERS {
                    return;
                }
                let Some(pos) = answer_rows.iter().position(|(row, _)| row == entry) else {
                    return;
                };

                answer_rows.remove(pos);
            }

            self.answers_list.remove(entry);
            self.update_answer_rows();
        }

        /// Update the state of the answer rows.
        fn update_answer_rows(&self) {
            let answer_rows = self.answer_rows.borrow();

            let can_remove = answer_rows.len() > MIN_ANSWERS;
            for (_, remove_button) in answer_rows.iter() {
                remove_button.set_visible(can_remove);
            }

            self.add_answer_row
                .set_visible(answer_rows.len() < MAX_ANSWERS);

            drop(answer_rows);
            self.validate();
        }

        /// The non-empty answers that were entered.
        fn answers(&self) -> Vec<String> {
            self.answer_rows
                .borrow()
                .iter()
                .filter_map(|(entry, _)| {
                    let text = entry.text().trim().to_owned();
                    (!text.is_empty()).then_some(text)
                })
                .collect()
        }

        /// Validate the current state of the poll and update the create button.
        #[template_callback]
        fn validate(&self) {
            let is_valid = !self.question_entry.text().trim().is_empty()
                && self.answers().len() >= MIN_ANSWERS;
            self.create_button.set_sensitive(is_valid);
        }

        /// Create the poll event content and close the dialog.
        #[template_callback]
        fn create(&self) {
            let question = self.question_entry.text().trim().to_owned();
            let answers = self.answers();

            if question.is_empty() || answers.len() < MIN_ANSWERS {
                return;
            }

            // Construct a text representation of the poll for clients that do
            // not support polls.
            let fallback_text = std::iter::once(question.clone())
                .chain(
                    answers
                        .iter()
                        .enumerate()
                        .map(|(i, answer)| format!("{}. {answer}", i + 1)),
                )
                .collect::<Vec<_>>()
                .join("\n");

            let poll_answers = answers
                .into_iter()
                .map(|answer| {
                    UnstablePollAnswer::new(glib::uuid_string_random().to_string(), answer)
                })
                .collect::<Vec<_>>();
            let Ok(poll_answers) = UnstablePollAnswers::try_from(poll_answers)
                .inspect_err(|error| error!("Could not construct poll answers: {error}"))
            else {
                return;
            };

            let mut poll_start = UnstablePollStartContentBlock::new(question, poll_answers);
            if self.disclosed_row.is_active() {
                poll_start.kind = PollKind::Disclosed;
            }

            let content = NewUnstablePollStartEventContent::plain_text(fallback_text, poll_start);

            self.notifier().notify_value(Some(content));
            self.obj().close();
        }

        /// Present the dialog and wait for the poll created by the user.
        ///
        /// Returns `None` if the dialog was closed without creating a poll.
        pub(super) async fn response_future(
            &self,
            parent: &gtk::Widget,
        ) -> Option<NewUnstablePollStartEventContent> {
            let receiver = self.notifier().listen();

            self.obj().present(Some(parent));

            receiver.await
        }
    }
}

glib::wrapper! {
    /// A dialog to create a poll.
    pub struct CreatePollDialog(ObjectSubclass<imp::CreatePollDialog>)
        @extends gtk::Widget, adw::Dialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl CreatePollDialog {
    /// Create a new `CreatePollDialog`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Present the dialog and wait for the poll created by the user.
    ///
    /// Returns `None` if the dialog was closed without creating a poll.
    pub(crate) async fn response_future(
        &self,
        parent: &impl IsA<gtk::Widget>,
    ) -> Option<NewUnstablePollStartEventContent> {
        self.imp().response_future(parent.upcast_ref()).await
    }
}

impl Default for CreatePollDialog {
    fn default() -> Self {
        Self::new()
    }
}
