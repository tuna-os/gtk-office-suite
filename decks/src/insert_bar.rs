//! insert_bar.rs — the Insert buttons in the header bar.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! docs/DESIGN-UI.md, "Insert buttons, not menus" (iWork's Text · Shape ·
//! Table · Media row): labelled, flat header-bar buttons, and a Shape
//! popover that is a searchable library of shapes, each drawn by the
//! canvas's own shape code. What each button inserts is decks_core::insert;
//! the buttons only fire the app actions (add-text-box, insert-shape,
//! insert-table, insert-chart, add-image), so keyboard and automation reach
//! the same code. The Chart popover draws each kind with the renderer the
//! slide draws it with.

use adw::prelude::*;
use decks_core::engine::shape::ShapeStyle;
use decks_core::insert::{matches, shape_library};
use gtk4::{self as gtk, glib};
use libadwaita as adw;

/// A flat header-bar button with an icon and a label.
fn labelled(icon: &str, label: &str, tooltip: &str) -> adw::ButtonContent {
    let content = adw::ButtonContent::builder().icon_name(icon).label(label).use_underline(true).build();
    content.set_tooltip_text(Some(tooltip));
    content
}

fn button(icon: &str, label: &str, tooltip: &str, action: &str) -> gtk::Button {
    let b = gtk::Button::builder().child(&labelled(icon, label, tooltip)).action_name(action).build();
    b.add_css_class("flat");
    // AdwButtonContent labels the button by its visible label ("Table"),
    // and the tooltip ("Insert Table") becomes its accessible description.
    b.set_tooltip_text(Some(tooltip));
    b
}

/// A small drawing of a library shape, in the default style.
fn preview(kind: &decks_core::engine::shape::ShapeKind) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(56);
    area.set_content_height(40);
    let kind = kind.clone();
    area.set_draw_func(move |_, cr, w, h| {
        let style = ShapeStyle::default();
        crate::canvas::draw_shape(cr, &kind, &style, (4.0, 4.0, w as f64 - 8.0, h as f64 - 8.0), 1.0);
    });
    area
}

/// Closes whatever holds a picker once a choice is made.
type Done = std::rc::Rc<dyn Fn()>;

/// The shape library: a search entry over the shapes, each inserting
/// itself and then calling `done`. Returns the view and its search entry.
fn shape_library_view(done: Done) -> (gtk::Box, gtk::SearchEntry) {
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search Shapes"));
    search.update_property(&[gtk::accessible::Property::Label("Search Shapes")]);
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(3)
        .min_children_per_line(3)
        .row_spacing(6)
        .column_spacing(6)
        .homogeneous(true)
        .build();
    let library = shape_library();
    for (i, s) in library.iter().enumerate() {
        let tile = gtk::Box::new(gtk::Orientation::Vertical, 6);
        tile.append(&preview(&s.kind));
        let label = gtk::Label::new(Some(s.name));
        label.add_css_class("caption");
        label.set_wrap(true);
        label.set_justify(gtk::Justification::Center);
        label.set_max_width_chars(10);
        tile.append(&label);
        let b = gtk::Button::builder().child(&tile).build();
        b.add_css_class("flat");
        b.update_property(&[gtk::accessible::Property::Label(s.name)]);
        b.set_action_name(Some("app.insert-shape"));
        b.set_action_target_value(Some(&(i as u32).to_variant()));
        let done = done.clone();
        b.connect_clicked(move |_| done());
        grid.append(&b);
    }
    {
        let search = search.clone();
        grid.set_filter_func(move |child| {
            let i = child.index() as usize;
            shape_library().get(i).is_some_and(|s| matches(s, &search.text()))
        });
    }
    {
        let grid = grid.clone();
        search.connect_search_changed(move |_| grid.invalidate_filter());
    }
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_top(6);
    body.set_margin_bottom(6);
    body.set_margin_start(6);
    body.set_margin_end(6);
    body.append(&search);
    body.append(&grid);
    (body, search)
}

