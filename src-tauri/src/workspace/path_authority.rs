use std::path::{Component, Path, PathBuf};

#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::fs::File;
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::os::windows::io::{FromRawHandle, RawHandle};
#[cfg(windows)]
use std::ptr::{null, null_mut};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FileAttributeTagInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
    GetFinalPathNameByHandleW, OPEN_EXISTING,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathAuthorityScope {
    ActiveWorkspace,
    BrokerAdministrator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathAuthorityError {
    InvalidPath,
    NotFound,
    OutsideAuthority,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowsFileIdentity {
    volume_serial: u32,
    file_index: u64,
}

#[cfg(windows)]
#[derive(Debug)]
pub(crate) struct WorkspaceLifetimePin {
    _file: File,
    identity: WindowsFileIdentity,
    execution_root: PathBuf,
}

#[cfg(windows)]
impl WorkspaceLifetimePin {
    pub(crate) fn validate_current(&self) -> Result<(), PathAuthorityError> {
        let current = WorkspaceResolver::active_workspace(&self.execution_root)?;
        let canonical_root = current
            .canonical_root
            .as_ref()
            .expect("active workspace authority has canonical root");
        let handle = current.open_validated_handle_with_share(
            canonical_root,
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )?;
        if file_identity(handle.raw_handle())? == self.identity {
            Ok(())
        } else {
            Err(PathAuthorityError::OutsideAuthority)
        }
    }
}

#[cfg(not(windows))]
#[derive(Debug)]
pub(crate) struct WorkspaceLifetimePin;

#[cfg(not(windows))]
impl WorkspaceLifetimePin {
    pub(crate) fn validate_current(&self) -> Result<(), PathAuthorityError> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceResolver {
    scope: PathAuthorityScope,
    execution_root: Option<PathBuf>,
    canonical_root: Option<PathBuf>,
    #[cfg(windows)]
    root_identity: Option<WindowsFileIdentity>,
}

#[cfg(windows)]
#[derive(Debug)]
pub(crate) struct ValidatedPathHandle {
    handle: HANDLE,
    final_path: PathBuf,
}

#[cfg(windows)]
impl ValidatedPathHandle {
    pub(crate) const fn raw_handle(&self) -> HANDLE {
        self.handle
    }

    pub(crate) fn final_path(&self) -> &Path {
        &self.final_path
    }

    pub(crate) fn metadata(&self) -> std::io::Result<std::fs::Metadata> {
        let file =
            std::mem::ManuallyDrop::new(unsafe { File::from_raw_handle(self.handle as RawHandle) });
        file.metadata()
    }

    pub(crate) fn regular_file_link_count(&self) -> Result<Option<u32>, PathAuthorityError> {
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(self.handle, &mut information) } == 0 {
            return Err(PathAuthorityError::InvalidPath);
        }
        if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            Ok(None)
        } else {
            Ok(Some(information.nNumberOfLinks))
        }
    }

    pub(crate) fn into_file(self) -> File {
        let handle = self.handle;
        std::mem::forget(self);
        unsafe { File::from_raw_handle(handle as RawHandle) }
    }
}

#[cfg(windows)]
impl Drop for ValidatedPathHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

impl WorkspaceResolver {
    pub fn active_workspace(root: &Path) -> Result<Self, PathAuthorityError> {
        if !root.is_absolute() || is_verbatim_path(root) || !root.is_dir() {
            return Err(PathAuthorityError::InvalidPath);
        }
        let canonical_root = ordinary_path(
            &std::fs::canonicalize(root).map_err(|_| PathAuthorityError::InvalidPath)?,
        )
        .ok_or(PathAuthorityError::InvalidPath)?;
        let mut authority = Self {
            scope: PathAuthorityScope::ActiveWorkspace,
            execution_root: Some(root.to_path_buf()),
            canonical_root: Some(canonical_root),
            #[cfg(windows)]
            root_identity: None,
        };
        #[cfg(windows)]
        {
            let canonical_root = authority
                .canonical_root
                .as_ref()
                .expect("active workspace authority has canonical root");
            let handle = authority.open_validated_handle_with_share(
                canonical_root,
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            )?;
            authority.root_identity = Some(file_identity(handle.raw_handle())?);
        }
        Ok(authority)
    }

    pub fn broker_administrator() -> Self {
        Self {
            scope: PathAuthorityScope::BrokerAdministrator,
            execution_root: None,
            canonical_root: None,
            #[cfg(windows)]
            root_identity: None,
        }
    }

    pub(crate) fn workspace_identity_token(&self) -> Option<String> {
        #[cfg(windows)]
        {
            self.root_identity.map(|identity| {
                format!(
                    "win32-file-id:{:08x}:{:016x}",
                    identity.volume_serial, identity.file_index
                )
            })
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    pub(crate) fn matches_workspace_identity_token(
        &self,
        expected: &str,
    ) -> Result<(), PathAuthorityError> {
        self.validate_root_identity()?;
        self.workspace_identity_token()
            .filter(|current| current == expected)
            .map(|_| ())
            .ok_or(PathAuthorityError::OutsideAuthority)
    }

    pub(crate) fn pin_active_workspace_lifetime(
        root: &Path,
    ) -> Result<WorkspaceLifetimePin, PathAuthorityError> {
        #[cfg(windows)]
        {
            let authority = Self::active_workspace(root)?;
            let canonical_root = authority
                .canonical_root
                .as_ref()
                .expect("active workspace authority has canonical root");
            // Retain the original authorization-root object and its File-ID for
            // the facade lifetime. Windows may allow a directory rename even
            // while a handle is open, so this handle is identity evidence, not
            // a rename lock; callers must revalidate the current textual root.
            let handle = authority.open_validated_handle_with_share(
                canonical_root,
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            )?;
            let identity = file_identity(handle.raw_handle())?;
            Ok(WorkspaceLifetimePin {
                _file: handle.into_file(),
                identity,
                execution_root: root.to_path_buf(),
            })
        }
        #[cfg(not(windows))]
        {
            let _ = root;
            Ok(WorkspaceLifetimePin)
        }
    }

    pub const fn scope(&self) -> PathAuthorityScope {
        self.scope
    }

    fn validate_root_identity(&self) -> Result<(), PathAuthorityError> {
        #[cfg(windows)]
        {
            if self.scope != PathAuthorityScope::ActiveWorkspace {
                return Ok(());
            }
            let Some(expected) = self.root_identity else {
                return Ok(());
            };
            let root = self
                .execution_root
                .as_ref()
                .expect("active workspace authority has execution root");
            let wide = root
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(PathAuthorityError::OutsideAuthority);
            }
            let checked = (|| {
                let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
                let tagged = unsafe {
                    GetFileInformationByHandleEx(
                        handle,
                        FileAttributeTagInfo,
                        (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                        std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
                    )
                };
                if tagged == 0 || tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err(PathAuthorityError::OutsideAuthority);
                }
                let final_path = final_path_from_handle(handle)?;
                if self.canonical_root.as_deref() != Some(final_path.as_path()) {
                    return Err(PathAuthorityError::OutsideAuthority);
                }
                if file_identity(handle)? != expected {
                    return Err(PathAuthorityError::OutsideAuthority);
                }
                Ok(())
            })();
            unsafe { CloseHandle(handle) };
            checked
        }
        #[cfg(not(windows))]
        {
            Ok(())
        }
    }

    pub fn input_path(&self, raw: &str) -> Result<PathBuf, PathAuthorityError> {
        self.validate_root_identity()?;
        match self.scope {
            PathAuthorityScope::ActiveWorkspace => {
                if !workspace_input_path_valid(raw) {
                    return Err(PathAuthorityError::InvalidPath);
                }
                let path = Path::new(raw);
                if path.is_absolute() {
                    Ok(path.to_path_buf())
                } else {
                    Ok(self
                        .execution_root
                        .as_ref()
                        .expect("active workspace authority has execution root")
                        .join(path))
                }
            }
            PathAuthorityScope::BrokerAdministrator => {
                if !administrator_absolute_path_valid(raw) {
                    return Err(PathAuthorityError::InvalidPath);
                }
                Ok(PathBuf::from(raw))
            }
        }
    }

    pub fn input_is_within_execution_root(&self, raw: &str) -> Result<bool, PathAuthorityError> {
        let candidate = self.input_path(raw)?;
        match self.scope {
            PathAuthorityScope::ActiveWorkspace => Ok(lexical_path_starts_with(
                &candidate,
                self.execution_root
                    .as_ref()
                    .expect("active workspace authority has execution root"),
            )),
            PathAuthorityScope::BrokerAdministrator => Ok(true),
        }
    }

    pub fn resolve_existing(&self, raw: &str) -> Result<PathBuf, PathAuthorityError> {
        let candidate = self.input_path(raw)?;
        if self.scope == PathAuthorityScope::ActiveWorkspace
            && !lexical_path_starts_with(
                &candidate,
                self.execution_root
                    .as_ref()
                    .expect("active workspace authority has execution root"),
            )
        {
            return Err(PathAuthorityError::OutsideAuthority);
        }
        let canonical = ordinary_path(
            &std::fs::canonicalize(candidate).map_err(|_| PathAuthorityError::NotFound)?,
        )
        .ok_or(PathAuthorityError::InvalidPath)?;
        self.validate_root_identity()?;
        self.allows_canonical(&canonical)
            .then_some(canonical)
            .ok_or(PathAuthorityError::OutsideAuthority)
    }

    /// Resolve the directory entry named by `raw` without following the final
    /// reparse point. Ancestors remain authority-validated, so mutation callers
    /// operate on the entry itself rather than accidentally on its referent.
    pub fn resolve_existing_entry(&self, raw: &str) -> Result<PathBuf, PathAuthorityError> {
        let candidate = self.input_path(raw)?;
        if self.scope == PathAuthorityScope::ActiveWorkspace
            && !lexical_path_starts_with(
                &candidate,
                self.execution_root
                    .as_ref()
                    .expect("active workspace authority has execution root"),
            )
        {
            return Err(PathAuthorityError::OutsideAuthority);
        }
        if std::fs::symlink_metadata(&candidate).is_err() {
            return Err(PathAuthorityError::NotFound);
        }
        let parent = candidate.parent().ok_or(PathAuthorityError::InvalidPath)?;
        let final_parent = self.revalidate_opened_path(parent)?;
        let name = candidate
            .file_name()
            .filter(|value| !value.is_empty())
            .ok_or(PathAuthorityError::InvalidPath)?;
        let entry = final_parent.join(name);
        self.validate_root_identity()?;
        self.allows_canonical(&entry)
            .then_some(entry)
            .ok_or(PathAuthorityError::OutsideAuthority)
    }

    pub fn resolve_missing_leaf(&self, raw: &str) -> Result<PathBuf, PathAuthorityError> {
        let candidate = self.input_path(raw)?;
        if self.scope == PathAuthorityScope::ActiveWorkspace
            && !lexical_path_starts_with(
                &candidate,
                self.execution_root
                    .as_ref()
                    .expect("active workspace authority has execution root"),
            )
        {
            return Err(PathAuthorityError::OutsideAuthority);
        }
        if std::fs::symlink_metadata(&candidate).is_ok() {
            return self.resolve_existing_entry(raw);
        }
        let parent = candidate.parent().ok_or(PathAuthorityError::InvalidPath)?;
        let final_parent = self.revalidate_opened_path(parent)?;
        self.validate_root_identity()?;
        if !final_parent.is_dir() {
            return Err(PathAuthorityError::InvalidPath);
        }
        let name = candidate
            .file_name()
            .filter(|value| !value.is_empty())
            .ok_or(PathAuthorityError::InvalidPath)?;
        let resolved = final_parent.join(name);
        self.allows_canonical(&resolved)
            .then_some(resolved)
            .ok_or(PathAuthorityError::OutsideAuthority)
    }

    pub fn resolve_workspace_path(
        &self,
        raw: Option<&str>,
        default_cwd: &str,
        allow_missing_leaf: bool,
    ) -> Result<PathBuf, PathAuthorityError> {
        if self.scope != PathAuthorityScope::ActiveWorkspace {
            return Err(PathAuthorityError::InvalidPath);
        }
        let default_cwd = if default_cwd.trim().is_empty() {
            "."
        } else {
            default_cwd
        };
        let selected = match raw.filter(|value| !value.trim().is_empty()) {
            Some(value) if Path::new(value).is_absolute() => PathBuf::from(value),
            Some(value) => Path::new(default_cwd).join(value),
            None => PathBuf::from(default_cwd),
        };
        let selected = selected.to_str().ok_or(PathAuthorityError::InvalidPath)?;
        if allow_missing_leaf {
            self.resolve_missing_leaf(selected)
        } else {
            self.resolve_existing(selected)
        }
    }

    pub fn revalidate_opened_path(&self, path: &Path) -> Result<PathBuf, PathAuthorityError> {
        #[cfg(windows)]
        {
            Ok(self
                .open_validated_handle(path, 0)?
                .final_path()
                .to_path_buf())
        }
        #[cfg(not(windows))]
        {
            let final_path = final_opened_path(path)?;
            self.allows_canonical(&final_path)
                .then_some(final_path)
                .ok_or(PathAuthorityError::OutsideAuthority)
        }
    }

    #[cfg(windows)]
    pub(crate) fn open_validated_handle(
        &self,
        path: &Path,
        desired_access: u32,
    ) -> Result<ValidatedPathHandle, PathAuthorityError> {
        self.open_validated_handle_with_share(
            path,
            desired_access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )
    }

    #[cfg(windows)]
    pub(crate) fn open_exclusive_validated_handle(
        &self,
        path: &Path,
        desired_access: u32,
    ) -> Result<ValidatedPathHandle, PathAuthorityError> {
        self.open_validated_handle_with_share(path, desired_access, 0)
    }

    #[cfg(windows)]
    pub(crate) fn open_move_root_validated_handle(
        &self,
        path: &Path,
        desired_access: u32,
    ) -> Result<ValidatedPathHandle, PathAuthorityError> {
        self.open_validated_handle_with_share(path, desired_access, FILE_SHARE_READ)
    }

    #[cfg(windows)]
    pub(crate) fn open_entry_validated_handle(
        &self,
        path: &Path,
        desired_access: u32,
    ) -> Result<ValidatedPathHandle, PathAuthorityError> {
        self.open_validated_handle_with_share_mode(
            path,
            desired_access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            true,
        )
    }

    #[cfg(windows)]
    pub(crate) fn open_move_entry_validated_handle(
        &self,
        path: &Path,
        desired_access: u32,
    ) -> Result<ValidatedPathHandle, PathAuthorityError> {
        self.open_validated_handle_with_share_mode(path, desired_access, FILE_SHARE_READ, true)
    }

    #[cfg(windows)]
    pub(crate) fn open_write_locked_validated_handle(
        &self,
        path: &Path,
        desired_access: u32,
    ) -> Result<ValidatedPathHandle, PathAuthorityError> {
        self.open_validated_handle_with_share(
            path,
            desired_access,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
        )
    }

    #[cfg(windows)]
    fn open_validated_handle_with_share(
        &self,
        path: &Path,
        desired_access: u32,
        share_mode: u32,
    ) -> Result<ValidatedPathHandle, PathAuthorityError> {
        self.open_validated_handle_with_share_mode(path, desired_access, share_mode, false)
    }

    #[cfg(windows)]
    fn open_validated_handle_with_share_mode(
        &self,
        path: &Path,
        desired_access: u32,
        share_mode: u32,
        allow_terminal_reparse: bool,
    ) -> Result<ValidatedPathHandle, PathAuthorityError> {
        self.validate_root_identity()?;
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                desired_access,
                share_mode,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(PathAuthorityError::NotFound);
        }
        let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
        let tagged = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileAttributeTagInfo,
                (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        };
        if tagged == 0
            || (!allow_terminal_reparse && tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0)
        {
            unsafe { CloseHandle(handle) };
            return Err(if tagged == 0 {
                PathAuthorityError::InvalidPath
            } else {
                PathAuthorityError::OutsideAuthority
            });
        }
        if self.scope == PathAuthorityScope::ActiveWorkspace
            && tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            && tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
        {
            let mut information = BY_HANDLE_FILE_INFORMATION::default();
            if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
                unsafe { CloseHandle(handle) };
                return Err(PathAuthorityError::InvalidPath);
            }
            // GetFinalPathNameByHandleW reports only the opened alias. A regular
            // file with multiple NTFS names therefore cannot be proven to be
            // exclusively owned by the active-workspace object boundary.
            if information.nNumberOfLinks > 1 {
                unsafe { CloseHandle(handle) };
                return Err(PathAuthorityError::OutsideAuthority);
            }
        }
        let final_path = match final_path_from_handle(handle) {
            Ok(path) if self.allows_canonical(&path) => path,
            Ok(_) => {
                unsafe { CloseHandle(handle) };
                return Err(PathAuthorityError::OutsideAuthority);
            }
            Err(error) => {
                unsafe { CloseHandle(handle) };
                return Err(error);
            }
        };
        if let Err(error) = self.validate_root_identity() {
            unsafe { CloseHandle(handle) };
            return Err(error);
        }
        Ok(ValidatedPathHandle { handle, final_path })
    }

    pub fn revalidate_parent(&self, path: &Path) -> Result<PathBuf, PathAuthorityError> {
        let parent = path.parent().ok_or(PathAuthorityError::InvalidPath)?;
        let final_parent = self.revalidate_opened_path(parent)?;
        final_parent
            .is_dir()
            .then_some(final_parent)
            .ok_or(PathAuthorityError::InvalidPath)
    }

    pub fn allows_canonical(&self, canonical: &Path) -> bool {
        let Some(canonical) = ordinary_path(canonical) else {
            return false;
        };
        match self.scope {
            PathAuthorityScope::ActiveWorkspace => canonical.starts_with(
                self.canonical_root
                    .as_ref()
                    .expect("active workspace authority has canonical root"),
            ),
            PathAuthorityScope::BrokerAdministrator => canonical.is_absolute(),
        }
    }

    pub fn discovery_stops_at(&self, canonical: &Path) -> bool {
        match self.scope {
            PathAuthorityScope::ActiveWorkspace => {
                ordinary_path(canonical).is_some_and(|canonical| {
                    self.canonical_root.as_deref() == Some(canonical.as_path())
                })
            }
            PathAuthorityScope::BrokerAdministrator => false,
        }
    }

    pub fn display_path(&self, canonical: &Path) -> Result<String, PathAuthorityError> {
        let canonical = ordinary_path(canonical).ok_or(PathAuthorityError::InvalidPath)?;
        let display = match self.scope {
            PathAuthorityScope::ActiveWorkspace => canonical
                .strip_prefix(
                    self.canonical_root
                        .as_ref()
                        .expect("active workspace authority has canonical root"),
                )
                .map_err(|_| PathAuthorityError::OutsideAuthority)?
                .to_string_lossy()
                .replace('\\', "/"),
            PathAuthorityScope::BrokerAdministrator => {
                if !canonical.is_absolute() {
                    return Err(PathAuthorityError::InvalidPath);
                }
                canonical.to_string_lossy().replace('\\', "/")
            }
        };
        Ok(if display.is_empty() {
            ".".to_string()
        } else {
            display
        })
    }

    pub fn canonical_root(&self) -> Option<&Path> {
        self.canonical_root.as_deref()
    }
}

