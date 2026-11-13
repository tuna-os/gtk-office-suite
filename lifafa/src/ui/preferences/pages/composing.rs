//! Settings → Composing: format, reply and signature placement, undo
//! send, spelling and grammar.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use super::super::{PrefInput, PrefOutput, ViewSender};
use crate::i18n::i18n;
use crate::mail::attachment_reminder::{normalized, DEFAULT_KEYWORDS};

/// The page, and the rows on it that the window reaches later.
#[derive(Debug)]
pub(in crate::ui::preferences) struct ComposingPage {
    pub(in crate::ui::preferences) page: adw::PreferencesPage,
    pub(in crate::ui::preferences) compose_inline_row: adw::SwitchRow,
    pub(in crate::ui::preferences) reply_inline_row: adw::SwitchRow,
    pub(in crate::ui::preferences) reply_fields_row: adw::SwitchRow,
    pub(in crate::ui::preferences) default_from_row: adw::ComboRow,
    pub(in crate::ui::preferences) paste_plain_row: adw::SwitchRow,
    pub(in crate::ui::preferences) return_paragraph_row: adw::SwitchRow,
    pub(in crate::ui::preferences) toolbar_row: adw::ComboRow,
    pub(in crate::ui::preferences) compose_format_row: adw::ComboRow,
    pub(in crate::ui::preferences) compose_font_row: adw::ComboRow,
    pub(in crate::ui::preferences) compose_size_row: adw::ComboRow,
    pub(in crate::ui::preferences) compose_zoom_row: adw::ComboRow,
    pub(in crate::ui::preferences) follow_plain_row: adw::SwitchRow,
    pub(in crate::ui::preferences) ask_plain_switch_row: adw::SwitchRow,
    pub(in crate::ui::preferences) signature_position_row: adw::ComboRow,
    pub(in crate::ui::preferences) signature_dashes_row: adw::SwitchRow,
    pub(in crate::ui::preferences) fold_quote_row: adw::SwitchRow,
    pub(in crate::ui::preferences) undo_send_row: adw::ComboRow,
    pub(in crate::ui::preferences) request_receipts_row: adw::SwitchRow,
    pub(in crate::ui::preferences) returned_receipts_row: adw::ComboRow,
    pub(in crate::ui::preferences) reply_header_row: adw::EntryRow,
    /// The reply header as last saved, as the row shows it: the apply
    /// button is up only while the text differs from it.
    reply_header_saved: Rc<RefCell<String>>,
    pub(in crate::ui::preferences) attachment_keywords_row: adw::EntryRow,
    /// The attachment reminder's words as last saved: the apply button is
    /// up only while the text differs from them.
    attachment_keywords_saved: Rc<RefCell<String>>,
    pub(in crate::ui::preferences) spelling_group: adw::PreferencesGroup,
    pub(in crate::ui::preferences) spellcheck_row: adw::SwitchRow,
    pub(in crate::ui::preferences) spell_lang_row: adw::ExpanderRow,
    pub(in crate::ui::preferences) spell_words_row: adw::ExpanderRow,
}

