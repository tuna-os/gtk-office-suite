use std::cmp::Ordering;

use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gio, glib, pango};
use ruma::{
    OwnedUserId, RoomVersionId,
    api::client::discovery::get_capabilities::v3::{RoomVersionStability, RoomVersionsCapability},
};
use tracing::error;

mod room_version;

use self::room_version::RoomVersion;
use crate::{session::JoinRuleValue, utils::OneshotNotifier};

mod imp {
    use std::cell::OnceCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/upgrade_dialog/mod.ui"
    )]
    pub struct UpgradeDialog {
        #[template_child]
        version_combo: TemplateChild<adw::ComboRow>,
        #[template_child]
        invite_only_warning_label: TemplateChild<gtk::Label>,
        #[template_child]
        creators_warning_label: TemplateChild<gtk::Label>,
        header_factory: OnceCell<gtk::SignalListItemFactory>,
        /// The notifier for the response of the user.
        notifier: OnceCell<OneshotNotifier<Option<RoomVersionId>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for UpgradeDialog {
        const NAME: &'static str = "RoomDetailsUpgradeDialog";
        type Type = super::UpgradeDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for UpgradeDialog {
        fn constructed(&self) {
            self.parent_constructed();

            self.version_combo
                .set_expression(Some(RoomVersion::this_expression("id-string")));
        }
    }

    impl WidgetImpl for UpgradeDialog {}

    impl AdwDialogImpl for UpgradeDialog {
        fn closed(&self) {
            if let Some(notifier) = self.notifier.get() {
                notifier.notify();
            }
        }
    }

    #[gtk::template_callbacks]
    impl UpgradeDialog {
        /// The notifier for the response of the user.
        fn notifier(&self) -> &OneshotNotifier<Option<RoomVersionId>> {
            self.notifier
                .get_or_init(|| OneshotNotifier::new("UpgradeDialog"))
        }

        /// The header factory to separate stable from experimental versions.
        fn header_factory(&self) -> &gtk::SignalListItemFactory {
            self.header_factory.get_or_init(|| {
                let header_factory = gtk::SignalListItemFactory::new();

                header_factory.connect_setup(|_, header| {
                    let Some(header) = header.downcast_ref::<gtk::ListHeader>() else {
                        error!("List item factory did not receive a list header: {header:?}");
                        return;
                    };

                    let label = gtk::Label::builder()
                        .margin_start(12)
                        .xalign(0.0)
                        .ellipsize(pango::EllipsizeMode::End)
                        .css_classes(["heading"])
                        .build();
                    header.set_child(Some(&label));
                });
                header_factory.connect_bind(|_, header| {
                    let Some(header) = header.downcast_ref::<gtk::ListHeader>() else {
                        error!("List item factory did not receive a list header: {header:?}");
                        return;
                    };
                    let Some(label) = header.child().and_downcast::<gtk::Label>() else {
                        error!("List header does not have a child GtkLabel");
                        return;
                    };
                    let Some(version) = header.item().and_downcast::<RoomVersion>() else {
                        error!("List header does not have a RoomVersion item");
                        return;
                    };

                    let text = if version.is_stable() {
                        // Translators: As in 'Stable version'.
                        gettext("Stable")
                    } else {
                        // Translators: As in 'Experimental version'.
                        gettext("Experimental")
                    };
                    label.set_label(&text);
                });

                header_factory
            })
        }

        /// Ask the user to confirm the room upgrade and select a room version
        /// among the ones that are supported by the server.
        ///
        /// Returns the selected room version, or `None` if the user cancelled
        /// the upgrade.
        pub(super) async fn confirm_upgrade(
            &self,
            info: &UpgradeInfo,
            parent: &gtk::Widget,
        ) -> Option<RoomVersionId> {
            self.update_version_combo(info);
            self.update_invite_only_warning(info);
            self.update_creators_warning(info);

            let receiver = self.notifier().listen();

            self.obj().present(Some(parent));

            receiver.await
        }

        /// Update the room versions combo row with the given details.
        fn update_version_combo(&self, info: &UpgradeInfo) {
            // Construct the list models for the combo row.
            let stable_model = (!info.stable_room_versions.is_empty()).then(|| {
                info.stable_room_versions
                    .iter()
                    .map(|version| RoomVersion::new(version.clone(), true))
                    .collect::<gio::ListStore>()
            });
            let unstable_model = (!info.unstable_room_versions.is_empty()).then(|| {
                info.unstable_room_versions
                    .iter()
                    .map(|version| RoomVersion::new(version.clone(), false))
                    .collect::<gio::ListStore>()
            });

            let use_header_factory = unstable_model.is_some();
            let model = match (stable_model, unstable_model) {
                (Some(model), None) | (None, Some(model)) => model.upcast::<gio::ListModel>(),
                (Some(stable_model), Some(unstable_model)) => {
                    let model_list = gio::ListStore::new::<gio::ListStore>();
                    model_list.append(&stable_model);
                    model_list.append(&unstable_model);
                    gtk::FlattenListModel::new(Some(model_list)).upcast()
                }
                // We always have at least the current room version.
                (None, None) => unreachable!(),
            };

            self.version_combo
                .set_header_factory(use_header_factory.then(|| self.header_factory()));
            self.version_combo.set_model(Some(&model));
            self.version_combo
                .set_selected(info.selected.try_into().unwrap_or(u32::MAX));
        }

        /// Update the invite-only warning.
        fn update_invite_only_warning(&self, info: &UpgradeInfo) {
            self.invite_only_warning_label
                .set_visible(info.join_rule == JoinRuleValue::Invite);
        }

        /// Update the creators warning.
        fn update_creators_warning(&self, info: &UpgradeInfo) {
            if info.other_creators_count == 0 {
                // We are not changing the list of privileged creators.
                self.creators_warning_label.set_visible(false);
                return;
            }

            // We don't use the count in the strings so we use separate gettext calls for
            // singular and plural rather than using ngettext.
            let text = if info.own_user_is_creator {
                if info.other_creators_count == 1 {
                    gettext(
                        "After the upgrade, you will be the only creator in the room. The other creator will be demoted to the default power level.",
                    )
                } else {
                    gettext(
                        "After the upgrade, you will be the only creator in the room. The other creators will be demoted to the default power level.",
                    )
                }
            } else if info.other_creators_count == 1 {
                gettext(
                    "After the upgrade, you will be the only creator in the room. The current creator will be demoted to the default power level.",
                )
            } else {
                gettext(
                    "After the upgrade, you will be the only creator in the room. The current creators will be demoted to the default power level.",
                )
            };

            self.creators_warning_label.set_label(&text);
            self.creators_warning_label.set_visible(true);
        }

        /// Confirm the upgrade.
        #[template_callback]
        fn upgrade(&self) {
            let room_version = self
                .version_combo
                .selected_item()
                .and_downcast::<RoomVersion>()
                .map(|v| v.id().clone());

            self.notifier().notify_value(room_version);
            self.obj().close();
        }

        /// Cancel the upgrade.
        #[template_callback]
        fn cancel(&self) {
            self.obj().close();
        }
    }
}

glib::wrapper! {
    /// Dialog to confirm a room upgrade and select a room version.
    pub struct UpgradeDialog(ObjectSubclass<imp::UpgradeDialog>)
        @extends gtk::Widget, adw::Dialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl UpgradeDialog {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Ask the user to confirm the room upgrade and select a room version among
    /// the ones that are supported by the server.
    ///
    /// Returns the selected room version, or `None` if the user cancelled the
    /// upgrade.
    pub(crate) async fn confirm_upgrade(
        &self,
        info: &UpgradeInfo,
        parent: &impl IsA<gtk::Widget>,
    ) -> Option<RoomVersionId> {
        self.imp().confirm_upgrade(info, parent.upcast_ref()).await
    }
}

/// The information necessary for [`UpgradeDialog`].
#[derive(Debug, Clone)]
pub(crate) struct UpgradeInfo {
    /// The sorted stable room versions available for the upgrade.
    pub(crate) stable_room_versions: Vec<RoomVersionId>,
    /// The sorted unstable room versions available for the upgrade.
    pub(crate) unstable_room_versions: Vec<RoomVersionId>,
    /// The position of the room version that should be selected by default,
    /// when `stable_room_versions` and `unstable_room_versions` are
    /// concatenated.
    pub(crate) selected: usize,
    /// Whether our own user is a privileged creator in the current room.
    pub(crate) own_user_is_creator: bool,
    /// The number of privileged creators that are not our own user in the
    /// current room.
    pub(crate) other_creators_count: usize,
    /// The current join rule of the room.
    pub(crate) join_rule: JoinRuleValue,
}

impl UpgradeInfo {
    /// Construct an empty `UpgradeInfo`.
    pub(crate) fn new(join_rule: JoinRuleValue) -> Self {
        Self {
            stable_room_versions: vec![],
            unstable_room_versions: vec![],
            selected: 0,
            own_user_is_creator: false,
            other_creators_count: 0,
            join_rule,
        }
    }

    /// Add information about the possible room versions for the upgrade.
    ///
    /// We do not allow users to:
    ///
    /// - Downgrade the room, i.e. use a lower room version.
    /// - Upgrade to a version lower than the server's default, if the server's
    ///   default is stable.
    /// - Upgrade to an experimental version, unless it is the current version
    ///   or the server's default.
    ///
    /// If the server's default is experimental, we also allow to upgrade to the
    /// highest stable version.
    pub(crate) fn with_room_versions(
        mut self,
        current_room_version: &RoomVersionId,
        capability: &RoomVersionsCapability,
    ) -> Self {
        let current_is_stable = capability.is_stable_version(current_room_version);
        let default_is_stable = capability.is_stable_version(&capability.default);
        let maximum_stable_version = capability.maximum_stable_version();

        // The minimum stable version is the highest stable version between the current
        // version and the default version.
        let minimum_stable_version = match (current_is_stable, default_is_stable) {
            (true, false) => Some(current_room_version),
            (false, true) => Some(&capability.default),
            (true, true) => Some(
                match cmp_room_versions(current_room_version, &capability.default) {
                    Ordering::Less => &capability.default,
                    Ordering::Equal | Ordering::Greater => current_room_version,
                },
            ),
            (false, false) => None,
        };
        let selected_room_version = minimum_stable_version.unwrap_or(&capability.default);

        self.stable_room_versions = if let Some(minimum) = minimum_stable_version {
            // Keep all the stable versions higher than the minimum.
            capability
                .available
                .iter()
                .filter_map(|(version, stability)| {
                    // Discard unstable versions.
                    if *stability != RoomVersionStability::Stable {
                        return None;
                    }

                    if cmp_room_versions(version, minimum) != Ordering::Less
                        || maximum_stable_version.is_some_and(|maximum| maximum == version)
                    {
                        Some(version)
                    } else {
                        None
                    }
                })
                .cloned()
                .collect()
        } else {
            // The only allowed stable version will be the maximum.
            maximum_stable_version.into_iter().cloned().collect()
        };

        // Add the current version if it is unstable.
        if !current_is_stable {
            self.unstable_room_versions
                .push(current_room_version.clone());
        }

        // Add the default version if it is different from the current version and it is
        // unstable.
        if *current_room_version != capability.default && !default_is_stable {
            self.unstable_room_versions.push(capability.default.clone());
        }

        // Sort all the versions.
        self.stable_room_versions
            .sort_unstable_by(cmp_room_versions);
        self.unstable_room_versions
            .sort_unstable_by(cmp_room_versions);

        // Find the position of the selected version.
        self.selected = self
            .stable_room_versions
            .binary_search_by(|version| cmp_room_versions(version, selected_room_version))
            .or_else(|_| {
                self.unstable_room_versions
                    .binary_search_by(|version| cmp_room_versions(version, selected_room_version))
                    .map(|pos| self.stable_room_versions.len() + pos)
            })
            .unwrap_or_default();

        self
    }

    /// Add information about the privileged creators changes.
    pub(crate) fn with_privileged_creators(
        mut self,
        own_creator: &OwnedUserId,
        privileged_creators: &[OwnedUserId],
    ) -> Self {
        self.own_user_is_creator = privileged_creators.contains(own_creator);
        self.other_creators_count =
            privileged_creators.len() - usize::from(self.own_user_is_creator);
        self
    }
}

/// Helper trait for [`RoomVersionsCapability`].
trait RoomVersionsCapabilityExt {
    /// Whether the given room version is stable.
    fn is_stable_version(&self, version: &RoomVersionId) -> bool;

    /// The maximum stable room version in these capabilities.
    fn maximum_stable_version(&self) -> Option<&RoomVersionId>;
}

impl RoomVersionsCapabilityExt for RoomVersionsCapability {
    fn is_stable_version(&self, version: &RoomVersionId) -> bool {
        self.available
            .get(version)
            .is_some_and(|stability| *stability == RoomVersionStability::Stable)
    }

    fn maximum_stable_version(&self) -> Option<&RoomVersionId> {
        self.available
            .iter()
            .fold(None, |maximum, (version, stability)| {
                // Discard unstable versions.
                if *stability != RoomVersionStability::Stable {
                    return maximum;
                }

                // Keep the maximum.
                if maximum
                    .is_none_or(|maximum| cmp_room_versions(version, maximum) == Ordering::Greater)
                {
                    Some(version)
                } else {
                    maximum
                }
            })
    }
}

/// Compare the given room versions to attempt to order them.
///
/// Note that the Matrix specification doesn't actually define a grammar or an
/// order for room versions, but so far they have been numbers that grow
/// incrementally.
fn cmp_room_versions(lhs: &RoomVersionId, rhs: &RoomVersionId) -> Ordering {
    cmp_numbers_aware_str(lhs.as_str(), rhs.as_str())
}

/// Compare the given strings by using numbers-aware comparison.
///
/// When we encounter a sequence of digits at the same position in both strings,
/// we compare the values of the numbers. The rest is sorted lexicographically.
fn cmp_numbers_aware_str(lhs: &str, rhs: &str) -> Ordering {
    let lhs_seq = CharSequence::new(lhs);
    let rhs_seq = CharSequence::new(rhs);

    // Early return for empty strings.
    let (lhs_seq, rhs_seq) = match (lhs_seq, rhs_seq) {
        (None, None) => return Ordering::Equal,
        (None, Some(_)) => return Ordering::Less,
        (Some(_), None) => return Ordering::Greater,
        // We need to compare the sequences now.
        (Some(lhs_seq), Some(rhs_seq)) => (lhs_seq, rhs_seq),
    };

    let cmp = lhs_seq.cmp(rhs_seq);

    if cmp.is_eq() {
        // Compare the next sequences.
        cmp_numbers_aware_str(&lhs[lhs_seq.len()..], &rhs[rhs_seq.len()..])
    } else {
        cmp
    }
}

/// A sequence of consecutive digit or non-digit characters.
#[derive(Debug, Clone, Copy)]
enum CharSequence<'a> {
    /// A sequence of digits characters.
    Digits(&'a str),

    /// A sequence of non-digit characters.
    Other(&'a str),
}

impl<'a> CharSequence<'a> {
    /// Get the first sequence of consecutive digit or non-digit characters.
    ///
    /// Returns `None` if the string is empty.
    fn new(string: &'a str) -> Option<Self> {
        // The first character decides the kind of sequence. If there are no characters,
        // the string is empty so we return early.
        let is_digit_seq = string.chars().next()?.is_ascii_digit();

        // Find the end of the sequence.
        let seq_end = string
            .char_indices()
            .find(|(_, c)| is_digit_seq ^ c.is_ascii_digit())
            .map_or(string.len(), |(pos, _)| pos);

        let seq = &string[..seq_end];

        Some(if is_digit_seq {
            Self::Digits(seq)
        } else {
            Self::Other(seq)
        })
    }

    /// Get the length of this sequence in bytes.
    fn len(self) -> usize {
        match self {
            Self::Digits(s) | Self::Other(s) => s.len(),
        }
    }

    /// Compare this sequence with another one.
    fn cmp(self, other: Self) -> Ordering {
        match (self, other) {
            // If one is a sequence of digits and not the other, we can just compare the strings, it
            // will be ordered lexicographically by the first character. And if they are both
            // non-digit sequences, just compare the strings.
            (Self::Digits(lhs) | Self::Other(lhs), Self::Other(rhs))
            | (Self::Other(lhs), Self::Digits(rhs)) => lhs.cmp(rhs),
            // Compare the actual numeric values of digit sequences. To do that we just remove the
            // leading zeroes and compare the length, then the strings.
            (Self::Digits(lhs), Self::Digits(rhs)) => {
                let lhs = lhs.trim_start_matches('0');
                let rhs = rhs.trim_start_matches('0');

                lhs.len().cmp(&rhs.len()).then_with(|| lhs.cmp(rhs))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::cmp_numbers_aware_str;

    #[test]
    fn compare_room_versions() {
        // Compare digits.
        assert_eq!(cmp_numbers_aware_str("1", "1"), Ordering::Equal);
        assert_eq!(cmp_numbers_aware_str("1", "2"), Ordering::Less);
        assert_eq!(cmp_numbers_aware_str("2", "1"), Ordering::Greater);
        assert_eq!(cmp_numbers_aware_str("2", "10"), Ordering::Less);
        assert_eq!(cmp_numbers_aware_str("10", "2"), Ordering::Greater);
        assert_eq!(cmp_numbers_aware_str("0002", "010"), Ordering::Less);
        assert_eq!(cmp_numbers_aware_str("010", "0002"), Ordering::Greater);

        // Compare non-digits.
        assert_eq!(cmp_numbers_aware_str("", "abc"), Ordering::Less);
        assert_eq!(cmp_numbers_aware_str("abc", ""), Ordering::Greater);
        assert_eq!(cmp_numbers_aware_str("ab", "abc"), Ordering::Less);
        assert_eq!(cmp_numbers_aware_str("abc", "ab"), Ordering::Greater);
        assert_eq!(cmp_numbers_aware_str("abc", "abc"), Ordering::Equal);
        assert_eq!(cmp_numbers_aware_str("abc", "d"), Ordering::Less);
        assert_eq!(cmp_numbers_aware_str("d", "abc"), Ordering::Greater);

        // Compare digits with non-digits.
        assert_eq!(cmp_numbers_aware_str("1", "abc"), Ordering::Less);
        assert_eq!(cmp_numbers_aware_str("abc", "1"), Ordering::Greater);
        assert_eq!(cmp_numbers_aware_str("9999", "a"), Ordering::Less);
        assert_eq!(cmp_numbers_aware_str("a", "9999"), Ordering::Greater);
        assert_eq!(cmp_numbers_aware_str("1", "."), Ordering::Greater);
        assert_eq!(cmp_numbers_aware_str(".", "1"), Ordering::Less);

        // Compare mix of digits and non-digits.
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.msc3757.10", "org.matrix.msc3757.10"),
            Ordering::Equal
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.11.hydra", "org.matrix.11.hydra"),
            Ordering::Equal
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.msc3757.10", "org.matrix.msc3757.11"),
            Ordering::Less
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.msc3757.11", "org.matrix.msc3757.10"),
            Ordering::Greater
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.msc3757.10", "org.matrix.hydra.11"),
            Ordering::Greater
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.hydra.11", "org.matrix.msc3757.10"),
            Ordering::Less
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.msc3757.11", "org.matrix.hydra.11"),
            Ordering::Greater
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.hydra.11", "org.matrix.msc3757.11"),
            Ordering::Less
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.10.msc3757", "org.matrix.11.hydra"),
            Ordering::Less
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.11.hydra", "org.matrix.10.msc3757"),
            Ordering::Greater
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.12.msc3757", "org.matrix.11.hydra"),
            Ordering::Greater
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.11.hydra", "org.matrix.12.msc3757"),
            Ordering::Less
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.11.hydra", "org.matrix.0011.hydra"),
            Ordering::Equal
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.11.hydra", "org.matrix.0010.hydra"),
            Ordering::Greater
        );
        assert_eq!(
            cmp_numbers_aware_str("org.matrix.11.hydra", "org.matrix.1.hydra"),
            Ordering::Greater
        );
    }
}
