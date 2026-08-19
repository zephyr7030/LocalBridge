use std::ffi::{OsStr, OsString, c_void};
use std::fmt;
use std::io::Read;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use windows_sys::Win32::Foundation::{
    CloseHandle, FILETIME, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
    WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
    GetExitCodeProcess, GetProcessTimes, PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES,
    STARTUPINFOW, TerminateProcess, WaitForSingleObject,
};

const FORCED_EXIT_CODE: u32 = 0x4C42_0004;
const BOUNDED_TIMEOUT_EXIT_CODE: u32 = 0x4C42_0005;
const FORCED_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BOUNDED_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BOUNDED_COMMAND_OUTPUT: usize = 1024 * 1024;
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcessGeneration(u64);

impl ProcessGeneration {
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Reconstructs generation metadata read from persisted diagnostic state.
    /// This does not grant process ownership; Job handles remain authoritative.
    pub const fn from_persisted_value(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSnapshot {
    pub role: String,
    pub pid: u32,
    pub generation: ProcessGeneration,
    pub creation_time_100ns: u64,
    pub started_at: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotDisposition {
    CurrentGeneration,
    StaleGeneration,
    ProcessIdentityMismatch,
}

pub fn classify_persisted_snapshot(
    current: &ProcessSnapshot,
    persisted: &ProcessSnapshot,
) -> SnapshotDisposition {
    if current.generation != persisted.generation {
        return SnapshotDisposition::StaleGeneration;
    }
    if current.pid != persisted.pid
        || current.creation_time_100ns != persisted.creation_time_100ns
        || current.role != persisted.role
    {
        return SnapshotDisposition::ProcessIdentityMismatch;
    }
    SnapshotDisposition::CurrentGeneration
}

pub struct ManagedProcessSpec {
    role: String,
    executable: PathBuf,
    args: Vec<OsString>,
    current_dir: Option<PathBuf>,
    environment: Vec<(OsString, SecretEnvironmentValue)>,
    environment_removals: Vec<OsString>,
}

impl fmt::Debug for ManagedProcessSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let environment_keys = self
            .environment
            .iter()
            .map(|(key, _)| key.to_string_lossy())
            .collect::<Vec<_>>();
        let environment_removed_keys = self
            .environment_removals
            .iter()
            .map(|key| key.to_string_lossy())
            .collect::<Vec<_>>();
        f.debug_struct("ManagedProcessSpec")
            .field("role", &self.role)
            .field("executable", &self.executable)
            .field("args", &self.args)
            .field("current_dir", &self.current_dir)
            .field("environment_keys", &environment_keys)
            .field("environment_removed_keys", &environment_removed_keys)
            .finish()
    }
}

struct SecretEnvironmentValue(Vec<u16>);

impl SecretEnvironmentValue {
    fn from_str(value: &str) -> Result<Self, SupervisorError> {
        let wide = OsStr::new(value).encode_wide().collect::<Vec<_>>();
        if wide.contains(&0) {
            return Err(SupervisorError::InvalidSpec(
                "environment value contains NUL",
            ));
        }
        Ok(Self(wide))
    }
}

impl Drop for SecretEnvironmentValue {
    fn drop(&mut self) {
        for value in &mut self.0 {
            unsafe { std::ptr::write_volatile(value, 0) };
        }
    }
}

struct EnvironmentBlock(Vec<u16>);

impl EnvironmentBlock {
    fn as_ptr(&self) -> *const c_void {
        self.0.as_ptr().cast()
    }
}

impl Drop for EnvironmentBlock {
    fn drop(&mut self) {
        for value in &mut self.0 {
            unsafe { std::ptr::write_volatile(value, 0) };
        }
    }
}

impl ManagedProcessSpec {
    pub fn new(
        role: impl Into<String>,
        executable: impl Into<PathBuf>,
    ) -> Result<Self, SupervisorError> {
        let role = role.into();
        if role.trim().is_empty() {
            return Err(SupervisorError::InvalidSpec("empty process role"));
        }
        let executable = executable.into();
        if executable.as_os_str().is_empty() {
            return Err(SupervisorError::InvalidSpec("empty executable path"));
        }
        Ok(Self {
            role,
            executable,
            args: Vec::new(),
            current_dir: None,
            environment: Vec::new(),
            environment_removals: Vec::new(),
        })
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn current_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(path.into());
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Result<Self, SupervisorError> {
        validate_environment_name(key)?;
        let key = OsString::from(key);
        let value = SecretEnvironmentValue::from_str(value)?;
        self.environment_removals
            .retain(|candidate| !env_names_equal(candidate, &key));
        if let Some(existing) = self
            .environment
            .iter_mut()
            .find(|(candidate, _)| env_names_equal(candidate, &key))
        {
            *existing = (key, value);
        } else {
            self.environment.push((key, value));
        }
        Ok(self)
    }

    pub fn env_remove(mut self, key: &str) -> Result<Self, SupervisorError> {
        validate_environment_name(key)?;
        let key = OsString::from(key);
        self.environment
            .retain(|(candidate, _)| !env_names_equal(candidate, &key));
        if !self
            .environment_removals
            .iter()
            .any(|candidate| env_names_equal(candidate, &key))
        {
            self.environment_removals.push(key);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopDisposition {
    AlreadyStopped,
    Graceful,
    Forced,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedCommandOutput {
    pub exit_code: u32,
    pub output: Vec<u8>,
    pub truncated: bool,
    pub timed_out: bool,
}

#[derive(Debug)]
pub enum SupervisorError {
    InvalidSpec(&'static str),
    WindowsApi { operation: &'static str, code: u32 },
    ResumeFailed,
    UnexpectedWaitStatus { status: u32 },
    ForcedTerminationDidNotDrain { remaining_processes: u32 },
    RootProcessDidNotSignalAfterStop,
}

impl fmt::Display for SupervisorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpec(message) => write!(f, "invalid managed process spec: {message}"),
            Self::WindowsApi { operation, code } => {
                write!(f, "Windows API {operation} failed with code {code}")
            }
            Self::ResumeFailed => f.write_str("ResumeThread failed after Job assignment"),
            Self::UnexpectedWaitStatus { status } => {
                write!(
                    f,
                    "WaitForSingleObject returned unexpected status {status:#x}"
                )
            }
            Self::ForcedTerminationDidNotDrain {
                remaining_processes,
            } => write!(
                f,
                "owned Job still contains {remaining_processes} process(es) after forced termination"
            ),
            Self::RootProcessDidNotSignalAfterStop => {
                f.write_str("owned root process did not signal after process tree stop")
            }
        }
    }
}

impl std::error::Error for SupervisorError {}

pub struct WindowsProcessSupervisor {
    job: HANDLE,
    process: HANDLE,
    snapshot: ProcessSnapshot,
    stopped: bool,
}

unsafe impl Send for WindowsProcessSupervisor {}

impl WindowsProcessSupervisor {
    pub fn spawn(spec: &ManagedProcessSpec) -> Result<Self, SupervisorError> {
        let job = create_kill_on_close_job()?;
        let mut command_line = build_command_line(&spec.executable, &spec.args);
        let application = wide_null(spec.executable.as_os_str());
        let environment = build_environment_block(&spec.environment, &spec.environment_removals);
        let creation_flags = CREATE_SUSPENDED
            | CREATE_NO_WINDOW
            | if environment.is_some() {
                CREATE_UNICODE_ENVIRONMENT
            } else {
                0
            };
        let environment_ptr = environment
            .as_ref()
            .map_or(null(), EnvironmentBlock::as_ptr);
        let current_directory = spec
            .current_dir
            .as_ref()
            .map(|path| wide_null(path.as_os_str()));
        let current_directory_ptr = current_directory
            .as_ref()
            .map_or(null(), |value| value.as_ptr());

        let mut startup: STARTUPINFOW = unsafe { zeroed() };
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        let mut process_info: PROCESS_INFORMATION = unsafe { zeroed() };

        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                0,
                creation_flags,
                environment_ptr,
                current_directory_ptr,
                &startup,
                &mut process_info,
            )
        };
        if created == 0 {
            let error = last_error("CreateProcessW");
            unsafe { CloseHandle(job) };
            return Err(error);
        }

        let assign_ok = unsafe { AssignProcessToJobObject(job, process_info.hProcess) };
        if assign_ok == 0 {
            let error = last_error("AssignProcessToJobObject");
            unsafe {
                TerminateProcess(process_info.hProcess, FORCED_EXIT_CODE);
                CloseHandle(process_info.hThread);
                CloseHandle(process_info.hProcess);
                CloseHandle(job);
            }
            return Err(error);
        }

        let creation_time_100ns = match process_creation_time(process_info.hProcess) {
            Ok(value) => value,
            Err(error) => {
                unsafe {
                    TerminateJobObject(job, FORCED_EXIT_CODE);
                    CloseHandle(process_info.hThread);
                    CloseHandle(process_info.hProcess);
                    CloseHandle(job);
                }
                return Err(error);
            }
        };

        if unsafe { ResumeThread(process_info.hThread) } == u32::MAX {
            unsafe {
                TerminateJobObject(job, FORCED_EXIT_CODE);
                CloseHandle(process_info.hThread);
                CloseHandle(process_info.hProcess);
                CloseHandle(job);
            }
            return Err(SupervisorError::ResumeFailed);
        }
        unsafe { CloseHandle(process_info.hThread) };

        let generation = ProcessGeneration(NEXT_GENERATION.fetch_add(1, Ordering::Relaxed));
        Ok(Self {
            job,
            process: process_info.hProcess,
            snapshot: ProcessSnapshot {
                role: spec.role.clone(),
                pid: process_info.dwProcessId,
                generation,
                creation_time_100ns,
                started_at: SystemTime::now(),
            },
            stopped: false,
        })
    }

    pub const fn snapshot(&self) -> &ProcessSnapshot {
        &self.snapshot
    }

    pub fn reconcile_persisted(&self, persisted: &ProcessSnapshot) -> SnapshotDisposition {
        classify_persisted_snapshot(&self.snapshot, persisted)
    }

    pub fn active_processes(&self) -> Result<u32, SupervisorError> {
        if self.stopped {
            return Ok(0);
        }
        query_active_processes(self.job)
    }

    pub fn root_is_running(&self) -> Result<bool, SupervisorError> {
        if self.stopped {
            return Ok(false);
        }
        let wait = unsafe { WaitForSingleObject(self.process, 0) };
        match wait {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            WAIT_FAILED => Err(last_error("WaitForSingleObject")),
            status => Err(SupervisorError::UnexpectedWaitStatus { status }),
        }
    }

    pub fn stop_with<F>(
        &mut self,
        graceful_timeout: Duration,
        request_graceful: F,
    ) -> Result<StopDisposition, SupervisorError>
    where
        F: FnOnce(&ProcessSnapshot),
    {
        if self.stopped {
            return Ok(StopDisposition::AlreadyStopped);
        }
        if self.active_processes()? == 0 {
            if !wait_for_process_exit(self.process, FORCED_DRAIN_TIMEOUT)? {
                return Err(SupervisorError::RootProcessDidNotSignalAfterStop);
            }
            self.stopped = true;
            return Ok(StopDisposition::AlreadyStopped);
        }

        request_graceful(&self.snapshot);
        if wait_for_job_empty(self.job, graceful_timeout)? {
            if !wait_for_process_exit(self.process, FORCED_DRAIN_TIMEOUT)? {
                return Err(SupervisorError::RootProcessDidNotSignalAfterStop);
            }
            self.stopped = true;
            return Ok(StopDisposition::Graceful);
        }

        if unsafe { TerminateJobObject(self.job, FORCED_EXIT_CODE) } == 0 {
            return Err(last_error("TerminateJobObject"));
        }
        if !wait_for_job_empty(self.job, FORCED_DRAIN_TIMEOUT)? {
            return Err(SupervisorError::ForcedTerminationDidNotDrain {
                remaining_processes: query_active_processes(self.job)?,
            });
        }
        if !wait_for_process_exit(self.process, FORCED_DRAIN_TIMEOUT)? {
            return Err(SupervisorError::RootProcessDidNotSignalAfterStop);
        }
        self.stopped = true;
        Ok(StopDisposition::Forced)
    }

    pub fn force_stop(&mut self) -> Result<StopDisposition, SupervisorError> {
        self.stop_with(Duration::ZERO, |_| {})
    }
}

pub fn run_bounded_command(
    executable: &Path,
    args: &[OsString],
    current_dir: &Path,
    timeout: Duration,
    max_output_bytes: usize,
) -> Result<BoundedCommandOutput, SupervisorError> {
    if !executable.is_absolute()
        || !executable.is_file()
        || is_verbatim_path(executable)
        || !current_dir.is_absolute()
        || !current_dir.is_dir()
        || is_verbatim_path(current_dir)
        || timeout.is_zero()
        || timeout > MAX_BOUNDED_COMMAND_TIMEOUT
        || max_output_bytes == 0
        || max_output_bytes > MAX_BOUNDED_COMMAND_OUTPUT
        || executable.as_os_str().encode_wide().any(|value| value == 0)
        || current_dir
            .as_os_str()
            .encode_wide()
            .any(|value| value == 0)
        || args
            .iter()
            .any(|arg| arg.encode_wide().any(|value| value == 0))
    {
        return Err(SupervisorError::InvalidSpec("invalid bounded command spec"));
    }

    let (read_handle, write_handle) = create_bounded_output_pipe()?;
    let job = match create_kill_on_close_job() {
        Ok(job) => job,
        Err(error) => {
            close_handle(read_handle);
            close_handle(write_handle);
            return Err(error);
        }
    };
    let mut command_line = build_command_line(executable, args);
    let application = wide_null(executable.as_os_str());
    let current_directory = wide_null(current_dir.as_os_str());
    let mut startup: STARTUPINFOW = unsafe { zeroed() };
    startup.cb = size_of::<STARTUPINFOW>() as u32;
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdOutput = write_handle;
    startup.hStdError = write_handle;
    startup.hStdInput = null_mut();
    let mut process_info: PROCESS_INFORMATION = unsafe { zeroed() };
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED | CREATE_NO_WINDOW,
            null(),
            current_directory.as_ptr(),
            &startup,
            &mut process_info,
        )
    };
    if created == 0 {
        let error = last_error("CreateProcessW");
        close_handle(read_handle);
        close_handle(write_handle);
        close_handle(job);
        return Err(error);
    }
    close_handle(write_handle);

