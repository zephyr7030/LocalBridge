#[cfg(not(debug_assertions))]
use std::ffi::OsString;
use std::ffi::{OsStr, c_void};
use std::fmt;
#[cfg(debug_assertions)]
use std::fs::{File, OpenOptions};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
#[cfg(debug_assertions)]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(not(debug_assertions))]
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_CANCELLED, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY,
    ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
#[cfg(any(not(debug_assertions), test))]
use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, GENERIC_ALL};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
#[cfg(any(not(debug_assertions), test))]
use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
#[cfg(any(not(debug_assertions), test))]
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce, INHERIT_ONLY_ACE,
    OWNER_SECURITY_INFORMATION,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_SHARE_READ,
    FlushFileBuffers, OPEN_EXISTING, ReadFile, WriteFile,
};
#[cfg(any(not(debug_assertions), test))]
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_APPEND_DATA, FILE_DELETE_CHILD,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
    FILE_WRITE_DATA, FILE_WRITE_EA,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessId, OpenProcessToken, TerminateProcess, WaitForSingleObject,
};
#[cfg(not(debug_assertions))]
use windows_sys::Win32::UI::Shell::{CSIDL_PROGRAM_FILES, SHGFP_TYPE_CURRENT, SHGetFolderPathW};
use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};

use super::protocol::is_valid_broker_pipe_name;
use super::{MAX_BROKER_FRAME_BYTES, SESSION_NONCE_BYTES, SessionNonce};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const PIPE_BUFFER_BYTES: u32 = MAX_BROKER_FRAME_BYTES as u32 + 4;
const PIPE_ACCESS_DUPLEX_MODE: u32 = 0x0000_0003;
#[cfg(any(not(debug_assertions), test))]
const ACCESS_ALLOWED_ACE_KIND: u8 = 0x00;
#[cfg(any(not(debug_assertions), test))]
const ACCESS_DENIED_ACE_KIND: u8 = 0x01;
#[cfg(any(not(debug_assertions), test))]
const DELETE_ACCESS: u32 = 0x0001_0000;
#[cfg(any(not(debug_assertions), test))]
const WRITE_DAC_ACCESS: u32 = 0x0004_0000;
#[cfg(any(not(debug_assertions), test))]
const WRITE_OWNER_ACCESS: u32 = 0x0008_0000;
#[cfg(any(not(debug_assertions), test))]
const TRUSTED_INSTALL_MUTATION_SIDS: [&str; 5] = [
    "S-1-5-18",
    "S-1-5-32-544",
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464",
    "S-1-3-0",
    "S-1-3-4",
];
#[cfg(any(not(debug_assertions), test))]
const INSTALL_MUTATION_MASK: u32 = GENERIC_ALL
    | GENERIC_WRITE
    | DELETE_ACCESS
    | WRITE_DAC_ACCESS
    | WRITE_OWNER_ACCESS
    | FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_WRITE_EA
    | FILE_WRITE_ATTRIBUTES
    | FILE_ADD_FILE
    | FILE_ADD_SUBDIRECTORY
    | FILE_DELETE_CHILD;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegeIpcError {
    RandomUnavailable,
    CurrentUserSidUnavailable,
    SecurityDescriptorUnavailable,
    PipeCreateFailed(u32),
    PipeConnectFailed(u32),
    UnauthorizedPeer { expected_pid: u32, actual_pid: u32 },
    Disconnected,
    IoFailed { operation: &'static str, code: u32 },
    EmptyFrame,
    OversizedFrame,
}

impl fmt::Display for PrivilegeIpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RandomUnavailable => f.write_str("secure random generation unavailable"),
            Self::CurrentUserSidUnavailable => f.write_str("current user SID unavailable"),
            Self::SecurityDescriptorUnavailable => {
                f.write_str("pipe security descriptor unavailable")
            }
            Self::PipeCreateFailed(code) => {
                write!(f, "named pipe creation failed with code {code}")
            }
            Self::PipeConnectFailed(code) => {
                write!(f, "named pipe connection failed with code {code}")
            }
            Self::UnauthorizedPeer {
                expected_pid,
                actual_pid,
            } => write!(
                f,
                "named pipe peer mismatch: expected pid {expected_pid}, got {actual_pid}"
            ),
            Self::Disconnected => f.write_str("named pipe disconnected"),
            Self::IoFailed { operation, code } => {
                write!(f, "named pipe {operation} failed with code {code}")
            }
            Self::EmptyFrame => f.write_str("named pipe frame is empty"),
            Self::OversizedFrame => f.write_str("named pipe frame exceeds limit"),
        }
    }
}

