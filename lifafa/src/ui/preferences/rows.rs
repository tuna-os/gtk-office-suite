//! Helpers for the rows themselves: combo rows that show their values
//! whole, the composer font list, and page widths.

use super::*;

/// Set the content width of a preferences page: the clamp it lays its
/// groups out in is not exposed, so it is found among the descendants.
pub(super) fn widen_page(page: &gtk::Widget, max: i32) {
    fn walk(w: &gtk::Widget, max: i32) -> bool {
        if let Some(clamp) = w.downcast_ref::<adw::Clamp>() {
            clamp.set_maximum_size(max);
            return true;
        }
        let mut child = w.first_child();
        while let Some(c) = child {
            if walk(&c, max) {
                return true;
            }
            child = c.next_sibling();
        }
        false
    }
    if !walk(page, max) {
        tracing::warn!("preferences: no clamp found on the page to widen");
    }
}

/// Depth-first search for widgets with the given name under `root`.
pub(super) fn collect_named(root: &gtk::Widget, name: &str, out: &mut Vec<gtk::Widget>) {
    if root.widget_name() == name {
        out.push(root.clone());
        return;
    }
    let mut child = root.first_child();
    while let Some(c) = child {
        collect_named(&c, name, out);
        child = c.next_sibling();
    }
}

/// Addresses on one domain differ at the front and share the end, so the
/// libadwaita default, which cuts the end off, left every choice reading
/// "Jane Doe <jane.doe@exam…" (#261). The selected value shows the address
/// alone, the part that tells the choices apart, and gives way in the
/// middle; the popup list shows every choice whole.
pub(super) fn middle_ellipsize(row: &adw::ComboRow) {
    fn factory(selected: bool) -> gtk::SignalListItemFactory {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(move |_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                let label = gtk::Label::new(None);
                label.set_xalign(if selected { 1.0 } else { 0.0 });
                if selected {
                    label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                    label.set_width_chars(24);
                }
                item.set_child(Some(&label));
            }
        });
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            if let (Some(label), Some(s)) = (
                item.child().and_downcast::<gtk::Label>(),
                item.item().and_downcast::<gtk::StringObject>(),
            ) {
                let full = s.string();
                let shown = match (selected, full.rfind('<')) {
                    (true, Some(i)) if full.ends_with('>') => &full[i + 1..full.len() - 1],
                    _ => full.as_str(),
                };
                label.set_label(shown);
                label.set_tooltip_text(selected.then_some(full.as_str()));
            }
        });
        factory
    }
    row.set_factory(Some(&factory(true)));
    row.set_list_factory(Some(&factory(false)));
}

/// Give a combo row's selected-value label `extra` more pixels than the
/// ellipsized width libadwaita allows it, so a sentence-length choice
/// ("Account of the current folder") reads whole instead of trailing off.
/// The value label is the row's only descendant label showing the
/// selected string; nothing else in the row is touched.
pub(super) fn widen_combo_value(row: &adw::ComboRow, extra: i32) {
    let Some(want) = row
        .selected_item()
        .and_downcast::<gtk::StringObject>()
        .map(|s| s.string().to_string())
    else {
        return;
    };
    fn find(w: &gtk::Widget, want: &str) -> Option<gtk::Label> {
        if let Some(label) = w.downcast_ref::<gtk::Label>() {
            if label.label() == want {
                return Some(label.clone());
            }
        }
        let mut child = w.first_child();
        while let Some(c) = child {
            if let Some(hit) = find(&c, want) {
                return Some(hit);
            }
            child = c.next_sibling();
        }
        None
    }
    if let Some(label) = find(row.upcast_ref::<gtk::Widget>(), &want) {
        let (_, natural, _, _) = label.measure(gtk::Orientation::Horizontal, -1);
        label.set_width_request(natural + extra);
    }
}

