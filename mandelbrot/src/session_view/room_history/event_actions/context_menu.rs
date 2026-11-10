use gettextrs::gettext;
use gtk::{gio, prelude::*};

use super::QuickReactionChooser;
use crate::session::ReactionList;

/// Helper struct for the context menu of a row presenting an [`Event`].
///
/// [`Event`]: crate::session::Event
#[derive(Debug)]
pub(crate) struct EventActionsContextMenu {
    /// The popover of the context menu.
    pub(crate) popover: gtk::PopoverMenu,
    /// The menu model of the popover.
    menu_model: gio::Menu,
    /// The quick reaction chooser in the context menu.
    quick_reaction_chooser: QuickReactionChooser,
}

impl EventActionsContextMenu {
    /// The identifier in the context menu for the quick reaction chooser.
    const QUICK_REACTION_CHOOSER_ID: &str = "quick-reaction-chooser";

    /// Whether the menu includes an item for the quick reaction chooser.
    fn has_quick_reaction_chooser(&self) -> bool {
        let first_section = self
            .menu_model
            .item_link(0, gio::MENU_LINK_SECTION)
            .and_downcast::<gio::Menu>()
            .expect("event context menu should have at least one section");
        first_section
            .item_attribute_value(0, "custom", Some(&String::static_variant_type()))
            .and_then(|variant| variant.get::<String>())
            .is_some_and(|value| value == Self::QUICK_REACTION_CHOOSER_ID)
    }

    /// Add the quick reaction chooser to this menu, if it is not already
    /// present, and set the reaction list.
    pub(crate) fn add_quick_reaction_chooser(&self, reactions: ReactionList) {
        if !self.has_quick_reaction_chooser() {
            let section_menu = gio::Menu::new();
            let item = gio::MenuItem::new(None, None);
            item.set_attribute_value(
                "custom",
                Some(&Self::QUICK_REACTION_CHOOSER_ID.to_variant()),
            );
            section_menu.append_item(&item);
            self.menu_model.insert_section(0, None, &section_menu);

            self.popover.add_child(
                &self.quick_reaction_chooser,
                Self::QUICK_REACTION_CHOOSER_ID,
            );
        }

        self.quick_reaction_chooser.set_reactions(Some(reactions));
    }

    /// Remove the quick reaction chooser from this menu, if it is present.
    pub(crate) fn remove_quick_reaction_chooser(&self) {
        if !self.has_quick_reaction_chooser() {
            return;
        }

        self.popover.remove_child(&self.quick_reaction_chooser);
        self.menu_model.remove(0);
    }
}

impl Default for EventActionsContextMenu {
    fn default() -> Self {
        let menu_model = gtk::Builder::from_resource(
            "/org/tunaos/mandelbrot/ui/session_view/room_history/event_actions/context_menu.ui",
        )
        .object::<gio::Menu>("event-actions-menu")
        .expect("GResource and menu should exist");

        let popover = gtk::PopoverMenu::builder()
            .has_arrow(false)
            .halign(gtk::Align::Start)
            .menu_model(&menu_model)
            .build();
        popover.update_property(&[gtk::accessible::Property::Label(&gettext("Context Menu"))]);

        Self {
            popover,
            menu_model,
            quick_reaction_chooser: Default::default(),
        }
    }
}