impl std::error::Error for PrivilegeIpcError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UacLaunchError {
    InvalidBrokerExecutable,
    InvalidLaunchContext,
    UacDenied,
    LaunchFailed(u32),
}

impl fmt::Display for UacLaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBrokerExecutable => f.write_str("privileged broker executable is invalid"),
            Self::InvalidLaunchContext => {
                f.write_str("privileged broker launch context is invalid")
            }
            Self::UacDenied => f.write_str("privileged broker UAC request was denied"),
            Self::LaunchFailed(code) => {
                write!(f, "privileged broker launch failed with code {code}")
            }
        }
    }
}

impl std::error::Error for UacLaunchError {}

pub struct ElevatedBrokerProcess {
    handle: HANDLE,
    pid: u32,
    executable: PathBuf,
}

impl fmt::Debug for ElevatedBrokerProcess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElevatedBrokerProcess")
            .field("pid", &self.pid)
            .field("executable", &self.executable)
            .finish()
    }
}

impl ElevatedBrokerProcess {
    pub const fn pid(&self) -> u32 {
        self.pid
    }
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn is_running(&self) -> Result<bool, UacLaunchError> {
        match unsafe { WaitForSingleObject(self.handle, 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            _ => Err(UacLaunchError::LaunchFailed(last_error_code())),
        }
    }

    pub fn wait_for_exit(&self, timeout: Duration) -> Result<bool, UacLaunchError> {
        let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        match unsafe { WaitForSingleObject(self.handle, timeout_ms) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(UacLaunchError::LaunchFailed(last_error_code())),
        }
    }

    pub fn terminate(&mut self) -> Result<(), UacLaunchError> {
        if unsafe { TerminateProcess(self.handle, 0x4C42_1203) } == 0 {
            Err(UacLaunchError::LaunchFailed(last_error_code()))
        } else {
            Ok(())
        }
    }
}

// Win32 kernel handles are process-wide rather than thread-affine. Ownership moves as a unit.
unsafe impl Send for ElevatedBrokerProcess {}

impl Drop for ElevatedBrokerProcess {
    fn drop(&mut self) {
        if self.is_running().unwrap_or(false) {
            let _ = self.terminate();
            let _ = self.wait_for_exit(Duration::from_secs(5));
        }
        close_if_valid(&mut self.handle);
    }
}

pub fn launch_broker_with_explicit_uac(
    broker_executable: &Path,
    pipe_name: &str,
    generation: u64,
) -> Result<ElevatedBrokerProcess, UacLaunchError> {
    let trusted_broker = validate_broker_executable_for_current_install(broker_executable)?;
    #[cfg(debug_assertions)]
    let _development_pin = pin_development_broker(&trusted_broker)?;
    let parameters = build_uac_parameters(pipe_name, generation)?;
    let verb = wide_null(OsStr::new("runas"));
    let executable = wide_null(trusted_broker.as_os_str());
    let parameters = wide_null(OsStr::new(&parameters));
    let mut info: SHELLEXECUTEINFOW = unsafe { zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = executable.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = 0;
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let code = last_error_code();
        return Err(if code == ERROR_CANCELLED {
            UacLaunchError::UacDenied
        } else {
            UacLaunchError::LaunchFailed(code)
        });
    }
    if info.hProcess.is_null() {
        return Err(UacLaunchError::LaunchFailed(0));
    }
    let pid = unsafe { GetProcessId(info.hProcess) };
    if pid == 0 {
        let code = last_error_code();
        unsafe { CloseHandle(info.hProcess) };
        return Err(UacLaunchError::LaunchFailed(code));
    }
    Ok(ElevatedBrokerProcess {
        handle: info.hProcess,
        pid,
        executable: trusted_broker,
    })
}

fn validate_broker_executable_for_current_install(
    broker_executable: &Path,
) -> Result<PathBuf, UacLaunchError> {
    let current_executable =
        std::env::current_exe().map_err(|_| UacLaunchError::InvalidBrokerExecutable)?;
    #[cfg(debug_assertions)]
    {
        validate_broker_executable(broker_executable, &current_executable, None)
    }
    #[cfg(not(debug_assertions))]
    let protected_root = Some(protected_machine_install_root()?);
    #[cfg(not(debug_assertions))]
    let trusted_broker = validate_broker_executable(
        broker_executable,
        &current_executable,
        protected_root.as_deref(),
    )?;
    #[cfg(not(debug_assertions))]
    let current = current_executable
        .canonicalize()
        .map_err(|_| UacLaunchError::InvalidBrokerExecutable)?;
    #[cfg(not(debug_assertions))]
    let install_root = current
        .parent()
        .ok_or(UacLaunchError::InvalidBrokerExecutable)?;
    #[cfg(not(debug_assertions))]
    verify_broker_installation_not_mutable_by_unprivileged_principal(
        install_root,
        &trusted_broker,
        protected_root.as_deref(),
    )?;
    #[cfg(not(debug_assertions))]
    Ok(trusted_broker)
}

#[cfg(debug_assertions)]
struct DevelopmentBrokerPin {
    _file: File,
}

#[cfg(debug_assertions)]
fn pin_development_broker(path: &Path) -> Result<DevelopmentBrokerPin, UacLaunchError> {
    let expected = path
        .canonicalize()
        .map_err(|_| UacLaunchError::InvalidBrokerExecutable)?;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&expected)
        .map_err(|_| UacLaunchError::InvalidBrokerExecutable)?;
    let pinned_path = path
        .canonicalize()
        .map_err(|_| UacLaunchError::InvalidBrokerExecutable)?;
    if !same_windows_path(&expected, &pinned_path) {
        return Err(UacLaunchError::InvalidBrokerExecutable);
    }
    Ok(DevelopmentBrokerPin { _file: file })
}