#[cfg(windows)]
fn file_identity(handle: HANDLE) -> Result<WindowsFileIdentity, PathAuthorityError> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
        return Err(PathAuthorityError::InvalidPath);
    }
    Ok(WindowsFileIdentity {
        volume_serial: information.dwVolumeSerialNumber,
        file_index: ((information.nFileIndexHigh as u64) << 32) | information.nFileIndexLow as u64,
    })
}

pub fn workspace_relative_path_valid(value: &str) -> bool {
    if value.is_empty()
        || value.contains(['\0', '\n', '\r', ':'])
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.starts_with("//")
        || value.starts_with(r"\\?\")
        || value.starts_with("//?/")
        || value
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
        || Path::new(value).is_absolute()
    {
        return false;
    }
    !value
        .replace('\\', "/")
        .split('/')
        .any(|component| component == "..")
}

pub fn workspace_input_path_valid(value: &str) -> bool {
    workspace_relative_path_valid(value) || workspace_absolute_path_valid(value)
}

fn workspace_absolute_path_valid(value: &str) -> bool {
    if value.is_empty()
        || value.contains(['\0', '\n', '\r'])
        || value.starts_with('\\')
        || value.starts_with('/')
        || value.starts_with("//")
        || value.starts_with(r"\\?\")
        || value.starts_with("//?/")
    {
        return false;
    }
    let path = Path::new(value);
    if !path.is_absolute()
        || is_verbatim_path(path)
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return false;
    }
    #[cfg(windows)]
    {
        let bytes = value.as_bytes();
        if bytes.len() < 3
            || !bytes[0].is_ascii_alphabetic()
            || bytes[1] != b':'
            || !matches!(bytes[2], b'\\' | b'/')
            || value[2..].contains(':')
        {
            return false;
        }
    }
    true
}

fn administrator_absolute_path_valid(value: &str) -> bool {
    if value.is_empty()
        || value.contains(['\0', '\n', '\r'])
        || value.starts_with(r"\\?\")
        || value.starts_with("//?/")
    {
        return false;
    }
    let path = Path::new(value);
    path.is_absolute()
        && !is_verbatim_path(path)
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}

#[cfg(windows)]
fn lexical_path_starts_with(path: &Path, root: &Path) -> bool {
    let mut path_components = path.components();
    root.components().all(|root_component| {
        path_components.next().is_some_and(|path_component| {
            path_component
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&root_component.as_os_str().to_string_lossy())
        })
    })
}

#[cfg(not(windows))]
fn lexical_path_starts_with(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

#[cfg(windows)]
pub(crate) fn is_verbatim_path(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    let prefix = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    path.as_os_str().encode_wide().take(prefix.len()).eq(prefix)
}

#[cfg(not(windows))]
pub(crate) fn is_verbatim_path(_path: &Path) -> bool {
    false
}

#[cfg(windows)]
fn ordinary_path(path: &Path) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
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
    (!ordinary.is_empty()).then(|| PathBuf::from(OsString::from_wide(&ordinary)))
}

#[cfg(not(windows))]
fn ordinary_path(path: &Path) -> Option<PathBuf> {
    Some(path.to_path_buf())
}

#[cfg(windows)]
fn final_path_from_handle(handle: HANDLE) -> Result<PathBuf, PathAuthorityError> {
    let needed = unsafe { GetFinalPathNameByHandleW(handle, null_mut(), 0, 0) };
    if needed == 0 {
        return Err(PathAuthorityError::InvalidPath);
    }
    let mut buffer = vec![0u16; needed as usize + 1];
    let written =
        unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, 0) };
    if written == 0 || written as usize >= buffer.len() {
        return Err(PathAuthorityError::InvalidPath);
    }
    let final_path = PathBuf::from(OsString::from_wide(&buffer[..written as usize]));
    ordinary_path(&final_path).ok_or(PathAuthorityError::InvalidPath)
}

