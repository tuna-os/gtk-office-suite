// SPDX-License-Identifier: GPL-3.0-or-later
//
// thumbnails.rs — the page thumbnails view of the sidebar (DESIGN-UI,
// Letters from Pages: "Thumbnails are that tree drawn small, not a second
// layout").
//
// Each thumbnail draws its page from the page view's own typeset
// (`PageView::draw_page_at`), so it shows exactly the laid-out page. They
// follow the page view's `laid-out` signal, mark the page holding the
// caret, and activating one scrolls Print Layout to that page. Nothing is
// drawn while the view is hidden. The pageless Draft view has no pages, so
// the view says so there.

use gtk4::{self as gtk, glib, prelude::*};
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;

use crate::page_container::PageContainer;
use crate::page_view::PageView;

/// Thumbnail width in pixels.
const WIDTH: i32 = 128;

/// The active tab's page view, if it is showing its pages.
fn active_pages(tv: &adw::TabView) -> Option<PageView> {
    let pc = tv.selected_page()?.child().downcast::<PageContainer>().ok()?;
    pc.is_print_layout().then(|| pc.page_view()).flatten()
}

fn thumbnail(tv: &adw::TabView, index: usize) -> gtk::ListBoxRow {
    let area = gtk::DrawingArea::new();
    area.set_content_width(WIDTH);
    area.set_halign(gtk::Align::Center);
    let tv = tv.clone();
    area.set_draw_func(move |area, cr, w, _| {
        let Some(view) = active_pages(&tv) else { return };
        let Some((pw, ph)) = view.page_size_pt(index) else { return };
        let k = f64::from(w) / pw;
        let height = (ph * k).round() as i32;
        if area.content_height() != height {
            area.set_content_height(height);
        }
        cr.save().ok();
        view.draw_page_at(cr, index, k);
        cr.restore().ok();
        // A hairline edge: white paper on a light sidebar needs one.
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.25);
        cr.set_line_width(1.0);
        cr.rectangle(0.5, 0.5, f64::from(w) - 1.0, f64::from(height) - 1.0);
        let _ = cr.stroke();
    });
    let caption = gtk::Label::new(Some(&(index + 1).to_string()));
    caption.add_css_class("caption");
    caption.add_css_class("dim-label");
    let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
    column.set_margin_top(6);
    column.set_margin_bottom(6);
    column.append(&area);
    column.append(&caption);
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&column));
    let name = format!("Page {}", index + 1);
    row.update_property(&[gtk::accessible::Property::Label(&name)]);
    row.set_tooltip_text(Some(&name));
    row
}

/// The thumbnails view for the window's tabs.
pub fn build(tv: &adw::TabView) -> gtk::Widget {
    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    list.update_property(&[gtk::accessible::Property::Label("Pages")]);
    let scroll = gtk::ScrolledWindow::builder().child(&list).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
    let draft = adw::StatusPage::builder()
        .title("No Pages in Draft")
        .description("Pages are shown in Print Layout.")
        .icon_name("x-office-document-symbolic")
        .build();
    draft.add_css_class("compact");
    let stack = gtk::Stack::new();
    stack.add_named(&scroll, Some("pages"));
    stack.add_named(&draft, Some("draft"));

    let count = Rc::new(std::cell::Cell::new(0usize));
    let refresh: Rc<dyn Fn()> = {
        let (tv, list, stack, count) = (tv.clone(), list.clone(), stack.clone(), count.clone());
        Rc::new(move || {
            if !stack.is_mapped() {
                return;
            }
            let Some(view) = active_pages(&tv) else {
                stack.set_visible_child_name("draft");
                return;
            };
            stack.set_visible_child_name("pages");
            let n = view.page_count();
            if n != count.get() {
                while let Some(r) = list.first_child() {
                    list.remove(&r);
                }
                for i in 0..n {
                    list.append(&thumbnail(&tv, i));
                }
                count.set(n);
            } else {
                let mut row = list.first_child();
                while let Some(r) = row {
                    if let Some(area) = r.first_child().and_then(|c| c.first_child()) {
                        area.queue_draw();
                    }
                    row = r.next_sibling();
                }
            }
            if let Some(buf) = view.buffer() {
                let caret = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
                let current = view.page_of_offset(caret).and_then(|p| list.row_at_index(p as i32));
                list.select_row(current.as_ref());
            }
        })
    };

    // Follow the active tab's page view: its pages, and the caret.
    let laid_out: RefCell<Option<(glib::WeakRef<PageView>, glib::SignalHandlerId)>> = RefCell::default();
    {
        let refresh = refresh.clone();
        let count = count.clone();
        tv.connect_selected_page_notify(move |tv| {
            if let Some((view, id)) = laid_out.borrow_mut().take() {
                if let Some(view) = view.upgrade() {
                    view.disconnect(id);
                }
            }
            let pc = tv.selected_page().and_then(|p| p.child().downcast::<PageContainer>().ok());
            if let Some(view) = pc.and_then(|pc| pc.page_view()) {
                let r = refresh.clone();
                let id = view.connect_laid_out(move |_| r());
                *laid_out.borrow_mut() = Some((view.downgrade(), id));
            }
            // Another document: rebuild its thumbnails.
            count.set(usize::MAX);
            refresh();
        });
    }
    {
        let refresh = refresh.clone();
        crate::dialogs::watch_active_buffer(tv, move |_, _| refresh());
    }
    {
        let refresh = refresh.clone();
        stack.connect_map(move |_| refresh());
    }
    {
        let tv = tv.clone();
        list.connect_row_activated(move |_, row| {
            if let Some(view) = active_pages(&tv) {
                // Focus first: the viewport scrolls to a newly focused
                // child, which would undo the jump.
                view.grab_focus();
                let index = row.index().max(0) as usize;
                glib::idle_add_local_once(move || view.scroll_to_page(index));
            }
        });
    }
    stack.upcast()
}

