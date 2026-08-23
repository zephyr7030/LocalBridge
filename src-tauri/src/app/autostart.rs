#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(windows)]
use std::ptr::{null, null_mut};

#[cfg(windows)]
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
#[cfg(windows)]
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};

pub const CURRENT_USER_RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const LOCALBRIDGE_RUN_VALUE: &str = "LocalBridge";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutostartError {
    InvalidExecutable,
    InvalidRegistryData,
    WindowsApi { operation: &'static str, code: u32 },
    UnsupportedPlatform,
}

impl std::fmt::Display for AutostartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidExecutable => f.write_str("autostart executable must be an absolute path"),
            Self::InvalidRegistryData => f.write_str("autostart registry data is invalid"),
            Self::WindowsApi { operation, code } => {
                write!(
                    f,
                    "autostart Windows registry operation {operation} failed with code {code}"
                )
            }
            Self::UnsupportedPlatform => f.write_str("autostart is supported only on Windows"),
        }
    }
}

impl std::error::Error for AutostartError {}

#[derive(Debug, Clone)]
pub struct AutostartManager {
    executable: PathBuf,
    subkey: String,
    value_name: String,
}

impl AutostartManager {
    pub fn for_current_executable() -> Result<Self, AutostartError> {
        let executable = std::env::current_exe().map_err(|_| AutostartError::InvalidExecutable)?;
        Self::new(executable, CURRENT_USER_RUN_KEY, LOCALBRIDGE_RUN_VALUE)
    }

    fn new(
        executable: impl Into<PathBuf>,
        subkey: impl Into<String>,
        value_name: impl Into<String>,
    ) -> Result<Self, AutostartError> {
        let executable = executable.into();
        if !executable.is_absolute() {
            return Err(AutostartError::InvalidExecutable);
        }
        Ok(Self {
            executable,
            subkey: subkey.into(),
            value_name: value_name.into(),
        })
    }

    pub fn command_line(&self) -> String {
        format!("\"{}\" --background", self.executable.display())
    }

    #[cfg(windows)]
    pub fn set_enabled(&self, enabled: bool) -> Result<(), AutostartError> {
        if enabled {
            let key = RegistryKey::create(&self.subkey)?;
            let data = wide(&self.command_line());
            let name = wide(&self.value_name);
            let code = unsafe {
                RegSetValueExW(
                    key.0,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * std::mem::size_of::<u16>()) as u32,
                )
            };
            if code != ERROR_SUCCESS {
                return Err(registry_error("RegSetValueExW", code));
            }
            Ok(())
        } else {
            let Some(key) = RegistryKey::open(&self.subkey, KEY_SET_VALUE)? else {
                return Ok(());
            };
            let name = wide(&self.value_name);
            let code = unsafe { RegDeleteValueW(key.0, name.as_ptr()) };
            if code != ERROR_SUCCESS && code != ERROR_FILE_NOT_FOUND {
                return Err(registry_error("RegDeleteValueW", code));
            }
            Ok(())
        }
    }

    #[cfg(not(windows))]
    pub fn set_enabled(&self, _enabled: bool) -> Result<(), AutostartError> {
        Err(AutostartError::UnsupportedPlatform)
    }

    #[cfg(windows)]
    pub fn registered_command(&self) -> Result<Option<String>, AutostartError> {
        let Some(key) = RegistryKey::open(&self.subkey, KEY_QUERY_VALUE)? else {
            return Ok(None);
        };
        let name = wide(&self.value_name);
        let mut value_type = 0u32;
        let mut bytes = 0u32;
        let code = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                null(),
                &mut value_type,
                null_mut(),
                &mut bytes,
            )
        };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if code != ERROR_SUCCESS {
            return Err(registry_error("RegQueryValueExW(size)", code));
        }
        if value_type != REG_SZ || bytes == 0 || bytes as usize % 2 != 0 {
            return Err(AutostartError::InvalidRegistryData);
        }
        let mut buffer = vec![0u16; bytes as usize / 2];
        let code = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                null(),
                &mut value_type,
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(registry_error("RegQueryValueExW(data)", code));
        }
        if buffer.last() == Some(&0) {
            buffer.pop();
        }
        String::from_utf16(&buffer)
            .map(Some)
            .map_err(|_| AutostartError::InvalidRegistryData)
    }

    #[cfg(not(windows))]
    pub fn registered_command(&self) -> Result<Option<String>, AutostartError> {
        Err(AutostartError::UnsupportedPlatform)
    }

    #[cfg(test)]
    fn for_test(
        executable: &Path,
        subkey: String,
        value_name: String,
    ) -> Result<Self, AutostartError> {
        Self::new(executable, subkey, value_name)
    }
}

#[cfg(windows)]
struct RegistryKey(HKEY);

#[cfg(windows)]
impl RegistryKey {
    fn create(subkey: &str) -> Result<Self, AutostartError> {
        let subkey = wide(subkey);
        let mut key = null_mut();
        let mut disposition = 0u32;
        let code = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                null_mut(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                null(),
                &mut key,
                &mut disposition,
            )
        };
        let _ = disposition;
        if code != ERROR_SUCCESS {
            return Err(registry_error("RegCreateKeyExW", code));
        }
        Ok(Self(key))
    }

    fn open(subkey: &str, access: u32) -> Result<Option<Self>, AutostartError> {
        let subkey = wide(subkey);
        let mut key = null_mut();
        let code =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, access, &mut key) };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if code != ERROR_SUCCESS {
            return Err(registry_error("RegOpenKeyExW", code));
        }
        Ok(Some(Self(key)))
    }
}

#[cfg(windows)]
impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn registry_error(operation: &'static str, code: u32) -> AutostartError {
    AutostartError::WindowsApi { operation, code }
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/integration/autostart/windows_autostart.rs"
    ));
}
