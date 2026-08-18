use std::fmt;
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::runtime::{
    ManagedProcessSpec, ProcessSnapshot, StopDisposition, SupervisorError, WindowsProcessSupervisor,
};
use crate::state::RuntimeFault;

use super::bundle::verify_bundle;
use super::git_adapter::handle_git_tool;
use super::http::{McpCancellationClient, McpHealthClient, McpSession, unauthenticated_initialize_status};

const LOOPBACK_HOST: &str = "127.0.0.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodingToolsPermissionMode {
    Safe,
    Trusted,
}

impl CodingToolsPermissionMode {
    const fn as_upstream_arg(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Trusted => "trusted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingToolsRuntimeConfig {
    pub install_root: PathBuf,
    pub workspace: PathBuf,
    pub port: u16,
    pub permission_mode: CodingToolsPermissionMode,
}

impl CodingToolsRuntimeConfig {
    pub fn new(
        install_root: impl Into<PathBuf>,
        workspace: impl Into<PathBuf>,
        port: u16,
        permission_mode: CodingToolsPermissionMode,
    ) -> Self {
        Self {
            install_root: install_root.into(),
            workspace: workspace.into(),
            port,
            permission_mode,
        }
    }
}

pub struct InternalBearer(Vec<u8>);

impl InternalBearer {
    pub fn new(value: impl AsRef<str>) -> Result<Self, CodingToolsRuntimeError> {
        let value = value.as_ref();
        if value.is_empty() || value.as_bytes().contains(&0) || value.len() > 4096 {
            return Err(CodingToolsRuntimeError::InvalidConfiguration);
        }
        Ok(Self(value.as_bytes().to_vec()))
    }

    pub(crate) fn expose(&self) -> &str {
        std::str::from_utf8(&self.0).expect("InternalBearer is constructed from UTF-8")
    }
}

impl fmt::Debug for InternalBearer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InternalBearer([REDACTED])")
    }
}

impl Drop for InternalBearer {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeIntegrityComponent {
    Python,
    CodingTools,
}

#[derive(Debug)]
pub enum CodingToolsRuntimeError {
    InvalidConfiguration,
    WorkspaceMissing,
    WorkspaceInvalid,
    RuntimeMissing(RuntimeIntegrityComponent),
    RuntimeChecksumMismatch(RuntimeIntegrityComponent),
    PortUnavailable,
    Supervisor(SupervisorError),
    ConnectionUnavailable,
    HttpStatus(u16),
    ProtocolMismatch,
    UpstreamRpcError,
    McpExited,
    HealthTimeout,
    Cancelled,
}

impl CodingToolsRuntimeError {
    pub fn runtime_fault(&self) -> RuntimeFault {
        match self {
            Self::InvalidConfiguration | Self::ProtocolMismatch | Self::UpstreamRpcError => {
                RuntimeFault::ConfigurationInvalid
            }
            Self::WorkspaceMissing => RuntimeFault::WorkspaceMissing,
            Self::WorkspaceInvalid => RuntimeFault::WorkspaceInvalid,
            Self::RuntimeMissing(_) => RuntimeFault::RuntimeMissing,
            Self::RuntimeChecksumMismatch(_) => RuntimeFault::RuntimeChecksumMismatch,
            Self::PortUnavailable => RuntimeFault::PortUnavailable,
            Self::Supervisor(SupervisorError::WindowsApi {
                operation: "CreateProcessW",
                ..
            }) => RuntimeFault::McpSpawnFailed,
            Self::Supervisor(_) => RuntimeFault::ProcessOwnershipFailed,
            Self::ConnectionUnavailable | Self::HttpStatus(_) | Self::HealthTimeout => {
                RuntimeFault::McpHealthTimeout
            }
            Self::McpExited => RuntimeFault::McpExited,
            Self::Cancelled => RuntimeFault::UserStopped,
        }
    }
}

impl fmt::Display for CodingToolsRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => f.write_str("invalid coding runtime configuration"),
            Self::WorkspaceMissing => f.write_str("workspace is missing"),
            Self::WorkspaceInvalid => f.write_str("workspace is not a directory"),
            Self::RuntimeMissing(component) => {
                write!(f, "bundled runtime component is missing: {component:?}")
            }
            Self::RuntimeChecksumMismatch(component) => {
                write!(f, "bundled runtime integrity check failed: {component:?}")
            }
            Self::PortUnavailable => f.write_str("coding runtime loopback port is unavailable"),
            Self::Supervisor(error) => write!(f, "coding runtime supervisor failure: {error}"),
            Self::ConnectionUnavailable => {
                f.write_str("coding runtime loopback connection is unavailable")
            }
            Self::HttpStatus(status) => {
                write!(f, "coding runtime returned unexpected HTTP status {status}")
            }
            Self::ProtocolMismatch => f.write_str("coding runtime protocol identity mismatch"),
            Self::UpstreamRpcError => f.write_str("coding runtime returned an MCP RPC error"),
            Self::McpExited => f.write_str("coding runtime exited before readiness"),
            Self::HealthTimeout => f.write_str("coding runtime readiness timed out"),
            Self::Cancelled => f.write_str("coding runtime recovery was cancelled"),
        }
    }
}

