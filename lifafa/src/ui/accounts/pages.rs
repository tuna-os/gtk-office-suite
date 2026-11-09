//! The accounts panel's pages (accounts, tags, smart folders, filters,
//! senders) and the account editor's form, one builder each.
//!
//! Each is a standalone `relm4::view!` run from `AccountsWindow::init`
//! before the panel's own view, which places it with `#[local_ref]`; the
//! widgets the panel reaches later are kept in the page's struct
//! (`widgets.form.name_row`). The panel's view has no `#[watch]`, so there
//! is nothing to update here.

mod accounts;
mod filters;
mod form;
mod senders;
mod smart;
mod tags;

pub(super) use accounts::AccountsPage;
pub(super) use filters::FiltersPage;
pub(super) use form::EditorForm;
pub(super) use senders::SendersPage;
pub(super) use smart::SmartPage;
pub(super) use tags::TagsPage;
