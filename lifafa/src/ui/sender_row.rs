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

/// The row's widgets are all set once, from the address, so nothing is kept
/// to update; this is what `#[relm4::factory]`'s `view!` block used to
/// generate.
impl FactoryComponent for SenderRow {
    type Init = String;
    type Input = ();
    type Output = SenderRowOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::ListBox;
    type Root = adw::ActionRow;
    type Widgets = ();
    type Index = DynamicIndex;

    fn init_model(addr: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        Self { addr }
    }

    fn init_root(&self) -> Self::Root {
        adw::ActionRow::builder().title(self.addr.as_str()).build()
    }

    fn init_widgets(
        &mut self,
        _index: &DynamicIndex,
        root: Self::Root,
        _returned_widget: &gtk::Widget,
        sender: FactorySender<Self>,
    ) -> Self::Widgets {
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(i18n("Remove").as_str())
            .css_classes(["flat"])
            .build();
        let addr = self.addr.clone();
        remove.connect_clicked(move |_| {
            let _ = sender.output(SenderRowOutput::Remove(addr.clone()));
        });
        root.add_suffix(&remove);
    }
}
