//! The contact form's parts: value rows with labels, postal addresses,
//! the birthday and the photo.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use relm4::prelude::*;

use crate::contacts::ContactDetails;
use crate::i18n::i18n;

use super::rows::{avatar_for, group};

/// The value rows of one kind (emails, phones…), each with its label menu
/// when the kind has labels: `labels[i]` is what item `i` stands for, the
/// first being none.
pub(super) struct ValueRowsInner {
    pub(super) rows: Vec<(adw::EntryRow, Option<gtk::DropDown>)>,
    pub(super) labels: &'static [&'static str],
}
pub(super) type ValueRows = Rc<RefCell<ValueRowsInner>>;

/// The labels an email can have, as vCard TYPEs name them.
pub(super) const EMAIL_LABELS: &[&str] = &["", "Home", "Work"];
pub(super) const PHONE_LABELS: &[&str] = &["", "Mobile", "Home", "Work", "Fax"];
const ADDRESS_LABELS: &[&str] = &["", "Home", "Work"];

/// A label menu over `labels`, on `current`.
fn label_menu(labels: &'static [&'static str], current: &str) -> gtk::DropDown {
    let names: Vec<String> = labels
        .iter()
        .map(|l| match *l {
            "" => i18n("No label"),
            "Home" => i18n("Home"),
            "Work" => i18n("Work"),
            "Mobile" => i18n("Mobile"),
            "Fax" => i18n("Fax"),
            other => other.to_string(),
        })
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let menu = gtk::DropDown::from_strings(&refs);
    menu.add_css_class("flat");
    menu.set_valign(gtk::Align::Center);
    menu.set_tooltip_text(Some(&i18n("Label")));
    menu.set_selected(labels.iter().position(|l| *l == current).unwrap_or(0) as u32);
    menu
}

/// A titled group of editable value rows with a remove button each and an
/// "add" row at the bottom. Returns the live handle used at save time.
pub(super) fn editable_values(
    detail: &gtk::Box,
    title: &str,
    add_label: &str,
    initial: &[(String, String)],
    labels: &'static [&'static str],
) -> ValueRows {
    let list = group(detail, title);
    let rows: ValueRows = Rc::new(RefCell::new(ValueRowsInner {
        rows: Vec::new(),
        labels,
    }));

    // The "add" row sits last; value rows are inserted above it.
    let add_row = gtk::ListBoxRow::new();
    add_row.set_activatable(true);
    let add_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    add_box.set_margin_top(10);
    add_box.set_margin_bottom(10);
    add_box.set_margin_start(12);
    let plus = gtk::Image::from_icon_name("list-add-symbolic");
    plus.add_css_class("dim-label");
    let label = gtk::Label::new(Some(add_label));
    label.add_css_class("dim-label");
    add_box.append(&plus);
    add_box.append(&label);
    add_row.set_child(Some(&add_box));

    let field = title.to_string();
    let add_value = {
        let list = list.clone();
        let rows = rows.clone();
        let add_row = add_row.clone();
        move |value: &str, label: String, grab: bool| {
            let entry = adw::EntryRow::new();
            entry.set_title(&field);
            entry.set_text(value);
            let menu = (!labels.is_empty()).then(|| label_menu(labels, &label));
            if let Some(menu) = &menu {
                entry.add_suffix(menu);
            }
            let remove = gtk::Button::from_icon_name("user-trash-symbolic");
            remove.set_tooltip_text(Some(i18n("Remove").as_str()));
            remove.set_valign(gtk::Align::Center);
            remove.add_css_class("flat");
            {
                let list = list.clone();
                let rows = rows.clone();
                let entry = entry.clone();
                remove.connect_clicked(move |_| {
                    rows.borrow_mut().rows.retain(|(e, _)| e != &entry);
                    list.remove(&entry);
                });
            }
            entry.add_suffix(&remove);
            // Insert above the trailing "add" row.
            list.insert(&entry, add_row.index());
            rows.borrow_mut().rows.push((entry.clone(), menu));
            if grab {
                entry.grab_focus();
            }
        }
    };

    for (value, label) in initial {
        add_value(value, label.clone(), false);
    }
    {
        let add_value = add_value.clone();
        let add_row = add_row.clone();
        list.connect_row_activated(move |_, row| {
            if row == &add_row {
                add_value("", String::new(), true);
            }
        });
    }
    list.append(&add_row);
    rows
}

/// One postal address in the contact form: its parts, and its label. The
/// post office box and the extended part, which the form does not show,
/// are kept as they were.
pub(super) struct AddressForm {
    base: crate::contacts::Postal,
    street: adw::EntryRow,
    locality: adw::EntryRow,
    region: adw::EntryRow,
    code: adw::EntryRow,
    country: adw::EntryRow,
    label: gtk::DropDown,
}

