#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::ptr::null;
#[cfg(windows)]
use std::sync::Mutex;
#[cfg(windows)]
use std::thread::{self, JoinHandle};

#[cfg(windows)]
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0,
};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, INFINITE, SetEvent, WaitForMultipleObjects,
};

#[cfg(windows)]
const INSTANCE_MUTEX_NAME: &str = r"Local\LocalBridge.SingleInstance.v1";
#[cfg(windows)]
const WAKE_EVENT_NAME: &str = r"Local\LocalBridge.Wake.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SingleInstanceError {
    WindowsApi { operation: &'static str, code: u32 },
    ListenerAlreadyStarted,
    ListenerThreadSpawn,
}

impl std::fmt::Display for SingleInstanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WindowsApi { operation, code } => {
                write!(
                    f,
                    "single-instance Windows API {operation} failed with code {code}"
                )
            }
            Self::ListenerAlreadyStarted => {
                f.write_str("single-instance wake listener already started")
            }
            Self::ListenerThreadSpawn => {
                f.write_str("single-instance wake listener thread failed to start")
            }
        }
    }
}

impl std::error::Error for SingleInstanceError {}

pub enum SingleInstanceAcquire {
    Primary(SingleInstanceGuard),
    Secondary,
}

#[cfg(windows)]
pub struct SingleInstanceGuard {
    mutex_handle: isize,
    wake_handle: isize,
    stop_handle: isize,
    listener: Mutex<Option<JoinHandle<()>>>,
}

#[cfg(windows)]
impl std::fmt::Debug for SingleInstanceGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingleInstanceGuard")
            .field(
                "listener_started",
                &self
                    .listener
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_some(),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(windows)]
impl SingleInstanceGuard {
    pub fn acquire() -> Result<SingleInstanceAcquire, SingleInstanceError> {
        Self::acquire_named(INSTANCE_MUTEX_NAME, WAKE_EVENT_NAME)
    }

    fn acquire_named(
        mutex_name: &str,
        wake_name: &str,
    ) -> Result<SingleInstanceAcquire, SingleInstanceError> {
        let wake_name = wide(wake_name);
        let wake = unsafe { CreateEventW(null(), 0, 0, wake_name.as_ptr()) };
        if wake.is_null() {
            return Err(last_error("CreateEventW(wake)"));
        }

        let mutex_name = wide(mutex_name);
        let mutex = unsafe { CreateMutexW(null(), 0, mutex_name.as_ptr()) };
        if mutex.is_null() {
            unsafe { CloseHandle(wake) };
            return Err(last_error("CreateMutexW"));
        }
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if already_exists {
            let signaled = unsafe { SetEvent(wake) };
            unsafe {
                CloseHandle(mutex);
                CloseHandle(wake);
            }
            if signaled == 0 {
                return Err(last_error("SetEvent(wake)"));
            }
            return Ok(SingleInstanceAcquire::Secondary);
        }

        let stop = unsafe { CreateEventW(null(), 1, 0, null()) };
        if stop.is_null() {
            unsafe {
                CloseHandle(mutex);
                CloseHandle(wake);
            }
            return Err(last_error("CreateEventW(stop)"));
        }

        Ok(SingleInstanceAcquire::Primary(Self {
            mutex_handle: mutex as isize,
            wake_handle: wake as isize,
            stop_handle: stop as isize,
            listener: Mutex::new(None),
        }))
    }

    pub fn start_wake_listener<F>(&self, callback: F) -> Result<(), SingleInstanceError>
    where
        F: Fn() + Send + 'static,
    {
        let mut listener = self
            .listener
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if listener.is_some() {
            return Err(SingleInstanceError::ListenerAlreadyStarted);
        }
        let wake = self.wake_handle;
        let stop = self.stop_handle;
        let thread = thread::Builder::new()
            .name("localbridge-single-instance-wake".to_owned())
            .spawn(move || {
                let handles = [wake as HANDLE, stop as HANDLE];
                loop {
                    let wait = unsafe {
                        WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, INFINITE)
                    };
                    if wait == WAIT_OBJECT_0 {
                        callback();
                    } else {
                        break;
                    }
                }
            })
            .map_err(|_| SingleInstanceError::ListenerThreadSpawn)?;
        *listener = Some(thread);
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe { SetEvent(self.stop_handle as HANDLE) };
        if let Some(listener) = self
            .listener
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = listener.join();
        }
        unsafe {
            CloseHandle(self.stop_handle as HANDLE);
            CloseHandle(self.wake_handle as HANDLE);
            CloseHandle(self.mutex_handle as HANDLE);
        }
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn last_error(operation: &'static str) -> SingleInstanceError {
    SingleInstanceError::WindowsApi {
        operation,
        code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32,
    }
}

#[cfg(not(windows))]
pub struct SingleInstanceGuard;

#[cfg(not(windows))]
impl SingleInstanceGuard {
    pub fn acquire() -> Result<SingleInstanceAcquire, SingleInstanceError> {
        Err(SingleInstanceError::WindowsApi {
            operation: "unsupported platform",
            code: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/integration/single_instance/single_instance.rs"
    ));
}
