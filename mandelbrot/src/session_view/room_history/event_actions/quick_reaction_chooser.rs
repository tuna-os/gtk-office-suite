use adw::{prelude::*, subclass::prelude::*};
use gtk::{
    glib,
    glib::{clone, closure_local},
};

use crate::{session::ReactionList, utils::BoundObject};

/// A quick reaction.
#[derive(Debug, Clone, Copy)]
struct QuickReaction {
    /// The emoji that is presented.
    key: &'static str,
    /// The number of the column where this reaction is presented.
    ///
    /// There are 4 columns in total.
    column: i32,
    /// The number of the row where this reaction is presented.
    ///
    /// There are 2 rows in total.
    row: i32,
}

/// The quick reactions to present.
static QUICK_REACTIONS: &[QuickReaction] = &[
    QuickReaction {
        key: "👍️",
        column: 0,
        row: 0,
    },
    QuickReaction {
        key: "👎️",
        column: 1,
        row: 0,
    },
    QuickReaction {
        key: "😄",
        column: 2,
        row: 0,
    },
    QuickReaction {
        key: "🎉",
        column: 3,
        row: 0,
    },
    QuickReaction {
        key: "😕",
        column: 0,
        row: 1,
    },
    QuickReaction {
        key: "❤️",
        column: 1,
        row: 1,
    },
    QuickReaction {
        key: "🚀",
        column: 2,
        row: 1,
    },
];

mod imp {

    use std::{cell::RefCell, collections::HashMap, sync::LazyLock};

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/event_actions/quick_reaction_chooser.ui"
    )]
    #[properties(wrapper_type = super::QuickReactionChooser)]
    pub struct QuickReactionChooser {
        #[template_child]
        reaction_grid: TemplateChild<gtk::Grid>,
        /// The list of reactions of the event for which this chooser is
        /// presented.
        #[property(get, set = Self::set_reactions, explicit_notify, nullable)]
        reactions: BoundObject<ReactionList>,
        reaction_bindings: RefCell<HashMap<String, glib::Binding>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for QuickReactionChooser {
        const NAME: &'static str = "QuickReactionChooser";
        type Type = super::QuickReactionChooser;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for QuickReactionChooser {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("more-reactions-activated").build()]);
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            // Construct the quick reactions.
            let grid = &self.reaction_grid;
            for reaction in QUICK_REACTIONS {
                let button = gtk::ToggleButton::builder()
                    .label(reaction.key)
                    .action_name("event.toggle-reaction")
                    .action_target(&reaction.key.to_variant())
                    .css_classes(["flat", "circular"])
                    .build();
                button.connect_clicked(|button| {
                    button.activate_action("context-menu.close", None).unwrap();
                });
                grid.attach(&button, reaction.column, reaction.row, 1, 1);
            }
        }
    }

    impl WidgetImpl for QuickReactionChooser {}
    impl BinImpl for QuickReactionChooser {}

    #[gtk::template_callbacks]
    impl QuickReactionChooser {
        /// Set the list of reactions of the event for which this chooser is
        /// presented.
        fn set_reactions(&self, reactions: Option<ReactionList>) {
            let prev_reactions = self.reactions.obj();

            if prev_reactions == reactions {
                return;
            }

            self.reactions.disconnect_signals();
            for (_, binding) in self.reaction_bindings.borrow_mut().drain() {
                binding.unbind();
            }

            // Reset the state of the buttons.
            for row in 0..=1 {
                for column in 0..=3 {
                    if let Some(button) = self
                        .reaction_grid
                        .child_at(column, row)
                        .and_downcast::<gtk::ToggleButton>()
                    {
                        button.set_active(false);
                    }
                }
            }

            if let Some(reactions) = reactions {
                let signal_handler = reactions.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, _, _, _| {
                        imp.update_reactions();
                    }
                ));
                self.reactions.set(reactions, vec![signal_handler]);
            }

            self.update_reactions();
        }

        /// Update the state of the quick reactions.
        fn update_reactions(&self) {
            let mut reaction_bindings = self.reaction_bindings.borrow_mut();
            let reactions = self.reactions.obj();

            for reaction_item in QUICK_REACTIONS {
                if let Some(reaction) = reactions
                    .as_ref()
                    .and_then(|reactions| reactions.reaction_group_by_key(reaction_item.key))
                {
                    if reaction_bindings.get(reaction_item.key).is_none() {
                        let button = self
                            .reaction_grid
                            .child_at(reaction_item.column, reaction_item.row)
                            .unwrap();
                        let binding = reaction
                            .bind_property("has-own-user", &button, "active")
                            .sync_create()
                            .build();
                        reaction_bindings.insert(reaction_item.key.to_string(), binding);
                    }
                } else if let Some(binding) = reaction_bindings.remove(reaction_item.key) {
                    if let Some(button) = self
                        .reaction_grid
                        .child_at(reaction_item.column, reaction_item.row)
                        .and_downcast::<gtk::ToggleButton>()
                    {
                        button.set_active(false);
                    }

                    binding.unbind();
                }
            }
        }

        /// Handle when the "More reactions" button is activated.
        #[template_callback]
        fn more_reactions_activated(&self) {
            self.obj()
                .emit_by_name::<()>("more-reactions-activated", &[]);
        }
    }
}

glib::wrapper! {
    /// A widget displaying quick reactions and taking its state from a [`ReactionList`].
    pub struct QuickReactionChooser(ObjectSubclass<imp::QuickReactionChooser>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl QuickReactionChooser {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Connect to the signal emitted when the "More reactions" button is
    /// activated.
    pub fn connect_more_reactions_activated<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "more-reactions-activated",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}

impl Default for QuickReactionChooser {
    fn default() -> Self {
        Self::new()
    }
}
