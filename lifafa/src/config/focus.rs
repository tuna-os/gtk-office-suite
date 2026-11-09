//! Focus Mode and focus.toml.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::{config_file, default_on, read_toml, save_plain_toml};

// ---- Focus Mode -----------------------------------------------------------

/// Focus Mode: a distraction-free layout for reading, switched on from the
/// main menu, Ctrl+Shift+F or Settings → Appearance. Each part can be left
/// out, so it strips exactly what its owner finds distracting. Stored in
/// focus.toml (like the toolbar layout) so the ordinary settings file and
/// its long save call stay as they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FocusMode {
    /// The master switch: on, the parts below apply.
    #[serde(default)]
    pub(crate) enabled: bool,
    /// Whether the app opens in Focus Mode. Not a part — it strips nothing
    /// itself, it decides what [`Self::enabled`] starts each launch as, so
    /// the mode is a deliberate choice rather than whatever the last
    /// session happened to leave behind.
    #[serde(default)]
    pub(crate) start_focused: bool,
    /// Every button of the reading pane's toolbar folds into its ⋯ menu.
    #[serde(default = "default_on")]
    pub(crate) reader_toolbar: bool,
    /// The message list's search, filters, count and sort fold into a ⋯
    /// menu; only the sidebar toggle stays.
    #[serde(default = "default_on")]
    pub(crate) list_header: bool,
    /// The sidebar's account sections are hidden.
    #[serde(default = "default_on")]
    pub(crate) hide_accounts: bool,
    /// The sidebar's unified rows (Inboxes, Starred, …) fold up.
    #[serde(default = "default_on")]
    pub(crate) fold_unified: bool,
    /// The message list's sender avatars are hidden.
    #[serde(default = "default_on")]
    pub(crate) hide_avatars: bool,
    /// The message list shows at most one line of preview text.
    #[serde(default = "default_on")]
    pub(crate) one_preview_line: bool,
    /// The message list shows no preview text at all. Wins over
    /// [`Self::one_preview_line`], which has nothing left to cap.
    #[serde(default = "default_on")]
    pub(crate) hide_preview: bool,
    /// The message list shows no subject either: the sender, the date and
    /// the row's own marks, and nothing else. Off unless it is asked for —
    /// the subject is how most people find a message again, so this one is
    /// a step past what the mode does by default.
    #[serde(default)]
    pub(crate) hide_subject: bool,
    /// The sidebar shows as the icon rail, with unread dots in place of the
    /// counts. The user's own rail choice is untouched: this is on screen
    /// for as long as Focus Mode is, and the sidebar comes back as it was.
    #[serde(default = "default_on")]
    pub(crate) rail_sidebar: bool,
    /// Every message opens in Reader View (the switch still works).
    #[serde(default = "default_on")]
    pub(crate) reader_view: bool,
}

impl Default for FocusMode {
    fn default() -> Self {
        FocusMode {
            enabled: false,
            start_focused: false,
            reader_toolbar: true,
            list_header: true,
            hide_accounts: true,
            fold_unified: true,
            hide_avatars: true,
            one_preview_line: true,
            hide_preview: true,
            hide_subject: false,
            rail_sidebar: true,
            reader_view: true,
        }
    }
}

/// One switch of Focus Mode, for Settings rows that all set the same struct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FocusPart {
    Enabled,
    /// Not a layout part: the launch switch (see [`FocusMode::start_focused`]).
    /// It rides here so Settings can set it the same way as the rest;
    /// [`FocusMode::active`] is meaningless for it and nothing asks.
    StartFocused,
    ReaderToolbar,
    ListHeader,
    HideAccounts,
    FoldUnified,
    HideAvatars,
    OnePreviewLine,
    HidePreview,
    HideSubject,
    RailSidebar,
    ReaderView,
}