impl std::error::Error for CodingToolsRuntimeError {}

impl From<SupervisorError> for CodingToolsRuntimeError {
    fn from(value: SupervisorError) -> Self {
        Self::Supervisor(value)
    }
}

pub struct CodingToolsRuntime {
    supervisor: WindowsProcessSupervisor,
    session: McpSession,
    port: u16,
    workspace: PathBuf,
    install_root: PathBuf,
}

impl fmt::Debug for CodingToolsRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodingToolsRuntime")
            .field("endpoint", &self.endpoint())
            .field("process", self.supervisor.snapshot())
            .field("session", &self.session)
            .finish()
    }
}

impl CodingToolsRuntime {
    pub fn start(
        config: CodingToolsRuntimeConfig,
        bearer: InternalBearer,
        readiness_timeout: Duration,
    ) -> Result<Self, CodingToolsRuntimeError> {
        let mut runtime = Self::spawn_unready(config, bearer)?;
        if let Err(error) = runtime.wait_ready(readiness_timeout) {
            let _ = runtime.supervisor.force_stop();
            return Err(error);
        }
        Ok(runtime)
    }

    pub fn start_for_recovery(
        config: CodingToolsRuntimeConfig,
        bearer: InternalBearer,
        readiness_timeout: Duration,
        probe_timeout: Duration,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self, CodingToolsRuntimeError> {
        if cancelled() {
            return Err(CodingToolsRuntimeError::Cancelled);
        }
        let mut runtime = Self::spawn_unready(config, bearer)?;
        if let Err(error) =
            runtime.wait_ready_for_recovery(readiness_timeout, probe_timeout, &cancelled)
        {
            let _ = runtime.supervisor.force_stop();
            return Err(error);
        }
        Ok(runtime)
    }

    fn spawn_unready(
        config: CodingToolsRuntimeConfig,
        bearer: InternalBearer,
    ) -> Result<Self, CodingToolsRuntimeError> {
        if is_verbatim_workspace_path(&config.workspace) {
            return Err(CodingToolsRuntimeError::InvalidConfiguration);
        }
        validate_workspace(&config.workspace)?;
        if config.port == 0 {
            return Err(CodingToolsRuntimeError::InvalidConfiguration);
        }
        let verified = verify_bundle(&config.install_root)?;
        reserve_loopback_port(config.port)?;

        let mut spec = ManagedProcessSpec::new("coding-tools-mcp", verified.python_executable)?
            .arg("-I")
            .arg("-B")
            .arg("-m")
            .arg("coding_tools_mcp")
            .arg("--workspace")
            .arg(config.workspace.as_os_str())
            .arg("--host")
            .arg(LOOPBACK_HOST)
            .arg("--port")
            .arg(config.port.to_string())
            .arg("--permission-mode")
            .arg(config.permission_mode.as_upstream_arg())
            .current_dir(&config.workspace);

        for (key, value) in [
            ("CODING_TOOLS_MCP_AUTH_TOKEN", bearer.expose()),
            ("CODING_TOOLS_MCP_AUTH_MODE", "bearer"),
            ("CODING_TOOLS_MCP_TELEMETRY", "off"),
            ("CODING_TOOLS_MCP_TRACE", "0"),
            ("CODING_TOOLS_MCP_SHELL_ENV_INHERIT", "core"),
            ("CODING_TOOLS_MCP_SHELL_ENV_SET", "{}"),
            ("CODING_TOOLS_MCP_DANGEROUSLY_SKIP_ALL_PERMISSIONS", "0"),
            (
                "CODING_TOOLS_MCP_DANGEROUSLY_FAKE_READONLY_ANNOTATIONS",
                "0",
            ),
            ("CODING_TOOLS_MCP_GENERATE_AUTH_TOKEN", "0"),
            ("CODING_TOOLS_MCP_OAUTH_MODE", "0"),
            ("CODING_TOOLS_MCP_ALLOWED_ORIGINS", ""),
            ("DO_NOT_TRACK", "1"),
            ("PYTHONNOUSERSITE", "1"),
            ("PYTHONDONTWRITEBYTECODE", "1"),
        ] {
            spec = spec.env(key, value)?;
        }

        let supervisor = WindowsProcessSupervisor::spawn(&spec)?;
        let session = McpSession::new(config.port, bearer);
        Ok(Self {
            supervisor,
            session,
            port: config.port,
            workspace: config.workspace,
            install_root: config.install_root,
        })
    }

    pub fn endpoint(&self) -> String {
        format!("http://{LOOPBACK_HOST}:{}/mcp", self.port)
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn install_root(&self) -> &Path {
        &self.install_root
    }

    pub const fn process_snapshot(&self) -> &ProcessSnapshot {
        self.supervisor.snapshot()
    }

    pub fn root_is_running(&self) -> Result<bool, CodingToolsRuntimeError> {
        self.supervisor.root_is_running().map_err(Into::into)
    }

    pub fn active_processes(&self) -> Result<u32, CodingToolsRuntimeError> {
        self.supervisor.active_processes().map_err(Into::into)
    }

    pub fn list_tools(&mut self) -> Result<Value, CodingToolsRuntimeError> {
        self.session.list_tools()
    }

    pub fn call_tool(
        &mut self,
        name: &str,
        arguments: Value,
    ) -> Result<Value, CodingToolsRuntimeError> {
        if let Some(result) = handle_git_tool(&self.workspace, name, &arguments) {
            return Ok(result);
        }
        self.session.call_tool(name, arguments)
    }

    pub(crate) fn call_tool_with_request_id(
        &mut self,
        name: &str,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, CodingToolsRuntimeError> {
        if let Some(result) = handle_git_tool(&self.workspace, name, &arguments) {
            return Ok(result);
        }
        match request_id {
            Some(request_id) => self
                .session
                .call_tool_with_request_id(name, arguments, request_id),
            None => self.session.call_tool(name, arguments),
        }
    }

    pub(crate) fn call_tool_with_request_id_and_timeout(
        &mut self,
        name: &str,
        arguments: Value,
        request_id: Option<&Value>,
        transport_timeout: Duration,
    ) -> Result<Value, CodingToolsRuntimeError> {
        if let Some(result) = handle_git_tool(&self.workspace, name, &arguments) {
            return Ok(result);
        }
        match request_id {
            Some(request_id) => self.session.call_tool_with_request_id_and_timeout(
                name,
                arguments,
                request_id,
                transport_timeout,
            ),
            None => self
                .session
                .call_tool_with_timeout(name, arguments, transport_timeout),
        }
    }

    pub(crate) fn cancellation_client(
        &self,
    ) -> Result<McpCancellationClient, CodingToolsRuntimeError> {
        self.session.cancellation_client()
    }

    pub(crate) fn health_client(&self) -> Result<McpHealthClient, CodingToolsRuntimeError> {
        self.session.health_client()
    }

    pub fn unauthenticated_initialize_is_rejected(&self) -> Result<bool, CodingToolsRuntimeError> {
        Ok(unauthenticated_initialize_status(self.port)? == 401)
    }

    pub fn stop(&mut self) -> Result<StopDisposition, CodingToolsRuntimeError> {
        self.supervisor.force_stop().map_err(Into::into)
    }

    fn wait_ready(&mut self, timeout: Duration) -> Result<(), CodingToolsRuntimeError> {
        let deadline = Instant::now() + timeout;
        loop {
            if !self.supervisor.root_is_running()? {
                return Err(CodingToolsRuntimeError::McpExited);
            }
            match self.session.initialize() {
                Ok(_) => return Ok(()),
                Err(CodingToolsRuntimeError::ConnectionUnavailable) => {}
                Err(CodingToolsRuntimeError::HttpStatus(503)) => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(CodingToolsRuntimeError::HealthTimeout);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn wait_ready_for_recovery(
        &mut self,
        timeout: Duration,
        probe_timeout: Duration,
        cancelled: &impl Fn() -> bool,
    ) -> Result<(), CodingToolsRuntimeError> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancelled() {
                return Err(CodingToolsRuntimeError::Cancelled);
            }
            if !self.supervisor.root_is_running()? {
                return Err(CodingToolsRuntimeError::McpExited);
            }
            match self.session.initialize_with_timeout(probe_timeout) {
                Ok(_) => {
                    return if cancelled() {
                        Err(CodingToolsRuntimeError::Cancelled)
                    } else {
                        Ok(())
                    };
                }
                Err(CodingToolsRuntimeError::ConnectionUnavailable) => {}
                Err(CodingToolsRuntimeError::HttpStatus(503)) => {}
                Err(error) => return Err(error),
            }
            if cancelled() {
                return Err(CodingToolsRuntimeError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(CodingToolsRuntimeError::HealthTimeout);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn validate_workspace(workspace: &Path) -> Result<(), CodingToolsRuntimeError> {
    match std::fs::metadata(workspace) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(CodingToolsRuntimeError::WorkspaceInvalid),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(CodingToolsRuntimeError::WorkspaceMissing)
        }
        Err(_) => Err(CodingToolsRuntimeError::WorkspaceInvalid),
    }
}

#[cfg(windows)]
fn is_verbatim_workspace_path(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    let prefix = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    path.as_os_str().encode_wide().take(prefix.len()).eq(prefix)
}

#[cfg(not(windows))]
fn is_verbatim_workspace_path(_path: &Path) -> bool {
    false
}

fn reserve_loopback_port(port: u16) -> Result<(), CodingToolsRuntimeError> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .map_err(|_| CodingToolsRuntimeError::PortUnavailable)?;
    drop(listener);
    Ok(())
}
