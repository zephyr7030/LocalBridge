use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::credentials::{CredentialStore, SecretString};
use crate::runtime::{
    ManagedProcessSpec, ProcessSnapshot, StopDisposition, SupervisorError, WindowsProcessSupervisor,
};

use super::bundle::{VerifiedTunnelBundle, verify_bundle};
use super::config::TunnelRuntimeConfig;
use super::fault::{Retryability, TunnelError};
use super::health::{ConnectorEndpoint, HealthEndpoint};

const API_KEY_ENV: &str = "LOCALBRIDGE_RUNTIME_API_KEY";
const TUNNEL_ID_ENV: &str = "CONTROL_PLANE_TUNNEL_ID";
const API_KEY_REFERENCE: &str = "env:LOCALBRIDGE_RUNTIME_API_KEY";
static HEALTH_GENERATION: AtomicU64 = AtomicU64::new(1);

const REMOVED_PARENT_ENV: &[&str] = &[
    "CONTROL_PLANE_API_KEY",
    "OPENAI_API_KEY",
    "CONTROL_PLANE_URL_PATH",
    "CONTROL_PLANE_ORGANIZATION_ID",
    "CONTROL_PLANE_HTTP_PROXY",
    "CONTROL_PLANE_MAX_INFLIGHT_REQUESTS",
    "CONTROL_PLANE_POLL_CHANNELS",
    "CONTROL_PLANE_POLL_DEADLINE_GUARDRAIL",
    "CONTROL_PLANE_POLL_TIMEOUT",
    "CONTROL_PLANE_EXTRA_HEADERS",
    "CONTROL_PLANE_CLIENT_CERT",
    "CONTROL_PLANE_CLIENT_KEY",
    "TUNNEL_CLIENT_CONFIG",
    "TUNNEL_CLIENT_PROFILE",
    "TUNNEL_CLIENT_PROFILE_FILE",
    "TUNNEL_CLIENT_PROFILE_DIR",
    "XDG_CONFIG_HOME",
    "CA_BUNDLE",
    "HEALTH_UNIX_SOCKET",
    "HEALTH_URL_FILE",
    "MCP_COMMAND",
    "MCP_SERVER_URL",
    "MCP_HTTP_PROXY",
    "MCP_CONNECTION_MAX_TTL",
    "MCP_MAX_CONCURRENT_REQUESTS",
    "MCP_EXTRA_HEADERS",
    "MCP_DISCOVERY_EXTRA_HEADERS",
    "MCP_CLIENT_CERT",
    "MCP_CLIENT_KEY",
    "HARPOON_TARGETS",
    "HARPOON_ADDITIONAL_TRANSPORTS",
    "HARPOON_ALLOW_PLAINTEXT_HTTP",
    "HARPOON_CAPTURE_PAYLOADS",
    "HARPOON_HOSTS_INCLUDE_LOOPBACK",
    "HARPOON_HOSTS_INCLUDE_PRIVATE",
    "HARPOON_HOSTS_INCLUDE_REGEX",
    "HARPOON_HOSTS_INCLUDE_SUFFIX",
    "HARPOON_HTTP_PROXY",
    "HARPOON_MAX_REDIRECTS",
    "HARPOON_MAX_RESPONSE_BYTES",
    "CLOUDFLARED_TUNNEL_TOKEN",
    "CLOUDFLARED_MANAGED",
    "CLOUDFLARED_PATH",
    "CLOUDFLARED_READY_TIMEOUT",
    "ALLOW_REMOTE_UI",
    "OPEN_WEB_UI",
    "ADMIN_UI_LOG_BUFFER_EVENTS",
    "PID_FILE",
    "LOG_HTTP_RAW_UNSAFE",
    "LOG_FILE",
];

pub struct PreparedTunnelStart {
    config: TunnelRuntimeConfig,
    bundle: VerifiedTunnelBundle,
    secret: SecretString,
    health_url_file: PathBuf,
}