impl ComposingPage {
    pub(in crate::ui::preferences) fn build(sender: &ViewSender) -> Self {
        let page = adw::PreferencesPage::default();
        let preferences_group_1 = adw::PreferencesGroup::default();
        preferences_group_1.set_title(&i18n("Composing"));
        let compose_inline_row = adw::SwitchRow::default();
        compose_inline_row.set_title(&i18n("Compose in the main window"));
        compose_inline_row.set_subtitle(&i18n("Off opens new messages in their own window."));
        {
            let sender = sender.clone();
            compose_inline_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetComposeInline(row.is_active()));
            });
        }
        preferences_group_1.add(&compose_inline_row);
        let reply_inline_row = adw::SwitchRow::default();
        reply_inline_row.set_title(&i18n("Reply and forward in the main window"));
        {
            let sender = sender.clone();
            reply_inline_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetReplyInline(row.is_active()));
            });
        }
        preferences_group_1.add(&reply_inline_row);
        let reply_fields_row = adw::SwitchRow::default();
        reply_fields_row.set_title(&i18n("Show From, To and Subject in the reply panel"));
        reply_fields_row.set_subtitle(&i18n("Off folds them behind a button."));
        {
            let sender = sender.clone();
            reply_fields_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetReplyFields(row.is_active()));
            });
        }
        preferences_group_1.add(&reply_fields_row);
        let default_from_row = adw::ComboRow::default();
        default_from_row.set_title(&i18n("Send new messages from"));
        default_from_row.set_subtitle(&i18n("Replies answer from the address they were sent to."));
        {
            let sender = sender.clone();
            default_from_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeComposeDefaultFrom(row.selected()));
            });
        }
        preferences_group_1.add(&default_from_row);
        let paste_plain_row = adw::SwitchRow::default();
        paste_plain_row.set_title(&i18n("Paste as plain text"));
        paste_plain_row.set_subtitle(&i18n("Right-clicking the editor offers both either way."));
        {
            let sender = sender.clone();
            paste_plain_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetPastePlain(row.is_active()));
            });
        }
        preferences_group_1.add(&paste_plain_row);
        let return_paragraph_row = adw::SwitchRow::default();
        return_paragraph_row.set_title(&i18n("Return starts a new paragraph"));
        return_paragraph_row.set_subtitle(&i18n("Shift+Return does the other."));
        {
            let sender = sender.clone();
            return_paragraph_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetReturnParagraph(row.is_active()));
            });
        }
        preferences_group_1.add(&return_paragraph_row);
        let toolbar_row = adw::ComboRow::default();
        toolbar_row.set_title(&i18n("Formatting toolbar"));
        toolbar_row.set_subtitle(&i18n("For new messages. The toolbar's chevron toggles it."));
        toolbar_row.set_model(Some(&gtk::StringList::new(&[
            &i18n("Always collapsed"),
            &i18n("Always expanded"),
        ])));
        {
            let sender = sender.clone();
            toolbar_row.connect_selected_notify(move |row| {
                let _ = sender.output(PrefOutput::SetToolbarExpanded(row.selected() == 1));
            });
        }
        preferences_group_1.add(&toolbar_row);
        let compose_format_row = adw::ComboRow::default();
        compose_format_row.set_title(&i18n("Start new messages in"));
        {
            let sender = sender.clone();
            compose_format_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeComposeFormat(row.selected()));
            });
        }
        preferences_group_1.add(&compose_format_row);
        let follow_plain_row = adw::SwitchRow::default();
        follow_plain_row.set_title(&i18n("Reply in the original's format"));
        {
            let sender = sender.clone();
            follow_plain_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetFollowPlain(row.is_active()));
            });
        }
        preferences_group_1.add(&follow_plain_row);
        let ask_plain_switch_row = adw::SwitchRow::default();
        ask_plain_switch_row.set_title(&i18n("Ask before switching to plain text"));
        ask_plain_switch_row.set_subtitle(&i18n("Only when formatting, images or links would be removed."));
        {
            let sender = sender.clone();
            ask_plain_switch_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetAskPlainSwitch(row.is_active()));
            });
        }
        preferences_group_1.add(&ask_plain_switch_row);
        // Filled and connected in init: the
        // list grows with the fonts picked
        // through Other Font….
        let compose_font_row = adw::ComboRow::default();
        compose_font_row.set_title(&i18n("Font"));
        compose_font_row.set_subtitle(&i18n(
            "For rich text. Default leaves it to the recipient's own font.",
        ));
        preferences_group_1.add(&compose_font_row);
        let compose_size_row = adw::ComboRow::default();
        compose_size_row.set_title(&i18n("Font size"));
        compose_size_row.set_subtitle(&i18n("For rich text, in points."));
        preferences_group_1.add(&compose_size_row);
        let compose_zoom_row = adw::ComboRow::default();
        compose_zoom_row.set_title(&i18n("Default zoom"));
        preferences_group_1.add(&compose_zoom_row);
        let signature_position_row = adw::ComboRow::default();
        signature_position_row.set_title(&i18n("Signature in replies"));
        {
            let sender = sender.clone();
            signature_position_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeSignaturePosition(row.selected()));
            });
        }
        preferences_group_1.add(&signature_position_row);
        let signature_dashes_row = adw::SwitchRow::default();
        signature_dashes_row.set_title(&i18n("Separator line above the signature"));
        signature_dashes_row.set_subtitle(&i18n(
            "The standard “-- ” line, which other mail apps recognize.",
        ));
        {
            let sender = sender.clone();
            signature_dashes_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetSignatureDashes(row.is_active()));
            });
        }
        preferences_group_1.add(&signature_dashes_row);
        let fold_quote_row = adw::SwitchRow::default();
        fold_quote_row.set_title(&i18n("Fold quoted text"));
        fold_quote_row.set_subtitle(&i18n(
            "Replies and forwards open with the original behind •••.",
        ));
        {
            let sender = sender.clone();
            fold_quote_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetFoldQuote(row.is_active()));
            });
        }
        preferences_group_1.add(&fold_quote_row);
        let undo_send_row = adw::ComboRow::default();
        undo_send_row.set_title(&i18n("Undo Send"));
        undo_send_row.set_subtitle(&i18n("How long Undo is offered after sending."));
        {
            let sender = sender.clone();
            undo_send_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeUndoSend(row.selected()));
            });
        }
        preferences_group_1.add(&undo_send_row);
        let request_receipts_row = adw::SwitchRow::default();
        request_receipts_row.set_title(&i18n("Request read receipts"));
        request_receipts_row.set_subtitle(&i18n("Not every mail app sends one."));
        {
            let sender = sender.clone();
            request_receipts_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetRequestReceipts(row.is_active()));
            });
        }
        preferences_group_1.add(&request_receipts_row);
        let returned_receipts_row = adw::ComboRow::default();
        returned_receipts_row.set_title(&i18n("Returned receipts"));
        returned_receipts_row
            .set_subtitle(&i18n("Sent mail shows who read it; the receipt goes here."));
        returned_receipts_row.set_model(Some(&gtk::StringList::new(&[
            &i18n("Move to Trash"),
            &i18n("Archive"),
            &i18n("Keep in Inbox"),
        ])));
        {
            let sender = sender.clone();
            returned_receipts_row.connect_selected_notify(move |row| {
                let _ = sender.output(PrefOutput::SetReturnedReceipts(
                    crate::ui::preferences::choices::returned_receipts_at(row.selected()),
                ));
            });
        }
        preferences_group_1.add(&returned_receipts_row);
        page.add(&preferences_group_1);
        let preferences_group_2 = adw::PreferencesGroup::default();
        preferences_group_2.set_title(&i18n("Reply Header"));
        preferences_group_2.set_description(Some(&i18n(
            "{name}, {address}, {date} and {time} are filled in from the original.",
        )));
        // Saved on Apply; the text is set and wired in build
        // below, beside the button that restores the default.
        let reply_header_row = adw::EntryRow::default();
        reply_header_row.set_title(&i18n("Line above the quoted message"));
        reply_header_row.set_show_apply_button(true);
        let reply_header_reset = gtk::Button::default();
        reply_header_reset.set_icon_name("edit-undo-symbolic");
        reply_header_reset.set_valign(gtk::Align::Center);
        reply_header_reset.set_tooltip_text(Some(i18n("Restore Default").as_str()));
        reply_header_reset.add_css_class("flat");
        reply_header_row.add_suffix(&reply_header_reset);
        preferences_group_2.add(&reply_header_row);
        page.add(&preferences_group_2);
        let preferences_group_3 = adw::PreferencesGroup::default();
        preferences_group_3.set_title(&i18n("Attachment Reminder"));
        preferences_group_3.set_description(Some(&i18n(
            "Send asks first when the message mentions one of these words and nothing is attached.",
        )));
        // Saved on Apply; the text is set and wired in build
        // below, beside the button that restores the default.
        let attachment_keywords_row = adw::EntryRow::default();
        attachment_keywords_row.set_title(&i18n("Words, separated by commas"));
        attachment_keywords_row.set_show_apply_button(true);
        let attachment_keywords_reset = gtk::Button::default();
        attachment_keywords_reset.set_icon_name("edit-undo-symbolic");
        attachment_keywords_reset.set_valign(gtk::Align::Center);
        attachment_keywords_reset.set_tooltip_text(Some(i18n("Restore Default").as_str()));
        attachment_keywords_reset.add_css_class("flat");
        attachment_keywords_row.add_suffix(&attachment_keywords_reset);
        preferences_group_3.add(&attachment_keywords_row);
        page.add(&preferences_group_3);
        let spelling_group = adw::PreferencesGroup::default();
        spelling_group.set_title(&i18n("Spelling"));
        // The description is filled in at init with the
        // dictionaries the app can actually see — checking a
        // language without one silently checks nothing, so
        // honesty about what is installed beats silence.
        let spellcheck_row = adw::SwitchRow::default();
        spellcheck_row.set_title(&i18n("Check spelling as you type"));
        spellcheck_row.set_subtitle(&i18n("Right-click a word for corrections."));
        {
            let sender = sender.clone();
            spellcheck_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetSpellcheck(row.is_active()));
            });
        }
        spelling_group.add(&spellcheck_row);
        // One switch per installed dictionary, filled
        // in at init (#365); the subtitle names the
        // languages in use.
        let spell_lang_row = adw::ExpanderRow::default();
        spell_lang_row.set_title(&i18n("Languages"));
        spelling_group.add(&spell_lang_row);
        // Words the user taught the checker ("Learn Spelling"
        // in the composer, or added right here). Populated
        // and maintained imperatively in init.
        let spell_words_row = adw::ExpanderRow::default();
        spell_words_row.set_title(&i18n("Added words"));
        spell_words_row.set_subtitle(&i18n("Changes apply after a restart."));
        spelling_group.add(&spell_words_row);
        page.add(&spelling_group);
        page.add(&super::super::grammar::group());
        let reply_header_saved = Rc::new(RefCell::new(String::new()));
        wire_reply_header(
            &reply_header_row,
            &reply_header_reset,
            &reply_header_saved,
            sender,
        );
        let attachment_keywords_saved = Rc::new(RefCell::new(String::new()));
        wire_attachment_keywords(
            &attachment_keywords_row,
            &attachment_keywords_reset,
            &attachment_keywords_saved,
            sender,
        );
        Self {
            page,
            compose_inline_row,
            reply_inline_row,
            reply_fields_row,
            default_from_row,
            paste_plain_row,
            return_paragraph_row,
            toolbar_row,
            compose_format_row,
            compose_font_row,
            compose_size_row,
            compose_zoom_row,
            follow_plain_row,
            ask_plain_switch_row,
            signature_position_row,
            signature_dashes_row,
            fold_quote_row,
            undo_send_row,
            request_receipts_row,
            returned_receipts_row,
            reply_header_row,
            reply_header_saved,
            attachment_keywords_row,
            attachment_keywords_saved,
            spelling_group,
            spellcheck_row,
            spell_lang_row,
            spell_words_row,
        }
    }
}

