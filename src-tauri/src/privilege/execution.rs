use std::ffi::{OsStr, c_void};
use std::fmt;
use std::io::Read;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
};
use windows_sys::Win32::Globalization::{GetOEMCP, MultiByteToWideChar};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CreateProcessW, GetExitCodeProcess, PROCESS_INFORMATION,
    ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOW,
};

use super::{ElevatedExecOutcome, ElevatedExecResult, ElevatedExecSpec};

const CANCEL_EXIT_CODE: u32 = 0x4C42_1201;
const TIMEOUT_EXIT_CODE: u32 = 0x4C42_1202;
const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Default)]
pub struct ExecutionCancel(Arc<AtomicBool>);

impl ExecutionCancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl fmt::Debug for ExecutionCancel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutionCancel")
            .field("cancelled", &self.cancelled())
            .finish()
    }
}

#[derive(Debug)]
pub(crate) enum ExecutionError {
    InvalidSpec,
    CreatePipe(u32),
    CreateJob(u32),
    ConfigureJob(u32),
    CreateProcess(u32),
    AssignJob(u32),
    Resume,
    Wait(u32),
    ExitCode(u32),
    DrainTimeout,
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpec => f.write_str("invalid elevated execution specification"),
            Self::CreatePipe(code) => write!(f, "CreatePipe failed with code {code}"),
            Self::CreateJob(code) => write!(f, "CreateJobObjectW failed with code {code}"),
            Self::ConfigureJob(code) => {
                write!(f, "SetInformationJobObject failed with code {code}")
            }
            Self::CreateProcess(code) => write!(f, "CreateProcessW failed with code {code}"),
            Self::AssignJob(code) => write!(f, "AssignProcessToJobObject failed with code {code}"),
            Self::Resume => f.write_str("ResumeThread failed"),
            Self::Wait(code) => write!(f, "QueryInformationJobObject failed with code {code}"),
            Self::ExitCode(code) => write!(f, "GetExitCodeProcess failed with code {code}"),
            Self::DrainTimeout => f.write_str("elevated process tree did not drain"),
        }
    }
}

pub(crate) fn run_elevated_exec(
    spec: ElevatedExecSpec,
    cancel: ExecutionCancel,
) -> Result<ElevatedExecResult, ExecutionError> {
    spec.validate().map_err(|_| ExecutionError::InvalidSpec)?;
    let program = Path::new(&spec.program);
    if !program.is_file() {
        return Err(ExecutionError::InvalidSpec);
    }
    if let Some(workdir) = spec.workdir.as_deref() {
        if !Path::new(workdir).is_dir() {
            return Err(ExecutionError::InvalidSpec);
        }
    }

    let (stdout_read, stdout_write) = create_output_pipe()?;
    let (stderr_read, stderr_write) = match create_output_pipe() {
        Ok(pipe) => pipe,
        Err(error) => {
            close_handle(stdout_read);
            close_handle(stdout_write);
            return Err(error);
        }
    };
    let job = create_kill_on_close_job()?;
    let mut command_line = build_command_line(&spec.program, &spec.args);
    let application = wide_null(OsStr::new(&spec.program));
    let current_dir = spec
        .workdir
        .as_deref()
        .map(|value| wide_null(OsStr::new(value)));
    let current_dir_ptr = current_dir.as_ref().map_or(null(), |value| value.as_ptr());
    let mut startup: STARTUPINFOW = unsafe { zeroed() };
    startup.cb = size_of::<STARTUPINFOW>() as u32;
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdOutput = stdout_write;
    startup.hStdError = stderr_write;
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
            current_dir_ptr,
            &startup,
            &mut process_info,
        )
    };
    if created == 0 {
        let code = last_error();
        close_handle(stdout_read);
        close_handle(stdout_write);
        close_handle(stderr_read);
        close_handle(stderr_write);
        close_handle(job);
        return Err(ExecutionError::CreateProcess(code));
    }
    close_handle(stdout_write);
    close_handle(stderr_write);

    if unsafe { AssignProcessToJobObject(job, process_info.hProcess) } == 0 {
        let code = last_error();
        unsafe {
            TerminateJobObject(job, CANCEL_EXIT_CODE);
        }
        close_handle(process_info.hThread);
        close_handle(process_info.hProcess);
        close_handle(job);
        close_handle(stdout_read);
        close_handle(stderr_read);
        return Err(ExecutionError::AssignJob(code));
    }
    if unsafe { ResumeThread(process_info.hThread) } == u32::MAX {
        unsafe {
            TerminateJobObject(job, CANCEL_EXIT_CODE);
        }
        close_handle(process_info.hThread);
        close_handle(process_info.hProcess);
        close_handle(job);
        close_handle(stdout_read);
        close_handle(stderr_read);
        return Err(ExecutionError::Resume);
    }
    close_handle(process_info.hThread);

    let output_limit = spec.max_output_bytes as usize;
    let remaining = Arc::new(AtomicUsize::new(output_limit));
    let stdout_handle = stdout_read as usize;
    let stdout_budget = Arc::clone(&remaining);
    let stdout_reader = thread::spawn(move || drain_output(stdout_handle as HANDLE, stdout_budget));
    let stderr_handle = stderr_read as usize;
    let stderr_budget = Arc::clone(&remaining);
    let stderr_reader = thread::spawn(move || drain_output(stderr_handle as HANDLE, stderr_budget));
    let started = Instant::now();
    let timeout = Duration::from_millis(spec.timeout_ms as u64);
    let outcome = loop {
        if cancel.cancelled() {
            unsafe {
                TerminateJobObject(job, CANCEL_EXIT_CODE);
            }
            break ElevatedExecOutcome::Cancelled;
        }
        if started.elapsed() >= timeout {
            unsafe {
                TerminateJobObject(job, TIMEOUT_EXIT_CODE);
            }
            break ElevatedExecOutcome::TimedOut;
        }
        let active = active_processes(job)?;
        if active == 0 {
            break ElevatedExecOutcome::Completed;
        }
        thread::sleep(Duration::from_millis(10));
    };

    wait_for_job_empty(job, DRAIN_TIMEOUT)?;
    let mut exit_code = 0u32;
    let exit_code = if unsafe { GetExitCodeProcess(process_info.hProcess, &mut exit_code) } == 0 {
        let code = last_error();
        close_handle(process_info.hProcess);
        close_handle(job);
        return Err(ExecutionError::ExitCode(code));
    } else {
        Some(exit_code)
    };
    close_handle(process_info.hProcess);
    close_handle(job);

    let (stdout_bytes, stdout_truncated) =
        stdout_reader.join().unwrap_or_else(|_| (Vec::new(), true));
    let (stderr_bytes, stderr_truncated) =
        stderr_reader.join().unwrap_or_else(|_| (Vec::new(), true));
    let stdout = redact_output(decode_process_output(&stdout_bytes), &spec);
    let stderr = redact_output(decode_process_output(&stderr_bytes), &spec);
    Ok(ElevatedExecResult {
        outcome,
        exit_code,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        truncated: stdout_truncated || stderr_truncated,
    })
}

