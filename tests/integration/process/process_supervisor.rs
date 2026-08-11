#![cfg(windows)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use localbridge_lib::runtime::{
    ManagedProcessSpec, ProcessSnapshot, SnapshotDisposition, StopDisposition,
    WindowsProcessSupervisor,
};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};

const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;

fn helper_dir(root_pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("localbridge-lb004-{root_pid}"))
}

fn helper_spec() -> ManagedProcessSpec {
    ManagedProcessSpec::new("lb004-test-root", std::env::current_exe().unwrap())
        .unwrap()
        .args(["--ignored", "--exact", "helper_root", "--nocapture"])
}

fn exit_259_spec() -> ManagedProcessSpec {
    ManagedProcessSpec::new("lb004-exit-259", std::env::current_exe().unwrap())
        .unwrap()
        .args(["--ignored", "--exact", "helper_exit_259", "--nocapture"])
}

fn wait_for_file(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {}", path.display());
}

fn wait_for_dead(pid: u32, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_alive(pid) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("pid {pid} remained alive");
}

fn pid_alive(pid: u32) -> bool {
    let handle: HANDLE = unsafe { OpenProcess(SYNCHRONIZE_ACCESS, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let wait = unsafe { WaitForSingleObject(handle, 0) };
    unsafe { CloseHandle(handle) };
    match wait {
        WAIT_TIMEOUT => true,
        WAIT_OBJECT_0 => false,
        _ => false,
    }
}

fn nested_pid(root_pid: u32) -> u32 {
    let path = helper_dir(root_pid).join("nested.pid");
    wait_for_file(&path, Duration::from_secs(5));
    fs::read_to_string(path).unwrap().trim().parse().unwrap()
}

fn cleanup(root_pid: u32) {
    let _ = fs::remove_dir_all(helper_dir(root_pid));
}

#[test]
fn job_close_kills_root_and_nested_descendant() {
    let supervisor = WindowsProcessSupervisor::spawn(&helper_spec()).unwrap();
    let root_pid = supervisor.snapshot().pid;
    wait_for_file(&helper_dir(root_pid).join("ready"), Duration::from_secs(5));
    let child_pid = nested_pid(root_pid);
    assert!(pid_alive(root_pid));
    assert!(pid_alive(child_pid));
    assert!(supervisor.active_processes().unwrap() >= 2);

    drop(supervisor);
    wait_for_dead(root_pid, Duration::from_secs(5));
    wait_for_dead(child_pid, Duration::from_secs(5));
    cleanup(root_pid);
}

#[test]
fn graceful_stop_drains_owned_tree_without_forced_termination() {
    let mut supervisor = WindowsProcessSupervisor::spawn(&helper_spec()).unwrap();
    let root_pid = supervisor.snapshot().pid;
    wait_for_file(&helper_dir(root_pid).join("ready"), Duration::from_secs(5));
    let child_pid = nested_pid(root_pid);
    let signal = helper_dir(root_pid).join("graceful.stop");

    let disposition = supervisor
        .stop_with(Duration::from_secs(5), |_| {
            fs::write(&signal, b"stop").unwrap()
        })
        .unwrap();
    assert_eq!(disposition, StopDisposition::Graceful);
    assert_eq!(supervisor.active_processes().unwrap(), 0);
    wait_for_dead(root_pid, Duration::from_secs(5));
    wait_for_dead(child_pid, Duration::from_secs(5));
    cleanup(root_pid);
}

#[test]
fn graceful_timeout_forces_entire_owned_tree() {
    let mut supervisor = WindowsProcessSupervisor::spawn(&helper_spec()).unwrap();
    let root_pid = supervisor.snapshot().pid;
    wait_for_file(&helper_dir(root_pid).join("ready"), Duration::from_secs(5));
    let child_pid = nested_pid(root_pid);

    let disposition = supervisor
        .stop_with(Duration::from_millis(50), |_| {})
        .unwrap();
    assert_eq!(disposition, StopDisposition::Forced);
    wait_for_dead(root_pid, Duration::from_secs(5));
    wait_for_dead(child_pid, Duration::from_secs(5));
    cleanup(root_pid);
}

#[test]
fn stale_persisted_pid_cannot_target_unrelated_process() {
    let mut unrelated = spawn_unrelated_leaf();
    let unrelated_pid = unrelated.id();
    assert!(pid_alive(unrelated_pid));

    let supervisor = WindowsProcessSupervisor::spawn(&helper_spec()).unwrap();
    let current = supervisor.snapshot().clone();
    let stale = ProcessSnapshot {
        pid: unrelated_pid,
        generation: localbridge_lib::runtime::ProcessGeneration::from_persisted_value(
            current.generation.as_u64().saturating_sub(1),
        ),
        ..current
    };
    assert_eq!(
        supervisor.reconcile_persisted(&stale),
        SnapshotDisposition::StaleGeneration
    );

    drop(supervisor);
    thread::sleep(Duration::from_millis(100));
    assert!(
        pid_alive(unrelated_pid),
        "unrelated process was killed by stale PID data"
    );
    unrelated.kill().unwrap();
    unrelated.wait().unwrap();
}

#[test]
fn real_exit_code_259_is_not_misclassified_as_running() {
    let supervisor = WindowsProcessSupervisor::spawn(&exit_259_spec()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && supervisor.active_processes().unwrap() != 0 {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(supervisor.active_processes().unwrap(), 0);
    assert!(!supervisor.root_is_running().unwrap());
}

fn spawn_unrelated_leaf() -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "helper_leaf", "--nocapture"])
        .spawn()
        .unwrap()
}

#[test]
#[ignore]
fn helper_root() {
    let root_pid = std::process::id();
    let dir = helper_dir(root_pid);
    fs::create_dir_all(&dir).unwrap();
    let mut nested = spawn_unrelated_leaf();
    fs::write(dir.join("nested.pid"), nested.id().to_string()).unwrap();
    fs::write(dir.join("ready"), b"ready").unwrap();

    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if dir.join("graceful.stop").exists() {
            let _ = nested.kill();
            let _ = nested.wait();
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let _ = nested.kill();
    let _ = nested.wait();
}

#[test]
#[ignore]
fn helper_leaf() {
    thread::sleep(Duration::from_secs(60));
}

#[test]
#[ignore]
fn helper_exit_259() {
    std::process::exit(259);
}
