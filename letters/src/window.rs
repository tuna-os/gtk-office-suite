use gtk4 as gtk;
use gtk::prelude::*;

pub struct LettersWindow { window: gtk::ApplicationWindow, editor: gtk::TextView }

impl LettersWindow {
    pub fn new(app: &gtk::Application) -> Self {
        let win = gtk::ApplicationWindow::builder().application(app).build();
        win.set_title(Some("Letters"));
        win.set_default_size(800, 600);

        let header = suite_common::make_header_bar();
        let toolbar = suite_common::make_toolbar();

        // Style dropdown
        let styles = gtk::DropDown::from_strings(&["Paragraph", "Heading 1", "Heading 2", "Heading 3", "Code", "Quote"]);
        toolbar.append(&styles);
        let table_btn = gtk::Button::with_label("Table");
        toolbar.append(&table_btn);

        let notebook = gtk::Notebook::new();
        let tab1 = gtk::Label::new(Some("📄 Document 1"));
        let editor = gtk::TextView::new();
        editor.set_monospace(true); editor.set_wrap_mode(gtk::WrapMode::Word);
        let s = gtk::ScrolledWindow::new(); s.set_child(Some(&editor)); s.set_vexpand(true);
        notebook.append_page(&s, Some(&tab1));

        let status = gtk::Label::new(Some("0 words"));
        status.set_halign(gtk::Align::End);

        let m = gtk::Box::new(gtk::Orientation::Vertical, 2);
        m.append(&toolbar); m.append(&notebook); m.append(&status);
        let c = gtk::Box::new(gtk::Orientation::Vertical, 0);
        c.append(&header); c.append(&m);
        win.set_child(Some(&c));

        // ── Keyboard shortcuts ──────────────────────────────────
        let key = gtk::EventControllerKey::new();
        let editor_ref = editor.clone();
        key.connect_key_pressed(move |_, keyval, keycode, state| {
            let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            let shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
            let alt = state.contains(gtk::gdk::ModifierType::ALT_MASK);

            if !ctrl && !alt { return gtk::glib::Propagation::Proceed; }

            let buf = editor_ref.buffer();
            match (ctrl, shift, alt, keyval) {
                // Bold / Italic / Underline
                (true, false, false, 0x62) => toggle_tag(&buf, "bold"),        // Ctrl+B
                (true, false, false, 0x69) => toggle_tag(&buf, "italic"),      // Ctrl+I
                (true, false, false, 0x75) => toggle_tag(&buf, "underline"),   // Ctrl+U
                // Alignment (Google Docs uses Ctrl+Shift+L/E/R/J)
                (true, true, false, 0x6c) => apply_align(&buf, "left"),        // Ctrl+Shift+L
                (true, true, false, 0x65) => apply_align(&buf, "center"),      // Ctrl+Shift+E
                (true, true, false, 0x72) => apply_align(&buf, "right"),       // Ctrl+Shift+R
                (true, true, false, 0x6a) => apply_align(&buf, "justify"),     // Ctrl+Shift+J
                // Lists (Ctrl+Shift+7/8)
                (true, true, false, 0x37) => insert_list(&buf, "bullet"),      // Ctrl+Shift+7
                (true, true, false, 0x38) => insert_list(&buf, "number"),      // Ctrl+Shift+8
                // Font size (Ctrl+Shift+. / Ctrl+Shift+,)
                (true, true, false, 0x2e) => change_font_size(&buf, 2),       // Ctrl+Shift+.
                (true, true, false, 0x2c) => change_font_size(&buf, -2),      // Ctrl+Shift+,
                // Indent / Outdent (Ctrl+] / Ctrl+[)
                (true, false, false, 0x5d) => change_indent(&buf, 20),        // Ctrl+]
                (true, false, false, 0x5b) => change_indent(&buf, -20),       // Ctrl+[
                // Heading styles (Ctrl+Alt+0-6)
                (true, false, true, k) if (0x30..=0x36).contains(&k) => {
                    let lvl = k - 0x30; apply_heading(&buf, lvl);
                }
                (true, false, false, 0x6b) => insert_link(&buf),              // Ctrl+K
                (true, false, false, 0x66) => show_find(&editor_ref),          // Ctrl+F
                (true, false, false, 0x68) => show_replace(&editor_ref),       // Ctrl+H
                _ => return gtk::glib::Propagation::Proceed,
            }
            gtk::glib::Propagation::Stop
        });
        editor.add_controller(key);

        Self { window: win, editor }
    }
    pub fn present(&self) { self.window.present(); }
}