fn create_output_pipe() -> Result<(HANDLE, HANDLE), ExecutionError> {
    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let mut read_handle: HANDLE = null_mut();
    let mut write_handle: HANDLE = null_mut();
    if unsafe { CreatePipe(&mut read_handle, &mut write_handle, &security, 0) } == 0 {
        return Err(ExecutionError::CreatePipe(last_error()));
    }
    if unsafe { SetHandleInformation(read_handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
        let code = last_error();
        close_handle(read_handle);
        close_handle(write_handle);
        return Err(ExecutionError::CreatePipe(code));
    }
    Ok((read_handle, write_handle))
}

fn create_kill_on_close_job() -> Result<HANDLE, ExecutionError> {
    let job = unsafe { CreateJobObjectW(null(), null()) };
    if job.is_null() {
        return Err(ExecutionError::CreateJob(last_error()));
    }
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const c_void,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        let code = last_error();
        close_handle(job);
        return Err(ExecutionError::ConfigureJob(code));
    }
    Ok(job)
}

fn active_processes(job: HANDLE) -> Result<u32, ExecutionError> {
    let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
    if unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            &mut accounting as *mut _ as *mut c_void,
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            null_mut(),
        )
    } == 0
    {
        return Err(ExecutionError::Wait(last_error()));
    }
    Ok(accounting.ActiveProcesses)
}