    if unsafe { AssignProcessToJobObject(job, process_info.hProcess) } == 0 {
        let error = last_error("AssignProcessToJobObject");
        unsafe { TerminateProcess(process_info.hProcess, FORCED_EXIT_CODE) };
        close_handle(process_info.hThread);
        close_handle(process_info.hProcess);
        close_handle(job);
        close_handle(read_handle);
        return Err(error);
    }
    if unsafe { ResumeThread(process_info.hThread) } == u32::MAX {
        unsafe { TerminateJobObject(job, FORCED_EXIT_CODE) };
        close_handle(process_info.hThread);
        close_handle(process_info.hProcess);
        close_handle(job);
        close_handle(read_handle);
        return Err(SupervisorError::ResumeFailed);
    }
    close_handle(process_info.hThread);

    let reader_handle = read_handle as usize;
    let reader =
        thread::spawn(move || drain_bounded_output(reader_handle as HANDLE, max_output_bytes));
    let started = Instant::now();
    let timed_out = loop {
        match query_active_processes(job) {
            Ok(0) => break false,
            Ok(_) => {}
            Err(error) => {
                unsafe { TerminateJobObject(job, FORCED_EXIT_CODE) };
                let _ = wait_for_job_empty(job, FORCED_DRAIN_TIMEOUT);
                close_handle(process_info.hProcess);
                close_handle(job);
                let _ = reader.join();
                return Err(error);
            }
        }
        if started.elapsed() >= timeout {
            if unsafe { TerminateJobObject(job, BOUNDED_TIMEOUT_EXIT_CODE) } == 0 {
                let error = last_error("TerminateJobObject");
                close_handle(process_info.hProcess);
                close_handle(job);
                let _ = reader.join();
                return Err(error);
            }
            break true;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let job_empty = match wait_for_job_empty(job, FORCED_DRAIN_TIMEOUT) {
        Ok(empty) => empty,
        Err(error) => {
            unsafe { TerminateJobObject(job, FORCED_EXIT_CODE) };
            let _ = wait_for_job_empty(job, FORCED_DRAIN_TIMEOUT);
            close_handle(process_info.hProcess);
            close_handle(job);
            let _ = reader.join();
            return Err(error);
        }
    };
    if !job_empty {
        unsafe { TerminateJobObject(job, FORCED_EXIT_CODE) };
        let _ = wait_for_job_empty(job, FORCED_DRAIN_TIMEOUT);
        let remaining_processes = query_active_processes(job).unwrap_or(1);
        close_handle(process_info.hProcess);
        close_handle(job);
        let _ = reader.join();
        return Err(SupervisorError::ForcedTerminationDidNotDrain {
            remaining_processes,
        });
    }
    let process_exited = match wait_for_process_exit(process_info.hProcess, FORCED_DRAIN_TIMEOUT) {
        Ok(exited) => exited,
        Err(error) => {
            unsafe { TerminateJobObject(job, FORCED_EXIT_CODE) };
            let _ = wait_for_job_empty(job, FORCED_DRAIN_TIMEOUT);
            close_handle(process_info.hProcess);
            close_handle(job);
            let _ = reader.join();
            return Err(error);
        }
    };
    if !process_exited {
        unsafe { TerminateJobObject(job, FORCED_EXIT_CODE) };
        let _ = wait_for_job_empty(job, FORCED_DRAIN_TIMEOUT);
        close_handle(process_info.hProcess);
        close_handle(job);
        let _ = reader.join();
        return Err(SupervisorError::RootProcessDidNotSignalAfterStop);
    }
    let mut exit_code = 0u32;
    if unsafe { GetExitCodeProcess(process_info.hProcess, &mut exit_code) } == 0 {
        let error = last_error("GetExitCodeProcess");
        close_handle(process_info.hProcess);
        close_handle(job);
        let _ = reader.join();
        return Err(error);
    }
    close_handle(process_info.hProcess);
    close_handle(job);
    let (output, truncated) = reader.join().unwrap_or_else(|_| (Vec::new(), true));
    Ok(BoundedCommandOutput {
        exit_code,
        output,
        truncated,
        timed_out,
    })
}

impl Drop for WindowsProcessSupervisor {
    fn drop(&mut self) {
        unsafe {
            if !self.job.is_null() {
                CloseHandle(self.job);
                self.job = null_mut();
            }
            if !self.process.is_null() {
                CloseHandle(self.process);
                self.process = null_mut();
            }
        }
    }
}

fn create_kill_on_close_job() -> Result<HANDLE, SupervisorError> {
    let job = unsafe { CreateJobObjectW(null(), null()) };
    if job.is_null() {
        return Err(last_error("CreateJobObjectW"));
    }
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const c_void,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if ok == 0 {
        let error = last_error("SetInformationJobObject");
        unsafe { CloseHandle(job) };
        return Err(error);
    }
    Ok(job)
}

fn create_bounded_output_pipe() -> Result<(HANDLE, HANDLE), SupervisorError> {
    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let mut read_handle: HANDLE = null_mut();
    let mut write_handle: HANDLE = null_mut();
    if unsafe { CreatePipe(&mut read_handle, &mut write_handle, &security, 0) } == 0 {
        return Err(last_error("CreatePipe"));
    }
    if unsafe { SetHandleInformation(read_handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
        let error = last_error("SetHandleInformation");
        close_handle(read_handle);
        close_handle(write_handle);
        return Err(error);
    }
    Ok((read_handle, write_handle))
}

fn drain_bounded_output(handle: HANDLE, limit: usize) -> (Vec<u8>, bool) {
    let mut file = unsafe { std::fs::File::from_raw_handle(handle.cast()) };
    let mut retained = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut truncated = false;
    loop {
        match file.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                let available = limit.saturating_sub(retained.len());
                let keep = count.min(available);
                retained.extend_from_slice(&chunk[..keep]);
                truncated |= keep < count;
            }
        }
    }
    (retained, truncated)
}

fn close_handle(handle: HANDLE) {
    if handle != INVALID_HANDLE_VALUE && !handle.is_null() {
        unsafe { CloseHandle(handle) };
    }
}

fn is_verbatim_path(path: &Path) -> bool {
    let prefix = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    path.as_os_str().encode_wide().take(prefix.len()).eq(prefix)
}

fn query_active_processes(job: HANDLE) -> Result<u32, SupervisorError> {
    let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
    let ok = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            &mut accounting as *mut _ as *mut c_void,
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            null_mut(),
        )
    };
    if ok == 0 {
        return Err(last_error("QueryInformationJobObject"));
    }
    Ok(accounting.ActiveProcesses)
}

