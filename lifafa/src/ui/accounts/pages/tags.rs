//! Settings → Tags: the tag list, Add Tag and the tag finder.

use adw::prelude::*;
use relm4::prelude::*;

use super::super::{AccountsInput, AccountsWindow};
use crate::i18n::i18n;

/// The page, and the widgets on it that the panel reaches later.
#[derive(Debug)]
pub(in crate::ui::accounts) struct TagsPage {
    pub(in crate::ui::accounts) page: adw::PreferencesPage,
    pub(in crate::ui::accounts) find_tags_spinner: gtk::Spinner,
    pub(in crate::ui::accounts) find_tags_label: gtk::Label,
    pub(in crate::ui::accounts) tags_list: gtk::ListBox,
}

impl TagsPage {
    pub(in crate::ui::accounts) fn build(sender: &ComponentSender<AccountsWindow>) -> Self {
        relm4::view! {
            page = adw::PreferencesPage {
                // Tags (#71): a name and color per keyword.
                add = &adw::PreferencesGroup {
                    set_title: &i18n("Tags"),
                    set_description: Some(
                        i18n("Tags are stored on the server as IMAP keywords, so Thunderbird and other clients show them too.\n\nDrag to reorder. The first nine are mapped to the 1–9 keys.").as_str()
                    ),
                    #[wrap(Some)]
                    // Stacked like the Cloud Storage panel's
                    // buttons: a column at the header's end.
                    set_header_suffix = &gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 12,
                        set_valign: gtk::Align::Start,
                        set_halign: gtk::Align::End,
                        set_margin_start: 24,
                        // Both 130px wide (filling the column,
                        // so they always share one width), and
                        // the finder's label change moves nothing.
                        gtk::Button {
                            set_label: &i18n("Add Tag…"),
                            set_size_request: (130, -1),
                            connect_clicked => AccountsInput::AddTag,
                        },
                        // The tag finder: read every mailbox
                        // for keywords already in use and
                        // offer them as tags.
                        #[name = "find_tags_btn"]
                        gtk::Button {
                            set_tooltip_text: Some(i18n("Look through every mailbox for tags other clients have set").as_str()),
                            set_size_request: (130, -1),
                            connect_clicked => AccountsInput::FindTags,
                            // A spinner inside the button
                            // while the mailboxes are read.
                            gtk::Box {
                                set_spacing: 6,
                                set_halign: gtk::Align::Center,
                                #[name = "find_tags_spinner"]
                                gtk::Spinner {
                                    set_visible: false,
                                },
                                #[name = "find_tags_label"]
                                gtk::Label {
                                    set_label: &i18n("Find Tags…"),
                                },
                            },
                        },
                    },

                    #[name = "tags_list"]
                    gtk::ListBox {
                        add_css_class: "boxed-list",
                        set_selection_mode: gtk::SelectionMode::None,
                    },
                },
            }
        }
        Self {
            page,
            find_tags_spinner,
            find_tags_label,
            tags_list,
        }
    }
}