fn wait_for_job_empty(job: HANDLE, timeout: Duration) -> Result<(), ExecutionError> {
    let deadline = Instant::now() + timeout;
    while active_processes(job)? != 0 {
        if Instant::now() >= deadline {
            return Err(ExecutionError::DrainTimeout);
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn drain_output(handle: HANDLE, remaining: Arc<AtomicUsize>) -> (Vec<u8>, bool) {
    let mut file = unsafe { std::fs::File::from_raw_handle(handle.cast()) };
    let mut retained = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut truncated = false;
    loop {
        match file.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                let keep = claim_output_budget(&remaining, count);
                retained.extend_from_slice(&chunk[..keep]);
                if keep < count {
                    truncated = true;
                }
            }
        }
    }
    (retained, truncated)
}

fn claim_output_budget(remaining: &AtomicUsize, requested: usize) -> usize {
    loop {
        let available = remaining.load(Ordering::Acquire);
        let keep = requested.min(available);
        if remaining
            .compare_exchange_weak(
                available,
                available - keep,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            return keep;
        }
    }
}

fn decode_process_output(bytes: &[u8]) -> String {
    if let Ok(utf8) = std::str::from_utf8(bytes) {
        return utf8.to_string();
    }
    let Ok(byte_count) = i32::try_from(bytes.len()) else {
        return String::from_utf8_lossy(bytes).into_owned();
    };
    let code_page = unsafe { GetOEMCP() };
    let wide_count =
        unsafe { MultiByteToWideChar(code_page, 0, bytes.as_ptr(), byte_count, null_mut(), 0) };
    if wide_count <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; wide_count as usize];
    if unsafe {
        MultiByteToWideChar(
            code_page,
            0,
            bytes.as_ptr(),
            byte_count,
            wide.as_mut_ptr(),
            wide_count,
        )
    } != wide_count
    {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    String::from_utf16_lossy(&wide)
}

fn redact_output(output: String, spec: &ElevatedExecSpec) -> String {
    if output.is_empty() {
        return output;
    }
    let markers = [
        "password",
        "passwd",
        "passphrase",
        "token",
        "secret",
        "api-key",
        "api_key",
        "authorization",
        "nonce",
        "credential",
    ];
    if spec.args.iter().any(|arg| {
        let lower = arg.to_ascii_lowercase();
        markers.iter().any(|marker| lower.contains(marker))
    }) {
        return "[REDACTED]".to_string();
    }
    let lower = output.to_ascii_lowercase();
    if [
        "authorization: bearer ",
        "api_key=",
        "api-key=",
        "password=",
        "token=",
        "secret=",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return "[REDACTED]".to_string();
    }
    output
}

fn build_command_line(program: &str, args: &[String]) -> Vec<u16> {
    let mut command = quote_windows_arg(OsStr::new(program));
    for arg in args {
        command.push(' ');
        command.push_str(&quote_windows_arg(OsStr::new(arg)));
    }
    wide_null(OsStr::new(&command))
}

fn quote_windows_arg(arg: &OsStr) -> String {
    let text = arg.to_string_lossy();
    if !text.is_empty() && !text.chars().any(|ch| ch.is_whitespace() || ch == '"') {
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

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}
fn close_handle(handle: HANDLE) {
    if handle != INVALID_HANDLE_VALUE && !handle.is_null() {
        unsafe {
            CloseHandle(handle);
        }
    }
}
fn last_error() -> u32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd() -> String {
        r"C:\Windows\System32\cmd.exe".to_string()
    }
    fn spec(args: &[&str]) -> ElevatedExecSpec {
        ElevatedExecSpec {
            program: cmd(),
            args: args.iter().map(|value| value.to_string()).collect(),
            workdir: Some(r"C:\Windows\Temp".to_string()),
            timeout_ms: 5_000,
            max_output_bytes: 4096,
        }
    }

    #[test]
    fn structured_process_exec_captures_output_without_shell_default() {
        let result = run_elevated_exec(
            spec(&["/d", "/c", "echo LB012_STRUCTURED"]),
            ExecutionCancel::default(),
        )
        .unwrap();
        assert_eq!(result.outcome, ElevatedExecOutcome::Completed);
        assert!(result.stdout.contains("LB012_STRUCTURED"));
        assert!(result.stderr.is_empty());
        assert!(!result.truncated);
    }

    #[test]
    fn localized_console_output_is_not_decoded_as_lossy_utf8() {
        let result = run_elevated_exec(
            ElevatedExecSpec {
                program: r"C:\Windows\System32\whoami.exe".to_string(),
                args: vec!["/user".to_string()],
                workdir: Some(r"C:\Windows\Temp".to_string()),
                timeout_ms: 5_000,
                max_output_bytes: 4096,
            },
            ExecutionCancel::default(),
        )
        .unwrap();

        assert_eq!(result.outcome, ElevatedExecOutcome::Completed);
        assert!(result.stdout.contains("SID"), "{}", result.stdout);
        assert!(
            !result.stdout.contains('\u{fffd}'),
            "localized whoami output contains UTF-8 replacement characters: {}",
            result.stdout
        );
    }

    #[test]
    fn timeout_cancel_output_limit_and_secret_redaction_are_enforced() {
        let mut timeout_spec = spec(&["/d", "/c", "ping -n 6 127.0.0.1 >nul"]);
        timeout_spec.timeout_ms = 100;
        let timed = run_elevated_exec(timeout_spec, ExecutionCancel::default()).unwrap();
        assert_eq!(timed.outcome, ElevatedExecOutcome::TimedOut);

        let cancel = ExecutionCancel::default();
        let cancel_worker = cancel.clone();
        let handle = thread::spawn(move || {
            run_elevated_exec(
                spec(&["/d", "/c", "ping -n 6 127.0.0.1 >nul"]),
                cancel_worker,
            )
            .unwrap()
        });
        thread::sleep(Duration::from_millis(80));
        cancel.cancel();
        assert_eq!(
            handle.join().unwrap().outcome,
            ElevatedExecOutcome::Cancelled
        );

        let mut limited_spec = spec(&["/d", "/c", "for /L %i in (1,1,1000) do @echo 1234567890"]);
        limited_spec.max_output_bytes = 128;
        let limited = run_elevated_exec(limited_spec, ExecutionCancel::default()).unwrap();
        assert!(limited.stdout.len() + limited.stderr.len() <= 128);
        assert!(limited.truncated);

        let secret = "LB012_SYNTHETIC_SECRET_VALUE";
        let redacted = run_elevated_exec(
            spec(&["/d", "/c", &format!("echo {secret}"), "--api-key", secret]),
            ExecutionCancel::default(),
        )
        .unwrap();
        assert!(!redacted.stdout.contains(secret));
        assert!(!redacted.stderr.contains(secret));
        assert!(redacted.stdout.contains("[REDACTED]"));
    }
}
