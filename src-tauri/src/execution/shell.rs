use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

use crate::runtime::{BoundedCommandOutput, SupervisorError, run_bounded_command};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellSelector {
    Auto,
    Powershell,
    Pwsh,
    WindowsPowershell,
    Cmd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemanticVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub revision: u64,
}

impl SemanticVersion {
    pub fn parse(value: &str) -> Option<Self> {
        let mut values = [0u64; 4];
        let mut count = 0usize;
        for part in value.trim().split('.') {
            if count >= values.len()
                || part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            values[count] = part.parse().ok()?;
            count += 1;
        }
        if count < 2 {
            return None;
        }
        Some(Self {
            major: values[0],
            minor: values[1],
            patch: values[2],
            revision: values[3],
        })
    }

    fn parse_installation_directory(value: &str) -> Option<Self> {
        let mut values = [0u64; 4];
        let mut count = 0usize;
        for part in value.trim().split('.') {
            if count >= values.len()
                || part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            values[count] = part.parse().ok()?;
            count += 1;
        }
        (count > 0).then_some(Self {
            major: values[0],
            minor: values[1],
            patch: values[2],
            revision: values[3],
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedShellKind {
    PowerShellCore,
    WindowsPowerShell,
    Cmd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedShell {
    pub kind: ResolvedShellKind,
    pub executable: PathBuf,
    pub version: Option<SemanticVersion>,
    pub management_module: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellResolveError {
    NoShellAvailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellDiscoverySummary {
    pub cmd_available: bool,
    pub powershell_core_available: bool,
    pub powershell_core_version: Option<SemanticVersion>,
    pub windows_powershell_available: bool,
    pub auto_resolved: Option<ResolvedShellKind>,
}

pub trait ShellDiscovery {
    fn pwsh_candidates(&self) -> Vec<PathBuf>;
    fn windows_powershell_candidate(&self) -> Option<PathBuf>;
    fn cmd_candidate(&self) -> Option<PathBuf>;
    fn trusted_pwsh(&self, candidate: &Path) -> bool;
    fn trusted_windows_powershell(&self, candidate: &Path) -> bool;
    fn trusted_cmd(&self, candidate: &Path) -> bool;
    fn trusted_management_module(&self, candidate: &Path) -> Option<PathBuf>;
}

pub trait ShellVersionProbe {
    fn probe_powershell_core(&self, executable: &Path) -> Option<SemanticVersion>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemShellDiscovery;

impl SystemShellDiscovery {
    fn system_directory() -> Option<PathBuf> {
        let mut buffer = [0u16; 32768];
        let length =
            unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
        if length == 0 || length >= buffer.len() {
            return None;
        }
        Some(PathBuf::from(OsString::from_wide(&buffer[..length])))
    }

    fn trusted_powershell_root() -> Option<PathBuf> {
        let system = Self::system_directory()?;
        let windows = system.parent()?;
        let drive = windows.parent()?;
        Some(drive.join("Program Files").join("PowerShell"))
    }

    fn trusted_regular_file(candidate: &Path) -> bool {
        let Ok(metadata) = fs::symlink_metadata(candidate) else {
            return false;
        };
        metadata.is_file() && !metadata.file_type().is_symlink()
    }

    fn same_file_path(candidate: &Path, expected: &Path) -> bool {
        if !Self::trusted_regular_file(candidate) || !Self::trusted_regular_file(expected) {
            return false;
        }
        let Ok(candidate) = fs::canonicalize(candidate) else {
            return false;
        };
        let Ok(expected) = fs::canonicalize(expected) else {
            return false;
        };
        candidate
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected.to_string_lossy())
    }

    fn validated_management_module(candidate: &Path) -> Option<PathBuf> {
        if !Self::trusted_regular_file(candidate) {
            return None;
        }
        let shell_home = candidate.parent()?;
        let manifest = shell_home
            .join("Modules")
            .join("Microsoft.PowerShell.Management")
            .join("Microsoft.PowerShell.Management.psd1");
        if !Self::trusted_regular_file(&manifest) {
            return None;
        }
        let canonical_home = fs::canonicalize(shell_home).ok()?;
        let canonical_manifest = fs::canonicalize(&manifest).ok()?;
        canonical_manifest
            .starts_with(&canonical_home)
            .then_some(manifest)
    }
}

impl ShellDiscovery for SystemShellDiscovery {
    fn pwsh_candidates(&self) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(root) = Self::trusted_powershell_root() {
            if let Ok(entries) = fs::read_dir(root) {
                for entry in entries.flatten() {
                    let installation = entry.path();
                    if installation.is_dir() {
                        candidates.push(installation.join("pwsh.exe"));
                    }
                }
            }
        }
        let mut seen = HashSet::new();
        candidates
            .retain(|candidate| seen.insert(candidate.to_string_lossy().to_ascii_lowercase()));
        candidates
    }

    fn windows_powershell_candidate(&self) -> Option<PathBuf> {
        Some(
            Self::system_directory()?
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe"),
        )
    }

    fn cmd_candidate(&self) -> Option<PathBuf> {
        Some(Self::system_directory()?.join("cmd.exe"))
    }

    fn trusted_pwsh(&self, candidate: &Path) -> bool {
        let Some(root) = Self::trusted_powershell_root() else {
            return false;
        };
        if !Self::trusted_regular_file(candidate) {
            return false;
        }
        let Ok(candidate) = fs::canonicalize(candidate) else {
            return false;
        };
        let Ok(root) = fs::canonicalize(root) else {
            return false;
        };
        candidate.starts_with(&root)
            && candidate
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case(OsStr::new("pwsh.exe")))
    }

    fn trusted_windows_powershell(&self, candidate: &Path) -> bool {
        self.windows_powershell_candidate()
            .is_some_and(|expected| Self::same_file_path(candidate, &expected))
    }

    fn trusted_cmd(&self, candidate: &Path) -> bool {
        self.cmd_candidate()
            .is_some_and(|expected| Self::same_file_path(candidate, &expected))
    }

    fn trusted_management_module(&self, candidate: &Path) -> Option<PathBuf> {
        let trusted_shell =
            self.trusted_pwsh(candidate) || self.trusted_windows_powershell(candidate);
        trusted_shell
            .then(|| Self::validated_management_module(candidate))
            .flatten()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemShellVersionProbe;

impl ShellVersionProbe for SystemShellVersionProbe {
    fn probe_powershell_core(&self, executable: &Path) -> Option<SemanticVersion> {
        let current_dir = executable.parent()?;
        let args = [
            OsString::from("-NoLogo"),
            OsString::from("-NoProfile"),
            OsString::from("-NonInteractive"),
            OsString::from("-Command"),
            OsString::from("$PSVersionTable.PSVersion.ToString()"),
        ];
        let output =
            run_bounded_command(executable, &args, current_dir, Duration::from_secs(3), 4096)
                .ok()?;
        if output.timed_out || output.exit_code != 0 {
            return None;
        }
        SemanticVersion::parse(String::from_utf8_lossy(&output.output).trim())
    }
}

#[derive(Debug, Clone)]
pub struct ShellResolver<D = SystemShellDiscovery, P = SystemShellVersionProbe> {
    discovery: D,
    probe: P,
}

impl Default for ShellResolver<SystemShellDiscovery, SystemShellVersionProbe> {
    fn default() -> Self {
        Self {
            discovery: SystemShellDiscovery,
            probe: SystemShellVersionProbe,
        }
    }
}

impl<D, P> ShellResolver<D, P>
where
    D: ShellDiscovery,
    P: ShellVersionProbe,
{
    pub fn new(discovery: D, probe: P) -> Self {
        Self { discovery, probe }
    }

    pub fn resolve(&self, selector: ShellSelector) -> Result<ResolvedShell, ShellResolveError> {
        match selector {
            ShellSelector::Auto => self
                .highest_core()
                .or_else(|| self.windows_powershell())
                .or_else(|| self.cmd())
                .ok_or(ShellResolveError::NoShellAvailable),
            ShellSelector::Powershell => self
                .highest_core()
                .or_else(|| self.windows_powershell())
                .ok_or(ShellResolveError::NoShellAvailable),
            ShellSelector::Pwsh => self
                .highest_core()
                .ok_or(ShellResolveError::NoShellAvailable),
            ShellSelector::WindowsPowershell => self
                .windows_powershell()
                .ok_or(ShellResolveError::NoShellAvailable),
            ShellSelector::Cmd => self.cmd().ok_or(ShellResolveError::NoShellAvailable),
        }
    }

    pub fn discovery_summary(&self) -> ShellDiscoverySummary {
        let core = self.highest_core();
        let windows = self.windows_powershell();
        let cmd = self.cmd();
        let auto_resolved = core
            .as_ref()
            .map(|shell| shell.kind)
            .or_else(|| windows.as_ref().map(|shell| shell.kind))
            .or_else(|| cmd.as_ref().map(|shell| shell.kind));
        ShellDiscoverySummary {
            cmd_available: cmd.is_some(),
            powershell_core_available: core.is_some(),
            powershell_core_version: core.as_ref().and_then(|shell| shell.version),
            windows_powershell_available: windows.is_some(),
            auto_resolved,
        }
    }

    /// Resolve a trusted shell for the privileged Broker without launching candidates under the
    /// ordinary LocalBridge token. PowerShell Core ordering comes from the protected install
    /// directory version rather than a process probe.
    pub fn resolve_for_broker(
        &self,
        selector: ShellSelector,
    ) -> Result<ResolvedShell, ShellResolveError> {
        match selector {
            ShellSelector::Auto => self
                .highest_core_for_broker()
                .or_else(|| self.windows_powershell())
                .or_else(|| self.cmd())
                .ok_or(ShellResolveError::NoShellAvailable),
            ShellSelector::Powershell => self
                .highest_core_for_broker()
                .or_else(|| self.windows_powershell())
                .ok_or(ShellResolveError::NoShellAvailable),
            ShellSelector::Pwsh => self
                .highest_core_for_broker()
                .ok_or(ShellResolveError::NoShellAvailable),
            ShellSelector::WindowsPowershell => self
                .windows_powershell()
                .ok_or(ShellResolveError::NoShellAvailable),
            ShellSelector::Cmd => self.cmd().ok_or(ShellResolveError::NoShellAvailable),
        }
    }

    fn highest_core(&self) -> Option<ResolvedShell> {
        self.discovery
            .pwsh_candidates()
            .into_iter()
            .filter(|candidate| self.discovery.trusted_pwsh(candidate))
            .filter_map(|candidate| {
                let version = self.probe.probe_powershell_core(&candidate)?;
                Some(ResolvedShell {
                    kind: ResolvedShellKind::PowerShellCore,
                    management_module: self.discovery.trusted_management_module(&candidate),
                    executable: candidate,
                    version: Some(version),
                })
            })
            .filter(|shell| shell.management_module.is_some())
            .max_by_key(|shell| shell.version)
    }

    fn highest_core_for_broker(&self) -> Option<ResolvedShell> {
        self.discovery
            .pwsh_candidates()
            .into_iter()
            .filter(|candidate| self.discovery.trusted_pwsh(candidate))
            .filter_map(|candidate| {
                let version = candidate
                    .parent()?
                    .file_name()?
                    .to_str()
                    .and_then(SemanticVersion::parse_installation_directory)?;
                Some(ResolvedShell {
                    kind: ResolvedShellKind::PowerShellCore,
                    management_module: self.discovery.trusted_management_module(&candidate),
                    executable: candidate,
                    version: Some(version),
                })
            })
            .filter(|shell| shell.management_module.is_some())
            .max_by_key(|shell| shell.version)
    }

    fn windows_powershell(&self) -> Option<ResolvedShell> {
        let candidate = self.discovery.windows_powershell_candidate()?;
        self.discovery
            .trusted_windows_powershell(&candidate)
            .then(|| ResolvedShell {
                kind: ResolvedShellKind::WindowsPowerShell,
                management_module: self.discovery.trusted_management_module(&candidate),
                executable: candidate,
                version: Some(SemanticVersion {
                    major: 5,
                    minor: 1,
                    patch: 0,
                    revision: 0,
                }),
            })
            .filter(|shell| shell.management_module.is_some())
    }

    fn cmd(&self) -> Option<ResolvedShell> {
        let candidate = self.discovery.cmd_candidate()?;
        self.discovery
            .trusted_cmd(&candidate)
            .then_some(ResolvedShell {
                kind: ResolvedShellKind::Cmd,
                executable: candidate,
                version: None,
                management_module: None,
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectProcessSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DirectProcessExecutor;

impl DirectProcessExecutor {
    pub fn execute(
        &self,
        spec: &DirectProcessSpec,
    ) -> Result<BoundedCommandOutput, SupervisorError> {
        run_bounded_command(
            &spec.program,
            &spec.args,
            &spec.cwd,
            spec.timeout,
            spec.max_output_bytes,
        )
    }
}

fn powershell_single_quoted_literal(value: &Path) -> String {
    value.to_string_lossy().replace('\'', "''")
}

fn hardened_powershell_script(command: &str, management_module: &Path) -> String {
    let management_module = powershell_single_quoted_literal(management_module);
    format!(
        "Set-Variable -Name PSModuleAutoLoadingPreference -Value None -Option Constant -Force;Import-Module -Name '{management_module}' -ErrorAction Stop;Remove-Item Alias:curl -Force -ErrorAction SilentlyContinue;[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false);$OutputEncoding=[Console]::OutputEncoding;{command}"
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellExecutionSpec {
    pub shell: ShellSelector,
    pub command: String,
    pub cwd: PathBuf,
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellRuntimeInvocation {
    pub command_line: String,
    pub comspec: PathBuf,
    pub output_encoding: &'static str,
}

pub struct ShellExecutor<D = SystemShellDiscovery, P = SystemShellVersionProbe> {
    resolver: ShellResolver<D, P>,
    direct: DirectProcessExecutor,
}

impl Default for ShellExecutor<SystemShellDiscovery, SystemShellVersionProbe> {
    fn default() -> Self {
        Self {
            resolver: ShellResolver::default(),
            direct: DirectProcessExecutor,
        }
    }
}

impl<D, P> ShellExecutor<D, P>
where
    D: ShellDiscovery,
    P: ShellVersionProbe,
{
    pub fn new(resolver: ShellResolver<D, P>) -> Self {
        Self {
            resolver,
            direct: DirectProcessExecutor,
        }
    }

    pub fn discovery_summary(&self) -> ShellDiscoverySummary {
        self.resolver.discovery_summary()
    }

    pub fn resolved_kind(
        &self,
        selector: ShellSelector,
    ) -> Result<ResolvedShellKind, ShellResolveError> {
        self.resolver.resolve(selector).map(|shell| shell.kind)
    }

    pub fn direct_spec(
        &self,
        spec: &ShellExecutionSpec,
    ) -> Result<DirectProcessSpec, ShellResolveError> {
        let shell = self.resolver.resolve(spec.shell)?;
        Self::direct_spec_for_resolved_shell(spec, shell)
    }

    pub fn broker_direct_spec(
        &self,
        spec: &ShellExecutionSpec,
    ) -> Result<DirectProcessSpec, ShellResolveError> {
        let shell = self.resolver.resolve_for_broker(spec.shell)?;
        Self::direct_spec_for_resolved_shell(spec, shell)
    }

    fn direct_spec_for_resolved_shell(
        spec: &ShellExecutionSpec,
        shell: ResolvedShell,
    ) -> Result<DirectProcessSpec, ShellResolveError> {
        let args = match shell.kind {
            ResolvedShellKind::PowerShellCore | ResolvedShellKind::WindowsPowerShell => {
                let management_module = shell
                    .management_module
                    .as_deref()
                    .ok_or(ShellResolveError::NoShellAvailable)?;
                vec![
                    OsString::from("-NoLogo"),
                    OsString::from("-NoProfile"),
                    OsString::from("-NonInteractive"),
                    OsString::from("-Command"),
                    OsString::from(hardened_powershell_script(&spec.command, management_module)),
                ]
            }
            ResolvedShellKind::Cmd => vec![
                OsString::from("/d"),
                OsString::from("/s"),
                OsString::from("/c"),
                OsString::from(&spec.command),
            ],
        };
        Ok(DirectProcessSpec {
            program: shell.executable,
            args,
            cwd: spec.cwd.clone(),
            timeout: Duration::from_millis(spec.timeout_ms),
            max_output_bytes: spec.max_output_bytes,
        })
    }

    pub fn runtime_invocation(
        &self,
        spec: &ShellExecutionSpec,
    ) -> Result<ShellRuntimeInvocation, ShellResolveError> {
        let shell = self.resolver.resolve(spec.shell)?;
        let trusted_cmd = self.resolver.resolve(ShellSelector::Cmd)?;
        let output_encoding = match shell.kind {
            ResolvedShellKind::Cmd => "windows_oem",
            ResolvedShellKind::PowerShellCore | ResolvedShellKind::WindowsPowerShell => "utf-8",
        };
        let command_line = match shell.kind {
            ResolvedShellKind::Cmd => spec.command.clone(),
            ResolvedShellKind::PowerShellCore | ResolvedShellKind::WindowsPowerShell => {
                let management_module = shell
                    .management_module
                    .as_deref()
                    .ok_or(ShellResolveError::NoShellAvailable)?;
                let script = hardened_powershell_script(&spec.command, management_module);
                let mut utf16le = Vec::with_capacity(script.len() * 2);
                for unit in script.encode_utf16() {
                    utf16le.extend_from_slice(&unit.to_le_bytes());
                }
                let encoded = base64::engine::general_purpose::STANDARD.encode(utf16le);
                format!(
                    "{} -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
                    windows_shell_quote(shell.executable.as_os_str()),
                    encoded
                )
            }
        };
        Ok(ShellRuntimeInvocation {
            command_line,
            comspec: trusted_cmd.executable,
            output_encoding,
        })
    }

    pub fn execute(
        &self,
        spec: &ShellExecutionSpec,
    ) -> Result<BoundedCommandOutput, ShellExecutionError> {
        let direct = self
            .direct_spec(spec)
            .map_err(ShellExecutionError::Resolve)?;
        self.direct
            .execute(&direct)
            .map_err(ShellExecutionError::Process)
    }
}

fn windows_shell_quote(value: &OsStr) -> String {
    let value = value.to_string_lossy();
    if !value.contains([' ', '\t', '"']) {
        return value.into_owned();
    }
    let mut quoted = String::from("\"");
    let mut backslashes = 0usize;
    for ch in value.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                quoted.push(ch);
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

#[derive(Debug)]
pub enum ShellExecutionError {
    Resolve(ShellResolveError),
    Process(SupervisorError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct FakeDiscovery {
        pwsh: Vec<PathBuf>,
        trusted: HashSet<PathBuf>,
        windows: Option<PathBuf>,
        cmd: Option<PathBuf>,
    }

    impl ShellDiscovery for FakeDiscovery {
        fn pwsh_candidates(&self) -> Vec<PathBuf> {
            self.pwsh.clone()
        }
        fn windows_powershell_candidate(&self) -> Option<PathBuf> {
            self.windows.clone()
        }
        fn cmd_candidate(&self) -> Option<PathBuf> {
            self.cmd.clone()
        }
        fn trusted_pwsh(&self, candidate: &Path) -> bool {
            self.trusted.contains(candidate)
        }
        fn trusted_windows_powershell(&self, candidate: &Path) -> bool {
            self.windows.as_deref() == Some(candidate)
        }
        fn trusted_cmd(&self, candidate: &Path) -> bool {
            self.cmd.as_deref() == Some(candidate)
        }
        fn trusted_management_module(&self, candidate: &Path) -> Option<PathBuf> {
            (self.trusted.contains(candidate) || self.windows.as_deref() == Some(candidate)).then(
                || {
                    candidate
                        .parent()
                        .unwrap_or_else(|| Path::new(r"C:\\trusted"))
                        .join("Modules")
                        .join("Microsoft.PowerShell.Management")
                        .join("Microsoft.PowerShell.Management.psd1")
                },
            )
        }
    }

    #[derive(Clone)]
    struct FakeProbe {
        versions: HashMap<PathBuf, SemanticVersion>,
        probed: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl ShellVersionProbe for FakeProbe {
        fn probe_powershell_core(&self, executable: &Path) -> Option<SemanticVersion> {
            self.probed.lock().unwrap().push(executable.to_path_buf());
            self.versions.get(executable).copied()
        }
    }

    fn version(value: &str) -> SemanticVersion {
        SemanticVersion::parse(value).unwrap()
    }

    #[test]
    fn semantic_version_is_numeric_not_lexical() {
        assert!(version("7.10.0") > version("7.9.99"));
        assert!(version("8.0.0.1") > version("8.0.0"));
    }

    #[test]
    fn auto_chooses_highest_trusted_core_and_never_probes_untrusted_path_candidate() {
        let malicious = PathBuf::from(r"C:\attacker\pwsh.exe");
        let core7 = PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe");
        let core8 = PathBuf::from(r"C:\Program Files\PowerShell\8\pwsh.exe");
        let probed = Arc::new(Mutex::new(Vec::new()));
        let resolver = ShellResolver::new(
            FakeDiscovery {
                pwsh: vec![malicious.clone(), core7.clone(), core8.clone()],
                trusted: [core7.clone(), core8.clone()].into_iter().collect(),
                windows: Some(PathBuf::from(
                    r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
                )),
                cmd: Some(PathBuf::from(r"C:\Windows\System32\cmd.exe")),
            },
            FakeProbe {
                versions: [
                    (core7.clone(), version("7.5.1")),
                    (core8.clone(), version("8.0.0")),
                ]
                .into_iter()
                .collect(),
                probed: Arc::clone(&probed),
            },
        );
        let resolved = resolver.resolve(ShellSelector::Auto).unwrap();
        assert_eq!(resolved.executable, core8);
        assert!(!probed.lock().unwrap().contains(&malicious));
    }

    #[test]
    fn broker_shell_preparation_never_runs_version_probe_under_ordinary_token() {
        let core7 = PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe");
        let core81 = PathBuf::from(r"C:\Program Files\PowerShell\8.1\pwsh.exe");
        let probed = Arc::new(Mutex::new(Vec::new()));
        let resolver = ShellResolver::new(
            FakeDiscovery {
                pwsh: vec![core7.clone(), core81.clone()],
                trusted: [core7, core81.clone()].into_iter().collect(),
                windows: None,
                cmd: Some(PathBuf::from(r"C:\Windows\System32\cmd.exe")),
            },
            FakeProbe {
                versions: HashMap::new(),
                probed: Arc::clone(&probed),
            },
        );
        let direct = ShellExecutor::new(resolver)
            .broker_direct_spec(&ShellExecutionSpec {
                shell: ShellSelector::Pwsh,
                command: "Write-Output LB012_BROKER_SHELL".into(),
                cwd: PathBuf::from(r"C:\Windows\Temp"),
                timeout_ms: 1_000,
                max_output_bytes: 4_096,
            })
            .unwrap();
        assert_eq!(direct.program, core81);
        assert!(probed.lock().unwrap().is_empty());
        assert!(
            direct
                .args
                .last()
                .unwrap()
                .to_string_lossy()
                .ends_with("Write-Output LB012_BROKER_SHELL")
        );
    }

    #[test]
    fn production_core_discovery_is_not_path_authoritative() {
        let source = include_str!("shell.rs");
        let discovery = source
            .split("impl ShellDiscovery for SystemShellDiscovery")
            .nth(1)
            .and_then(|tail| tail.split("impl ShellVersionProbe").next())
            .expect("production shell discovery source exists");
        assert!(!discovery.contains("var_os(\"PATH\")"));
        assert!(!discovery.contains("split_paths"));
        assert!(discovery.contains("trusted_powershell_root"));
    }

    #[test]
    fn selectors_and_fallbacks_are_deterministic() {
        let win = PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
        let cmd = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        let resolver = ShellResolver::new(
            FakeDiscovery {
                pwsh: vec![],
                trusted: HashSet::new(),
                windows: Some(win.clone()),
                cmd: Some(cmd.clone()),
            },
            FakeProbe {
                versions: HashMap::new(),
                probed: Arc::new(Mutex::new(Vec::new())),
            },
        );
        assert_eq!(
            resolver.resolve(ShellSelector::Auto).unwrap().executable,
            win
        );
        assert_eq!(
            resolver.resolve(ShellSelector::Powershell).unwrap().kind,
            ResolvedShellKind::WindowsPowerShell
        );
        assert!(matches!(
            resolver.resolve(ShellSelector::Pwsh),
            Err(ShellResolveError::NoShellAvailable)
        ));
        assert_eq!(
            resolver
                .resolve(ShellSelector::WindowsPowershell)
                .unwrap()
                .kind,
            ResolvedShellKind::WindowsPowerShell
        );
        assert_eq!(
            resolver.resolve(ShellSelector::Cmd).unwrap().executable,
            cmd
        );

        let resolver = ShellResolver::new(
            FakeDiscovery {
                pwsh: vec![],
                trusted: HashSet::new(),
                windows: None,
                cmd: Some(cmd.clone()),
            },
            FakeProbe {
                versions: HashMap::new(),
                probed: Arc::new(Mutex::new(Vec::new())),
            },
        );
        assert_eq!(
            resolver.resolve(ShellSelector::Auto).unwrap().executable,
            cmd
        );
        assert!(matches!(
            resolver.resolve(ShellSelector::Powershell),
            Err(ShellResolveError::NoShellAvailable)
        ));
    }

    #[test]
    fn shell_spec_exposes_only_logical_selector_not_executable_path() {
        let value = serde_json::to_value(ShellExecutionSpec {
            shell: ShellSelector::Pwsh,
            command: "Get-Location".into(),
            cwd: PathBuf::from(r"C:\work"),
            timeout_ms: 1000,
            max_output_bytes: 4096,
        })
        .unwrap();
        assert_eq!(value["shell"], "pwsh");
        assert!(value.get("program").is_none());
        assert!(value.get("executable").is_none());
        assert!(
            serde_json::from_value::<ShellExecutionSpec>(serde_json::json!({
                "shell":"C:\\attacker\\pwsh.exe",
                "command":"whoami",
                "cwd":"C:\\work",
                "timeout_ms":1000,
                "max_output_bytes":4096
            }))
            .is_err()
        );
    }

    #[test]
    fn no_candidate_fails_closed_without_install_or_update_fallback() {
        let resolver = ShellResolver::new(
            FakeDiscovery {
                pwsh: vec![],
                trusted: HashSet::new(),
                windows: None,
                cmd: None,
            },
            FakeProbe {
                versions: HashMap::new(),
                probed: Arc::new(Mutex::new(Vec::new())),
            },
        );
        assert!(matches!(
            resolver.resolve(ShellSelector::Auto),
            Err(ShellResolveError::NoShellAvailable)
        ));
    }

    #[test]
    fn direct_process_and_shell_specs_are_structurally_separate() {
        let cmd = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        let resolver = ShellResolver::new(
            FakeDiscovery {
                pwsh: vec![],
                trusted: HashSet::new(),
                windows: None,
                cmd: Some(cmd.clone()),
            },
            FakeProbe {
                versions: HashMap::new(),
                probed: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let executor = ShellExecutor::new(resolver);
        let shell = ShellExecutionSpec {
            shell: ShellSelector::Cmd,
            command: "echo ok".into(),
            cwd: PathBuf::from(r"C:\work"),
            timeout_ms: 1000,
            max_output_bytes: 4096,
        };
        let direct = executor.direct_spec(&shell).unwrap();
        assert_eq!(direct.program, cmd);
        assert_eq!(direct.args[0], OsString::from("/d"));
        assert_ne!(
            std::any::TypeId::of::<DirectProcessSpec>(),
            std::any::TypeId::of::<ShellExecutionSpec>()
        );
    }

    #[test]
    fn powershell_runtime_invocation_encodes_user_text_for_single_parse_and_utf8_prologue() {
        let win = PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
        let cmd = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        let resolver = ShellResolver::new(
            FakeDiscovery {
                pwsh: vec![],
                trusted: HashSet::new(),
                windows: Some(win),
                cmd: Some(cmd.clone()),
            },
            FakeProbe {
                versions: HashMap::new(),
                probed: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let executor = ShellExecutor::new(resolver);
        let user = "Write-Output \"a|b\"; Write-Output \"a&b\"; Write-Output 'q|b'; Write-Output 'q&b'; Write-Output '中文输出✓'";
        let invocation = executor
            .runtime_invocation(&ShellExecutionSpec {
                shell: ShellSelector::WindowsPowershell,
                command: user.into(),
                cwd: PathBuf::from(r"C:\work"),
                timeout_ms: 1000,
                max_output_bytes: 4096,
            })
            .unwrap();
        assert_eq!(invocation.comspec, cmd);
        assert!(!invocation.command_line.contains("a|b"));
        assert!(!invocation.command_line.contains("a&b"));
        assert!(!invocation.command_line.contains("中文输出"));
        let encoded = invocation.command_line.split_whitespace().last().unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert_eq!(bytes.len() % 2, 0);
        let units = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        let decoded = String::from_utf16(&units).unwrap();
        assert!(decoded.starts_with(
            "Set-Variable -Name PSModuleAutoLoadingPreference -Value None -Option Constant -Force;"
        ));
        assert!(decoded.contains("PSModuleAutoLoadingPreference"));
        assert!(decoded.contains("Microsoft.PowerShell.Management.psd1"));
        assert!(decoded.contains("Import-Module -Name '"));
        assert!(decoded.contains("OutputEncoding"));
        assert!(decoded.ends_with(user));
    }

    #[test]
    fn powershell_direct_spec_locks_module_autoload_before_user_text() {
        let win = PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
        let cmd = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        let resolver = ShellResolver::new(
            FakeDiscovery {
                pwsh: vec![],
                trusted: HashSet::new(),
                windows: Some(win),
                cmd: Some(cmd),
            },
            FakeProbe {
                versions: HashMap::new(),
                probed: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let executor = ShellExecutor::new(resolver);
        let user = "Write-Output 'LB_DIRECT_USER'";
        let direct = executor
            .direct_spec(&ShellExecutionSpec {
                shell: ShellSelector::WindowsPowershell,
                command: user.into(),
                cwd: PathBuf::from(r"C:\workspace"),
                timeout_ms: 1_000,
                max_output_bytes: 4_096,
            })
            .unwrap();
        let script = direct.args.last().unwrap().to_string_lossy();
        assert!(script.starts_with(
            "Set-Variable -Name PSModuleAutoLoadingPreference -Value None -Option Constant -Force;"
        ));
        assert!(script.contains("Microsoft.PowerShell.Management.psd1"));
        assert!(script.contains("Import-Module -Name '"));
        assert!(script.contains("OutputEncoding"));
        assert!(script.ends_with(user));
    }

    #[test]
    fn cmd_runtime_invocation_preserves_native_nul_device_redirection() {
        let cmd = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        let resolver = ShellResolver::new(
            FakeDiscovery {
                pwsh: vec![],
                trusted: HashSet::new(),
                windows: None,
                cmd: Some(cmd.clone()),
            },
            FakeProbe {
                versions: HashMap::new(),
                probed: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let executor = ShellExecutor::new(resolver);
        let invocation = executor
            .runtime_invocation(&ShellExecutionSpec {
                shell: ShellSelector::Cmd,
                command: "echo ok>nul && echo done 2> NUL".into(),
                cwd: PathBuf::from(r"C:\work"),
                timeout_ms: 1000,
                max_output_bytes: 4096,
            })
            .unwrap();
        assert_eq!(invocation.comspec, cmd);
        assert_eq!(invocation.command_line, "echo ok>nul && echo done 2> NUL");
        assert_eq!(invocation.output_encoding, "windows_oem");
        assert!(!invocation.command_line.contains("nul.localbridge"));
    }

    #[test]
    fn cmd_runtime_invocation_keeps_user_text_raw_and_binds_trusted_comspec() {
        let cmd = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        let resolver = ShellResolver::new(
            FakeDiscovery {
                pwsh: vec![],
                trusted: HashSet::new(),
                windows: None,
                cmd: Some(cmd.clone()),
            },
            FakeProbe {
                versions: HashMap::new(),
                probed: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let executor = ShellExecutor::new(resolver);
        let command = "echo a^|b & echo a^&b";
        let invocation = executor
            .runtime_invocation(&ShellExecutionSpec {
                shell: ShellSelector::Cmd,
                command: command.into(),
                cwd: PathBuf::from(r"C:\work"),
                timeout_ms: 1000,
                max_output_bytes: 4096,
            })
            .unwrap();
        assert_eq!(invocation.comspec, cmd);
        assert_eq!(invocation.command_line, command);
        assert_eq!(invocation.output_encoding, "windows_oem");
    }
}
