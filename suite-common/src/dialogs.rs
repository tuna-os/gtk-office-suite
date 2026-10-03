// SPDX-License-Identifier: GPL-3.0-or-later
//
// The suite's dialogs, one builder for each kind docs/GNOME-GUIDELINES.md
// names: alerts (a failure, a question), prompts, action dialogs and
// viewers. The apps build their dialogs from these rather than from
// libadwaita directly, so a dialog looks and behaves the same in all three.

use crate::i18n;
use gtk4 as gtk;
use libadwaita as adw;
use adw::prelude::*;

/// Tell the user an operation failed, in a dialog they must acknowledge.
///
/// All three apps had places that reported a failure with `eprintln!` and
/// nothing else — most consequentially a document that would not open, where
/// the user is left looking at an empty window with no indication that
/// anything went wrong (#447). stderr is not a user interface: under a
/// Flatpak or a desktop launcher nobody ever sees it.
///
/// `heading` should name the failure in words that read as a failure; the
/// GUI journeys match on phrases like "could not" / "cannot open" / "failed
/// to", and an accessible name that avoids all of them is invisible to them.
pub fn show_error_dialog(parent: Option<&adw::ApplicationWindow>, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response("ok", &i18n("_OK"));
    dialog.set_default_response(Some("ok"));
    dialog.present(parent);
}

/// The question asked before closing documents with unsaved changes, the
/// same in all three apps: "Save Changes?", with Cancel, Discard and Save
/// (Discard All and Save All for several documents). Save is the default;
/// Discard is destructive; Escape cancels. The caller runs it with
/// `choose` and acts on "save", "discard" or "cancel".
///
/// Each app used to word it its own way: "Save document?" in Letters,
/// "Save changes?" in Tables and Decks, "Unsaved changes" for several.
pub fn save_changes_question(names: &[String]) -> adw::AlertDialog {
    let (body, discard, save) = match names {
        [name] => (
            i18n("“%s” has unsaved changes. Changes that are not saved will be lost.").replace("%s", name),
            i18n("_Discard"),
            i18n("_Save"),
        ),
        _ => (
            format!(
                "{}\n\n{}",
                i18n("These documents have unsaved changes. Changes that are not saved will be lost."),
                names.iter().map(|n| format!("• {n}")).collect::<Vec<_>>().join("\n")
            ),
            i18n("_Discard All"),
            i18n("_Save All"),
        ),
    };
    let dialog = adw::AlertDialog::new(Some(&i18n("Save Changes?")), Some(&body));
    dialog.add_responses(&[("cancel", &i18n("_Cancel")), ("discard", &discard), ("save", &save)]);
    dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");
    dialog
}

/// The name of the document a window shows, from its title ("Budget —
/// Tables" is "Budget"), for questions about it.
pub fn document_name(window: &impl IsA<gtk::Window>) -> String {
    let title = window.as_ref().title().map(|t| t.to_string()).unwrap_or_default();
    let name = title.split(" — ").next().unwrap_or("").trim_end_matches(" (Recovered)").trim();
    if name.is_empty() { i18n("Untitled") } else { name.to_string() }
}

/// Run `replace`, which swaps the window's document for another one (Open,
/// New, a template, a file handed over by the file manager), but ask first
/// when the current document has unsaved changes. Tables and Decks hold one
/// document per window, and every one of those paths used to replace a
/// dirty document without a word, losing the edits.
///
/// Cancel is the default and the close response: dismissing the dialog
/// keeps the document. `kind` names the document ("workbook",
/// "presentation") in the question.
pub fn confirm_discarding(parent: &adw::ApplicationWindow, dirty: bool, kind: &str, replace: impl FnOnce() + 'static) {
    if !dirty {
        replace();
        return;
    }
    let dialog = adw::AlertDialog::new(
        Some(&i18n("Discard Unsaved Changes?")),
        Some(&i18n("This %s has unsaved changes. Opening another one replaces it, and the changes will be lost.").replace("%s", &i18n(kind))),
    );
    dialog.add_response("cancel", &i18n("_Cancel"));
    dialog.add_response("discard", &i18n("_Discard"));
    dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.choose(Some(parent), None::<&gtk::gio::Cancellable>, move |response| {
        if response == "discard" {
            replace();
        }
    });
}

/// A prompt: an alert that asks for one line of text (Rename Sheet, Insert
/// Link, Go to Cell). Cancel and `action` are its responses; `action` is
/// the default, so Enter in the field answers, and Escape cancels. The
/// field has keyboard focus when the dialog opens and is named `field`
/// for screen readers.
///
/// Every prompt in the suite comes from here, so they look and behave the
/// same: before, some focused their field and some left focus on the
/// buttons, some had mnemonics and some didn't, and only some cancelled on
/// Escape. A form with more than one field is an [`action_dialog`].
pub struct Prompt {
    pub dialog: adw::AlertDialog,
    pub entry: gtk::Entry,
}

