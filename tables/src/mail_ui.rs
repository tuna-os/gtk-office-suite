//! mail_ui.rs — Send by Email: the workbook Save would write, attached to a
//! new message in the desktop's mail client (Lifafa on a suite install).
//! SPDX-License-Identifier: GPL-3.0-or-later

use adw::prelude::*;
use gtk4::gio;
use libadwaita as adw;
use std::{cell::RefCell, rc::Rc};
use tables_core::controller::WorkbookController;

type Ctl = Rc<RefCell<WorkbookController>>;

/// The workbook's title for the attachment: its file's stem, or "Workbook".
fn title(path: Option<&std::path::Path>) -> String {
    path.and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Workbook".into())
}

pub(crate) fn install(app: &impl IsA<gio::ActionMap>, ctl: &Ctl, window: &adw::ApplicationWindow) {
    let (ctl, window) = (ctl.clone(), window.clone());
    let act = gio::SimpleAction::new("send-by-email", None);
    act.connect_activate(move |_, _| {
        let (path, state) = {
            let c = ctl.borrow();
            let path = c.file_path.borrow().clone();
            (path, c.state.clone())
        };
        let name = suite_common::mail::attachment_name(&title(path.as_deref()), "xlsx");
        let dir = std::env::temp_dir().join(format!("tables-mail-{}", std::process::id()));
        let target = dir.join(&name);
        let source = path.as_ref().map(|p| p.to_string_lossy().to_string());
        let written = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|()| crate::persistence::save_engine_to_xlsx(&target.to_string_lossy(), source.as_deref(), &state.borrow()));
        let parent = window.clone();
        let fail = move |e: String| suite_common::show_error_dialog(Some(&parent), &suite_common::i18n("Could Not Send by Email"), &e);
        match written {
            Ok(()) => {
                let draft = suite_common::mail::MailDraft::for_document("Tables", &name, target);
                suite_common::mail::compose(&window, draft, move |result| {
                    if let Err(e) = result {
                        fail(e);
                    }
                });
            }
            Err(e) => fail(e),
        }
    });
    app.add_action(&act);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_is_the_file_stem_or_workbook() {
        assert_eq!(title(Some(std::path::Path::new("/home/a/Budget 2026.xlsx"))), "Budget 2026");
        assert_eq!(title(None), "Workbook");
    }
}
