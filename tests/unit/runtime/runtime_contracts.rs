#![cfg(windows)]

use std::time::{Duration, SystemTime};

use localbridge_lib::runtime::{
    ProcessGeneration, ProcessSnapshot, SnapshotDisposition, classify_persisted_snapshot,
};

fn snapshot(generation: u64, pid: u32, creation: u64, role: &str) -> ProcessSnapshot {
    ProcessSnapshot {
        role: role.to_string(),
        pid,
        generation: ProcessGeneration::from_persisted_value(generation),
        creation_time_100ns: creation,
        started_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1),
    }
}

#[test]
fn stale_generation_is_never_current_even_if_pid_matches() {
    let current = snapshot(8, 4242, 100, "runtime");
    let stale = snapshot(7, 4242, 100, "runtime");
    assert_eq!(
        classify_persisted_snapshot(&current, &stale),
        SnapshotDisposition::StaleGeneration
    );
}

#[test]
fn pid_reuse_is_rejected_by_creation_identity() {
    let current = snapshot(8, 4242, 200, "runtime");
    let reused_pid = snapshot(8, 4242, 100, "runtime");
    assert_eq!(
        classify_persisted_snapshot(&current, &reused_pid),
        SnapshotDisposition::ProcessIdentityMismatch
    );
}

#[test]
fn role_is_part_of_diagnostic_identity() {
    let current = snapshot(8, 4242, 200, "runtime-a");
    let wrong_role = snapshot(8, 4242, 200, "runtime-b");
    assert_eq!(
        classify_persisted_snapshot(&current, &wrong_role),
        SnapshotDisposition::ProcessIdentityMismatch
    );
}