/// The response id of a prompt's action.
pub const PROMPT_ACTION: &str = "action";

pub fn prompt(heading: &str, body: Option<&str>, field: &str, text: &str, action: &str) -> Prompt {
    let dialog = adw::AlertDialog::new(Some(heading), body);
    let entry = gtk::Entry::builder().text(text).activates_default(true).build();
    entry.update_property(&[gtk::accessible::Property::Label(field)]);
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", &i18n("_Cancel")), (PROMPT_ACTION, action)]);
    dialog.set_response_appearance(PROMPT_ACTION, adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some(PROMPT_ACTION));
    dialog.set_close_response("cancel");
    Prompt { dialog, entry }
}

impl Prompt {
    /// Show the prompt over `parent`. `done` gets the field's text when the
    /// action is chosen, and `None` when the prompt is cancelled.
    pub fn present(self, parent: Option<&impl IsA<gtk::Widget>>, done: impl Fn(Option<String>) + 'static) {
        let Prompt { dialog, entry } = self;
        let field = entry.clone();
        dialog.connect_response(None, move |_, response| {
            done((response == PROMPT_ACTION).then(|| field.text().to_string()));
        });
        dialog.present(parent);
        // Typing goes into the field, not onto the buttons.
        dialog.set_focus(Some(&entry));
        entry.select_region(0, -1);
    }
}

/// A dialog that collects settings before doing something (Page Setup,
/// Format Cells, Choose a Theme): a header bar with the title, Cancel at
/// its start and `action` (a verb, with a mnemonic) at its end, above
/// `content`. The action is the default widget, so Enter does it, and
/// Escape cancels, as in GNOME's own action dialogs.
///
/// Every action dialog in the suite comes from here. Before, Letters asked
/// for its header and footer in an alert, Tables' forms had no header bar
/// and a full-width button below the form, the chart dialog had its button
/// at the bottom right, and the template picker had no buttons at all.
pub struct ActionDialog {
    pub dialog: adw::Dialog,
    pub action: gtk::Button,
}

pub fn action_dialog(title: &str, action: &str, width: i32, content: &impl IsA<gtk::Widget>) -> ActionDialog {
    let dialog = adw::Dialog::builder().title(title).content_width(width).build();
    let action = gtk::Button::with_mnemonic(action);
    action.add_css_class("suggested-action");
    let cancel = gtk::Button::with_mnemonic(&i18n("_Cancel"));
    {
        let d = dialog.clone();
        cancel.connect_clicked(move |_| {
            d.close();
        });
    }
    let header = adw::HeaderBar::builder().show_start_title_buttons(false).show_end_title_buttons(false).build();
    header.pack_start(&cancel);
    header.pack_end(&action);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(content));
    dialog.set_child(Some(&view));
    dialog.set_default_widget(Some(&action));
    ActionDialog { dialog, action }
}

/// A dialog that only shows something (Keyboard Shortcuts): a header bar
/// with the title and a close button, above `content`.
pub fn viewer_dialog(title: &str, width: i32, height: i32, content: &impl IsA<gtk::Widget>) -> adw::Dialog {
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(content));
    adw::Dialog::builder().title(title).content_width(width).content_height(height).child(&view).build()
}

/// The body of a form: its groups of rows, one above the other, with the
/// suite's spacing (12 above, 24 at the sides and below, 24 between
/// groups). Fields are libadwaita rows: `adw::EntryRow` for text,
/// `adw::SpinRow` for a number, `adw::ComboRow` for a choice,
/// `adw::SwitchRow` for on and off, and an `adw::ActionRow` with the
/// control as its suffix only for a control no row type holds (a colour).
pub fn form_body(groups: &[&gtk::Widget]) -> gtk::Box {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 24);
    body.set_margin_top(12);
    body.set_margin_bottom(24);
    body.set_margin_start(24);
    body.set_margin_end(24);
    for group in groups {
        body.append(*group);
    }
    body
}

/// A form's fields as rows of a boxed list, each named by its row's title
/// with the field at the row's end: the libadwaita form, for fields that
/// are not themselves rows (a drop-down, a spin button, a colour button).
/// Returns the list inside a padded box, to which a dialog can append
/// more (an error label).
pub fn form_rows(rows: &[(&str, &gtk::Widget)]) -> gtk::Box {
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    for (title, field) in rows {
        let row = adw::ActionRow::builder().title(*title).build();
        field.set_valign(gtk::Align::Center);
        row.add_suffix(*field);
        list.append(&row);
    }
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_top(12);
    body.set_margin_bottom(24);
    body.set_margin_start(24);
    body.set_margin_end(24);
    body.append(&list);
    body
}
