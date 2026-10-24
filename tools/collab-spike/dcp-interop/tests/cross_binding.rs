//! The Matrix and WOPI bindings against each other on a real Synapse (relaxed
//! rate limits): one test per cross-binding check in `dcp_interop::checks`,
//! each on a fresh document. Needs `DCP_SYNAPSE_URL`; fails without it unless
//! `DCP_SKIP_SYNAPSE=1`.

use dcp_interop::checks;
use dcp_interop::rig::synapse;
use dcp_interop::{Rig, RigOptions};

/// One `#[tokio::test]` per check. The list is also compared with
/// `checks::ALL`, so a check that has no test here fails
/// `every_check_has_a_test`.
macro_rules! cross_binding_tests {
    ($($name:ident),+ $(,)?) => {
        $(
            #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
            async fn $name() {
                let Some(syn) = synapse() else { return };
                let rig = Rig::new(&syn, RigOptions::honest()).await.expect("rig");
                if let Err(f) = checks::$name(&rig).await {
                    panic!("cross-binding check {} failed: {f}", stringify!($name));
                }
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
    removed_members_wopi_token_gets_no_new_content_and_no_write,
);

/// Needs no Synapse: the registry and the tests cannot drift apart.
#[test]
fn every_check_has_a_test() {
    let all: Vec<&str> = checks::ALL.iter().map(|c| c.name).collect();
    assert_eq!(all, TESTED, "checks::ALL and the tests in this file differ");
}