fn validate_broker_executable(
    broker_executable: &Path,
    current_executable: &Path,
    protected_root: Option<&Path>,
) -> Result<PathBuf, UacLaunchError> {
    let valid_name = broker_executable
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("localbridge-privileged-broker.exe"));
    if !broker_executable.is_absolute() || !current_executable.is_absolute() || !valid_name {
        return Err(UacLaunchError::InvalidBrokerExecutable);
    }
    let metadata = std::fs::symlink_metadata(broker_executable)
        .map_err(|_| UacLaunchError::InvalidBrokerExecutable)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(UacLaunchError::InvalidBrokerExecutable);
    }
    let current = current_executable
        .canonicalize()
        .map_err(|_| UacLaunchError::InvalidBrokerExecutable)?;
    let install_root = current
        .parent()
        .ok_or(UacLaunchError::InvalidBrokerExecutable)?
        .to_path_buf();
    let expected = install_root
        .join("localbridge-privileged-broker.exe")
        .canonicalize()
        .map_err(|_| UacLaunchError::InvalidBrokerExecutable)?;
    let requested = broker_executable
        .canonicalize()
        .map_err(|_| UacLaunchError::InvalidBrokerExecutable)?;
    if !same_windows_path(&requested, &expected)
        || !requested
            .parent()
            .is_some_and(|parent| same_windows_path(parent, &install_root))
    {
        return Err(UacLaunchError::InvalidBrokerExecutable);
    }
    if let Some(protected_root) = protected_root {
        let protected_root = protected_root
            .canonicalize()
            .map_err(|_| UacLaunchError::InvalidBrokerExecutable)?;
        if !windows_path_is_within(&install_root, &protected_root) {
            return Err(UacLaunchError::InvalidBrokerExecutable);
        }
    }
    Ok(requested)
}

