use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
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

    #[allow(dead_code)] // schema41 internal semantic surface; retained for exact replacement callers.
    pub(crate) fn replace_exact(&self, path: &str, expected_sha256: &str, old: &str, new: &str) -> Result<String, CodingEditError> {
        let target = self.authority.resolve_existing(path).map_err(map_path_error)?;
        if !target.is_file() { return Err(CodingEditError::NotFound); }
        let updated = conditional_transform_existing(&target, expected_sha256, |bytes| {
            let text = std::str::from_utf8(bytes).map_err(|_| CodingEditError::PatchConflict)?;
            let count = text.match_indices(old).count();
            if count == 0 { return Err(CodingEditError::PatchConflict); }
            if count > 1 { return Err(CodingEditError::AmbiguousMatch); }
            Ok(text.replacen(old, new, 1).into_bytes())
        })?;
        Ok(sha256_hex(&updated))
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for search/replace callers.
    pub(crate) fn search_replace(&self, path: &str, expected_sha256: &str, needle: &str, replacement: &str) -> Result<String, CodingEditError> {
        self.replace_exact(path, expected_sha256, needle, replacement)
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for structured file creation.
    pub(crate) fn create_file(&self, path: &str, content: &[u8]) -> Result<String, CodingEditError> {
        let target = self.resolve_missing_leaf(path)?;
        create_new_file(&target, content)?;
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

        let mut updates = Vec::<(PathBuf, Option<PathBuf>, Vec<(String, String)>, String)>::new();
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
                    for (old, new) in &hunks {
                        let count = text.match_indices(old.as_str()).count();
                        if count == 0 { return Err(CodingEditError::PatchConflict); }
                        if count > 1 { return Err(CodingEditError::AmbiguousMatch); }
                        text = text.replacen(old.as_str(), new.as_str(), 1);
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
                    updates.push((source, target, hunks, identity.to_string()));
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

        for (source, destination, hunks, identity) in updates {
            conditional_transform_existing(&source, &identity, |bytes| {
                let mut text = std::str::from_utf8(bytes)
                    .map_err(|_| CodingEditError::PatchConflict)?
                    .to_string();
                for (old, new) in &hunks {
                    let count = text.match_indices(old.as_str()).count();
                    if count == 0 { return Err(CodingEditError::PatchConflict); }
                    if count > 1 { return Err(CodingEditError::AmbiguousMatch); }
                    text = text.replacen(old.as_str(), new.as_str(), 1);
                }
                Ok(text.into_bytes())
            })?;
            if let Some(destination) = destination {
                if destination.exists() { return Err(CodingEditError::FileChanged); }
                fs::rename(source, destination).map_err(|error| {
                    if error.kind() == std::io::ErrorKind::AlreadyExists { CodingEditError::FileChanged } else { CodingEditError::Io }
                })?;
            }
        }
        for (target, content) in adds { create_new_file(&target, &content)?; }
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

fn map_existing_open_error(error: std::io::Error) -> CodingEditError {
    if error.kind() == std::io::ErrorKind::NotFound {
        return CodingEditError::NotFound;
    }
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        return CodingEditError::FileChanged;
    }
    CodingEditError::Io
}

fn create_new_file(target: &Path, content: &[u8]) -> Result<(), CodingEditError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let mut file = options.open(target).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists { CodingEditError::FileChanged } else { CodingEditError::Io }
    })?;
    if file.write_all(content).and_then(|_| file.sync_all()).is_err() {
        drop(file);
        let _ = fs::remove_file(target);
        return Err(CodingEditError::Io);
    }
    Ok(())
}

fn conditional_transform_existing<F>(
    target: &Path,
    expected_sha256: &str,
    transform: F,
) -> Result<Vec<u8>, CodingEditError>
where
    F: FnOnce(&[u8]) -> Result<Vec<u8>, CodingEditError>,
{
    if expected_sha256.len() != 64 { return Err(CodingEditError::InvalidPath); }
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let mut file = options.open(target).map_err(map_existing_open_error)?;
    let mut original = Vec::new();
    file.read_to_end(&mut original).map_err(|_| CodingEditError::Io)?;
    if sha256_hex(&original) != expected_sha256 { return Err(CodingEditError::FileChanged); }
    let updated = transform(&original)?;
    let write_result = (|| -> std::io::Result<()> {
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&updated)?;
        file.set_len(updated.len() as u64)?;
        file.sync_all()?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = file.seek(SeekFrom::Start(0));
        let _ = file.write_all(&original);
        let _ = file.set_len(original.len() as u64);
        let _ = file.sync_all();
        return Err(CodingEditError::Io);
    }
    Ok(updated)
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

    #[test]
    fn concurrent_existing_writer_handle_fails_closed_before_any_overwrite() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE};
        let root = workspace("writer-race");
        let path = root.join("a.txt");
        fs::write(&path, b"before\n").unwrap();
        let expected = sha256_hex(b"before\n");
        let _writer = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&path)
            .unwrap();
        let service = CodingEditService::new(&root).unwrap();
        assert_eq!(
            service.replace_exact("a.txt", &expected, "before", "after"),
            Err(CodingEditError::FileChanged)
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "before\n");
        drop(_writer);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn concurrent_create_new_has_exactly_one_winner_and_never_overwrites() {
        use std::sync::{Arc, Barrier};
        let root = workspace("create-race");
        let service = CodingEditService::new(&root).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let mut threads = Vec::new();
        for content in [b"first".as_slice(), b"second".as_slice()] {
            let service = service.clone();
            let barrier = barrier.clone();
            let bytes = content.to_vec();
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                service.create_file("new.txt", &bytes)
            }));
        }
        barrier.wait();
        let results = threads.into_iter().map(|thread| thread.join().unwrap()).collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1, "{results:?}");
        assert_eq!(results.iter().filter(|result| **result == Err(CodingEditError::FileChanged)).count(), 1, "{results:?}");
        let final_bytes = fs::read(root.join("new.txt")).unwrap();
        assert!(final_bytes == b"first" || final_bytes == b"second");
        let _ = fs::remove_dir_all(root);
    }
}
