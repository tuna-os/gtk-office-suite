//! Spike Q15 (spec 8.1, question 15): a Matrix reader judges a DCP event's
//! sender by the power level at the event. Six scenarios on a real Synapse,
//! relaxed or default rate limits (`DCP_SYNAPSE_PROFILE`), each with negative
//! controls inside it (`dcp_interop::power_at_event` says which). Needs
//! `DCP_SYNAPSE_URL`; fails without it unless `DCP_SKIP_SYNAPSE=1`, which
//! prints SKIPPED.

use dcp_interop::power_at_event as q15;
use dcp_interop::profile;
use dcp_interop::rig::synapse;
use dcp_interop::RigOptions;

/// `test => scenario` on a room with the literal draft-0 power-level table: the
/// server accepts what a demoted or never-promoted user sends, so only the
/// reader's rule decides.
macro_rules! draft0 {
    ($($test:ident => $scenario:ident),+ $(,)?) => {
        $(
            #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
            async fn $test() {
                let Some(syn) = synapse() else { return };
                let name = concat!(stringify!($scenario), "@draft0");
                let r = profile::run_check(&syn, name, RigOptions::honest().draft0_power_levels(), q15::by_name(stringify!($scenario)).run).await;
                profile::judge(&syn, name, r);
            }
        )+

        const TESTED_ON_DRAFT0: &[&str] = &[$(stringify!($scenario)),+];
    };
}

draft0!(
    s1_an_editor_demoted_after_publishing_keeps_their_version => an_editor_demoted_after_publishing_keeps_their_version_and_the_owners_later_save,
    s2_a_demoted_editors_later_events_are_dropped => a_demoted_editors_later_events_are_dropped_and_their_earlier_ones_stay,
    s3_a_later_promotion_does_not_validate_an_earlier_forgery => a_later_promotion_does_not_validate_an_earlier_forged_event,
    s4_a_late_joiner_sees_the_verdicts_of_a_live_reader => a_late_joiner_sees_the_verdicts_of_a_live_reader,
    s5_power_levels_changed_several_times_in_a_burst => power_levels_changed_several_times_in_a_burst,
    s6_a_hidden_power_levels_change_does_not_mislead_the_readers_that_check => a_hidden_power_levels_change_does_not_mislead_the_readers_that_check,
);

/// Scenario 1 again in a room with the spec's table (the server gates
/// `m.room.encrypted` at 50): every reader, the current-table one included, is
/// right, so the new rule changes nothing where the server already protects.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s1_in_a_gated_room_every_reader_is_right() {
    let Some(syn) = synapse() else { return };
    let name = "an_editor_demoted_after_publishing_keeps_their_version_and_the_owners_later_save@gated";
    let r = profile::run_check(
        &syn,
        name,
        RigOptions::honest(),
        q15::by_name("an_editor_demoted_after_publishing_keeps_their_version_and_the_owners_later_save").run,
    )
    .await;
    profile::judge(&syn, name, r);
}

/// Needs no Synapse: every scenario has a draft-0 test above (scenario 1 also a gated one).
#[test]
fn every_scenario_has_a_test() {
    let all: Vec<&str> = q15::ALL.iter().map(|c| c.name).collect();
    assert_eq!(all, TESTED_ON_DRAFT0, "power_at_event::ALL and the tests in this file differ");
}