fn toggle_tag(buf: &gtk::TextBuffer, tag_name: &str) {
    let tags = buf.tag_table();
    let tag = tags.lookup(tag_name).unwrap_or_else(|| {
        let t = gtk::TextTag::new(Some(tag_name));
        match tag_name {
            "bold" => t.set_weight(700),
            "italic" => t.set_style(gtk::pango::Style::Italic),
            "underline" => t.set_underline(gtk::pango::Underline::Single),
            _ => {}
        }
        tags.add(&t);
        t
    });
    if let Some(iter) = buf.selection_bounds() {
        buf.apply_tag(&tag, &iter.0, &iter.1);
    }
}

fn apply_align(buf: &gtk::TextBuffer, align: &str) {
    let just = match align {
        "left" => gtk::Justification::Left,
        "center" => gtk::Justification::Center,
        "right" => gtk::Justification::Right,
        "justify" => gtk::Justification::Fill,
        _ => return,
    };
    // Create a per-paragraph tag
    let tag_name = format!("align-{}", align);
    let tags = buf.tag_table();
    let tag = tags.lookup(&tag_name).unwrap_or_else(|| {
        let t = gtk::TextTag::new(Some(&tag_name));
        t.set_justification(just);
        tags.add(&t);
        t
    });
    if let Some(iter) = buf.selection_bounds() {
        buf.apply_tag(&tag, &iter.0, &iter.1);
    }
}

fn insert_list(buf: &gtk::TextBuffer, list_type: &str) {
    let prefix = if list_type == "bullet" { "• " } else { "1. " };
    if let Some(iter) = buf.selection_bounds() {
        let start = iter.0;
        let mut text = start.slice(&iter.1).to_string();
        text = text.lines().map(|l| format!("{}{}", prefix, l)).collect::<Vec<_>>().join("\n");
        buf.delete(&start, &iter.1);
        buf.insert(&start, &text);
    }
}

fn change_font_size(buf: &gtk::TextBuffer, delta: i32) {
    let tag_name = format!("fs-{}", delta);
    let tags = buf.tag_table();
    let tag = tags.lookup(&tag_name).unwrap_or_else(|| {
        let t = gtk::TextTag::new(Some(&tag_name));
        t.set_scale(1.0 + delta as f64 * 0.1);
        tags.add(&t);
        t
    });
    if let Some(iter) = buf.selection_bounds() {
        buf.apply_tag(&tag, &iter.0, &iter.1);
    }
}

fn change_indent(buf: &gtk::TextBuffer, delta: i32) {
    if let Some(iter) = buf.selection_bounds() {
        let start = iter.0;
        let mut text = start.slice(&iter.1).to_string();
        if delta > 0 {
            text = text.lines().map(|l| format!("\t{}", l)).collect::<Vec<_>>().join("\n");
        } else {
            text = text.lines().map(|l| l.strip_prefix('\t').unwrap_or(l).to_string()).collect::<Vec<_>>().join("\n");
        }
        buf.delete(&start, &iter.1);
        buf.insert(&start, &text);
    }
}

fn apply_heading(buf: &gtk::TextBuffer, level: u32) {
    let tag_name = format!("heading-{}", level);
    let tags = buf.tag_table();
    let tag = tags.lookup(&tag_name).unwrap_or_else(|| {
        let t = gtk::TextTag::new(Some(&tag_name));
        if level == 0 {
            t.set_weight(400);
            t.set_scale(1.0);
        } else {
            t.set_weight(700);
            t.set_scale(1.0 + (7 - level) as f64 * 0.15);
        }
        tags.add(&t);
        t
    });
    if let Some(iter) = buf.selection_bounds() {
        buf.apply_tag(&tag, &iter.0, &iter.1);
    }
}

fn insert_link(buf: &gtk::TextBuffer) {
    if let Some(iter) = buf.selection_bounds() {
        let text = iter.0.slice(&iter.1).to_string();
        let tag_name = format!("link-{}", text);
        let tags = buf.tag_table();
        let tag = tags.lookup(&tag_name).unwrap_or_else(|| {
            let t = gtk::TextTag::new(Some(&tag_name));
            t.set_underline(gtk::pango::Underline::Single);
            tags.add(&t);
            t
        });
        buf.apply_tag(&tag, &iter.0, &iter.1);
    }
}

fn show_find(view: &gtk::TextView) {
    // Stub: open a find dialog
    println!("Find dialog (Ctrl+F) — not yet implemented");
}

fn show_replace(view: &gtk::TextView) {
    println!("Replace dialog (Ctrl+H) — not yet implemented");
}
