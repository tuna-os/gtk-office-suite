//! One row of Settings → Accounts → Senders: an allowed or blocked address
//! with a button that removes it.

use adw::prelude::*;
use relm4::prelude::*;

use crate::i18n::i18n;

/// An address on the allow list or the blocklist.
pub(super) struct SenderRow {
    addr: String,
}

#[derive(Debug)]
pub(super) enum SenderRowOutput {
    /// The row's trash button: take this address off its list.
    Remove(String),
}

#[relm4::factory(pub(crate))]
impl FactoryComponent for SenderRow {
    type Init = String;
    type Input = ();
    type Output = SenderRowOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::ListBox;

    view! {
        adw::ActionRow {
            set_title: &self.addr,
            add_suffix = &gtk::Button {
                set_icon_name: "user-trash-symbolic",
                set_valign: gtk::Align::Center,
                set_tooltip_text: Some(i18n("Remove").as_str()),
                add_css_class: "flat",
                connect_clicked[sender, addr = self.addr.clone()] => move |_| {
                    let _ = sender.output(SenderRowOutput::Remove(addr.clone()));
                },
            },
        }
    }

    fn init_model(addr: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        Self { addr }
    }
}
