//! The Matrix and WOPI bindings against each other on a real Synapse, relaxed
//! or default rate limits (`DCP_SYNAPSE_PROFILE`): one test per cross-binding
//! check in `dcp_interop::checks`, each on a fresh document. Needs
//! `DCP_SYNAPSE_URL`; fails without it unless `DCP_SKIP_SYNAPSE=1`. Each check
//! is timed (`TIMING` lines, and `DCP_MEASURE_OUT`) with the setup of its rig
//! counted apart; `dcp_interop::profile` says what the default profile changes.

use dcp_interop::checks;
use dcp_interop::profile;
use dcp_interop::rig::synapse;
use dcp_interop::RigOptions;

/// One `#[tokio::test]` per check. The list is also compared with
/// `checks::ALL`, so a check that has no test here fails
/// `every_check_has_a_test`.
macro_rules! cross_binding_tests {
    ($($name:ident),+ $(,)?) => {
        $(
            #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
            async fn $name() {
                let Some(syn) = synapse() else { return };
                let name = stringify!($name);
                let r = profile::run_check(&syn, name, RigOptions::honest(), checks::by_name(name).run).await;
                profile::judge(&syn, name, r);
            }
        )+

        const TESTED: &[&str] = &[$(stringify!($name)),+];
    };
}

cross_binding_tests!(
    wopi_publish_is_read_by_a_matrix_user,
    native_publish_opens_through_wopi_getfile,
    native_publish_between_getfile_and_putfile_is_a_409,
    forced_wopi_save_forks_and_matrix_shows_the_conflict,
    native_offline_publish_shows_as_a_conflict_to_the_matrix_reader,
    viewer_token_cannot_write,
    wopi_lock_blocks_native_acquire_and_the_reverse,
    native_lease_blocks_wopi_save,
    owner_break_invalidates_the_wopi_lock,
    native_lease_expiry_frees_the_wopi_lock,
    wopi_lock_refresh_is_a_new_record_on_matrix,
    wopi_lock_on_an_empty_document_omits_base_version,
    removed_members_wopi_token_gets_no_new_content_and_no_write,
    forged_version_is_dropped_by_matrix_readers,
    forged_version_is_dropped_by_wopi_readers,
    forged_update_from_a_viewer_is_dropped_by_readers,
    a_demoted_editor_cannot_publish_and_readers_drop_what_slips_through,
    put_file_on_a_conflicted_document_is_refused,
    refresh_lock_and_unlock_and_relock_are_new_lease_records_on_matrix,
    two_wopi_sessions_on_one_document_never_share_a_lock,
    two_wopi_hosts_racing_for_one_lock_have_one_winner,
);

/// The forgery checks (and the demoted-editor one) in a room created with the
/// literal draft-0 power-level table: Synapse accepts the forged events (the
/// checks assert that it did, so they cannot pass vacuously), and every reader
/// drops them (native and through WOPI for versions; native for updates).
macro_rules! draft0_tests {
    ($($test:ident => $check:ident),+ $(,)?) => {
        $(
            #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
            async fn $test() {
                let Some(syn) = synapse() else { return };
                // Its own key in the timing records and in the default-profile
                // failure table: the draft-0 room is a different run.
                let name = concat!(stringify!($check), "@draft0");
                let r = profile::run_check(&syn, name, RigOptions::honest().draft0_power_levels(), checks::by_name(stringify!($check)).run).await;
                profile::judge(&syn, name, r);
            }
        )+
    };
}

draft0_tests!(
    a_draft0_server_accepts_the_forgery_and_matrix_readers_drop_it => forged_version_is_dropped_by_matrix_readers,
    a_draft0_server_accepts_the_forgery_and_wopi_readers_drop_it => forged_version_is_dropped_by_wopi_readers,
    a_draft0_server_accepts_a_forged_update_and_matrix_readers_drop_it => forged_update_from_a_viewer_is_dropped_by_readers,
    a_draft0_server_accepts_a_demoted_editors_events_and_readers_drop_them => a_demoted_editor_cannot_publish_and_readers_drop_what_slips_through,
);

/// Needs no Synapse: the registry and the tests cannot drift apart.
#[test]
fn every_check_has_a_test() {
    let all: Vec<&str> = checks::ALL.iter().map(|c| c.name).collect();
    assert_eq!(all, TESTED, "checks::ALL and the tests in this file differ");
}