impl ComposingPage {
    /// Show the saved reply header, "" for the default.
    pub(in crate::ui::preferences) fn set_reply_header(&self, stored: &str) {
        let text = shown_template(stored);
        set_quietly(&self.reply_header_row, &text);
        *self.reply_header_saved.borrow_mut() = text;
    }

    /// Show the attachment reminder's saved words.
    pub(in crate::ui::preferences) fn set_attachment_keywords(&self, stored: &str) {
        let text = normalized(stored);
        set_quietly(&self.attachment_keywords_row, &text);
        *self.attachment_keywords_saved.borrow_mut() = text;
    }
}

/// The text the row shows for a stored header, "" being the default.
fn shown_template(stored: &str) -> String {
    if stored.is_empty() {
        crate::mail::attribution::default_template()
    } else {
        stored.to_string()
    }
}

/// Set the row's text without raising its apply button, which libadwaita
/// raises for any change made while the row has focus, the app's own
/// included: the text put back after Apply looked like an edit not yet
/// saved.
fn set_quietly(row: &adw::EntryRow, text: &str) {
    row.set_show_apply_button(false);
    if row.text() != text {
        row.set_text(text);
    }
    row.set_show_apply_button(true);
}

/// The reply header row saves what Apply confirms, and its button goes back
/// to the default, shown only while the text is something else. The apply
/// button goes down again when the text is edited back to what is saved.
fn wire_reply_header(
    row: &adw::EntryRow,
    reset: &gtk::Button,
    saved: &Rc<RefCell<String>>,
    sender: &ViewSender,
) {
    use crate::mail::attribution::stored_template;
    {
        let (reset, saved) = (reset.clone(), saved.clone());
        row.connect_changed(move |row| {
            reset.set_visible(!stored_template(&row.text()).is_empty());
            if row.text().trim() == saved.borrow().trim() {
                // After the row's own handler, which raises the button.
                let (row, saved) = (row.clone(), saved.clone());
                gtk::glib::idle_add_local_once(move || {
                    if row.text().trim() == saved.borrow().trim() {
                        row.set_show_apply_button(false);
                        row.set_show_apply_button(true);
                    }
                });
            }
        });
    }
    {
        let (saved, sender) = (saved.clone(), sender.clone());
        row.connect_apply(move |row| {
            let stored = stored_template(&row.text());
            let text = shown_template(&stored);
            set_quietly(row, &text);
            *saved.borrow_mut() = text;
            let _ = sender.output(PrefOutput::SetReplyHeader(stored));
        });
    }
    {
        let (row, saved, sender) = (row.clone(), saved.clone(), sender.clone());
        reset.connect_clicked(move |_| {
            let text = shown_template("");
            set_quietly(&row, &text);
            *saved.borrow_mut() = text;
            let _ = sender.output(PrefOutput::SetReplyHeader(String::new()));
        });
    }
}

