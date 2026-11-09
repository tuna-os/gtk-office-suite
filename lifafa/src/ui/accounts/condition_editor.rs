//! The condition groups the filter and smart-folder pages share (#192,
//! #417): one titled group per condition, an Add Condition row, and the
//! all-or-any chooser once there is more than one.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use super::FormSave;
use crate::i18n::{i18n, i18n_f};

/// A form page's scrolling body: groups stacked the way an
/// `AdwPreferencesPage` stacks them, in a box of our own so condition
/// groups can slot in between the fixed ones.
pub(super) fn form_scaffold() -> (gtk::Box, gtk::ScrolledWindow) {
    let page_box = gtk::Box::new(gtk::Orientation::Vertical, 24);
    page_box.set_margin_top(24);
    page_box.set_margin_bottom(24);
    page_box.set_margin_start(12);
    page_box.set_margin_end(12);
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(600);
    clamp.set_tightening_threshold(400);
    clamp.set_child(Some(&page_box));
    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&clamp));
    (page_box, scrolled)
}

/// One condition's group: Where, Match and the text to match, with a
/// remove button in its header (hidden while it is the only one).
pub(super) struct CondRows {
    pub(super) group: adw::PreferencesGroup,
    remove: gtk::Button,
    pub(super) field: adw::ComboRow,
    pub(super) matcher: adw::ComboRow,
    pub(super) value: adw::EntryRow,
}

impl CondRows {
    /// The group and its rows, not yet added to the group: the page sets
    /// them from the condition first.
    pub(super) fn new(field_names: &[String], match_names: &[String]) -> Self {
        let group = adw::PreferencesGroup::new();
        let remove = gtk::Button::from_icon_name("user-trash-symbolic");
        remove.add_css_class("flat");
        remove.set_valign(gtk::Align::Center);
        remove.set_tooltip_text(Some(i18n("Remove condition").as_str()));
        group.set_header_suffix(Some(&remove));
        let field = adw::ComboRow::new();
        field.set_title(&i18n("Where"));
        let refs: Vec<&str> = field_names.iter().map(|s| s.as_str()).collect();
        field.set_model(Some(&gtk::StringList::new(&refs)));
        let matcher = adw::ComboRow::new();
        matcher.set_title(&i18n("Match"));
        matcher.set_subtitle(&i18n("Separate alternatives with commas."));
        let refs: Vec<&str> = match_names.iter().map(|s| s.as_str()).collect();
        matcher.set_model(Some(&gtk::StringList::new(&refs)));
        let value = adw::EntryRow::new();
        value.set_title(&i18n("Text to match"));
        CondRows {
            group,
            remove,
            field,
            matcher,
            value,
        }
    }

    /// Put Where, Match and the text in the group, in that order.
    pub(super) fn add_rows(&self) {
        self.group.add(&self.field);
        self.group.add(&self.matcher);
        self.group.add(&self.value);
    }
}

impl AsRef<CondRows> for CondRows {
    fn as_ref(&self) -> &CondRows {
        self
    }
}

/// The rows under the conditions: Add Condition, and the all-or-any
/// chooser in a group of its own, hidden until there is a second
/// condition.
pub(super) struct CondControls {
    pub(super) add_group: adw::PreferencesGroup,
    pub(super) add_row: adw::ActionRow,
    pub(super) combine_row: adw::ComboRow,
    pub(super) combine_group: adw::PreferencesGroup,
}

impl CondControls {
    pub(super) fn new() -> Self {
        let add_group = adw::PreferencesGroup::new();
        let add_row = adw::ActionRow::new();
        add_row.set_title(&i18n("Add Condition"));
        add_row.set_activatable(true);
        add_row.add_prefix(&gtk::Image::from_icon_name("list-add-symbolic"));
        add_group.add(&add_row);
        let combine_row = adw::ComboRow::new();
        combine_row.set_title(&i18n("Condition matching"));
        combine_row.set_model(Some(&gtk::StringList::new(&[
            i18n("All must match").as_str(),
            i18n("Any one may match").as_str(),
        ])));
        let combine_group = adw::PreferencesGroup::new();
        combine_group.add(&combine_row);
        combine_group.set_visible(false);
        CondControls {
            add_group,
            add_row,
            combine_row,
            combine_group,
        }
    }