impl fmt::Debug for PreparedTunnelStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedTunnelStart")
            .field("config", &self.config)
            .field("bundle", &self.bundle)
            .field("secret", &"[REDACTED]")
            .field("health_url_file", &self.health_url_file)
            .field("arguments", &self.command_line_arguments())
            .finish()
    }
}

impl PreparedTunnelStart {
    pub fn prepare<C: CredentialStore>(
        config: TunnelRuntimeConfig,
        store: &C,
    ) -> Result<Self, TunnelError> {
        let bundle = verify_bundle(&config.install_root)?;
        let secret = store
            .read_runtime_api_key()
            .map_err(TunnelError::SecretStoreFailed)?
            .ok_or(TunnelError::RuntimeKeyMissing)?;
        fs::create_dir_all(&config.health_state_dir).map_err(|_| TunnelError::HealthStateIo)?;
        let generation = HEALTH_GENERATION.fetch_add(1, Ordering::Relaxed);
        let health_url_file = config.health_state_dir.join(format!(
            "tunnel-health-{}-{generation}.url",
            std::process::id()
        ));
        if health_url_file.exists() {
            fs::remove_file(&health_url_file).map_err(|_| TunnelError::HealthStateIo)?;
        }
        Ok(Self {
            config,
            bundle,
            secret,
            health_url_file,
        })
    }

    pub fn command_line_arguments(&self) -> Vec<String> {
        let arguments = vec![
            "run".into(),
            "--control-plane.api-key".into(),
            API_KEY_REFERENCE.into(),
            "--control-plane.base-url".into(),
            self.config.control_plane_base_url().into(),
            "--mcp.server-url".into(),
            self.config.mcp_target(),
            "--health.listen-addr".into(),
            "127.0.0.1:0".into(),
            "--health.url-file".into(),
            self.health_url_file.to_string_lossy().into_owned(),
            "--log.format".into(),
            "struct-text".into(),
            "--log.level".into(),
            "warn".into(),
        ];
        #[cfg(test)]
        let arguments = if self.config.embedded_mcp_stub() {
            let mut test_arguments = arguments;
            let mcp_index = test_arguments
                .iter()
                .position(|argument| argument == "--mcp.server-url")
                .expect("production argument set contains MCP Guard target");
            test_arguments.drain(mcp_index..=(mcp_index + 1));
            test_arguments.insert(1, "--embedded-mcp-stub".into());
            test_arguments
        } else {
            arguments
        };
        arguments
    }

    pub fn health_url_file(&self) -> &Path {
        &self.health_url_file
    }

    pub fn spawn(self) -> Result<TunnelRuntime, TunnelError> {
        validate_api_key_reference(API_KEY_REFERENCE)?;
        let mut spec = ManagedProcessSpec::new("tunnel-client", &self.bundle.executable)
            .map_err(classify_supervisor)?
            .args(self.command_line_arguments())
            .current_dir(
                self.bundle
                    .executable
                    .parent()
                    .ok_or(TunnelError::RuntimeMissing)?,
            );
        for key in REMOVED_PARENT_ENV {
            spec = spec.env_remove(key).map_err(classify_supervisor)?;
        }
        spec = spec
            .env(API_KEY_ENV, self.secret.expose_secret())
            .map_err(classify_supervisor)?;
        spec = spec
            .env(TUNNEL_ID_ENV, self.config.tunnel_id.expose())
            .map_err(classify_supervisor)?;
        spec = spec
            .env(
                "CONTROL_PLANE_BASE_URL",
                self.config.control_plane_base_url(),
            )
            .map_err(classify_supervisor)?;
        spec = spec
            .env("HEALTH_LISTEN_ADDR", "127.0.0.1:0")
            .map_err(classify_supervisor)?;
        spec = spec.env("DO_NOT_TRACK", "1").map_err(classify_supervisor)?;
        let supervisor = WindowsProcessSupervisor::spawn(&spec).map_err(classify_supervisor)?;
        Ok(TunnelRuntime {
            config: self.config,
            supervisor,
            health_url_file: self.health_url_file,
            health: None,
            connector_endpoint: None,
        })
    }
}