impl FocusMode {
    pub(crate) fn get(&self, part: FocusPart) -> bool {
        match part {
            FocusPart::Enabled => self.enabled,
            FocusPart::StartFocused => self.start_focused,
            FocusPart::ReaderToolbar => self.reader_toolbar,
            FocusPart::ListHeader => self.list_header,
            FocusPart::HideAccounts => self.hide_accounts,
            FocusPart::FoldUnified => self.fold_unified,
            FocusPart::HideAvatars => self.hide_avatars,
            FocusPart::OnePreviewLine => self.one_preview_line,
            FocusPart::HidePreview => self.hide_preview,
            FocusPart::HideSubject => self.hide_subject,
            FocusPart::RailSidebar => self.rail_sidebar,
            FocusPart::ReaderView => self.reader_view,
        }
    }

    pub(crate) fn set(&mut self, part: FocusPart, on: bool) {
        match part {
            FocusPart::Enabled => self.enabled = on,
            FocusPart::StartFocused => self.start_focused = on,
            FocusPart::ReaderToolbar => self.reader_toolbar = on,
            FocusPart::ListHeader => self.list_header = on,
            FocusPart::HideAccounts => self.hide_accounts = on,
            FocusPart::FoldUnified => self.fold_unified = on,
            FocusPart::HideAvatars => self.hide_avatars = on,
            FocusPart::OnePreviewLine => self.one_preview_line = on,
            FocusPart::HidePreview => self.hide_preview = on,
            FocusPart::HideSubject => self.hide_subject = on,
            FocusPart::RailSidebar => self.rail_sidebar = on,
            FocusPart::ReaderView => self.reader_view = on,
        }
    }

    /// Whether `part` is in force: Focus Mode is on and the part is ticked.
    pub(crate) fn active(&self, part: FocusPart) -> bool {
        self.enabled && self.get(part)
    }
}

fn focus_path() -> Option<PathBuf> {
    config_file("focus.toml")
}

/// The saved Focus Mode settings, or the defaults (off) when there are none.
pub(crate) fn load_focus_mode() -> FocusMode {
    read_toml::<FocusMode>(focus_path()).unwrap_or_default()
}

pub(crate) fn save_focus_mode(focus: &FocusMode) {
    save_plain_toml(focus_path(), focus);
}

#[cfg(test)]
mod focus_tests {
    use super::*;

    #[test]
    fn focus_defaults_off_with_every_part_ticked() {
        let f = FocusMode::default();
        assert!(!f.enabled);
        // The app opens out of the mode until it is asked to do otherwise.
        assert!(!f.start_focused);
        assert!(f.reader_toolbar && f.list_header && f.hide_accounts && f.fold_unified);
        assert!(f.hide_avatars && f.one_preview_line && f.reader_view);
        assert!(f.hide_preview && f.rail_sidebar);
        // The exception: the subject stays until it is asked for.
        assert!(!f.hide_subject);
        // Off, no part is in force whatever its switch says.
        assert!(!f.active(FocusPart::ReaderView));
    }

    #[test]
    fn focus_round_trips_and_fills_missing_parts() {
        let mut f = FocusMode::default();
        f.set(FocusPart::Enabled, true);
        f.set(FocusPart::HideAvatars, false);
        let text = toml::to_string(&f).unwrap();
        let back: FocusMode = toml::from_str(&text).unwrap();
        assert_eq!(back, f);
        assert!(back.active(FocusPart::ReaderToolbar));
        assert!(!back.active(FocusPart::HideAvatars));
        // A file from before a part existed reads it as ticked — the
        // preview and rail parts included, so an upgrade gets the whole
        // mode without anyone going looking for the new switches.
        let partial: FocusMode = toml::from_str("enabled = true").unwrap();
        assert!(partial.enabled && partial.fold_unified);
        assert!(partial.hide_preview && partial.rail_sidebar);
        // Except the subject, which is off wherever it comes from.
        assert!(!partial.hide_subject);
    }
}
