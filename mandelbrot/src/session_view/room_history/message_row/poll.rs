use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use matrix_sdk_ui::timeline::PollState;
use ruma::{
    OwnedEventId,
    events::{
        AnyMessageLikeEventContent,
        poll::{start::PollKind, unstable_response::UnstablePollResponseEventContent},
    },
};
use tracing::error;

use super::ContentFormat;
use crate::{i18n::ngettext_f, prelude::*, session::Room, spawn, spawn_tokio, toast};

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_row/poll.ui"
    )]
    pub struct MessagePoll {
        #[template_child]
        question_label: TemplateChild<gtk::Label>,
        #[template_child]
        answers_box: TemplateChild<gtk::Box>,
        #[template_child]
        votes_label: TemplateChild<gtk::Label>,
        /// The room containing the poll.
        room: glib::WeakRef<Room>,
        /// The ID of the poll start event.
        event_id: RefCell<Option<OwnedEventId>>,
        /// The maximum number of answers a user can select.
        max_selections: Cell<u64>,
        /// The check buttons to vote, per answer ID.
        checks: RefCell<Vec<(String, gtk::CheckButton)>>,
        /// Whether the answers are currently being rebuilt.
        ///
        /// This is used to ignore programmatic `toggled` signals.
        updating: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessagePoll {
        const NAME: &'static str = "ContentMessagePoll";
        type Type = super::MessagePoll;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_css_name("message-poll");
            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for MessagePoll {}
    impl WidgetImpl for MessagePoll {}
    impl BinImpl for MessagePoll {}

    impl MessagePoll {
        /// Display the given poll in the given room.
        #[allow(clippy::too_many_lines)]
        pub(super) fn set_poll(
            &self,
            room: &Room,
            event_id: Option<OwnedEventId>,
            poll: &PollState,
            format: ContentFormat,
        ) {
            let results = poll.results();

            let has_event_id = event_id.is_some();
            self.room.set(Some(room));
            self.event_id.replace(event_id);
            self.max_selections.set(results.max_selections);

            self.question_label.set_label(&results.question);

            let compact = matches!(format, ContentFormat::Compact | ContentFormat::Ellipsized);
            if matches!(format, ContentFormat::Ellipsized) {
                self.question_label.set_wrap(false);
                self.question_label
                    .set_ellipsize(gtk::pango::EllipsizeMode::End);
            } else {
                self.question_label.set_wrap(true);
                self.question_label
                    .set_ellipsize(gtk::pango::EllipsizeMode::None);
            }

            self.answers_box.set_visible(!compact);
            self.votes_label.set_visible(!compact);

            if compact {
                return;
            }

            let own_user_id = room.own_member().user_id().clone();
            let is_ended = results.end_time.is_some();
            let is_disclosed = matches!(results.kind, PollKind::Disclosed);
            let own_answers = results
                .answers
                .iter()
                .filter(|answer| {
                    results.votes.get(&answer.id).is_some_and(|users| {
                        users
                            .iter()
                            .any(|user| user.as_str() == own_user_id.as_str())
                    })
                })
                .map(|answer| answer.id.clone())
                .collect::<Vec<_>>();
            let has_voted = !own_answers.is_empty();
            let total_votes = results.votes.values().map(Vec::len).sum::<usize>();
            let max_votes = results
                .answers
                .iter()
                .map(|answer| {
                    results
                        .votes
                        .get(&answer.id)
                        .map(Vec::len)
                        .unwrap_or_default()
                })
                .max()
                .unwrap_or_default();

            let show_results = is_ended || (is_disclosed && has_voted);
            let can_vote = !is_ended && has_event_id && room.permissions().can_send_message();

            // Rebuild the answers.
            self.updating.set(true);

            let mut checks = Vec::new();
            while let Some(child) = self.answers_box.first_child() {
                self.answers_box.remove(&child);
            }

            let mut prev_check: Option<gtk::CheckButton> = None;
            for answer in &results.answers {
                let count = results
                    .votes
                    .get(&answer.id)
                    .map(Vec::len)
                    .unwrap_or_default();
                let is_selected = own_answers.contains(&answer.id);
                let is_winner = is_ended && count > 0 && count == max_votes;

                let answer_label = gtk::Label::builder()
                    .label(&answer.text)
                    .xalign(0.0)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .hexpand(true)
                    .build();
                if is_winner {
                    answer_label.add_css_class("heading");
                }

                let top_box = gtk::Box::builder()
                    .orientation(gtk::Orientation::Horizontal)
                    .spacing(6)
                    .build();

                if can_vote {
                    let check = gtk::CheckButton::builder()
                        .active(is_selected)
                        .hexpand(true)
                        .build();
                    check.set_child(Some(&answer_label));

                    if results.max_selections <= 1 {
                        check.set_group(prev_check.as_ref());
                        prev_check = Some(check.clone());
                    }

                    let answer_id = answer.id.clone();
                    check.connect_toggled(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |check| {
                            if imp.updating.get() {
                                return;
                            }
                            imp.answer_toggled(&answer_id, check.is_active());
                        }
                    ));

                    checks.push((answer.id.clone(), check.clone()));
                    top_box.append(&check);
                } else {
                    top_box.append(&answer_label);

                    if is_selected {
                        let checkmark = gtk::Image::builder()
                            .icon_name("checkmark-symbolic")
                            .tooltip_text(gettext("Your vote"))
                            .build();
                        top_box.append(&checkmark);
                    }
                }

                let row = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .spacing(3)
                    .build();
                row.append(&top_box);

                if show_results {
                    let count_label = gtk::Label::builder()
                        .label(ngettext_f(
                            // Translators: Do NOT translate the content between '{' and '}',
                            // this is a variable name.
                            "{n} vote",
                            "{n} votes",
                            count.try_into().unwrap_or(u32::MAX),
                            &[("n", &count.to_string())],
                        ))
                        .xalign(0.0)
                        .css_classes(["caption", "dim-label"])
                        .build();
                    top_box.append(&count_label);

                    let fraction = if total_votes > 0 {
                        count as f64 / total_votes as f64
                    } else {
                        0.0
                    };
                    let progress = gtk::ProgressBar::builder().fraction(fraction).build();
                    row.append(&progress);
                }

                self.answers_box.append(&row);
            }

            self.checks.replace(checks);
            self.updating.set(false);

            // Update the footer.
            let n = total_votes.try_into().unwrap_or(u32::MAX);
            let footer = if is_ended {
                ngettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "Final result based on {n} vote",
                    "Final result based on {n} votes",
                    n,
                    &[("n", &total_votes.to_string())],
                )
            } else if show_results {
                ngettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "{n} vote cast",
                    "{n} votes cast",
                    n,
                    &[("n", &total_votes.to_string())],
                )
            } else if is_disclosed {
                gettext("Vote to see the results")
            } else {
                gettext("Results will be revealed when the poll ends")
            };
            self.votes_label.set_label(&footer);
        }

        /// Handle the given answer being toggled by the user.
        fn answer_toggled(&self, answer_id: &str, active: bool) {
            let selections = if self.max_selections.get() <= 1 {
                if !active {
                    // This is the signal for the deselected radio button, the
                    // signal for the newly selected one will handle the vote.
                    return;
                }

                vec![answer_id.to_owned()]
            } else {
                let checks = self.checks.borrow();
                let selections = checks
                    .iter()
                    .filter(|(_, check)| check.is_active())
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();

                if selections.len() as u64 > self.max_selections.get() {
                    // Do not allow to select more answers than permitted.
                    if let Some((_, check)) = checks.iter().find(|(id, _)| id.as_str() == answer_id)
                    {
                        self.updating.set(true);
                        check.set_active(false);
                        self.updating.set(false);
                    }

                    return;
                }

                selections
            };

            self.send_selections(selections);
        }

        /// Send the given selections as a response to the poll.
        fn send_selections(&self, selections: Vec<String>) {
            let Some(room) = self.room.upgrade() else {
                return;
            };
            let Some(event_id) = self.event_id.borrow().clone() else {
                return;
            };

            let content = UnstablePollResponseEventContent::new(selections, event_id);
            let matrix_timeline = room.live_timeline().matrix_timeline();

            let obj = self.obj().clone();
            spawn!(async move {
                let handle = spawn_tokio!(async move {
                    matrix_timeline
                        .send(AnyMessageLikeEventContent::UnstablePollResponse(content))
                        .await
                });

                if let Err(error) = handle.await.expect("task was not aborted") {
                    error!("Could not send poll vote: {error}");
                    toast!(obj, gettext("Could not send poll vote"));
                }
            });
        }
    }
}

glib::wrapper! {
    /// A widget displaying a poll in the timeline.
    pub struct MessagePoll(ObjectSubclass<imp::MessagePoll>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MessagePoll {
    /// Create a new poll message.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Display the given poll in the given room.
    pub(crate) fn set_poll(
        &self,
        room: &Room,
        event_id: Option<OwnedEventId>,
        poll: &PollState,
        format: ContentFormat,
    ) {
        self.imp().set_poll(room, event_id, poll, format);
    }
}

impl Default for MessagePoll {
    fn default() -> Self {
        Self::new()
    }
}
