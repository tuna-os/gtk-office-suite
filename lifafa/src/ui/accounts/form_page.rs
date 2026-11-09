//! The pages pushed over the panel to edit one filter, tag or smart folder.

use super::*;

impl AccountsWindow {
    /// Slide a form in as its own page, the way the account editor does:
    /// its header carries the window's close button and a Save button;
    /// the back button (or a swipe) leaves without saving. `save` reads
    /// the form and returns true once it went through, which pops the page.
    pub(super) fn push_form_page(
        &self,
        nav: &adw::NavigationView,
        tag: &str,
        title: &str,
        verb: &str,
        form: &gtk::ListBox,
        save: impl Fn() -> bool + 'static,
    ) {
        let group = adw::PreferencesGroup::new();
        group.add(form);
        let page = adw::PreferencesPage::new();
        page.add(&group);
        // Enter in an entry saves, like the dialog's default response did.
        let mut entries = Vec::new();
        let mut child = form.first_child();
        while let Some(c) = child {
            if let Some(entry) = c.downcast_ref::<adw::EntryRow>() {
                entries.push(entry.clone());
            }
            child = c.next_sibling();
        }
        self.push_form_content(nav, tag, title, verb, page.upcast_ref(), entries, save);
    }

    /// [`push_form_page`] for any content widget: the filter editor builds
    /// its own page of groups. `entries` are the entry rows whose Enter
    /// should save. Returns the Save button, for a form that dims it while
    /// it cannot save.
    // Each argument is a different part of the page; a struct would only
    // name them twice.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_form_content(
        &self,
        nav: &adw::NavigationView,
        tag: &str,
        title: &str,
        verb: &str,
        content: &gtk::Widget,
        entries: Vec<adw::EntryRow>,
        save: impl Fn() -> bool + 'static,
    ) -> gtk::Button {
        let save: std::rc::Rc<dyn Fn() -> bool> = std::rc::Rc::new(save);
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(true);
        let save_btn = gtk::Button::with_label(verb);
        save_btn.add_css_class("suggested-action");
        {
            let nav = nav.clone();
            let save = save.clone();
            save_btn.connect_clicked(move |_| {
                if save() {
                    nav.pop();
                }
            });
        }
        header.pack_end(&save_btn);

        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(content));

        let nav_page = adw::NavigationPage::new(&view, title);
        nav_page.set_tag(Some(tag));
        for entry in entries {
            let nav = nav.clone();
            let save = save.clone();
            entry.connect_entry_activated(move |_| {
                if save() {
                    nav.pop();
                }
            });
        }
        *self.form_save.borrow_mut() = Some(Box::new(move || save()));
        nav.push(&nav_page);
        save_btn
    }
}