/// The attachment reminder row saves what Apply confirms, and its button
/// puts the default words back, shown only while the text is something
/// else. The apply button goes down again when the text is edited back to
/// what is saved.
fn wire_attachment_keywords(
    row: &adw::EntryRow,
    reset: &gtk::Button,
    saved: &Rc<RefCell<String>>,
    sender: &ViewSender,
) {
    {
        let (reset, saved) = (reset.clone(), saved.clone());
        row.connect_changed(move |row| {
            reset.set_visible(normalized(&row.text()) != DEFAULT_KEYWORDS);
            if normalized(&row.text()) == *saved.borrow() {
                // After the row's own handler, which raises the button.
                let (row, saved) = (row.clone(), saved.clone());
                gtk::glib::idle_add_local_once(move || {
                    if normalized(&row.text()) == *saved.borrow() {
                        row.set_show_apply_button(false);
                        row.set_show_apply_button(true);
                    }
                });
            }
        });
    }
    {
        let (saved, sender) = (saved.clone(), sender.clone());
        row.connect_apply(move |row| {
            let text = normalized(&row.text());
            set_quietly(row, &text);
            *saved.borrow_mut() = text.clone();
            let _ = sender.output(PrefOutput::SetAttachmentKeywords(text));
        });
    }
    {
        let (row, saved, sender) = (row.clone(), saved.clone(), sender.clone());
        reset.connect_clicked(move |_| {
            set_quietly(&row, DEFAULT_KEYWORDS);
            *saved.borrow_mut() = DEFAULT_KEYWORDS.to_string();
            let _ = sender.output(PrefOutput::SetAttachmentKeywords(
                DEFAULT_KEYWORDS.to_string(),
            ));
        });
    }
}