    /// Number the groups, title the later Where rows after the combining
    /// rule ("And where" / "Or where"), and show the remove buttons and
    /// the chooser once there is more than one condition. Runs again
    /// whenever the rule changes.
    pub(super) fn relabeller<T: AsRef<CondRows> + 'static>(
        &self,
        conds: &Rc<RefCell<Vec<T>>>,
    ) -> impl Fn() + Clone + 'static {
        let relabel = {
            let conds = conds.clone();
            let combine_row = self.combine_row.clone();
            let combine_group = self.combine_group.clone();
            move || {
                let conds = conds.borrow();
                let any = combine_row.selected() == 1;
                let several = conds.len() > 1;
                for (i, c) in conds.iter().enumerate() {
                    let c = c.as_ref();
                    c.group
                        .set_title(&i18n_f("Condition {n}", &[("n", &(i + 1).to_string())]));
                    c.field.set_title(&if i == 0 {
                        i18n("Where")
                    } else if any {
                        i18n("Or where")
                    } else {
                        i18n("And where")
                    });
                    c.remove.set_visible(several);
                }
                combine_group.set_visible(several);
            }
        };
        {
            let relabel = relabel.clone();
            self.combine_row.connect_selected_notify(move |_| relabel());
        }
        relabel
    }
}

/// Everything adding a condition needs besides the condition itself.
pub(super) struct CondAdder<T, M, R, C> {
    pub(super) conds: Rc<RefCell<Vec<T>>>,
    pub(super) page_box: gtk::Box,
    /// The group the first condition goes under; later ones go under the
    /// last condition.
    pub(super) anchor: gtk::Widget,
    pub(super) make: M,
    pub(super) relabel: R,
    pub(super) form_save: FormSave,
    pub(super) nav: adw::NavigationView,
    /// Called when a condition's field or text changes, after one is
    /// removed and after one is added (the filter page's Save check).
    pub(super) changed: Option<C>,
}

impl<T, M, R, C> CondAdder<T, M, R, C>
where
    T: AsRef<CondRows> + 'static,
    R: Fn() + Clone + 'static,
    C: Fn() + Clone + 'static,
{
    /// Add a condition group below the ones there are, above the Add
    /// Condition group.
    pub(super) fn add<I>(&self, init: Option<&I>)
    where
        M: Fn(Option<&I>) -> T,
    {
        let item = (self.make)(init);
        let c = item.as_ref();
        if let Some(changed) = &self.changed {
            {
                let changed = changed.clone();
                c.value.connect_changed(move |_| changed());
            }
            {
                let changed = changed.clone();
                c.field.connect_selected_notify(move |_| changed());
            }
        }
        let after: gtk::Widget = self
            .conds
            .borrow()
            .last()
            .map(|last| last.as_ref().group.clone().upcast())
            .unwrap_or_else(|| self.anchor.clone());
        self.page_box.insert_child_after(&c.group, Some(&after));
        {
            let conds = self.conds.clone();
            let page_box = self.page_box.clone();
            let relabel = self.relabel.clone();
            let changed = self.changed.clone();
            let group = c.group.clone();
            c.remove.connect_clicked(move |_| {
                let gone = {
                    let mut conds = conds.borrow_mut();
                    if conds.len() < 2 {
                        return;
                    }
                    conds
                        .iter()
                        .position(|c| c.as_ref().group == group)
                        .map(|i| conds.remove(i))
                };
                if let Some(gone) = gone {
                    page_box.remove(&gone.as_ref().group);
                    relabel();
                    if let Some(changed) = &changed {
                        changed();
                    }
                }
            });
        }
        // Enter in the text saves, as in every form page; the save
        // closure is looked up when pressed, so a condition added
        // before the page is up gets it too.
        {
            let form_save = self.form_save.clone();
            let nav = self.nav.clone();
            c.value.connect_entry_activated(move |_| {
                let saved = form_save.borrow().as_ref().is_some_and(|save| save());
                if saved {
                    nav.pop();
                }
            });
        }
        self.conds.borrow_mut().push(item);
        (self.relabel)();
        if let Some(changed) = &self.changed {
            changed();
        }
    }
}
