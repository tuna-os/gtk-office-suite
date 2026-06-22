use gtk4 as gtk;
use libadwaita::prelude::*;

pub struct LettersWindow {
    window: libadwaita::ApplicationWindow,
}

impl LettersWindow {
    pub fn new(app: &libadwaita::Application) -> Self {
        let win = libadwaita::ApplicationWindow::builder()
            .application(app)
            .build();
        win.set_title(Some("Letters"));
        win.set_default_size(800, 600);

        let header = suite_common::make_header_bar("Letters");
        let toolbar = suite_common::make_toolbar();

        // Style dropdown
        let styles = gtk::DropDown::from_strings(&[
            "Paragraph",
            "Heading 1",
            "Heading 2",
            "Heading 3",
            "Code",
            "Quote",
        ]);
        toolbar.append(&styles);

        // Table insert button
        let table_btn = gtk::Button::builder()
            .icon_name("view-grid-symbolic")
            .tooltip_text("Insert Table")
            .css_classes(vec!["flat".to_string()])
            .build();
        toolbar.append(&table_btn);

        // Find button
        let find_btn = gtk::Button::builder()
            .icon_name("edit-find-symbolic")
            .tooltip_text("Find and Replace")
            .css_classes(vec!["flat".to_string()])
            .build();
        toolbar.append(&find_btn);

        let tab_view = libadwaita::TabView::new();
        let tab_bar = libadwaita::TabBar::new();
        tab_bar.set_view(Some(&tab_view));
        let editor = gtk::TextView::new();
        editor.set_monospace(false); // WYSIWYG should feel like a word processor
        editor.set_wrap_mode(gtk::WrapMode::Word);
        editor.set_left_margin(40);
        editor.set_right_margin(40);
        editor.set_top_margin(20);
        editor.set_bottom_margin(20);

        // Setup tags
        let buffer = editor.buffer();
        let tag_h1 = gtk::TextTag::builder()
            .name("h1")
            .scale(1.8)
            .weight(700)
            .pixels_above_lines(10)
            .pixels_below_lines(6)
            .build();
        let tag_h2 = gtk::TextTag::builder()
            .name("h2")
            .scale(1.4)
            .weight(700)
            .pixels_above_lines(8)
            .pixels_below_lines(4)
            .build();
        let tag_h3 = gtk::TextTag::builder()
            .name("h3")
            .scale(1.2)
            .weight(700)
            .pixels_above_lines(6)
            .pixels_below_lines(2)
            .build();
        let tag_bold = gtk::TextTag::builder().name("bold").weight(700).build();
        let tag_italic = gtk::TextTag::builder()
            .name("italic")
            .style(gtk::pango::Style::Italic)
            .build();
        let tag_code = gtk::TextTag::builder()
            .name("code")
            .family("monospace")
            .background("lightgray")
            .build();
        let tag_quote = gtk::TextTag::builder()
            .name("quote")
            .style(gtk::pango::Style::Italic)
            .foreground("gray")
            .left_margin(20)
            .build();

        let tag_table = buffer.tag_table();
        tag_table.add(&tag_h1);
        tag_table.add(&tag_h2);
        tag_table.add(&tag_h3);
        tag_table.add(&tag_bold);
        tag_table.add(&tag_italic);
        tag_table.add(&tag_code);
        tag_table.add(&tag_quote);

        let s = gtk::ScrolledWindow::new();
        s.set_child(Some(&editor));
        s.set_vexpand(true);

        let tab_page = tab_view.add_page(&s, None::<&libadwaita::TabPage>);
        tab_page.set_title("Document 1");
        tab_page.set_icon(Some(&gtk::gio::ThemedIcon::new(
            "document-x-generic-symbolic",
        )));

        let status = gtk::Label::new(Some("0 words"));
        status.set_halign(gtk::Align::End);

        // Find and replace overlay bar
        let find_replace_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        find_replace_bar.set_margin_start(12);
        find_replace_bar.set_margin_end(12);
        find_replace_bar.set_margin_top(4);
        find_replace_bar.set_margin_bottom(4);

        let find_entry = gtk::Entry::builder().placeholder_text("Find...").build();
        let replace_entry = gtk::Entry::builder()
            .placeholder_text("Replace with...")
            .build();
        let next_btn = gtk::Button::with_label("Next");
        let replace_btn = gtk::Button::with_label("Replace");
        let replace_all_btn = gtk::Button::with_label("Replace All");

        find_replace_bar.append(&find_entry);
        find_replace_bar.append(&replace_entry);
        find_replace_bar.append(&next_btn);
        find_replace_bar.append(&replace_btn);
        find_replace_bar.append(&replace_all_btn);

        // Use standard GtkSearchBar
        let search_bar = gtk::SearchBar::builder().show_close_button(true).build();
        search_bar.set_child(Some(&find_replace_bar));
        search_bar.connect_entry(&find_entry);

        // Toggle Find/Replace panel visibility
        let search_bar_clone = search_bar.clone();
        find_btn.connect_clicked(move |_| {
            let vis = search_bar_clone.is_search_mode();
            search_bar_clone.set_search_mode(!vis);
        });

        // Setup Search/Replace logic
        let buffer_clone = buffer.clone();
        let editor_clone = editor.clone();
        let find_entry_clone = find_entry.clone();
        next_btn.connect_clicked(move |_| {
            let search_text = find_entry_clone.text().to_string();
            if search_text.is_empty() {
                return;
            }

            let cursor = buffer_clone.iter_at_mark(&buffer_clone.mark("insert").unwrap());
            if let Some((start_match, end_match)) =
                cursor.forward_search(&search_text, gtk::TextSearchFlags::VISIBLE_ONLY, None)
            {
                buffer_clone.select_range(&start_match, &end_match);
                let mut sm = start_match;
                editor_clone.scroll_to_iter(&mut sm, 0.0, false, 0.0, 0.0);
            } else {
                let start_buf = buffer_clone.start_iter();
                if let Some((start_match, end_match)) =
                    start_buf.forward_search(&search_text, gtk::TextSearchFlags::VISIBLE_ONLY, None)
                {
                    buffer_clone.select_range(&start_match, &end_match);
                    let mut sm = start_match;
                    editor_clone.scroll_to_iter(&mut sm, 0.0, false, 0.0, 0.0);
                }
            }
        });

        let buffer_clone2 = buffer.clone();
        let replace_entry_clone = replace_entry.clone();
        let next_btn_clone = next_btn.clone();
        replace_btn.connect_clicked(move |_| {
            let replace_text = replace_entry_clone.text().to_string();
            if let Some((mut start_sel, mut end_sel)) = buffer_clone2.selection_bounds() {
                buffer_clone2.delete(&mut start_sel, &mut end_sel);
                buffer_clone2.insert(&mut start_sel, &replace_text);
                next_btn_clone.emit_clicked();
            }
        });

        let buffer_clone3 = buffer.clone();
        let find_entry_clone2 = find_entry.clone();
        let replace_entry_clone2 = replace_entry.clone();
        replace_all_btn.connect_clicked(move |_| {
            let search_text = find_entry_clone2.text().to_string();
            let replace_text = replace_entry_clone2.text().to_string();
            if search_text.is_empty() {
                return;
            }

            let mut iter = buffer_clone3.start_iter();
            let mut count = 0;
            while let Some((mut start_match, mut end_match)) =
                iter.forward_search(&search_text, gtk::TextSearchFlags::VISIBLE_ONLY, None)
            {
                buffer_clone3.delete(&mut start_match, &mut end_match);
                buffer_clone3.insert(&mut start_match, &replace_text);
                iter = start_match;
                count += 1;
                if count > 1000 {
                    break;
                }
            }
        });

        // Insert table action
        let buffer_clone4 = buffer.clone();
        table_btn.connect_clicked(move |_| {
            let mut cursor = buffer_clone4.iter_at_mark(&buffer_clone4.mark("insert").unwrap());
            buffer_clone4.insert(&mut cursor, "\n| Header 1 | Header 2 | Header 3 |\n|---|---|---|\n| Cell 1 | Cell 2 | Cell 3 |\n| Cell 4 | Cell 5 | Cell 6 |\n");
        });

        // Dynamic word counter listener
        let status_clone = status.clone();
        buffer.connect_changed(move |buf| {
            let (start, end) = buf.bounds();
            let text = buf.text(&start, &end, false).to_string();
            let count = text.split_whitespace().count();
            status_clone.set_text(&format!("{} words", count));
        });

        // Keyboard / typing Markdown Macros
        let in_change = std::rc::Rc::new(std::cell::Cell::new(false));
        buffer.connect_insert_text(move |buf, iter, text| {
            if in_change.get() {
                return;
            }

            if text == " " {
                let mut prev = *iter;
                prev.backward_char();
                let mut line_start = *iter;
                line_start.set_line_offset(0);

                if prev != line_start {
                    let prefix = buf.text(&line_start, iter, false).to_string();
                    if prefix == "#" {
                        in_change.set(true);
                        let mut start_del = line_start;
                        let mut end_del = *iter;
                        buf.delete(&mut start_del, &mut end_del);
                        let mut line_end = start_del;
                        line_end.forward_to_line_end();
                        buf.apply_tag_by_name("h1", &start_del, &line_end);
                        in_change.set(false);
                    } else if prefix == "##" {
                        in_change.set(true);
                        let mut start_del = line_start;
                        let mut end_del = *iter;
                        buf.delete(&mut start_del, &mut end_del);
                        let mut line_end = start_del;
                        line_end.forward_to_line_end();
                        buf.apply_tag_by_name("h2", &start_del, &line_end);
                        in_change.set(false);
                    } else if prefix == "###" {
                        in_change.set(true);
                        let mut start_del = line_start;
                        let mut end_del = *iter;
                        buf.delete(&mut start_del, &mut end_del);
                        let mut line_end = start_del;
                        line_end.forward_to_line_end();
                        buf.apply_tag_by_name("h3", &start_del, &line_end);
                        in_change.set(false);
                    } else if prefix == "*" || prefix == "-" {
                        in_change.set(true);
                        let mut start_del = line_start;
                        let mut end_del = *iter;
                        buf.delete(&mut start_del, &mut end_del);
                        buf.insert(&mut start_del, "• ");
                        in_change.set(false);
                    }
                }
            }

            if text == " " {
                let mut line_start = *iter;
                line_start.set_line_offset(0);
                let line_prefix = buf.text(&line_start, iter, false).to_string();

                if line_prefix.ends_with("**") && line_prefix.len() > 4 {
                    if let Some(start_idx) = line_prefix[..line_prefix.len() - 2].rfind("**") {
                        let start_offset = line_start.offset() + start_idx as i32;
                        let end_offset = line_start.offset() + line_prefix.len() as i32 - 2;

                        in_change.set(true);
                        let mut del_start1 = buf.iter_at_offset(start_offset);
                        let mut del_end1 = buf.iter_at_offset(start_offset);
                        del_end1.forward_chars(2);
                        buf.delete(&mut del_start1, &mut del_end1);

                        let mut del_start2 = buf.iter_at_offset(end_offset - 2);
                        let mut del_end2 = buf.iter_at_offset(end_offset);
                        buf.delete(&mut del_start2, &mut del_end2);

                        let bold_start = buf.iter_at_offset(start_offset);
                        let bold_end = buf.iter_at_offset(end_offset - 2);
                        buf.apply_tag_by_name("bold", &bold_start, &bold_end);
                        in_change.set(false);
                    }
                }
            }
        });

        let m = gtk::Box::new(gtk::Orientation::Vertical, 2);
        m.append(&toolbar);
        m.append(&search_bar);
        m.append(&tab_bar);
        m.append(&tab_view);
        m.append(&status);

        let toolbar_view = libadwaita::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(&m));
        win.set_content(Some(&toolbar_view));
        Self { window: win }
    }
    pub fn present(&self) {
        self.window.present();
    }
}