#[cfg(not(debug_assertions))]
fn protected_machine_install_root() -> Result<PathBuf, UacLaunchError> {
    let mut buffer = [0u16; 260];
    let result = unsafe {
        SHGetFolderPathW(
            null_mut(),
            CSIDL_PROGRAM_FILES as i32,
            null_mut(),
            SHGFP_TYPE_CURRENT as u32,
            buffer.as_mut_ptr(),
        )
    };
    if result < 0 {
        return Err(UacLaunchError::InvalidBrokerExecutable);
    }
    let length = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    if length == 0 {
        return Err(UacLaunchError::InvalidBrokerExecutable);
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

fn same_windows_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn windows_path_is_within(path: &Path, root: &Path) -> bool {
    let normalize = |value: &Path| {
        value
            .to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    };
    let path = normalize(path);
    let root = normalize(root);
    path == root || path.strip_prefix(&(root + "\\")).is_some()
}

#[cfg(any(not(debug_assertions), test))]
fn verify_broker_installation_not_mutable_by_unprivileged_principal(
    install_root: &Path,
    broker_executable: &Path,
    protected_root: Option<&Path>,
) -> Result<(), UacLaunchError> {
    validate_install_object_security(broker_executable)?;
    require_each_access_right_denied(
        broker_executable,
        &[
            FILE_WRITE_DATA,
            FILE_APPEND_DATA,
            DELETE_ACCESS,
            WRITE_DAC_ACCESS,
            WRITE_OWNER_ACCESS,
        ],
        FILE_ATTRIBUTE_NORMAL,
    )?;
    validate_install_directory_security(install_root)?;

    if let Some(protected_root) = protected_root {
        let protected_root = protected_root
            .canonicalize()
            .map_err(|_| UacLaunchError::InvalidBrokerExecutable)?;
        let mut ancestor = install_root.parent();
        let mut reached_protected_root = same_windows_path(install_root, &protected_root);
        while let Some(directory) = ancestor {
            if !windows_path_is_within(directory, &protected_root) {
                break;
            }
            validate_install_directory_security(directory)?;
            if same_windows_path(directory, &protected_root) {
                reached_protected_root = true;
                break;
            }
            ancestor = directory.parent();
        }
        if !reached_protected_root {
            return Err(UacLaunchError::InvalidBrokerExecutable);
        }
    }
    Ok(())
}

#[cfg(any(not(debug_assertions), test))]
fn validate_install_directory_security(path: &Path) -> Result<(), UacLaunchError> {
    validate_install_object_security(path)?;
    require_each_access_right_denied(
        path,
        &[
            FILE_ADD_FILE,
            FILE_ADD_SUBDIRECTORY,
            FILE_DELETE_CHILD,
            DELETE_ACCESS,
            WRITE_DAC_ACCESS,
            WRITE_OWNER_ACCESS,
        ],
        FILE_FLAG_BACKUP_SEMANTICS,
    )
}

#[cfg(any(not(debug_assertions), test))]
fn validate_install_object_security(path: &Path) -> Result<(), UacLaunchError> {
    let path_wide = wide_null(path.as_os_str());
    let mut owner = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut descriptor = null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 || descriptor.is_null() || owner.is_null() || dacl.is_null() {
        if !descriptor.is_null() {
            unsafe { LocalFree(descriptor) };
        }
        return Err(UacLaunchError::InvalidBrokerExecutable);
    }

    let validation = (|| {
        let owner_sid = sid_to_string(owner)?;
        if !trusted_install_mutation_sid(&owner_sid) {
            return Err(UacLaunchError::InvalidBrokerExecutable);
        }
        let ace_count = unsafe { (*dacl).AceCount };
        for index in 0..ace_count {
            let mut ace = null_mut();
            if unsafe { GetAce(dacl, u32::from(index), &mut ace) } == 0 || ace.is_null() {
                return Err(UacLaunchError::InvalidBrokerExecutable);
            }
            let header = unsafe { &*ace.cast::<ACE_HEADER>() };
            if header.AceFlags & INHERIT_ONLY_ACE as u8 != 0 {
                continue;
            }
            match header.AceType {
                ACCESS_ALLOWED_ACE_KIND => {
                    let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
                    if allowed.Mask & INSTALL_MUTATION_MASK == 0 {
                        continue;
                    }
                    let sid = (&raw const allowed.SidStart).cast_mut().cast();
                    let sid = sid_to_string(sid)?;
                    if !trusted_install_mutation_sid(&sid) {
                        return Err(UacLaunchError::InvalidBrokerExecutable);
                    }
                }
                ACCESS_DENIED_ACE_KIND => {}
                _ => return Err(UacLaunchError::InvalidBrokerExecutable),
            }
        }
        Ok(())
    })();
    unsafe { LocalFree(descriptor) };
    validation
}

#[cfg(any(not(debug_assertions), test))]
fn sid_to_string(sid: *mut c_void) -> Result<String, UacLaunchError> {
    let mut sid_text: *mut u16 = null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut sid_text) } == 0 || sid_text.is_null() {
        return Err(UacLaunchError::InvalidBrokerExecutable);
    }
    let text = unsafe { wide_ptr_to_string(sid_text) };
    unsafe { LocalFree(sid_text.cast()) };
    Ok(text)
}