#[cfg(not(windows))]
fn final_opened_path(path: &Path) -> Result<PathBuf, PathAuthorityError> {
    std::fs::canonicalize(path).map_err(|_| PathAuthorityError::NotFound)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "localbridge-schema33-path-authority-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn active_workspace_accepts_relative_or_absolute_inside_and_is_canonical_root_bound() {
        let root = temp_root();
        let workspace = root.join("workspace");
        let outside = root.join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(workspace.join("inside.txt"), b"inside").unwrap();
        std::fs::write(outside.join("outside.txt"), b"outside").unwrap();

        let authority = WorkspaceResolver::active_workspace(&workspace).unwrap();
        let canonical_workspace = std::fs::canonicalize(&workspace).unwrap();
        let canonical_inside = std::fs::canonicalize(workspace.join("inside.txt")).unwrap();
        let canonical_outside = std::fs::canonicalize(outside.join("outside.txt")).unwrap();
        assert!(authority.allows_canonical(&canonical_workspace));
        assert!(authority.discovery_stops_at(&canonical_workspace));
        assert_eq!(
            authority.display_path(&canonical_inside).unwrap(),
            "inside.txt"
        );
        assert!(!authority.allows_canonical(&canonical_outside));
        let inside = authority.resolve_existing("inside.txt").unwrap();
        assert!(inside.starts_with(authority.canonical_root().unwrap()));
        assert_eq!(authority.display_path(&inside).unwrap(), "inside.txt");
        let absolute_inside = authority
            .resolve_existing(workspace.join("inside.txt").to_string_lossy().as_ref())
            .unwrap();
        assert_eq!(absolute_inside, inside);
        assert_eq!(
            authority
                .display_path(
                    &authority
                        .resolve_existing(workspace.to_string_lossy().as_ref())
                        .unwrap()
                )
                .unwrap(),
            "."
        );
        assert_eq!(
            authority.resolve_existing(outside.join("outside.txt").to_string_lossy().as_ref()),
            Err(PathAuthorityError::OutsideAuthority)
        );
        assert_eq!(
            authority.resolve_existing(outside.join("missing.txt").to_string_lossy().as_ref()),
            Err(PathAuthorityError::OutsideAuthority)
        );
        assert_eq!(
            authority.resolve_missing_leaf(
                outside
                    .join("must-not-be-created.txt")
                    .to_string_lossy()
                    .as_ref()
            ),
            Err(PathAuthorityError::OutsideAuthority)
        );
        assert_eq!(
            authority.input_path("../outside/outside.txt"),
            Err(PathAuthorityError::InvalidPath)
        );
        assert!(!workspace_input_path_valid(r"\\server\share\file.txt"));
        assert!(!workspace_input_path_valid(r"\\?\C:\project\file.txt"));
        assert!(!workspace_input_path_valid(r"C:\project\file.txt:ads"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_resolver_unifies_absolute_relative_and_default_cwd_inputs() {
        let root = temp_root();
        let workspace = root.join("workspace");
        let nested = workspace.join("nested");
        let file = nested.join("inside.txt");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(&file, b"inside").unwrap();

        let resolver = WorkspaceResolver::active_workspace(&workspace).unwrap();
        let expected = ordinary_path(&std::fs::canonicalize(&file).unwrap()).unwrap();
        assert_eq!(
            resolver
                .resolve_workspace_path(Some("inside.txt"), "nested", false)
                .unwrap(),
            expected
        );
        assert_eq!(
            resolver
                .resolve_workspace_path(Some(file.to_string_lossy().as_ref()), ".", false)
                .unwrap(),
            expected
        );
        assert_eq!(
            resolver
                .resolve_workspace_path(None, "nested", false)
                .unwrap(),
            ordinary_path(&std::fs::canonicalize(&nested).unwrap()).unwrap()
        );
        assert_eq!(
            resolver.resolve_workspace_path(Some("../outside"), "nested", false),
            Err(PathAuthorityError::InvalidPath)
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn active_workspace_rejects_regular_file_hard_link_aliases() {
        use windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_READ;

        let root = temp_root();
        let workspace = root.join("workspace");
        let outside = root.join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let outside_file = outside.join("shared.txt");
        let workspace_alias = workspace.join("alias.txt");
        std::fs::write(&outside_file, b"outside").unwrap();
        std::fs::hard_link(&outside_file, &workspace_alias).unwrap();

        let authority = WorkspaceResolver::active_workspace(&workspace).unwrap();
        assert!(matches!(
            authority.open_validated_handle(&workspace_alias, FILE_GENERIC_READ),
            Err(PathAuthorityError::OutsideAuthority)
        ));

        let broker = WorkspaceResolver::broker_administrator();
        assert!(
            broker
                .open_validated_handle(&workspace_alias, FILE_GENERIC_READ)
                .is_ok()
        );
        assert_eq!(std::fs::read(&outside_file).unwrap(), b"outside");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn active_workspace_lifetime_pin_binds_the_original_root_file_id() {
        let root = temp_root();
        let workspace = root.join("workspace");
        let displaced = root.join("workspace-old");
        std::fs::create_dir(&workspace).unwrap();
        let pin = WorkspaceResolver::pin_active_workspace_lifetime(&workspace).unwrap();
        assert_eq!(pin.validate_current(), Ok(()));
        std::fs::rename(&workspace, &displaced).unwrap();
        std::fs::create_dir(&workspace).unwrap();
        assert_eq!(
            pin.validate_current(),
            Err(PathAuthorityError::OutsideAuthority)
        );
        std::fs::remove_dir(&workspace).unwrap();
        std::fs::rename(&displaced, &workspace).unwrap();
        assert_eq!(pin.validate_current(), Ok(()));
        drop(pin);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn broker_administrator_accepts_only_ordinary_absolute_dispatch_paths() {
        let root = temp_root();
        let target = root.join("outside.txt");
        std::fs::write(&target, b"outside").unwrap();
        let authority = WorkspaceResolver::broker_administrator();
        assert_eq!(authority.scope(), PathAuthorityScope::BrokerAdministrator);
        assert_eq!(
            authority.input_path("relative.txt"),
            Err(PathAuthorityError::InvalidPath)
        );
        let authorized = authority
            .input_path(target.to_string_lossy().as_ref())
            .unwrap();
        assert_eq!(authorized, target);
        let resolved = authority
            .resolve_existing(authorized.to_string_lossy().as_ref())
            .unwrap();
        assert!(resolved.is_absolute());
        assert!(
            authority
                .display_path(&resolved)
                .unwrap()
                .contains("outside.txt")
        );
        assert_eq!(
            authority.input_path(r"\\?\C:\Windows\System32"),
            Err(PathAuthorityError::InvalidPath)
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
