#![cfg(windows)]

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use localbridge_lib::credentials::{
    CredentialMetadata, CredentialStore, CredentialStoreError, SecretString,
};
use localbridge_lib::mcp::{
    CodingToolsPermissionMode, CodingToolsRuntime, CodingToolsRuntimeConfig, InternalBearer,
};
use localbridge_lib::tunnel::{TunnelError, TunnelId, TunnelRestartPrimitive, TunnelRuntimeConfig};
use serde_json::json;

const TUNNEL_ID: &str = "tunnel_0123456789abcdef0123456789abcdef";
const SYNTHETIC_SECRET: &str = "LB019PRE_SYNTHETIC_RUNTIME_KEY_DO_NOT_LEAK";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has repository parent")
        .to_path_buf()
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "localbridge-lb019pre-no-console-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("create no-console temp directory");
    path
}

fn create_workspace(label: &str) -> PathBuf {
    let path = unique_temp_dir(label);
    fs::write(path.join("probe.txt"), b"LB019PRE\n").expect("write workspace probe");
    path
}

fn free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind ephemeral loopback");
    listener.local_addr().expect("local addr").port()
}

fn visible_descendant_windows(root_pid: u32) -> Vec<String> {
    let script = format!(
        "$all=Get-CimInstance Win32_Process; $ids=@({root_pid}); $out=@(); for($i=0;$i -lt 5;$i++){{ $next=@(); foreach($id in $ids){{ foreach($p in $all | Where-Object {{$_.ParentProcessId -eq $id}}){{ $gp=Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue; if($gp -and $gp.MainWindowHandle -ne 0){{ $out += ($p.ProcessId.ToString()+'|'+$p.Name+'|'+$gp.MainWindowHandle.ToString()) }}; $next += $p.ProcessId }} }}; $ids=$next }}; $out"
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .expect("query managed process visible windows");
    assert!(
        output.status.success(),
        "visible-window process query failed"
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn assert_no_visible_descendant_windows(label: &str, pid: u32) {
    let windows = visible_descendant_windows(pid);
    assert!(
        windows.is_empty(),
        "{label} created visible console descendants: {windows:?}"
    );
}

fn release_executable() -> Option<PathBuf> {
    let value = std::env::var_os("LOCALBRIDGE_RELEASE_EXE")?;
    let path = PathBuf::from(value);
    assert!(
        path.is_absolute(),
        "release executable path must be absolute"
    );
    assert!(
        path.is_file(),
        "release executable missing: {}",
        path.display()
    );
    Some(path)
}

fn spawn_release(executable: &Path, background: bool, label: &str) -> (Child, PathBuf) {
    let local_app_data = unique_temp_dir(label);
    let roaming_app_data = local_app_data.join("Roaming");
    let temp = local_app_data.join("Temp");
    fs::create_dir_all(&roaming_app_data).unwrap();
    fs::create_dir_all(&temp).unwrap();
    let mut command = Command::new(executable);
    if background {
        command.arg("--background");
    }
    command
        .env("LOCALAPPDATA", &local_app_data)
        .env("APPDATA", &roaming_app_data)
        .env("TEMP", &temp)
        .env("TMP", &temp);
    let child = command.spawn().expect("spawn release LocalBridge");
    (child, local_app_data)
}

fn assert_release_launch_no_console(executable: &Path, background: bool, label: &str) {
    let (mut child, data_root) = spawn_release(executable, background, label);
    thread::sleep(Duration::from_millis(800));
    assert!(
        child.try_wait().expect("query release child").is_none(),
        "{label} release process exited before behavioral window observation; possible single-instance collision or startup failure"
    );
    assert_no_visible_descendant_windows(label, child.id());
    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_dir_all(data_root);
}

fn coding_config(workspace: &Path) -> CodingToolsRuntimeConfig {
    CodingToolsRuntimeConfig::new(
        repo_root(),
        workspace,
        free_port(),
        CodingToolsPermissionMode::Safe,
    )
}

struct FakeStore;

impl CredentialStore for FakeStore {
    fn save_runtime_api_key(
        &self,
        _secret: &SecretString,
    ) -> Result<CredentialMetadata, CredentialStoreError> {
        Err(CredentialStoreError::InvalidCredentialId)
    }

    fn read_runtime_api_key(&self) -> Result<Option<SecretString>, CredentialStoreError> {
        Ok(Some(SecretString::new(SYNTHETIC_SECRET)?))
    }

    fn delete_runtime_api_key(&self) -> Result<bool, CredentialStoreError> {
        Err(CredentialStoreError::InvalidCredentialId)
    }

    fn runtime_api_key_metadata(&self) -> Result<CredentialMetadata, CredentialStoreError> {
        Err(CredentialStoreError::InvalidCredentialId)
    }
}

fn blocked_control_plane() -> (String, Sender<()>, JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = [0u8; 4096];
                    let _ = stream.read(&mut request);
                    let _ = release_rx.recv_timeout(Duration::from_secs(10));
                    let body = r#"{"error":"synthetic blocked control plane"}"#;
                    let response = format!(
                        "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("fake control plane accept failed: {error}"),
            }
        }
    });
    (format!("http://127.0.0.1:{port}"), release_tx, handle)
}