/// Clear `search` and focus it, so typing finds a shape straight away.
fn focus_search(search: &gtk::SearchEntry) {
    search.set_text("");
    let s = search.clone();
    glib::idle_add_local_once(move || {
        s.grab_focus();
    });
}

/// The Shape button: a popover with a search entry over the library.
fn shape_menu() -> gtk::MenuButton {
    let popover = gtk::Popover::new();
    let p = popover.clone();
    let (body, search) = shape_library_view(std::rc::Rc::new(move || p.popdown()));
    popover.set_child(Some(&body));
    popover.connect_show(move |_| focus_search(&search));
    let menu = gtk::MenuButton::builder().child(&labelled("office-shapes-symbolic", "_Shape", "Insert Shape")).popover(&popover).build();
    menu.add_css_class("flat");
    menu.set_tooltip_text(Some("Insert Shape"));
    menu.update_property(&[gtk::accessible::Property::Label("Insert Shape")]);
    menu
}

/// The chart kinds, each drawn with its sample series, each inserting
/// itself and then calling `done`; at least `columns` to a row.
fn chart_kinds_view(done: Done, columns: u32) -> gtk::FlowBox {
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(3)
        .min_children_per_line(columns)
        .row_spacing(6)
        .column_spacing(6)
        .homogeneous(true)
        .build();
    for (i, kind) in decks_core::insert::CHART_KINDS.iter().enumerate() {
        let name = decks_core::engine::chart::ChartData::kind_name(*kind);
        let tile = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let area = gtk::DrawingArea::new();
        area.set_content_width(96);
        area.set_content_height(64);
        let data = decks_core::engine::chart::ChartData::sample(*kind);
        area.set_draw_func(move |_, cr, w, h| {
            // Drawn at twice the size and scaled down, so the sample's
            // axis labels fit the tile.
            cr.scale(0.5, 0.5);
            suite_common::charts::draw_chart(cr, &data.points, data.kind, w as f64 * 2.0, h as f64 * 2.0, None);
        });
        tile.append(&area);
        let label = gtk::Label::new(Some(name));
        label.add_css_class("caption");
        tile.append(&label);
        let b = gtk::Button::builder().child(&tile).build();
        b.add_css_class("flat");
        b.update_property(&[gtk::accessible::Property::Label(name)]);
        b.set_action_name(Some("app.insert-chart"));
        b.set_action_target_value(Some(&(i as u32).to_variant()));
        let done = done.clone();
        b.connect_clicked(move |_| done());
        grid.append(&b);
    }
    grid.set_margin_top(6);
    grid.set_margin_bottom(6);
    grid.set_margin_start(6);
    grid.set_margin_end(6);
    grid
}

/// The Chart button: a popover of the chart kinds.
fn chart_menu() -> gtk::MenuButton {
    let popover = gtk::Popover::new();
    let p = popover.clone();
    popover.set_child(Some(&chart_kinds_view(std::rc::Rc::new(move || p.popdown()), 3)));
    let menu = gtk::MenuButton::builder().child(&labelled("office-chart-symbolic", "C_hart", "Insert Chart")).popover(&popover).build();
    menu.add_css_class("flat");
    menu.set_tooltip_text(Some("Insert Chart"));
    menu.update_property(&[gtk::accessible::Property::Label("Insert Chart")]);
    menu
}

/// Pack the Insert buttons at the start of `header`: Text, Shape, Table,
/// Chart, Image. Under `narrow` they fold into one Insert menu: in a row
/// they make the header bar wider than a 400px window (even icon-only,
/// beside the title and the window controls), and AdwWindow then clips the
/// window's right-hand side, its menu, window controls and part of the
/// slide, instead of shrinking it. In the menu, Shape and Chart open their
/// pickers as pages with a back button, as a popover menu's submenus do;
/// a menu button inside a popover does not open its own reliably.
pub fn build(header: &adw::HeaderBar, narrow: &adw::Breakpoint) {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.append(&button("insert-text-symbolic", "_Text", "Insert Text Box", "app.add-text-box"));
    bar.append(&shape_menu());
    bar.append(&button("office-table-symbolic", "T_able", "Insert Table", "app.insert-table"));
    bar.append(&chart_menu());
    bar.append(&button("insert-image-symbolic", "_Image", "Insert Image", "app.add-image"));
    header.pack_start(&bar);

    let compact = compact_menu();
    header.pack_start(&compact);
    narrow.add_setter(&bar, "visible", Some(&false.to_value()));
    narrow.add_setter(&compact, "visible", Some(&true.to_value()));
}

