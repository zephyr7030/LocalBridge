use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::tunnel::{TunnelError, TunnelId};

pub const STARTUP_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const STARTUP_PROFILE_FILE_NAME: &str = "startup-profile.json";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupProfile {
    pub schema_version: u32,
    tunnel_id: Option<String>,
    manual_stop_latched: bool,
}

impl Default for StartupProfile {
    fn default() -> Self {
        Self {
            schema_version: STARTUP_PROFILE_SCHEMA_VERSION,
            tunnel_id: None,
            manual_stop_latched: false,
        }
    }
}

impl StartupProfile {
    pub fn manual_stop_latched(&self) -> bool {
        self.manual_stop_latched
    }

    pub fn record_manual_stop(&mut self) {
        self.manual_stop_latched = true;
    }

    pub fn clear_manual_stop(&mut self) {
        self.manual_stop_latched = false;
    }

    pub fn set_tunnel_id(&mut self, value: impl Into<String>) -> Result<(), StartupProfileError> {
        let value = value.into();
        TunnelId::new(value.clone()).map_err(StartupProfileError::Tunnel)?;
        self.tunnel_id = Some(value);
        Ok(())
    }

    pub fn clear_tunnel_id(&mut self) {
        self.tunnel_id = None;
    }

    pub fn validated_tunnel_id(&self) -> Result<Option<TunnelId>, StartupProfileError> {
        self.tunnel_id
            .as_ref()
            .map(|value| TunnelId::new(value.clone()).map_err(StartupProfileError::Tunnel))
            .transpose()
    }

    fn validate(&self) -> Result<(), StartupProfileError> {
        if self.schema_version != STARTUP_PROFILE_SCHEMA_VERSION {
            return Err(StartupProfileError::UnsupportedSchema {
                found: self.schema_version,
                expected: STARTUP_PROFILE_SCHEMA_VERSION,
            });
        }
        let _ = self.validated_tunnel_id()?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct StartupProfileStore {
    path: PathBuf,
}

impl StartupProfileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<StartupProfile, StartupProfileError> {
        if !self.path.exists() {
            return Ok(StartupProfile::default());
        }
        let mut file = File::open(&self.path).map_err(|error| io_error("open", error))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| io_error("read", error))?;
        let profile: StartupProfile = serde_json::from_slice(&bytes)
            .map_err(|error| StartupProfileError::Serialization(error.to_string()))?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn save(&self, profile: &StartupProfile) -> Result<(), StartupProfileError> {
        profile.validate()?;
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| io_error("create_parent", error))?;
        let mut bytes = serde_json::to_vec_pretty(profile)
            .map_err(|error| StartupProfileError::Serialization(error.to_string()))?;
        bytes.push(b'\n');
        let temp = self.path.with_file_name(format!(
            "{}.tmp-{}-{}",
            self.path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(STARTUP_PROFILE_FILE_NAME),
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| io_error("create_temp", error))?;
        if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
            let _ = fs::remove_file(&temp);
            return Err(io_error("write_temp", error));
        }
        drop(file);
        let result = atomic_replace(&temp, &self.path);
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

#[cfg(windows)]
fn atomic_replace(temp: &Path, target: &Path) -> Result<(), StartupProfileError> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_WRITE_THROUGH, MoveFileExW, ReplaceFileW,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let temp_w = wide(temp);
    let target_w = wide(target);
    let ok = if target.exists() {
        unsafe {
            ReplaceFileW(
                target_w.as_ptr(),
                temp_w.as_ptr(),
                null(),
                0,
                null_mut(),
                null_mut(),
            )
        }
    } else {
        unsafe { MoveFileExW(temp_w.as_ptr(), target_w.as_ptr(), MOVEFILE_WRITE_THROUGH) }
    };
    if ok == 0 {
        return Err(io_error("atomic_replace", std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(temp: &Path, target: &Path) -> Result<(), StartupProfileError> {
    fs::rename(temp, target).map_err(|error| io_error("atomic_replace", error))
}

#[derive(Debug)]
pub enum StartupProfileError {
    Io {
        operation: &'static str,
        message: String,
    },
    Serialization(String),
    UnsupportedSchema {
        found: u32,
        expected: u32,
    },
    Tunnel(TunnelError),
}

impl std::fmt::Display for StartupProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { operation, message } => {
                write!(f, "startup profile {operation} failed: {message}")
            }
            Self::Serialization(message) => {
                write!(f, "startup profile serialization failed: {message}")
            }
            Self::UnsupportedSchema { found, expected } => write!(
                f,
                "startup profile schema {found} is unsupported; expected {expected}"
            ),
            Self::Tunnel(error) => write!(f, "startup profile Tunnel ID is invalid: {error}"),
        }
    }
}

impl std::error::Error for StartupProfileError {}

fn io_error(operation: &'static str, error: std::io::Error) -> StartupProfileError {
    StartupProfileError::Io {
        operation,
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/integration/autostart/startup_profile.rs"
    ));
}