fn wait_for_job_empty(job: HANDLE, timeout: Duration) -> Result<bool, SupervisorError> {
    let deadline = Instant::now() + timeout;
    loop {
        if query_active_processes(job)? == 0 {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_process_exit(process: HANDLE, timeout: Duration) -> Result<bool, SupervisorError> {
    let deadline = Instant::now() + timeout;
    loop {
        match unsafe { WaitForSingleObject(process, 0) } {
            WAIT_OBJECT_0 => return Ok(true),
            WAIT_TIMEOUT => {}
            WAIT_FAILED => return Err(last_error("WaitForSingleObject")),
            status => return Err(SupervisorError::UnexpectedWaitStatus { status }),
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn process_creation_time(process: HANDLE) -> Result<u64, SupervisorError> {
    let mut creation: FILETIME = unsafe { zeroed() };
    let mut exit: FILETIME = unsafe { zeroed() };
    let mut kernel: FILETIME = unsafe { zeroed() };
    let mut user: FILETIME = unsafe { zeroed() };
    let ok = unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) };
    if ok == 0 {
        return Err(last_error("GetProcessTimes"));
    }
    Ok(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
}

fn last_error(operation: &'static str) -> SupervisorError {
    SupervisorError::WindowsApi {
        operation,
        code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32,
    }
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn env_names_equal(left: &OsStr, right: &OsStr) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn validate_environment_name(key: &str) -> Result<(), SupervisorError> {
    if key.is_empty() || key.contains('=') || key.contains('\0') {
        Err(SupervisorError::InvalidSpec("invalid environment name"))
    } else {
        Ok(())
    }
}

fn build_environment_block(
    overrides: &[(OsString, SecretEnvironmentValue)],
    removals: &[OsString],
) -> Option<EnvironmentBlock> {
    if overrides.is_empty() && removals.is_empty() {
        return None;
    }

    enum Value<'a> {
        Inherited(OsString),
        Override(&'a SecretEnvironmentValue),
    }

    let mut entries = std::env::vars_os()
        .filter(|(key, _)| {
            !overrides
                .iter()
                .any(|(override_key, _)| env_names_equal(key, override_key))
                && !removals
                    .iter()
                    .any(|removed_key| env_names_equal(key, removed_key))
        })
        .map(|(key, value)| (key, Value::Inherited(value)))
        .collect::<Vec<_>>();
    entries.extend(
        overrides
            .iter()
            .map(|(key, value)| (key.clone(), Value::Override(value))),
    );
    entries.sort_by(|(left, _), (right, _)| {
        left.to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&right.to_string_lossy().to_ascii_lowercase())
    });

    let mut block = Vec::new();
    for (key, value) in entries {
        block.extend(key.encode_wide());
        block.push('=' as u16);
        match value {
            Value::Inherited(value) => block.extend(value.encode_wide()),
            Value::Override(value) => block.extend_from_slice(&value.0),
        }
        block.push(0);
    }
    block.push(0);
    Some(EnvironmentBlock(block))
}

fn build_command_line(executable: &Path, args: &[OsString]) -> Vec<u16> {
    let mut command = quote_windows_arg(executable.as_os_str());
    for arg in args {
        command.push(' ');
        command.push_str(&quote_windows_arg(arg));
    }
    wide_null(OsStr::new(&command))
}

fn quote_windows_arg(arg: &OsStr) -> String {
    let text = arg.to_string_lossy();
    if !text.is_empty() && !text.chars().any(|c| c.is_whitespace() || c == '"') {
        return text.into_owned();
    }
    let mut out = String::from("\"");
    let mut slashes = 0usize;
    for ch in text.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        if ch == '"' {
            out.push_str(&"\\".repeat(slashes * 2 + 1));
            out.push('"');
            slashes = 0;
            continue;
        }
        out.push_str(&"\\".repeat(slashes));
        slashes = 0;
        out.push(ch);
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_argument_quoting_preserves_spaces_quotes_and_trailing_slashes() {
        assert_eq!(quote_windows_arg(OsStr::new("plain")), "plain");
        assert_eq!(quote_windows_arg(OsStr::new("two words")), "\"two words\"");
        assert_eq!(quote_windows_arg(OsStr::new("a\\\"b")), "\"a\\\\\\\"b\"");
        assert_eq!(
            quote_windows_arg(OsStr::new("C:\\with space\\")),
            "\"C:\\with space\\\\\""
        );
    }

    #[test]
    fn child_environment_override_is_case_insensitive_and_debug_redacted() {
        let spec = ManagedProcessSpec::new("env-test", r"C:\Windows\System32\cmd.exe")
            .unwrap()
            .env("Path", "LB006_ENV_SECRET_SENTINEL")
            .unwrap();
        let debug = format!("{spec:?}");
        assert!(debug.contains("Path"));
        assert!(!debug.contains("LB006_ENV_SECRET_SENTINEL"));

        let block = build_environment_block(&spec.environment, &spec.environment_removals).unwrap();
        let entries = block
            .0
            .split(|value| *value == 0)
            .filter(|entry| !entry.is_empty())
            .map(String::from_utf16_lossy)
            .collect::<Vec<_>>();
        let paths = entries
            .iter()
            .filter(|entry| {
                entry
                    .split_once('=')
                    .is_some_and(|(key, _)| key.eq_ignore_ascii_case("path"))
            })
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], "Path=LB006_ENV_SECRET_SENTINEL");
    }

    #[test]
    fn invalid_child_environment_name_fails_closed() {
        let error = ManagedProcessSpec::new("env-test", r"C:\Windows\System32\cmd.exe")
            .unwrap()
            .env("BAD=NAME", "value")
            .unwrap_err();
        assert!(matches!(
            error,
            SupervisorError::InvalidSpec("invalid environment name")
        ));
    }

    #[test]
    fn child_environment_removal_is_case_insensitive_and_not_an_empty_override() {
        let spec = ManagedProcessSpec::new("env-remove-test", r"C:\Windows\System32\cmd.exe")
            .unwrap()
            .env_remove("PaTh")
            .unwrap();
        let debug = format!("{spec:?}");
        assert!(debug.contains("PaTh"));

        let block = build_environment_block(&spec.environment, &spec.environment_removals).unwrap();
        let entries = block
            .0
            .split(|value| *value == 0)
            .filter(|entry| !entry.is_empty())
            .map(String::from_utf16_lossy)
            .collect::<Vec<_>>();
        assert!(!entries.iter().any(|entry| {
            entry
                .split_once('=')
                .is_some_and(|(key, _)| key.eq_ignore_ascii_case("path"))
        }));
        assert!(
            !entries
                .iter()
                .any(|entry| entry.eq_ignore_ascii_case("path="))
        );
    }

    #[test]
    fn explicit_environment_override_supersedes_prior_removal() {
        let spec = ManagedProcessSpec::new("env-remove-test", r"C:\Windows\System32\cmd.exe")
            .unwrap()
            .env_remove("LOCALBRIDGE_TEST_ENV")
            .unwrap()
            .env("localbridge_test_env", "synthetic-value")
            .unwrap();
        assert!(spec.environment_removals.is_empty());
        let block = build_environment_block(&spec.environment, &spec.environment_removals).unwrap();
        let entries = block
            .0
            .split(|value| *value == 0)
            .filter(|entry| !entry.is_empty())
            .map(String::from_utf16_lossy)
            .collect::<Vec<_>>();
        assert!(
            entries
                .iter()
                .any(|entry| entry == "localbridge_test_env=synthetic-value")
        );
    }
}