#[test]
fn schema43_release_no_console_behavior_covers_all_required_scenarios() {
    let Some(release) = release_executable() else {
        eprintln!("NO_CONSOLE_RELEASE_GATE=SKIPPED release_executable_not_provided");
        return;
    };

    // 1. The actual release GUI foreground entry stays alive and creates no
    // console descendants. Then the actual bundled coding runtime is started,
    // representing the configured foreground runtime-start portion.
    assert_release_launch_no_console(&release, false, "configured_foreground_runtime_start-gui");
    let workspace = create_workspace("configured-runtime");
    let bearer = InternalBearer::new("LB019PRE_NO_CONSOLE_BEARER").unwrap();
    let mut runtime =
        CodingToolsRuntime::start(coding_config(&workspace), bearer, Duration::from_secs(10))
            .expect("configured foreground bundled runtime start");
    assert_no_visible_descendant_windows(
        "configured_foreground_runtime_start",
        runtime.process_snapshot().pid,
    );
    println!("NO_CONSOLE_SCENARIO configured_foreground_runtime_start=PASS");

    // 6. A real long-running managed shell child must also remain windowless.
    let command = runtime
        .call_tool(
            "exec_command",
            json!({
                "cmd":"cmd.exe /d /c ping -n 8 127.0.0.1 >nul",
                "timeout_ms":12000,
                "yield_time_ms":0,
                "max_output_bytes":4096
            }),
        )
        .expect("start managed no-console command");
    let session = command
        .get("structuredContent")
        .and_then(|value| value.get("session_id"))
        .and_then(|value| value.as_str())
        .expect("managed command session id")
        .to_owned();
    thread::sleep(Duration::from_millis(300));
    assert_no_visible_descendant_windows(
        "managed_shell_or_direct_command_child",
        runtime.process_snapshot().pid,
    );
    runtime
        .call_tool(
            "kill_session",
            json!({"session_id":session,"signal":"KILL","wait_ms":1000,"max_output_bytes":4096}),
        )
        .expect("kill managed command probe");
    println!("NO_CONSOLE_SCENARIO managed_shell_or_direct_command_child=PASS");
    runtime.stop().unwrap();
    fs::remove_dir_all(&workspace).unwrap();

    // 2. Explicit background launch is exercised on the actual release GUI.
    assert_release_launch_no_console(&release, true, "background_launch");
    println!("NO_CONSOLE_SCENARIO background_launch=PASS");

    // 3. Recovery uses the production recovery start path, not normal start.
    let recovery_workspace = create_workspace("recovery");
    let mut recovered = CodingToolsRuntime::start_for_recovery(
        coding_config(&recovery_workspace),
        InternalBearer::new("LB019PRE_RECOVERY_BEARER").unwrap(),
        Duration::from_secs(10),
        Duration::from_secs(2),
        || false,
    )
    .expect("coding runtime recovery start");
    assert_no_visible_descendant_windows(
        "runtime_restart_or_recovery",
        recovered.process_snapshot().pid,
    );
    recovered.stop().unwrap();
    fs::remove_dir_all(&recovery_workspace).unwrap();
    println!("NO_CONSOLE_SCENARIO runtime_restart_or_recovery=PASS");

    // 4. Tunnel reconnect exercises TunnelRestartPrimitive and actually spawns
    // the bundled tunnel-client while the synthetic control plane holds it.
    let (control_plane, release_control_plane, control_plane_thread) = blocked_control_plane();
    let health_dir = unique_temp_dir("tunnel-reconnect");
    let tunnel_config = TunnelRuntimeConfig::new(
        repo_root(),
        &health_dir,
        TunnelId::new(TUNNEL_ID).unwrap(),
        free_port(),
    )
    .unwrap()
    .with_test_control_plane_base_url(&control_plane)
    .unwrap();
    let prepared =
        TunnelRestartPrimitive::prepare(tunnel_config, &FakeStore, &TunnelError::TunnelExited)
            .expect("prepare recoverable tunnel reconnect");
    let mut tunnel = prepared.spawn().expect("spawn reconnect tunnel runtime");
    thread::sleep(Duration::from_millis(400));
    assert!(
        tunnel.root_is_running().unwrap(),
        "tunnel reconnect process exited early"
    );
    assert_no_visible_descendant_windows("tunnel_reconnect", tunnel.process_snapshot().pid);
    let _ = release_control_plane.send(());
    tunnel.stop().unwrap();
    control_plane_thread.join().unwrap();
    fs::remove_dir_all(&health_dir).unwrap();
    println!("NO_CONSOLE_SCENARIO tunnel_reconnect=PASS");

    // 5. Login autostart uses the exact production `--background` command-line
    // contract, but is observed as a distinct launch instance/evidence item.
    assert_release_launch_no_console(&release, true, "login_autostart");
    println!("NO_CONSOLE_SCENARIO login_autostart=PASS");
}