impl AddressForm {
    pub(super) fn read(&self) -> crate::contacts::Postal {
        let text = |e: &adw::EntryRow| e.text().trim().to_string();
        crate::contacts::Postal {
            label: ADDRESS_LABELS
                .get(self.label.selected() as usize)
                .map_or(String::new(), |l| l.to_string()),
            street: text(&self.street),
            locality: text(&self.locality),
            region: text(&self.region),
            code: text(&self.code),
            country: text(&self.country),
            ..self.base.clone()
        }
    }
}

/// The contact form's addresses: a block of rows for each, and a row that
/// adds one.
pub(super) fn address_editor(
    detail: &gtk::Box,
    initial: &[crate::contacts::Postal],
) -> Rc<RefCell<Vec<AddressForm>>> {
    let heading = gtk::Label::new(Some(&i18n("Address")));
    heading.add_css_class("heading");
    heading.set_halign(gtk::Align::Start);
    heading.set_margin_top(20);
    heading.set_margin_bottom(6);
    detail.append(&heading);
    let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
    detail.append(&column);
    let forms: Rc<RefCell<Vec<AddressForm>>> = Rc::new(RefCell::new(Vec::new()));

    // The same "add" row as the other kinds of value have.
    let add_list = gtk::ListBox::new();
    add_list.add_css_class("boxed-list");
    add_list.set_selection_mode(gtk::SelectionMode::None);
    let add_row = gtk::ListBoxRow::new();
    add_row.set_activatable(true);
    let add_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    add_box.set_margin_top(10);
    add_box.set_margin_bottom(10);
    add_box.set_margin_start(12);
    let plus = gtk::Image::from_icon_name("list-add-symbolic");
    plus.add_css_class("dim-label");
    let add_label = gtk::Label::new(Some(&i18n("Add address")));
    add_label.add_css_class("dim-label");
    add_box.append(&plus);
    add_box.append(&add_label);
    add_row.set_child(Some(&add_box));
    add_list.append(&add_row);

    let add = {
        let (column, forms, add_list) = (column.downgrade(), forms.clone(), add_list.downgrade());
        Rc::new(move |p: &crate::contacts::Postal, grab: bool| {
            let (Some(column), Some(add_list)) = (column.upgrade(), add_list.upgrade()) else {
                return;
            };
            let list = gtk::ListBox::new();
            list.add_css_class("boxed-list");
            list.set_selection_mode(gtk::SelectionMode::None);
            let head = adw::ActionRow::new();
            head.set_title(&i18n("Label"));
            let label = label_menu(ADDRESS_LABELS, &p.label);
            head.add_suffix(&label);
            let remove = gtk::Button::from_icon_name("user-trash-symbolic");
            remove.set_tooltip_text(Some(i18n("Remove").as_str()));
            remove.set_valign(gtk::Align::Center);
            remove.add_css_class("flat");
            head.add_suffix(&remove);
            list.append(&head);
            let street = entry_row(&i18n("Street"), &p.street);
            let locality = entry_row(&i18n("City"), &p.locality);
            let region = entry_row(&i18n("State or region"), &p.region);
            let code = entry_row(&i18n("Postal code"), &p.code);
            let country = entry_row(&i18n("Country"), &p.country);
            for row in [&street, &locality, &region, &code, &country] {
                list.append(row);
            }
            {
                let (forms, list_weak, street) = (forms.clone(), list.downgrade(), street.clone());
                remove.connect_clicked(move |_| {
                    forms.borrow_mut().retain(|f| f.street != street);
                    if let Some(list) = list_weak.upgrade() {
                        if let Some(parent) = list.parent().and_downcast::<gtk::Box>() {
                            parent.remove(&list);
                        }
                    }
                });
            }
            column.insert_child_after(&list, add_list.prev_sibling().as_ref());
            if grab {
                street.grab_focus();
            }
            forms.borrow_mut().push(AddressForm {
                base: p.clone(),
                street,
                locality,
                region,
                code,
                country,
                label,
            });
        })
    };
    column.append(&add_list);
    for p in initial {
        add(p, false);
    }
    add_list.connect_row_activated(move |_, _| add(&crate::contacts::Postal::default(), true));
    forms
}

/// The contact form's birthday: day, month and an optional year. No month
/// is no birthday.
pub(super) struct BirthdayForm {
    day: gtk::SpinButton,
    month: gtk::DropDown,
    year: gtk::Entry,
}

