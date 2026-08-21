use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::migration::{MigrationError, migrate_bytes};
use super::model::AppData;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn backup_path(&self) -> PathBuf {
        sibling_with_suffix(&self.path, ".bak")
    }

    pub fn load(&self) -> Result<AppData, SettingsStoreError> {
        if !self.path.exists() {
            return Ok(AppData::default());
        }
        let original = read_all(&self.path)?;
        let outcome = migrate_bytes(&original).map_err(SettingsStoreError::Migration)?;
        if outcome.migrated {
            self.save(&outcome.data)?;
        }
        Ok(outcome.data)
    }

    pub fn save(&self, data: &AppData) -> Result<(), SettingsStoreError> {
        data.validate()
            .map_err(|error| SettingsStoreError::Validation(format!("{error:?}")))?;
        let mut bytes = serde_json::to_vec_pretty(data)
            .map_err(|error| SettingsStoreError::Serialization(error.to_string()))?;
        bytes.push(b'\n');
        self.write_atomic(&bytes)
    }

    fn write_atomic(&self, bytes: &[u8]) -> Result<(), SettingsStoreError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| io_error("create_parent", error))?;
        let temp = sibling_with_suffix(
            &self.path,
            &format!(
                ".tmp-{}-{}",
                std::process::id(),
                TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
        );
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| io_error("create_temp", error))?;
        if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
            let _ = fs::remove_file(&temp);
            return Err(io_error("write_temp", error));
        }
        drop(file);

        let result = atomic_replace(&temp, &self.path, &self.backup_path());
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

fn read_all(path: &Path) -> Result<Vec<u8>, SettingsStoreError> {
    let mut file = File::open(path).map_err(|error| io_error("open", error))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| io_error("read", error))?;
    Ok(bytes)
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("settings.json");
    path.with_file_name(format!("{name}{suffix}"))
}

#[cfg(windows)]
fn atomic_replace(temp: &Path, target: &Path, backup: &Path) -> Result<(), SettingsStoreError> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_WRITE_THROUGH, MoveFileExW, ReplaceFileW,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let temp_w = wide(temp);
    let target_w = wide(target);
    if target.exists() {
        if backup.exists() {
            fs::remove_file(backup).map_err(|error| io_error("remove_stale_backup", error))?;
        }
        let backup_w = wide(backup);
        let ok = unsafe {
            ReplaceFileW(
                target_w.as_ptr(),
                temp_w.as_ptr(),
                backup_w.as_ptr(),
                0,
                null(),
                null(),
            )
        };
        if ok == 0 {
            return Err(io_error("replace_file", std::io::Error::last_os_error()));
        }
    } else {
        let ok = unsafe { MoveFileExW(temp_w.as_ptr(), target_w.as_ptr(), MOVEFILE_WRITE_THROUGH) };
        if ok == 0 {
            return Err(io_error("move_file", std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(temp: &Path, target: &Path, backup: &Path) -> Result<(), SettingsStoreError> {
    if target.exists() {
        if backup.exists() {
            fs::remove_file(backup).map_err(|error| io_error("remove_stale_backup", error))?;
        }
        fs::copy(target, backup).map_err(|error| io_error("backup", error))?;
    }
    fs::rename(temp, target).map_err(|error| io_error("rename", error))
}

fn io_error(operation: &'static str, error: std::io::Error) -> SettingsStoreError {
    SettingsStoreError::Io {
        operation,
        message: error.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsStoreError {
    Io {
        operation: &'static str,
        message: String,
    },
    Migration(MigrationError),
    Validation(String),
    Serialization(String),
}
