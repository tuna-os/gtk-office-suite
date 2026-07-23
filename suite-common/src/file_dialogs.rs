// SPDX-License-Identifier: GPL-3.0-or-later
//
// FileDialogHelper — shared open/save/export file dialog helpers
// with standard office format filters. Used by Letters, Tables, Decks.

use gtk4::{self as gtk, gio, prelude::*};
use libadwaita as adw;
use std::path::PathBuf;

/// Standard office format file filters.
pub fn docx_filter() -> gtk::FileFilter {
    let f = gtk::FileFilter::new();
    f.add_pattern("*.docx");
    f.set_name(Some("Word Document (.docx)"));
    f
}

pub fn md_filter() -> gtk::FileFilter {
    let f = gtk::FileFilter::new();
    f.add_pattern("*.md");
    f.set_name(Some("Markdown (.md)"));
    f
}

pub fn txt_filter() -> gtk::FileFilter {
    let f = gtk::FileFilter::new();
    f.add_pattern("*.txt");
    f.set_name(Some("Plain Text (.txt)"));
    f
}

pub fn html_filter() -> gtk::FileFilter {
    let f = gtk::FileFilter::new();
    f.add_pattern("*.html");
    f.add_pattern("*.htm");
    f.set_name(Some("HTML (.html)"));
    f
}

pub fn pdf_filter() -> gtk::FileFilter {
    let f = gtk::FileFilter::new();
    f.add_pattern("*.pdf");
    f.set_name(Some("PDF (.pdf)"));
    f
}

pub fn odt_filter() -> gtk::FileFilter {
    let f = gtk::FileFilter::new();
    f.add_pattern("*.odt");
    f.set_name(Some("OpenDocument (.odt)"));
    f
}

/// All document formats for open dialogs.
pub fn all_documents_filter() -> gtk::FileFilter {
    let f = gtk::FileFilter::new();
    f.add_pattern("*.md");
    f.add_pattern("*.txt");
    f.add_pattern("*.html");
    f.add_pattern("*.htm");
    f.add_pattern("*.docx");
    f.set_name(Some("All Documents"));
    f
}

/// Helper for showing open/save/export file dialogs.
pub struct FileDialogHelper {
    parent: adw::ApplicationWindow,
}

impl FileDialogHelper {
    pub fn new(parent: &adw::ApplicationWindow) -> Self {
        Self { parent: parent.clone() }
    }

    /// Show open dialog with specified filters, return selected path.
    pub fn open<F>(&self, filters: &[gtk::FileFilter], callback: F)
    where
        F: Fn(Option<PathBuf>) + 'static,
    {
        let dlg = gtk::FileDialog::new();
        if !filters.is_empty() {
            let fl = gio::ListStore::new::<gtk::FileFilter>();
            for f in filters {
                fl.append(f);
            }
            dlg.set_filters(Some(&fl));
        }
        let w = self.parent.clone();
        dlg.open(Some(&w), None::<&gio::Cancellable>, move |result| {
            callback(result.ok().and_then(|f| f.path()))
        });
    }

    /// Show save dialog, return selected path.
    pub fn save<F>(&self, suggested_name: &str, filters: &[gtk::FileFilter], callback: F)
    where
        F: Fn(Option<PathBuf>) + 'static,
    {
        let dlg = gtk::FileDialog::new();
        dlg.set_initial_name(Some(suggested_name));
        if !filters.is_empty() {
            let fl = gio::ListStore::new::<gtk::FileFilter>();
            for f in filters {
                fl.append(f);
            }
            dlg.set_filters(Some(&fl));
        }
        let w = self.parent.clone();
        dlg.save(Some(&w), None::<&gio::Cancellable>, move |result| {
            callback(result.ok().and_then(|f| f.path()))
        });
    }

    /// Show export dialog (doesn't update current file tracking).
    pub fn export<F>(&self, suggested_name: &str, filters: &[gtk::FileFilter], callback: F)
    where
        F: Fn(Option<PathBuf>) + 'static,
    {
        self.save(suggested_name, filters, callback)
    }
}