/// The narrow window's Insert menu.
fn compact_menu() -> gtk::MenuButton {
    let popover = gtk::Popover::new();
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::SlideLeftRight)
        .vhomogeneous(false)
        .hhomogeneous(false)
        .interpolate_size(true)
        .build();
    let p = popover.clone();
    let done: Done = std::rc::Rc::new(move || p.popdown());

    let (shapes, search) = shape_library_view(done.clone());
    let charts = chart_kinds_view(done.clone(), 2);
    // An item that inserts at once, and one that opens its picker's page.
    let insert = |icon, label, tooltip, action| {
        let b = button(icon, label, tooltip, action);
        let done = done.clone();
        b.connect_clicked(move |_| done());
        b
    };
    let picker = |icon, label, tooltip, page: &'static str| {
        let b = gtk::Button::builder().child(&labelled(icon, label, tooltip)).build();
        b.add_css_class("flat");
        // Named by its label and described by its tooltip, like `button`.
        b.set_tooltip_text(Some(tooltip));
        let (stack, search, charts) = (stack.clone(), search.clone(), charts.clone());
        b.connect_clicked(move |_| {
            stack.set_visible_child_name(page);
            // Focus goes into the page, so the keyboard carries on there:
            // the button just pressed slides away with the list.
            if page == "shapes" {
                focus_search(&search);
            } else {
                let charts = charts.clone();
                glib::idle_add_local_once(move || {
                    charts.child_focus(gtk::DirectionType::TabForward);
                });
            }
        });
        b
    };
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.append(&insert("insert-text-symbolic", "_Text", "Insert Text Box", "app.add-text-box"));
    list.append(&picker("office-shapes-symbolic", "_Shape", "Insert Shape", "shapes"));
    list.append(&insert("office-table-symbolic", "T_able", "Insert Table", "app.insert-table"));
    list.append(&picker("office-chart-symbolic", "C_hart", "Insert Chart", "charts"));
    list.append(&insert("insert-image-symbolic", "_Image", "Insert Image", "app.add-image"));
    stack.add_named(&list, Some("main"));
    // The chart grid scrolls within a bounded height. Placed bare in the
    // page, the popover closed the moment the page opened (measured on GTK
    // 4.14: nothing inside it was showing half a second later); bounded,
    // it stays open, and it fits a short window too.
    let charts = gtk::ScrolledWindow::builder()
        .child(&charts)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(360)
        .build();
    for (name, title, view) in [("shapes", "Shape", shapes.upcast::<gtk::Widget>()), ("charts", "Chart", charts.upcast())] {
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let back = gtk::Button::builder().child(&labelled("go-previous-symbolic", title, "Back")).build();
        back.add_css_class("flat");
        back.update_property(&[gtk::accessible::Property::Label("Back")]);
        let stack2 = stack.clone();
        back.connect_clicked(move |_| stack2.set_visible_child_name("main"));
        page.append(&back);
        page.append(&view);
        stack.add_named(&page, Some(name));
    }
    popover.set_child(Some(&stack));
    // Each opening starts from the list.
    let stack2 = stack.clone();
    popover.connect_closed(move |_| stack2.set_visible_child_name("main"));

    let menu = gtk::MenuButton::builder().icon_name("list-add-symbolic").tooltip_text("Insert").popover(&popover).visible(false).build();
    menu.update_property(&[gtk::accessible::Property::Label("Insert")]);
    menu
}