impl BirthdayForm {
    pub(super) fn new(raw: &str) -> (adw::ActionRow, BirthdayForm) {
        let row = adw::ActionRow::new();
        row.set_title(&i18n("Birthday"));
        let parts = crate::contacts::birthday_parts(raw);
        let day = gtk::SpinButton::with_range(1.0, 31.0, 1.0);
        day.set_valign(gtk::Align::Center);
        day.set_value(parts.map_or(1, |(_, _, d)| d) as f64);
        let mut months = vec![i18n("No birthday")];
        months.extend((1..=12).map(|m| {
            gtk::glib::DateTime::from_local(2000, m, 1, 12, 0, 0.0)
                .ok()
                .and_then(|d| d.format("%B").ok())
                .map_or(m.to_string(), |s| s.to_string())
        }));
        let refs: Vec<&str> = months.iter().map(String::as_str).collect();
        let month = gtk::DropDown::from_strings(&refs);
        month.set_valign(gtk::Align::Center);
        month.set_selected(parts.map_or(0, |(_, m, _)| m));
        let year = gtk::Entry::new();
        year.set_valign(gtk::Align::Center);
        year.set_width_chars(5);
        year.set_max_width_chars(5);
        year.set_placeholder_text(Some(&i18n("Year")));
        year.set_input_purpose(gtk::InputPurpose::Digits);
        if let Some((Some(y), _, _)) = parts {
            year.set_text(&y.to_string());
        }
        let sync = {
            let (day, year) = (day.clone(), year.clone());
            move |month: &gtk::DropDown| {
                let on = month.selected() != 0;
                day.set_sensitive(on);
                year.set_sensitive(on);
            }
        };
        sync(&month);
        month.connect_selected_notify(sync);
        row.add_suffix(&day);
        row.add_suffix(&month);
        row.add_suffix(&year);
        (row, BirthdayForm { day, month, year })
    }

    /// The BDAY value, empty for none.
    pub(super) fn read(&self) -> String {
        let month = self.month.selected();
        if month == 0 {
            return String::new();
        }
        let year = self
            .year
            .text()
            .trim()
            .parse::<i32>()
            .ok()
            .filter(|y| (1..=9999).contains(y));
        crate::contacts::birthday_value(year, month, self.day.value() as u32)
    }
}

/// The contact form's photo: the picture, with buttons to change it or
/// remove it.
pub(super) fn photo_editor(
    detail: &gtk::Box,
    c: &ContactDetails,
    photo: Rc<RefCell<crate::contacts::PhotoEdit>>,
) {
    let avatar = avatar_for(c, 96);
    avatar.set_halign(gtk::Align::Center);
    avatar.set_margin_top(12);
    detail.append(&avatar);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::Center);
    buttons.set_margin_top(8);
    let change = gtk::Button::with_label(&i18n("Change Photo…"));
    change.add_css_class("pill");
    let remove = gtk::Button::with_label(&i18n("Remove Photo"));
    remove.add_css_class("pill");
    remove.set_visible(c.has_photo());
    buttons.append(&change);
    buttons.append(&remove);
    detail.append(&buttons);
    {
        let (photo, avatar, remove_btn) = (photo.clone(), avatar.clone(), remove.clone());
        remove.connect_clicked(move |_| {
            *photo.borrow_mut() = crate::contacts::PhotoEdit::Remove;
            avatar.set_custom_image(None::<&gtk::gdk::Paintable>);
            remove_btn.set_visible(false);
        });
    }
    change.connect_clicked(move |b| {
        let dialog = gtk::FileDialog::builder()
            .title(i18n("Choose a Photo"))
            .build();
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&i18n("Images")));
        filter.add_pixbuf_formats();
        let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        dialog.set_filters(Some(&filters));
        let parent = b.root().and_downcast::<gtk::Window>();
        let (photo, avatar, remove) = (photo.clone(), avatar.clone(), remove.clone());
        dialog.open(parent.as_ref(), gtk::gio::Cancellable::NONE, move |res| {
            let Some(path) = res.ok().and_then(|f| f.path()) else {
                return;
            };
            let Some(bytes) = small_photo(&path) else {
                return;
            };
            if let Ok(texture) = gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from(&bytes)) {
                avatar.set_custom_image(Some(&texture));
            }
            *photo.borrow_mut() = crate::contacts::PhotoEdit::Set(bytes);
            remove.set_visible(true);
        });
    });
}

/// An image file as a contact photo: at most 512 pixels a side, a JPEG (a
/// PNG when it has see-through parts), so it goes in the card itself.
fn small_photo(path: &std::path::Path) -> Option<Vec<u8>> {
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, 512, 512, true).ok()?;
    let (kind, options): (&str, &[(&str, &str)]) = if pixbuf.has_alpha() {
        ("png", &[])
    } else {
        ("jpeg", &[("quality", "90")])
    };
    pixbuf.save_to_bufferv(kind, options).ok()
}

pub(super) fn entry_row(title: &str, value: &str) -> adw::EntryRow {
    let row = adw::EntryRow::new();
    row.set_title(title);
    row.set_text(value);
    row
}
