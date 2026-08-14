use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::mem::zeroed;
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::ptr::{null, null_mut};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
    GetFinalPathNameByHandleW, OPEN_EXISTING,
};

use super::WorkspaceRegistryError;

/// Runtime-only filesystem identity. It intentionally has no serde traits.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ValidatedWorkspaceIdentity(String);

impl ValidatedWorkspaceIdentity {
    fn from_filesystem(value: String) -> Result<Self, WorkspaceRegistryError> {
        if value.trim().is_empty() {
            return Err(WorkspaceRegistryError::EmptyValidatedIdentity);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedWorkspace {
    identity: ValidatedWorkspaceIdentity,
    resolved_path: PathBuf,
    execution_path: PathBuf,
}

impl ValidatedWorkspace {
    pub fn identity(&self) -> &ValidatedWorkspaceIdentity {
        &self.identity
    }

    pub fn resolved_path(&self) -> &Path {
        &self.resolved_path
    }

    pub fn execution_path(&self) -> &Path {
        &self.execution_path
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkspaceValidator;

impl WorkspaceValidator {
    #[cfg(windows)]
    pub fn validate(&self, path: &Path) -> Result<ValidatedWorkspace, WorkspaceRegistryError> {
        let (identity, resolved_path) = inspect_workspace_path(path)?;
        let execution_path = ordinary_path_from_resolved(&resolved_path)
            .ok_or(WorkspaceRegistryError::ExecutionPathUnavailable)?;
        let (execution_identity, _) = inspect_workspace_path(&execution_path)?;
        if execution_identity != identity {
            return Err(WorkspaceRegistryError::ExecutionPathIdentityMismatch);
        }

        Ok(ValidatedWorkspace {
            identity,
            resolved_path,
            execution_path,
        })
    }

    #[cfg(not(windows))]
    pub fn validate(&self, _path: &Path) -> Result<ValidatedWorkspace, WorkspaceRegistryError> {
        Err(WorkspaceRegistryError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
fn inspect_workspace_path(
    path: &Path,
) -> Result<(ValidatedWorkspaceIdentity, PathBuf), WorkspaceRegistryError> {
    if path.as_os_str().is_empty() {
        return Err(WorkspaceRegistryError::EmptyDisplayPath);
    }
    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(last_windows_error("CreateFileW"));
    }
    let handle = OwnedHandle(handle);
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    if unsafe { GetFileInformationByHandle(handle.0, &mut info) } == 0 {
        return Err(last_windows_error("GetFileInformationByHandle"));
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(WorkspaceRegistryError::WorkspaceNotDirectory);
    }
    let resolved_path = final_path_from_handle(handle.0)?;
    let file_index = ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64;
    let identity = ValidatedWorkspaceIdentity::from_filesystem(format!(
        "win32-file-id:{:08x}:{file_index:016x}",
        info.dwVolumeSerialNumber
    ))?;
    Ok((identity, resolved_path))
}

#[cfg(windows)]
fn ordinary_path_from_resolved(resolved: &Path) -> Option<PathBuf> {
    let wide = resolved.as_os_str().encode_wide().collect::<Vec<_>>();
    let verbatim = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    let verbatim_unc = [
        b'\\' as u16,
        b'\\' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];
    let ordinary = if wide.starts_with(&verbatim_unc) {
        let mut value = vec![b'\\' as u16, b'\\' as u16];
        value.extend_from_slice(&wide[verbatim_unc.len()..]);
        value
    } else if wide.starts_with(&verbatim) {
        wide[verbatim.len()..].to_vec()
    } else {
        wide
    };
    if ordinary.is_empty() {
        return None;
    }
    let path = PathBuf::from(OsString::from_wide(&ordinary));
    (!is_verbatim_path(&path)).then_some(path)
}

#[cfg(windows)]
fn is_verbatim_path(path: &Path) -> bool {
    let prefix = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    path.as_os_str().encode_wide().take(prefix.len()).eq(prefix)
}

#[cfg(windows)]
struct OwnedHandle(HANDLE);

#[cfg(windows)]
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

#[cfg(windows)]
fn final_path_from_handle(handle: HANDLE) -> Result<PathBuf, WorkspaceRegistryError> {
    let needed = unsafe { GetFinalPathNameByHandleW(handle, null_mut(), 0, 0) };
    if needed == 0 {
        return Err(last_windows_error("GetFinalPathNameByHandleW"));
    }
    let mut buffer = vec![0u16; needed as usize + 1];
    let written =
        unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, 0) };
    if written == 0 || written as usize >= buffer.len() {
        return Err(last_windows_error("GetFinalPathNameByHandleW"));
    }
    Ok(PathBuf::from(OsString::from_wide(
        &buffer[..written as usize],
    )))
}

#[cfg(windows)]
fn last_windows_error(operation: &'static str) -> WorkspaceRegistryError {
    WorkspaceRegistryError::WorkspaceValidationWindowsApi {
        operation,
        code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32,
    }
}
