use std::path::{Component, Path, PathBuf};

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

#[derive(Debug, Clone)]
pub struct PathAuthority {
    scope: PathAuthorityScope,
    execution_root: Option<PathBuf>,
    canonical_root: Option<PathBuf>,
}

impl PathAuthority {
    pub fn active_workspace(root: &Path) -> Result<Self, PathAuthorityError> {
        if !root.is_absolute() || is_verbatim_path(root) || !root.is_dir() {
            return Err(PathAuthorityError::InvalidPath);
        }
        let canonical_root =
            std::fs::canonicalize(root).map_err(|_| PathAuthorityError::InvalidPath)?;
        Ok(Self {
            scope: PathAuthorityScope::ActiveWorkspace,
            execution_root: Some(root.to_path_buf()),
            canonical_root: Some(canonical_root),
        })
    }

    pub fn broker_administrator() -> Self {
        Self {
            scope: PathAuthorityScope::BrokerAdministrator,
            execution_root: None,
            canonical_root: None,
        }
    }

    pub const fn scope(&self) -> PathAuthorityScope {
        self.scope
    }

    pub fn input_path(&self, raw: &str) -> Result<PathBuf, PathAuthorityError> {
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

    pub fn resolve_existing(&self, raw: &str) -> Result<PathBuf, PathAuthorityError> {
        let candidate = self.input_path(raw)?;
        let canonical =
            std::fs::canonicalize(candidate).map_err(|_| PathAuthorityError::NotFound)?;
        self.allows_canonical(&canonical)
            .then_some(canonical)
            .ok_or(PathAuthorityError::OutsideAuthority)
    }

    pub fn allows_canonical(&self, canonical: &Path) -> bool {
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
                self.canonical_root.as_deref() == Some(canonical)
            }
            PathAuthorityScope::BrokerAdministrator => false,
        }
    }

    pub fn display_path(&self, canonical: &Path) -> Result<String, PathAuthorityError> {
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
                ordinary_path(canonical)
                    .ok_or(PathAuthorityError::InvalidPath)?
                    .to_string_lossy()
                    .replace('\\', "/")
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

        let authority = PathAuthority::active_workspace(&workspace).unwrap();
        let inside = authority.resolve_existing("inside.txt").unwrap();
        assert!(inside.starts_with(std::fs::canonicalize(&workspace).unwrap()));
        assert_eq!(authority.display_path(&inside).unwrap(), "inside.txt");
        let absolute_inside = authority
            .resolve_existing(workspace.join("inside.txt").to_string_lossy().as_ref())
            .unwrap();
        assert_eq!(absolute_inside, inside);
        assert_eq!(
            authority.display_path(
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
            authority.input_path("../outside/outside.txt"),
            Err(PathAuthorityError::InvalidPath)
        );
        assert!(!workspace_input_path_valid(r"\\server\share\file.txt"));
        assert!(!workspace_input_path_valid(r"\\?\C:\project\file.txt"));
        assert!(!workspace_input_path_valid(r"C:\project\file.txt:ads"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn broker_administrator_accepts_only_ordinary_absolute_dispatch_paths() {
        let root = temp_root();
        let target = root.join("outside.txt");
        std::fs::write(&target, b"outside").unwrap();
        let authority = PathAuthority::broker_administrator();
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
        assert!(authority
            .display_path(&resolved)
            .unwrap()
            .contains("outside.txt"));
        assert_eq!(
            authority.input_path(r"\\?\C:\Windows\System32"),
            Err(PathAuthorityError::InvalidPath)
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