#[cfg(any(not(debug_assertions), test))]
fn trusted_install_mutation_sid(sid: &str) -> bool {
    TRUSTED_INSTALL_MUTATION_SIDS
        .iter()
        .any(|trusted| sid.eq_ignore_ascii_case(trusted))
}

#[cfg(any(not(debug_assertions), test))]
fn require_each_access_right_denied(
    path: &Path,
    mutation_rights: &[u32],
    flags: u32,
) -> Result<(), UacLaunchError> {
    let path = wide_null(path.as_os_str());
    for desired_access in mutation_rights {
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                *desired_access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                flags,
                null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(handle) };
            return Err(UacLaunchError::InvalidBrokerExecutable);
        }
        if last_error_code() != ERROR_ACCESS_DENIED {
            return Err(UacLaunchError::InvalidBrokerExecutable);
        }
    }
    Ok(())
}

fn build_uac_parameters(pipe_name: &str, generation: u64) -> Result<String, UacLaunchError> {
    if generation == 0 || !is_valid_broker_pipe_name(pipe_name) {
        return Err(UacLaunchError::InvalidLaunchContext);
    }
    Ok(format!("--pipe {pipe_name} --generation {generation}"))
}

pub fn random_session_nonce() -> Result<SessionNonce, PrivilegeIpcError> {
    let mut bytes = [0u8; SESSION_NONCE_BYTES];
    fill_random(&mut bytes)?;
    Ok(SessionNonce::from_bytes(bytes))
}

fn fill_random(bytes: &mut [u8]) -> Result<(), PrivilegeIpcError> {
    let status = unsafe {
        BCryptGenRandom(
            null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(PrivilegeIpcError::RandomUnavailable)
    }
}

pub struct NamedPipeServer {
    handle: HANDLE,
    name: String,
    current_user_sid: String,
}

impl fmt::Debug for NamedPipeServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamedPipeServer")
            .field("name", &self.name)
            .field("acl", &"current-user-only")
            .finish()
    }
}

