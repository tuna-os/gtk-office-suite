//! What the settings window asks of the panel: closing, leaving and saving
//! the page that is open.

use super::*;

impl AccountsWindow {
    pub(super) fn on_close_editor(&mut self, widgets: &AccountsWindowWidgets) {
        if widgets
            .nav
            .visible_page()
            .and_then(|p| p.tag())
            .is_some_and(|t| t == "editor" || t == "filter" || t == "tag")
        {
            widgets.nav.pop();
        }
    }

    pub(super) fn on_leave_request(
        &mut self,
        page: String,
        widgets: &AccountsWindowWidgets,
        sender: ComponentSender<Self>,
    ) {
        match widgets.nav.visible_page().and_then(|p| p.tag()).as_deref() {
            // The account editor knows what it was opened with, so it
            // can tell an edited form from an untouched one. The
            // signature lives in a WebView and only answers when
            // asked, so that answer arrives separately.
            Some("editor") => {
                if self.editor_touched(widgets) {
                    sender.input_sender().emit(AccountsInput::LeaveVerdict {
                        page,
                        touched: true,
                    });
                } else {
                    let s = sender.clone();
                    self.sig_editor(widgets).is_dirty(move |touched| {
                        s.input_sender()
                            .emit(AccountsInput::LeaveVerdict { page, touched });
                    });
                }
            }
            // Nothing records what a filter or a tag was opened with,
            // so those are always worth asking about.
            Some("filter") | Some("tag") => {
                sender.input_sender().emit(AccountsInput::LeaveVerdict {
                    page,
                    touched: true,
                })
            }
            _ => sender.input_sender().emit(AccountsInput::LeaveVerdict {
                page,
                touched: false,
            }),
        }
    }

    pub(super) fn on_leave_verdict(
        &mut self,
        page: String,
        touched: bool,
        sender: ComponentSender<Self>,
    ) {
        if touched {
            let _ = sender.output(AccountsOutput::LeaveNeedsPrompt(page));
            return;
        }
        // Nothing to lose: close the editor and let the window move on.
        sender.input_sender().emit(AccountsInput::CloseEditor);
        let _ = sender.output(AccountsOutput::LeftEditor(page));
    }

    pub(super) fn on_save_open_page(
        &mut self,
        widgets: &AccountsWindowWidgets,
        sender: ComponentSender<Self>,
    ) {
        match widgets.nav.visible_page().and_then(|p| p.tag()).as_deref() {
            Some("editor") => sender.input_sender().emit(AccountsInput::Save),
            Some("filter") | Some("tag") => {
                let saved = self.form_save.borrow().as_ref().is_some_and(|f| f());
                if saved {
                    widgets.nav.pop();
                }
            }
            _ => {}
        }
    }
}