pub struct TunnelRuntime {
    config: TunnelRuntimeConfig,
    supervisor: WindowsProcessSupervisor,
    health_url_file: PathBuf,
    health: Option<HealthEndpoint>,
    connector_endpoint: Option<ConnectorEndpoint>,
}

impl fmt::Debug for TunnelRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TunnelRuntime")
            .field("config", &self.config)
            .field("process", self.supervisor.snapshot())
            .field("health_url_file", &self.health_url_file)
            .field("health_resolved", &self.health.is_some())
            .field(
                "connector_endpoint_available",
                &self.connector_endpoint.is_some(),
            )
            .finish()
    }
}

impl TunnelRuntime {
    pub fn start<C: CredentialStore>(
        config: TunnelRuntimeConfig,
        store: &C,
        timeout: Duration,
    ) -> Result<Self, TunnelError> {
        let mut runtime = PreparedTunnelStart::prepare(config, store)?.spawn()?;
        if let Err(error) = runtime.wait_ready(timeout) {
            let _ = runtime.stop();
            return Err(error);
        }
        Ok(runtime)
    }

    pub fn wait_ready(&mut self, timeout: Duration) -> Result<(), TunnelError> {
        let deadline = Instant::now() + timeout;
        loop {
            if !self
                .supervisor
                .root_is_running()
                .map_err(classify_supervisor)?
            {
                return Err(TunnelError::TunnelExited);
            }
            if self.health.is_none() {
                match fs::read_to_string(&self.health_url_file) {
                    Ok(value) => self.health = Some(HealthEndpoint::parse(&value)?),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err(TunnelError::HealthStateIo),
                }
            }
            if let Some(health) = self.health {
                match health.probe_ready_metadata() {
                    Ok(probe) if probe.ready => {
                        self.connector_endpoint = probe.connector_endpoint;
                        return Ok(());
                    }
                    Ok(_) | Err(TunnelError::HealthUnavailable) => {}
                    Err(error) => return Err(error),
                }
            }
            if Instant::now() >= deadline {
                return Err(TunnelError::HealthTimeout);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn wait_ready_for_recovery(
        &mut self,
        timeout: Duration,
        probe_timeout: Duration,
        cancelled: impl Fn() -> bool,
    ) -> Result<(), TunnelError> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancelled() {
                return Err(TunnelError::HealthUnavailable);
            }
            if !self
                .supervisor
                .root_is_running()
                .map_err(classify_supervisor)?
            {
                return Err(TunnelError::TunnelExited);
            }
            if self.health.is_none() {
                match fs::read_to_string(&self.health_url_file) {
                    Ok(value) => self.health = Some(HealthEndpoint::parse(&value)?),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err(TunnelError::HealthStateIo),
                }
            }
            if cancelled() {
                return Err(TunnelError::HealthUnavailable);
            }
            if let Some(health) = self.health {
                match health.probe_ready_metadata_with_timeout(probe_timeout) {
                    Ok(probe) if probe.ready => {
                        if cancelled() {
                            return Err(TunnelError::HealthUnavailable);
                        }
                        self.connector_endpoint = probe.connector_endpoint;
                        return Ok(());
                    }
                    Ok(_) | Err(TunnelError::HealthUnavailable) => {}
                    Err(error) => return Err(error),
                }
            }
            if cancelled() {
                return Err(TunnelError::HealthUnavailable);
            }
            if Instant::now() >= deadline {
                return Err(TunnelError::HealthTimeout);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub const fn process_snapshot(&self) -> &ProcessSnapshot {
        self.supervisor.snapshot()
    }
    pub fn root_is_running(&self) -> Result<bool, TunnelError> {
        self.supervisor
            .root_is_running()
            .map_err(classify_supervisor)
    }
    pub fn active_processes(&self) -> Result<u32, TunnelError> {
        self.supervisor
            .active_processes()
            .map_err(classify_supervisor)
    }
    pub fn stop(&mut self) -> Result<StopDisposition, TunnelError> {
        self.supervisor.force_stop().map_err(classify_supervisor)
    }
    pub fn config(&self) -> &TunnelRuntimeConfig {
        &self.config
    }
    pub fn connector_endpoint(&self) -> Option<ConnectorEndpoint> {
        self.connector_endpoint.clone()
    }
}

pub struct TunnelRestartPrimitive;

impl TunnelRestartPrimitive {
    pub fn prepare<C: CredentialStore>(
        config: TunnelRuntimeConfig,
        store: &C,
        fault: &TunnelError,
    ) -> Result<PreparedTunnelStart, TunnelError> {
        if fault.retryability() != Retryability::Recoverable {
            return Err(TunnelError::RestartDenied);
        }
        PreparedTunnelStart::prepare(config, store)
    }
}

fn validate_api_key_reference(reference: &str) -> Result<(), TunnelError> {
    if reference == API_KEY_REFERENCE && reference.strip_prefix("env:") == Some(API_KEY_ENV) {
        Ok(())
    } else {
        Err(TunnelError::SecretInjectionUnsupported)
    }
}

fn classify_supervisor(error: SupervisorError) -> TunnelError {
    match error {
        SupervisorError::WindowsApi {
            operation: "CreateProcessW",
            ..
        } => TunnelError::TunnelSpawnFailed(error),
        _ => TunnelError::ProcessOwnershipFailed(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{CredentialMetadata, CredentialStoreError};
    use crate::tunnel::{ControlPlaneFault, TunnelId};
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener};
    use std::process::Command;
    use std::sync::mpsc::{self, Sender};
    use std::thread::{self, JoinHandle};
    use std::time::{SystemTime, UNIX_EPOCH};

    const TUNNEL_ID: &str = "tunnel_0123456789abcdef0123456789abcdef";
    const SECRET_ONE: &str = "LB008_SYNTHETIC_RUNTIME_KEY_ONE_DO_NOT_LEAK";
    const SECRET_TWO: &str = "LB008_SYNTHETIC_RUNTIME_KEY_TWO_DO_NOT_LEAK";

    struct FakeStore {
        reads: Cell<usize>,
        values: RefCell<VecDeque<Option<String>>>,
    }

    impl FakeStore {
        fn new(values: impl IntoIterator<Item = Option<&'static str>>) -> Self {
            Self {
                reads: Cell::new(0),
                values: RefCell::new(
                    values
                        .into_iter()
                        .map(|value| value.map(str::to_owned))
                        .collect(),
                ),
            }
        }

        fn reads(&self) -> usize {
            self.reads.get()
        }
    }

    impl CredentialStore for FakeStore {
        fn save_runtime_api_key(
            &self,
            _secret: &SecretString,
        ) -> Result<CredentialMetadata, CredentialStoreError> {
            unreachable!("LB-008 fake store is read-only")
        }

        fn read_runtime_api_key(&self) -> Result<Option<SecretString>, CredentialStoreError> {
            self.reads.set(self.reads.get() + 1);
            self.values
                .borrow_mut()
                .pop_front()
                .unwrap_or(None)
                .map(SecretString::new)
                .transpose()
        }

        fn delete_runtime_api_key(&self) -> Result<bool, CredentialStoreError> {
            unreachable!("LB-008 fake store is read-only")
        }

        fn runtime_api_key_metadata(&self) -> Result<CredentialMetadata, CredentialStoreError> {
            unreachable!("LB-008 fake store is read-only")
        }
    }

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri has repository parent")
            .to_path_buf()
    }

    fn temp_health_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "localbridge-lb008-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn config(label: &str) -> TunnelRuntimeConfig {
        config_with_control_plane(label, "http://127.0.0.1:9")
    }

    fn config_with_control_plane(label: &str, base_url: &str) -> TunnelRuntimeConfig {
        let config = TunnelRuntimeConfig::new(
            repo_root(),
            temp_health_dir(label),
            TunnelId::new(TUNNEL_ID).unwrap(),
            65534,
        )
        .unwrap();
        #[cfg(debug_assertions)]
        {
            return config
                .with_test_control_plane_base_url(base_url)
                .unwrap()
                .with_test_embedded_mcp_stub();
        }
        #[allow(unreachable_code)]
        config
    }

    fn os_command_line(pid: u32) -> String {
        let script = format!(
            "$p=Get-CimInstance Win32_Process -Filter \"ProcessId = {pid}\"; if($p){{[Console]::Out.Write($p.CommandLine)}}"
        );
        for _ in 0..20 {
            let output = Command::new("powershell.exe")
                .args(["-NoProfile", "-Command", &script])
                .output()
                .expect("query Win32 process command line");
            if output.status.success() {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if !value.is_empty() {
                    return value;
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("tunnel-client command line was not observable");
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
                        let count = stream.read(&mut request).unwrap_or(0);
                        let request = String::from_utf8_lossy(&request[..count]);
                        if !request.starts_with("GET /v1/tunnels/") {
                            let body = r#"{"error":"not found"}"#;
                            let response = format!(
                                "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(response.as_bytes());
                            continue;
                        }
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
    fn prepare_is_secret_redacted_and_unsupported_injection_fails_closed() {
        let store = FakeStore::new([Some(SECRET_ONE)]);
        let prepared = PreparedTunnelStart::prepare(config("prepare"), &store).unwrap();
        assert_eq!(store.reads(), 1);
        let rendered = prepared.command_line_arguments().join(" ");
        assert!(rendered.contains(API_KEY_REFERENCE));
        assert!(!rendered.contains(SECRET_ONE));
        assert!(!rendered.contains(TUNNEL_ID));
        assert!(!rendered.contains("--cloudflared.managed"));
        assert!(!rendered.contains("--cloudflared.path"));
        assert!(!rendered.contains("--cloudflared.token"));
        let debug = format!("{prepared:?}");
        assert!(!debug.contains(SECRET_ONE));
        assert!(matches!(
            validate_api_key_reference("literal-secret"),
            Err(TunnelError::SecretInjectionUnsupported)
        ));
        assert!(matches!(
            validate_api_key_reference("file:C:/plaintext-secret.txt"),
            Err(TunnelError::SecretInjectionUnsupported)
        ));
        let health_dir = prepared.config.health_state_dir.clone();
        drop(prepared);
        fs::remove_dir_all(health_dir).unwrap();
    }

    #[test]
    fn inherited_runtime_override_isolation_covers_security_critical_surfaces() {
        for required in [
            "HEALTH_UNIX_SOCKET",
            "HEALTH_URL_FILE",
            "MCP_SERVER_URL",
            "MCP_COMMAND",
            "MCP_HTTP_PROXY",
            "HARPOON_ADDITIONAL_TRANSPORTS",
            "HARPOON_ALLOW_PLAINTEXT_HTTP",
            "HARPOON_TARGETS",
            "CONTROL_PLANE_EXTRA_HEADERS",
            "CONTROL_PLANE_POLL_CHANNELS",
            "CLOUDFLARED_TUNNEL_TOKEN",
            "TUNNEL_CLIENT_CONFIG",
            "TUNNEL_CLIENT_PROFILE",
            "LOG_HTTP_RAW_UNSAFE",
        ] {
            assert!(
                REMOVED_PARENT_ENV.contains(&required),
                "missing runtime env isolation: {required}"
            );
        }
        let mut names = REMOVED_PARENT_ENV.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            REMOVED_PARENT_ENV.len(),
            "duplicate runtime env isolation entry"
        );
        assert!(!REMOVED_PARENT_ENV.contains(&"HTTP_PROXY"));
        assert!(!REMOVED_PARENT_ENV.contains(&"HTTPS_PROXY"));
    }

    #[test]
    fn restart_primitive_refreshes_secret_only_for_recoverable_fault() {
        let store = FakeStore::new([Some(SECRET_ONE), Some(SECRET_TWO)]);
        let base = config("restart");
        let first = PreparedTunnelStart::prepare(base.clone(), &store).unwrap();
        assert_eq!(store.reads(), 1);
        drop(first);

        let second =
            TunnelRestartPrimitive::prepare(base.clone(), &store, &TunnelError::TunnelExited)
                .unwrap();
        assert_eq!(store.reads(), 2);
        assert!(!format!("{second:?}").contains(SECRET_TWO));
        drop(second);

        assert!(matches!(
            TunnelRestartPrimitive::prepare(
                base.clone(),
                &store,
                &TunnelError::ControlPlane(ControlPlaneFault::Authentication),
            ),
            Err(TunnelError::RestartDenied)
        ));
        assert_eq!(store.reads(), 2);
        fs::remove_dir_all(base.health_state_dir).unwrap();
    }

    #[test]
    fn credential_and_configuration_faults_never_enter_restart_or_reread_secret() {
        let store = FakeStore::new([Some(SECRET_ONE)]);
        let base = config("non-recoverable");
        for fault in [
            TunnelError::RuntimeKeyMissing,
            TunnelError::InvalidTunnelId,
            TunnelError::InvalidMcpTarget,
        ] {
            assert_eq!(fault.retryability(), Retryability::NonRecoverable);
            assert!(matches!(
                TunnelRestartPrimitive::prepare(base.clone(), &store, &fault),
                Err(TunnelError::RestartDenied)
            ));
        }
        assert_eq!(
            store.reads(),
            0,
            "non-recoverable credential/configuration faults must not reread the secret"
        );
        if base.health_state_dir.exists() {
            fs::remove_dir_all(base.health_state_dir).unwrap();
        }
    }

    #[test]
    fn actual_process_command_line_never_contains_runtime_secret_and_job_stop_drains() {
        let (control_plane, release_control_plane, control_plane_thread) = blocked_control_plane();
        let store = FakeStore::new([Some(SECRET_ONE)]);
        let prepared = PreparedTunnelStart::prepare(
            config_with_control_plane("process", &control_plane),
            &store,
        )
        .unwrap();
        let health_dir = prepared.config.health_state_dir.clone();
        let mut runtime = prepared
            .spawn()
            .expect("vendored tunnel-client must spawn locally");
        assert!(runtime.root_is_running().unwrap());
        assert!(runtime.active_processes().unwrap() >= 1);

        let command_line = os_command_line(runtime.process_snapshot().pid);
        assert!(command_line.contains("tunnel-client.exe"));
        assert!(command_line.contains(API_KEY_REFERENCE));
        assert!(!command_line.contains(SECRET_ONE));
        assert!(!command_line.contains(TUNNEL_ID));
        assert!(!command_line.contains("--cloudflared.managed"));
        assert!(!command_line.contains("--cloudflared.path"));
        assert!(!command_line.contains("--cloudflared.token"));
        assert!(!format!("{runtime:?}").contains(SECRET_ONE));

        let _ = release_control_plane.send(());
        runtime.stop().expect("Job-owned tunnel stop");
        assert!(!runtime.root_is_running().unwrap());
        assert_eq!(runtime.active_processes().unwrap(), 0);
        drop(runtime);
        control_plane_thread.join().unwrap();
        fs::remove_dir_all(health_dir).unwrap();
    }
}