impl NamedPipeServer {
    pub fn create() -> Result<Self, PrivilegeIpcError> {
        let mut suffix = [0u8; 16];
        fill_random(&mut suffix)?;
        let suffix = suffix
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let name = format!(r"\\.\pipe\LocalBridge-Privileged-{suffix}");
        let current_user_sid = current_user_sid_string()?;
        let sddl = current_user_pipe_sddl(&current_user_sid);
        let mut descriptor: *mut c_void = null_mut();
        let sddl_wide = wide_null(OsStr::new(&sddl));
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        };
        if converted == 0 || descriptor.is_null() {
            return Err(PrivilegeIpcError::SecurityDescriptorUnavailable);
        }
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let name_wide = wide_null(OsStr::new(&name));
        let handle = unsafe {
            CreateNamedPipeW(
                name_wide.as_ptr(),
                PIPE_ACCESS_DUPLEX_MODE | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                PIPE_BUFFER_BYTES,
                PIPE_BUFFER_BYTES,
                0,
                &security,
            )
        };
        unsafe { LocalFree(descriptor) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(PrivilegeIpcError::PipeCreateFailed(last_error_code()));
        }
        Ok(Self {
            handle,
            name,
            current_user_sid,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn current_user_sid(&self) -> &str {
        &self.current_user_sid
    }

    pub fn accept_expected_client(
        mut self,
        expected_pid: u32,
    ) -> Result<NamedPipeConnection, PrivilegeIpcError> {
        let connected = unsafe { ConnectNamedPipe(self.handle, null_mut()) };
        if connected == 0 {
            let code = last_error_code();
            if code != ERROR_PIPE_CONNECTED {
                return Err(PrivilegeIpcError::PipeConnectFailed(code));
            }
        }
        let mut actual_pid = 0u32;
        if unsafe { GetNamedPipeClientProcessId(self.handle, &mut actual_pid) } == 0 {
            return Err(PrivilegeIpcError::IoFailed {
                operation: "GetNamedPipeClientProcessId",
                code: last_error_code(),
            });
        }
        if actual_pid != expected_pid {
            unsafe { DisconnectNamedPipe(self.handle) };
            return Err(PrivilegeIpcError::UnauthorizedPeer {
                expected_pid,
                actual_pid,
            });
        }
        let handle = self.handle;
        self.handle = INVALID_HANDLE_VALUE;
        Ok(NamedPipeConnection {
            handle,
            server_side: true,
        })
    }

    pub fn accept_elevated_client(
        self,
        process: &ElevatedBrokerProcess,
    ) -> Result<NamedPipeConnection, PrivilegeIpcError> {
        self.accept_expected_client(process.pid())
    }
}

impl Drop for NamedPipeServer {
    fn drop(&mut self) {
        close_if_valid(&mut self.handle);
    }
}

pub struct NamedPipeClient;

impl NamedPipeClient {
    pub fn connect(name: &str) -> Result<NamedPipeConnection, PrivilegeIpcError> {
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        let name_wide = wide_null(OsStr::new(name));
        loop {
            let handle = unsafe {
                CreateFileW(
                    name_wide.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    null(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                return Ok(NamedPipeConnection {
                    handle,
                    server_side: false,
                });
            }
            let code = last_error_code();
            if code != ERROR_PIPE_BUSY && code != ERROR_FILE_NOT_FOUND {
                return Err(PrivilegeIpcError::PipeConnectFailed(code));
            }
            if Instant::now() >= deadline {
                return Err(PrivilegeIpcError::PipeConnectFailed(code));
            }
            unsafe { WaitNamedPipeW(name_wide.as_ptr(), 50) };
            thread::sleep(Duration::from_millis(10));
        }
    }
}

pub struct NamedPipeConnection {
    handle: HANDLE,
    server_side: bool,
}

// Win32 pipe HANDLE ownership may move between threads; higher layers serialize all I/O.
unsafe impl Send for NamedPipeConnection {}

impl fmt::Debug for NamedPipeConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamedPipeConnection")
            .field("server_side", &self.server_side)
            .finish()
    }
}

impl NamedPipeConnection {
    pub fn write_frame(&mut self, payload: &[u8]) -> Result<(), PrivilegeIpcError> {
        if payload.is_empty() {
            return Err(PrivilegeIpcError::EmptyFrame);
        }
        if payload.len() > MAX_BROKER_FRAME_BYTES {
            return Err(PrivilegeIpcError::OversizedFrame);
        }
        self.write_all(&(payload.len() as u32).to_le_bytes())?;
        self.write_all(payload)?;
        if unsafe { FlushFileBuffers(self.handle) } == 0 {
            return Err(PrivilegeIpcError::IoFailed {
                operation: "FlushFileBuffers",
                code: last_error_code(),
            });
        }
        Ok(())
    }

    pub fn read_frame(&mut self) -> Result<Vec<u8>, PrivilegeIpcError> {
        let mut length = [0u8; 4];
        self.read_exact(&mut length)?;
        let length = u32::from_le_bytes(length) as usize;
        if length == 0 {
            return Err(PrivilegeIpcError::EmptyFrame);
        }
        if length > MAX_BROKER_FRAME_BYTES {
            return Err(PrivilegeIpcError::OversizedFrame);
        }
        let mut payload = vec![0u8; length];
        self.read_exact(&mut payload)?;
        Ok(payload)
    }

    fn write_all(&mut self, mut bytes: &[u8]) -> Result<(), PrivilegeIpcError> {
        while !bytes.is_empty() {
            let mut written = 0u32;
            let ok = unsafe {
                WriteFile(
                    self.handle,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    &mut written,
                    null_mut(),
                )
            };
            if ok == 0 {
                return Err(PrivilegeIpcError::IoFailed {
                    operation: "WriteFile",
                    code: last_error_code(),
                });
            }
            if written == 0 {
                return Err(PrivilegeIpcError::Disconnected);
            }
            bytes = &bytes[written as usize..];
        }
        Ok(())
    }

    fn read_exact(&mut self, mut bytes: &mut [u8]) -> Result<(), PrivilegeIpcError> {
        while !bytes.is_empty() {
            let mut read = 0u32;
            let ok = unsafe {
                ReadFile(
                    self.handle,
                    bytes.as_mut_ptr(),
                    bytes.len() as u32,
                    &mut read,
                    null_mut(),
                )
            };
            if ok == 0 {
                let code = last_error_code();
                if code == ERROR_BROKEN_PIPE {
                    return Err(PrivilegeIpcError::Disconnected);
                }
                return Err(PrivilegeIpcError::IoFailed {
                    operation: "ReadFile",
                    code,
                });
            }
            if read == 0 {
                return Err(PrivilegeIpcError::Disconnected);
            }
            let (_, rest) = std::mem::take(&mut bytes).split_at_mut(read as usize);
            bytes = rest;
        }
        Ok(())
    }
}

impl Drop for NamedPipeConnection {
    fn drop(&mut self) {
        if self.server_side && self.handle != INVALID_HANDLE_VALUE {
            unsafe { DisconnectNamedPipe(self.handle) };
        }
        close_if_valid(&mut self.handle);
    }
}

fn current_user_sid_string() -> Result<String, PrivilegeIpcError> {
    let mut token: HANDLE = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(PrivilegeIpcError::CurrentUserSidUnavailable);
    }
    let result = (|| {
        let mut bytes_needed = 0u32;
        unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut bytes_needed) };
        if bytes_needed == 0 {
            return Err(PrivilegeIpcError::CurrentUserSidUnavailable);
        }
        let mut buffer = vec![0u8; bytes_needed as usize];
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                bytes_needed,
                &mut bytes_needed,
            )
        } == 0
        {
            return Err(PrivilegeIpcError::CurrentUserSidUnavailable);
        }
        let token_user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
        let mut sid_text: *mut u16 = null_mut();
        if unsafe { ConvertSidToStringSidW(token_user.User.Sid, &mut sid_text) } == 0
            || sid_text.is_null()
        {
            return Err(PrivilegeIpcError::CurrentUserSidUnavailable);
        }
        let text = unsafe { wide_ptr_to_string(sid_text) };
        unsafe { LocalFree(sid_text.cast()) };
        Ok(text)
    })();
    unsafe { CloseHandle(token) };
    result
}

