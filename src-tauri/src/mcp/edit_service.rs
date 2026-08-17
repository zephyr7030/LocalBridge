use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::context_service::sha256_hex;
use super::path_authority::{PathAuthority, PathAuthorityError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodingEditError {
    InvalidPath,
    NotFound,
    FileChanged,
    PatchConflict,
    AmbiguousMatch,
    Io,
}

#[derive(Debug, Clone)]
pub(crate) struct CodingEditService {
    authority: PathAuthority,
}

#[derive(Debug)]
enum PatchOperation {
    Update {
        path: String,
        destination: Option<String>,
        hunks: Vec<(String, String)>,
    },
    Add { path: String, content: Vec<u8> },
    Delete { path: String },
}

impl CodingEditService {
    pub(crate) fn new(workspace: &Path) -> Result<Self, PathAuthorityError> {
        Ok(Self { authority: PathAuthority::active_workspace(workspace)? })
    }

    pub(crate) fn verify_expected_files(&self, expected: &Map<String, Value>) -> Result<(), CodingEditError> {
        for (path, identity) in expected {
            let expected = identity.as_str().filter(|value| value.len() == 64).ok_or(CodingEditError::InvalidPath)?;
            let (_, bytes) = self.read_file(path)?;
            if sha256_hex(&bytes) != expected { return Err(CodingEditError::FileChanged); }
        }
        Ok(())
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for bounded range reads.
    pub(crate) fn read_range(&self, path: &str, start_line: usize, end_line: usize) -> Result<String, CodingEditError> {
        if start_line == 0 || end_line < start_line { return Err(CodingEditError::InvalidPath); }
        let (_, bytes) = self.read_file(path)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| CodingEditError::PatchConflict)?;
        let lines = text.lines().collect::<Vec<_>>();
        if start_line > lines.len() { return Err(CodingEditError::NotFound); }
        Ok(lines[start_line - 1..end_line.min(lines.len())].join("\n"))
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for exact replacement callers.
    pub(crate) fn replace_exact(&self, path: &str, expected_sha256: &str, old: &str, new: &str) -> Result<String, CodingEditError> {
        let (target, bytes) = self.read_file(path)?;
        self.require_identity(&bytes, expected_sha256)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| CodingEditError::PatchConflict)?;
        let count = text.match_indices(old).count();
        if count == 0 { return Err(CodingEditError::PatchConflict); }
        if count > 1 { return Err(CodingEditError::AmbiguousMatch); }
        let updated = text.replacen(old, new, 1);
        atomic_write(&target, updated.as_bytes())?;
        Ok(sha256_hex(updated.as_bytes()))
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for search/replace callers.
    pub(crate) fn search_replace(&self, path: &str, expected_sha256: &str, needle: &str, replacement: &str) -> Result<String, CodingEditError> {
        self.replace_exact(path, expected_sha256, needle, replacement)
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for structured file creation.
    pub(crate) fn create_file(&self, path: &str, content: &[u8]) -> Result<String, CodingEditError> {
        let target = self.resolve_missing_leaf(path)?;
        if target.exists() { return Err(CodingEditError::FileChanged); }
        atomic_write(&target, content)?;
        Ok(sha256_hex(content))
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for identity-bound deletion.
    pub(crate) fn delete_file(&self, path: &str, expected_sha256: &str) -> Result<(), CodingEditError> {
        let (target, bytes) = self.read_file(path)?;
        self.require_identity(&bytes, expected_sha256)?;
        fs::remove_file(target).map_err(|_| CodingEditError::Io)
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for identity-bound rename/move.
    pub(crate) fn rename_file(&self, path: &str, destination: &str, expected_sha256: &str) -> Result<(), CodingEditError> {
        let (source, bytes) = self.read_file(path)?;
        self.require_identity(&bytes, expected_sha256)?;
        let destination = self.resolve_missing_leaf(destination)?;
        if destination.exists() { return Err(CodingEditError::FileChanged); }
        fs::rename(source, destination).map_err(|_| CodingEditError::Io)
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for structured directory creation.
    pub(crate) fn mkdir(&self, path: &str) -> Result<(), CodingEditError> {
        let target = self.resolve_missing_leaf(path)?;
        if target.exists() { return Err(CodingEditError::FileChanged); }
        fs::create_dir(&target).map_err(|_| CodingEditError::Io)?;
        let canonical = fs::canonicalize(&target).map_err(|_| CodingEditError::Io)?;
        if !self.authority.allows_canonical(&canonical) {
            let _ = fs::remove_dir(&target);
            return Err(CodingEditError::InvalidPath);
        }
        Ok(())
    }

    pub(crate) fn apply_patch_preconditions(&self, expected: &Map<String, Value>) -> Result<(), CodingEditError> {
        self.verify_expected_files(expected)
    }

    pub(crate) fn apply_patch(
        &self,
        patch: &str,
        expected: &Map<String, Value>,
    ) -> Result<Vec<String>, CodingEditError> {
        let operations = parse_patch(patch)?;
        if operations.is_empty() { return Err(CodingEditError::PatchConflict); }

        let mut updates = Vec::<(PathBuf, Option<PathBuf>, Vec<u8>)>::new();
        let mut adds = Vec::<(PathBuf, Vec<u8>)>::new();
        let mut deletes = Vec::<PathBuf>::new();
        let mut modified = Vec::<String>::new();

        for operation in operations {
            match operation {
                PatchOperation::Update { path, destination, hunks } => {
                    let identity = expected
                        .get(&path)
                        .and_then(Value::as_str)
                        .ok_or(CodingEditError::FileChanged)?;
                    let (source, bytes) = self.read_file(&path)?;
                    self.require_identity(&bytes, identity)?;
                    let mut text = std::str::from_utf8(&bytes)
                        .map_err(|_| CodingEditError::PatchConflict)?
                        .to_string();
                    for (old, new) in hunks {
                        let count = text.match_indices(&old).count();
                        if count == 0 { return Err(CodingEditError::PatchConflict); }
                        if count > 1 { return Err(CodingEditError::AmbiguousMatch); }
                        text = text.replacen(&old, &new, 1);
                    }
                    let target = match destination.as_deref() {
                        Some(destination) => {
                            let target = self.resolve_missing_leaf(destination)?;
                            if target.exists() && target != source { return Err(CodingEditError::FileChanged); }
                            Some(target)
                        }
                        None => None,
                    };
                    modified.push(destination.unwrap_or_else(|| path.clone()));
                    updates.push((source, target, text.into_bytes()));
                }
                PatchOperation::Add { path, content } => {
                    let target = self.resolve_missing_leaf(&path)?;
                    if target.exists() { return Err(CodingEditError::FileChanged); }
                    modified.push(path);
                    adds.push((target, content));
                }
                PatchOperation::Delete { path } => {
                    let identity = expected
                        .get(&path)
                        .and_then(Value::as_str)
                        .ok_or(CodingEditError::FileChanged)?;
                    let (target, bytes) = self.read_file(&path)?;
                    self.require_identity(&bytes, identity)?;
                    modified.push(path);
                    deletes.push(target);
                }
            }
        }

        for (source, destination, bytes) in updates {
            atomic_write(&source, &bytes)?;
            if let Some(destination) = destination {
                fs::rename(source, destination).map_err(|_| CodingEditError::Io)?;
            }
        }
        for (target, content) in adds { atomic_write(&target, &content)?; }
        for target in deletes { fs::remove_file(target).map_err(|_| CodingEditError::Io)?; }
        modified.sort();
        modified.dedup();
        Ok(modified)
    }

    fn read_file(&self, path: &str) -> Result<(PathBuf, Vec<u8>), CodingEditError> {
        let target = self.authority.resolve_existing(path).map_err(map_path_error)?;
        if !target.is_file() { return Err(CodingEditError::NotFound); }
        let bytes = fs::read(&target).map_err(|_| CodingEditError::Io)?;
        Ok((target, bytes))
    }

    fn resolve_missing_leaf(&self, path: &str) -> Result<PathBuf, CodingEditError> {
        let target = self.authority.input_path(path).map_err(map_path_error)?;
        let parent = target.parent().ok_or(CodingEditError::InvalidPath)?;
        let parent = fs::canonicalize(parent).map_err(|_| CodingEditError::NotFound)?;
        if !self.authority.allows_canonical(&parent) || !parent.is_dir() { return Err(CodingEditError::InvalidPath); }
        let name = target.file_name().ok_or(CodingEditError::InvalidPath)?;
        Ok(parent.join(name))
    }

    fn require_identity(&self, bytes: &[u8], expected_sha256: &str) -> Result<(), CodingEditError> {
        if expected_sha256.len() != 64 { return Err(CodingEditError::InvalidPath); }
        if sha256_hex(bytes) != expected_sha256 { return Err(CodingEditError::FileChanged); }
        Ok(())
    }
}

fn map_path_error(error: PathAuthorityError) -> CodingEditError {
    match error {
        PathAuthorityError::NotFound => CodingEditError::NotFound,
        PathAuthorityError::InvalidPath | PathAuthorityError::OutsideAuthority => CodingEditError::InvalidPath,
    }
}

fn parse_patch(patch: &str) -> Result<Vec<PatchOperation>, CodingEditError> {
    let normalized = patch.replace("\r\n", "\n");
    let lines = normalized.lines().collect::<Vec<_>>();
    if lines.first().copied() != Some("*** Begin Patch")
        || lines.last().copied() != Some("*** End Patch")
    {
        return Err(CodingEditError::PatchConflict);
    }
    let mut operations = Vec::new();
    let mut index = 1usize;
    while index + 1 < lines.len() {
        let line = lines[index];
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            index += 1;
            let mut content = String::new();
            while index < lines.len() && !lines[index].starts_with("*** ") {
                let Some(added) = lines[index].strip_prefix('+') else {
                    return Err(CodingEditError::PatchConflict);
                };
                content.push_str(added);
                content.push('\n');
                index += 1;
            }
            operations.push(PatchOperation::Add {
                path: path.to_string(),
                content: content.into_bytes(),
            });
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Delete File: ") {
            operations.push(PatchOperation::Delete { path: path.to_string() });
            index += 1;
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Update File: ") {
            index += 1;
            let mut destination = None;
            if index < lines.len() {
                if let Some(value) = lines[index].strip_prefix("*** Move to: ") {
                    destination = Some(value.to_string());
                    index += 1;
                }
            }
            let mut hunks = Vec::new();
            while index < lines.len() && !lines[index].starts_with("*** ") {
                if !lines[index].starts_with("@@") {
                    return Err(CodingEditError::PatchConflict);
                }
                index += 1;
                let mut old = String::new();
                let mut new = String::new();
                while index < lines.len()
                    && !lines[index].starts_with("@@")
                    && !lines[index].starts_with("*** ")
                {
                    let current = lines[index];
                    if let Some(value) = current.strip_prefix('-') {
                        old.push_str(value);
                        old.push('\n');
                    } else if let Some(value) = current.strip_prefix('+') {
                        new.push_str(value);
                        new.push('\n');
                    } else if let Some(value) = current.strip_prefix(' ') {
                        old.push_str(value);
                        old.push('\n');
                        new.push_str(value);
                        new.push('\n');
                    } else {
                        return Err(CodingEditError::PatchConflict);
                    }
                    index += 1;
                }
                if old.is_empty() {
                    return Err(CodingEditError::PatchConflict);
                }
                hunks.push((old, new));
            }
            operations.push(PatchOperation::Update {
                path: path.to_string(),
                destination,
                hunks,
            });
            continue;
        }
        return Err(CodingEditError::PatchConflict);
    }
    Ok(operations)
}

fn atomic_write(target: &Path, content: &[u8]) -> Result<(), CodingEditError> {
    let parent = target.parent().ok_or(CodingEditError::InvalidPath)?;
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let tmp = parent.join(format!(".localbridge-edit-{}-{nonce}.tmp", std::process::id()));
    fs::write(&tmp, content).map_err(|_| CodingEditError::Io)?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};
        let source = tmp.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<_>>();
        let destination = target.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<_>>();
        let ok = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) };
        if ok == 0 { let _ = fs::remove_file(&tmp); return Err(CodingEditError::Io); }
    }
    #[cfg(not(windows))]
    {
        fs::rename(&tmp, target).map_err(|_| CodingEditError::Io)?;
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn workspace(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let root = std::env::temp_dir().join(format!("localbridge-edit-{label}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn exact_replace_is_identity_bound_atomic_and_ambiguous_fails_closed() {
        let root = workspace("replace");
        let path = root.join("a.txt");
        fs::write(&path, b"before\n").unwrap();
        let service = CodingEditService::new(&root).unwrap();
        let identity = sha256_hex(b"before\n");
        let after = service.replace_exact("a.txt", &identity, "before", "after").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "after\n");
        assert_eq!(after, sha256_hex(b"after\n"));
        assert_eq!(service.replace_exact("a.txt", &identity, "after", "again"), Err(CodingEditError::FileChanged));
        fs::write(&path, b"x x").unwrap();
        let identity = sha256_hex(b"x x");
        assert_eq!(service.search_replace("a.txt", &identity, "x", "y"), Err(CodingEditError::AmbiguousMatch));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn create_rename_delete_and_mkdir_stay_workspace_bound() {
        let root = workspace("lifecycle");
        let service = CodingEditService::new(&root).unwrap();
        let identity = service.create_file("a.txt", b"hello").unwrap();
        service.rename_file("a.txt", "b.txt", &identity).unwrap();
        service.mkdir("dir").unwrap();
        service.delete_file("b.txt", &identity).unwrap();
        assert!(!root.join("b.txt").exists());
        assert!(root.join("dir").is_dir());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn apply_patch_is_identity_bound_and_stale_content_fails_closed() {
        let root = workspace("patch");
        fs::write(root.join("a.txt"), b"before\ncontext\n").unwrap();
        let service = CodingEditService::new(&root).unwrap();
        let mut expected = Map::new();
        expected.insert(
            "a.txt".into(),
            Value::String(sha256_hex(b"before\ncontext\n")),
        );
        let changed = service
            .apply_patch(
                "*** Begin Patch\n*** Update File: a.txt\n@@\n-before\n+after\n context\n*** End Patch",
                &expected,
            )
            .unwrap();
        assert_eq!(changed, vec!["a.txt"]);
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "after\ncontext\n");
        assert_eq!(
            service.apply_patch(
                "*** Begin Patch\n*** Update File: a.txt\n@@\n-after\n+again\n*** End Patch",
                &expected,
            ),
            Err(CodingEditError::FileChanged)
        );
        let _ = fs::remove_dir_all(root);
    }
}
