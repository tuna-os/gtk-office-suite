use gtk4::gio;
use gio::prelude::*;
use suite_common_core::recent::RecentList;

const MAX_RECENT: usize = 10;
const KEY: &str = "recent-files";

/// App-local recent files backed by GSettings. It stores only URIs, caps the
/// list, prunes missing files, and exposes an explicit privacy clear action.
pub struct RecentFiles { settings: gio::Settings }

impl RecentFiles {
    pub fn new(settings: &gio::Settings) -> Self { Self { settings: settings.clone() } }
    pub fn entries(&self) -> Vec<gio::File> {
        let mut list = self.load();
        list.retain_existing(|uri| gio::File::for_uri(uri).query_exists(None::<&gio::Cancellable>));
        self.store(&list);
        list.entries().iter().map(|uri| gio::File::for_uri(uri)).collect()
    }
    pub fn record(&self, file: &gio::File) {
        let mut list = self.load();
        list.record(file.uri().to_string());
        self.store(&list);
    }
    pub fn remove(&self, file: &gio::File) {
        let mut list = self.load();
        list.remove(&file.uri());
        self.store(&list);
    }
    pub fn clear(&self) { self.store(&RecentList::new([], MAX_RECENT)); }
    fn load(&self) -> RecentList {
        RecentList::new(self.settings.strv(KEY).iter().cloned().collect::<Vec<_>>(), MAX_RECENT)
    }
    fn store(&self, list: &RecentList) {
        let entries: Vec<&str> = list.entries().iter().map(String::as_str).collect();
        let _ = self.settings.set_strv(KEY, &entries);
    }
}