fn current_user_pipe_sddl(sid: &str) -> String {
    format!("D:P(A;;GA;;;{sid})")
}

unsafe fn wide_ptr_to_string(ptr: *const u16) -> String {
    let mut len = 0usize;
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) })
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn close_if_valid(handle: &mut HANDLE) {
    if *handle != INVALID_HANDLE_VALUE && !handle.is_null() {
        unsafe { CloseHandle(*handle) };
        *handle = INVALID_HANDLE_VALUE;
    }
}

fn last_error_code() -> u32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn current_user_acl_sddl_contains_only_current_sid_allow_entry() {
        let sid = current_user_sid_string().unwrap();
        assert!(sid.starts_with("S-1-"));
        let sddl = current_user_pipe_sddl(&sid);
        assert_eq!(sddl, format!("D:P(A;;GA;;;{sid})"));
        assert!(!sddl.contains(";;;WD"));
        assert!(!sddl.contains(";;;AU"));
        assert!(!sddl.contains(";;;AN"));
    }

    #[test]
    fn random_nonce_and_pipe_names_are_not_deterministic_or_debug_exposed() {
        let first = random_session_nonce().unwrap();
        let second = random_session_nonce().unwrap();
        assert_ne!(first, second);
        assert_eq!(format!("{first:?}"), "SessionNonce([REDACTED])");
        let a = NamedPipeServer::create().unwrap();
        let b = NamedPipeServer::create().unwrap();
        assert_ne!(a.name(), b.name());
        assert!(a.name().starts_with(r"\\.\pipe\LocalBridge-Privileged-"));
    }

    #[test]
    fn explicit_uac_parameters_contain_only_pipe_and_generation() {
        let parameters = build_uac_parameters(
            r"\\.\pipe\LocalBridge-Privileged-0123456789abcdef0123456789abcdef",
            9,
        )
        .unwrap();
        assert_eq!(
            parameters,
            r"--pipe \\.\pipe\LocalBridge-Privileged-0123456789abcdef0123456789abcdef --generation 9"
        );
        for forbidden in ["nonce", "secret", "token", "password", "api-key"] {
            assert!(!parameters.to_ascii_lowercase().contains(forbidden));
        }
        assert!(build_uac_parameters("bad pipe", 1).is_err());
        assert!(build_uac_parameters(r"\\.\pipe\LocalBridge-Privileged-a", 0).is_err());
    }

    #[test]
    fn broker_launch_target_is_bound_to_canonical_protected_sibling() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "localbridge-broker-trust-{}-{nonce}",
            std::process::id()
        ));
        let protected = root.join("Program Files");
        let install = protected.join("LocalBridge");
        let decoy = root.join("attacker");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&decoy).unwrap();
        let app = install.join("localbridge.exe");
        let broker = install.join("localbridge-privileged-broker.exe");
        let decoy_broker = decoy.join("localbridge-privileged-broker.exe");
        fs::write(&app, b"test app").unwrap();
        fs::write(&broker, b"trusted test broker").unwrap();
        fs::write(&decoy_broker, b"attacker test broker").unwrap();

        let accepted = validate_broker_executable(&broker, &app, Some(&protected)).unwrap();
        assert!(same_windows_path(
            &accepted,
            &broker.canonicalize().unwrap()
        ));
        assert_eq!(
            validate_broker_executable(&decoy_broker, &app, Some(&protected)),
            Err(UacLaunchError::InvalidBrokerExecutable)
        );
        assert_eq!(
            validate_broker_executable(&broker, &app, Some(&decoy)),
            Err(UacLaunchError::InvalidBrokerExecutable)
        );
        assert_eq!(
            verify_broker_installation_not_mutable_by_unprivileged_principal(
                &install,
                &broker,
                Some(&protected),
            ),
            Err(UacLaunchError::InvalidBrokerExecutable),
            "a canonical Broker under a same-user-writable Program Files-shaped tree must fail ACL trust"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(debug_assertions)]
    #[test]
    fn development_broker_pin_blocks_write_and_delete_during_uac_handoff() {
        use std::os::windows::fs::OpenOptionsExt;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "localbridge-dev-broker-pin-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let broker = root.join("localbridge-privileged-broker.exe");
        fs::write(&broker, b"development broker fixture").unwrap();

        let pin = pin_development_broker(&broker).unwrap();
        assert!(OpenOptions::new()
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&broker)
            .is_err());
        assert!(fs::remove_file(&broker).is_err());

        drop(pin);
        assert!(OpenOptions::new().write(true).open(&broker).is_ok());
        fs::remove_file(&broker).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