/// Settings → Composing → Font: the default, the composer's own fonts and
/// the recent picks, each drawn in its own face, then Other Font…, which
/// opens the font chooser and adds what it picks to the list (#358).
pub(super) fn compose_font_setup(
    row: &adw::ComboRow,
    current: &str,
    sender: ComponentSender<Preferences>,
) {
    use std::cell::{Cell, RefCell};
    struct State {
        /// The CSS family list of each entry; Other Font… has none.
        choices: RefCell<Vec<String>>,
        /// The font in use, which the row goes back to when the chooser is
        /// cancelled.
        chosen: RefCell<String>,
        /// Set while the list is rebuilt, which selects as it goes.
        filling: Cell<bool>,
    }
    fn fill(row: &adw::ComboRow, state: &State) {
        let chosen = state.chosen.borrow().clone();
        let choices = crate::ui::rich_editor::compose_font_choices(&chosen);
        let mut names: Vec<String> = choices.iter().map(|(_, name)| name.clone()).collect();
        names.push(i18n("Other Font…"));
        let at = choices
            .iter()
            .position(|(css, _)| *css == chosen)
            .unwrap_or(0);
        *state.choices.borrow_mut() = choices.into_iter().map(|(css, _)| css).collect();
        state.filling.set(true);
        row.set_model(Some(&gtk::StringList::new(
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        )));
        row.set_selected(at as u32);
        state.filling.set(false);
    }
    let state = Rc::new(State {
        choices: RefCell::new(Vec::new()),
        chosen: RefCell::new(current.to_string()),
        filling: Cell::new(false),
    });

    let list = gtk::SignalListItemFactory::new();
    list.connect_setup(|_, item| {
        if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            item.set_child(Some(&label));
        }
    });
    {
        let state = state.clone();
        list.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(label) = item.child().and_downcast::<gtk::Label>() else {
                return;
            };
            let name = item
                .item()
                .and_downcast::<gtk::StringObject>()
                .map(|s| s.string());
            label.set_label(name.as_deref().unwrap_or(""));
            label.set_attributes(None);
            if let Some(css) = state
                .choices
                .borrow()
                .get(item.position() as usize)
                .filter(|css| !css.is_empty())
            {
                crate::ui::rich_editor::preview_font(&label, css);
            }
        });
    }
    row.set_list_factory(Some(&list));
    fill(row, &state);

    row.connect_selected_notify(move |row| {
        if state.filling.get() {
            return;
        }
        let picked = state.choices.borrow().get(row.selected() as usize).cloned();
        if let Some(css) = picked {
            *state.chosen.borrow_mut() = css.clone();
            let _ = sender.output(PrefOutput::SetComposeFont(css));
            return;
        }
        // Other Font…: the row goes back to the font in use until the
        // chooser answers. Rebuilt on idle, not from inside its own signal.
        let (row, state, sender) = (row.clone(), state.clone(), sender.clone());
        gtk::glib::idle_add_local_once(move || {
            fill(&row, &state);
            let parent = row.root().and_downcast::<gtk::Window>();
            crate::ui::rich_editor::choose_family(parent.as_ref(), move |family| {
                let Some(family) = family else { return };
                let font = crate::config::RecentFont {
                    name: family.name().to_string(),
                    mono: family.is_monospace(),
                };
                let css = crate::ui::rich_editor::recent_font_css(&font);
                crate::config::note_recent_font(font);
                *state.chosen.borrow_mut() = css.clone();
                fill(&row, &state);
                let _ = sender.output(PrefOutput::SetComposeFont(css));
            });
        });
    });
}

impl Preferences {
    /// Whether a notification-button switch can be used: notifications are
    /// on, and either this button is on already or there is room for it.
    pub(super) fn button_row_sensitive(&self, button: crate::config::NotificationButton) -> bool {
        self.notifications
            && (self.notification_buttons.get(button) || !self.notification_buttons.full())
    }
}

/// Settings never truncates. AdwComboRow's DEFAULT item factory builds
/// the selected-value display with an ellipsizing label — and rebuilds
/// it on every selection change, so fixing the widget after the fact
/// doesn't stick ("Follow system" → "Follow s…"). Give every combo a
/// plain factory whose labels never ellipsize; the short row titles
/// yield the space instead.
pub(super) fn no_truncate(row: &adw::ComboRow) {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            item.set_child(Some(&label));
        }
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        if let (Some(label), Some(s)) = (
            item.child().and_downcast::<gtk::Label>(),
            item.item().and_downcast::<gtk::StringObject>(),
        ) {
            label.set_label(&s.string());
        }
    });
    row.set_factory(Some(&factory));
}
