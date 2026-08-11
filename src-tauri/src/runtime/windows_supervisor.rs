use std::ffi::{OsStr, OsString, c_void};
use std::fmt;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use windows_sys::Win32::Foundation::{
    CloseHandle, FILETIME, HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CreateProcessW, GetProcessTimes, PROCESS_INFORMATION, ResumeThread,
    STARTUPINFOW, TerminateProcess, WaitForSingleObject,
};

const FORCED_EXIT_CODE: u32 = 0x4C42_0004;
const FORCED_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedProcessSpec {
    role: String,
    executable: PathBuf,
    args: Vec<OsString>,
    current_dir: Option<PathBuf>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopDisposition {
    AlreadyStopped,
    Graceful,
    Forced,
}

#[derive(Debug)]
pub enum SupervisorError {
    InvalidSpec(&'static str),
    WindowsApi { operation: &'static str, code: u32 },
    ResumeFailed,
    UnexpectedWaitStatus { status: u32 },
    ForcedTerminationDidNotDrain { remaining_processes: u32 },
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
                CREATE_SUSPENDED,
                null(),
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
        if self.stopped || self.active_processes()? == 0 {
            self.stopped = true;
            return Ok(StopDisposition::AlreadyStopped);
        }

        request_graceful(&self.snapshot);
        if wait_for_job_empty(self.job, graceful_timeout)? {
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
        self.stopped = true;
        Ok(StopDisposition::Forced)
    }

    pub fn force_stop(&mut self) -> Result<StopDisposition, SupervisorError> {
        self.stop_with(Duration::ZERO, |_| {})
    }
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
}
