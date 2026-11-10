use gtk::glib;

#[glib::flags(name = "HighlightFlags")]
pub enum HighlightFlags {
    HIGHLIGHT = 0b0000_0001,
    BOLD = 0b0000_0010,
}

impl Default for HighlightFlags {
    fn default() -> Self {
        HighlightFlags::empty()
    }
}
