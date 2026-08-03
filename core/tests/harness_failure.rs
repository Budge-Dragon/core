//! The shared dataset harness's failure mode is itself a contract: a load
//! failure must fail the calling test and leave the rest of the binary running.
//! A helper that kills the process instead reports no test name, prints no
//! result summary, and discards every sibling test — so the property is proven
//! by a pair, not by one test.

#[path = "common/dataset.rs"]
mod dataset;

use dataset::{or_fail, real_atlas};
use mu_core::components::units::MapNumber;

#[test]
#[should_panic(expected = "mu-core test harness: broken checkout probe")]
fn a_load_failure_fails_only_the_calling_test() {
    let _never_arrives: u8 = or_fail(Err("broken checkout probe"));
}

/// The co-victim: a process-killing helper takes this test down with it, so its
/// green is half the proof above.
#[test]
fn a_sibling_test_still_runs_and_loads_the_real_dataset() {
    assert!(
        real_atlas().map_handle(MapNumber(0)).is_some(),
        "the real shipped dataset still loads alongside a failing sibling"
    );
}
