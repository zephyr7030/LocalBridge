use super::*;
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteKeyW};

#[test]
fn current_user_run_registration_is_exact_background_command_and_reversible() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let subkey = format!(r"Software\LocalBridgeTests\LB014-{nonce}-{}", std::process::id());
    let value = format!("LocalBridge-LB014-{nonce}");
    let executable = std::env::current_exe().unwrap();
    let manager = AutostartManager::for_test(&executable, subkey.clone(), value).unwrap();

    manager.set_enabled(true).unwrap();
    assert_eq!(
        manager.registered_command().unwrap().as_deref(),
        Some(manager.command_line().as_str())
    );
    assert!(manager.command_line().starts_with('"'));
    assert!(manager.command_line().ends_with(" --background"));

    manager.set_enabled(false).unwrap();
    assert_eq!(manager.registered_command().unwrap(), None);
    let wide_subkey = wide(&subkey);
    unsafe {
        RegDeleteKeyW(HKEY_CURRENT_USER, wide_subkey.as_ptr());
    }
}
