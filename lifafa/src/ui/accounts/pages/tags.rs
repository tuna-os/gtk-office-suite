//! Settings → Tags: the tag list, Add Tag and the tag finder.

use adw::prelude::*;
use crate::relm::prelude::*;

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
        let page = adw::PreferencesPage::default();
        // Tags (#71): a name and color per keyword.
        let preferences_group_1 = adw::PreferencesGroup::default();
        preferences_group_1.set_title(&i18n("Tags"));
        preferences_group_1.set_description(Some(
                        i18n("Tags are stored on the server as IMAP keywords, so Thunderbird and other clients show them too.\n\nDrag to reorder. The first nine are mapped to the 1–9 keys.").as_str()
                    ));
        // Stacked like the Cloud Storage panel's
        // buttons: a column at the header's end.
        let box_2 = gtk::Box::default();
        box_2.set_orientation(gtk::Orientation::Vertical);
        box_2.set_spacing(12);
        box_2.set_valign(gtk::Align::Start);
        box_2.set_halign(gtk::Align::End);
        box_2.set_margin_start(24);
        // Both 130px wide (filling the column,
        // so they always share one width), and
        // the finder's label change moves nothing.
        let button_3 = gtk::Button::default();
        button_3.set_label(&i18n("Add Tag…"));
        button_3.set_size_request(130, -1);
        {
            let sender = sender.clone();
            button_3.connect_clicked(move |_| sender.input(AccountsInput::AddTag));
        }
        box_2.append(&button_3);
        // The tag finder: read every mailbox
        // for keywords already in use and
        // offer them as tags.
        let find_tags_btn = gtk::Button::default();
        find_tags_btn.set_tooltip_text(Some(
            i18n("Look through every mailbox for tags other clients have set").as_str(),
        ));
        find_tags_btn.set_size_request(130, -1);
        {
            let sender = sender.clone();
            find_tags_btn.connect_clicked(move |_| sender.input(AccountsInput::FindTags));
        }
        // A spinner inside the button
        // while the mailboxes are read.
        let box_4 = gtk::Box::default();
        box_4.set_spacing(6);
        box_4.set_halign(gtk::Align::Center);
        let find_tags_spinner = gtk::Spinner::default();
        find_tags_spinner.set_visible(false);
        box_4.append(&find_tags_spinner);
        let find_tags_label = gtk::Label::default();
        find_tags_label.set_label(&i18n("Find Tags…"));
        box_4.append(&find_tags_label);
        find_tags_btn.set_child(Some(&box_4));
        box_2.append(&find_tags_btn);
        preferences_group_1.set_header_suffix(Some(&box_2));
        let tags_list = gtk::ListBox::default();
        tags_list.add_css_class("boxed-list");
        tags_list.set_selection_mode(gtk::SelectionMode::None);
        preferences_group_1.add(&tags_list);
        page.add(&preferences_group_1);
        Self {
            page,
            find_tags_spinner,
            find_tags_label,
            tags_list,
        }
    }
}
