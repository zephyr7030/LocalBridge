use std::collections::BTreeMap;
#[cfg(test)]
use std::path::Path;

use super::service::{FilesystemError, FilesystemService};
use crate::workspace::context::sha256_hex;
use crate::workspace::path_authority::WorkspaceResolver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodingEditError {
    InvalidPath,
    NotFound,
    FileChanged,
    PatchConflict,
    AmbiguousMatch,
    MultiFilePatchUnsupported,
    Io,
}

#[derive(Debug, Clone)]
pub(crate) struct CodingEditService {
    filesystem: FilesystemService,
}

#[derive(Debug)]
enum PatchOperation {
    Update {
        path: String,
        destination: Option<String>,
        hunks: Vec<(String, String)>,
    },
    Add {
        path: String,
        content: Vec<u8>,
    },
    Delete {
        path: String,
    },
}

impl CodingEditService {
    #[cfg(test)]
    pub(crate) fn new(workspace: &Path) -> Result<Self, CodingEditError> {
        let authority = crate::workspace::WorkspaceResolver::active_workspace(workspace)
            .map_err(|_| CodingEditError::InvalidPath)?;
        Self::with_authority(authority)
    }

    pub(crate) fn with_authority(authority: WorkspaceResolver) -> Result<Self, CodingEditError> {
        Ok(Self {
            filesystem: FilesystemService::from_authority(authority)
                .map_err(map_filesystem_error)?,
        })
    }

    pub(crate) fn verify_expected_files(
        &self,
        expected: &BTreeMap<String, String>,
    ) -> Result<(), CodingEditError> {
        for (path, identity) in expected {
            if identity.len() != 64 || !identity.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(CodingEditError::InvalidPath);
            }
            let expected = identity.as_str();
            let bytes = self.read_file(path)?;
            if !sha256_hex(&bytes).eq_ignore_ascii_case(expected) {
                return Err(CodingEditError::FileChanged);
            }
        }
        Ok(())
    }

    pub(crate) fn replace_exact(
        &self,
        path: &str,
        expected_sha256: &str,
        old: &str,
        new: &str,
    ) -> Result<String, CodingEditError> {
        let bytes = self.read_file(path)?;
        self.require_identity(&bytes, expected_sha256)?;
        let updated = apply_text_hunks_preserving_format(
            &bytes,
            &[(normalize_patch_text(old), normalize_patch_text(new))],
        )?;
        self.filesystem
            .replace_file_if_sha256(path, expected_sha256, &updated)
            .map_err(map_filesystem_error)?;
        Ok(sha256_hex(&updated))
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for search/replace callers.
    pub(crate) fn search_replace(
        &self,
        path: &str,
        expected_sha256: &str,
        needle: &str,
        replacement: &str,
    ) -> Result<String, CodingEditError> {
        self.replace_exact(path, expected_sha256, needle, replacement)
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for structured file creation.
    pub(crate) fn create_file(
        &self,
        path: &str,
        content: &[u8],
    ) -> Result<String, CodingEditError> {
        self.filesystem
            .create_file_for_edit(path, content)
            .map_err(map_filesystem_error)?;
        Ok(sha256_hex(content))
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for identity-bound deletion.
    pub(crate) fn delete_file(
        &self,
        path: &str,
        expected_sha256: &str,
    ) -> Result<(), CodingEditError> {
        self.filesystem
            .delete_file_if_sha256(path, expected_sha256)
            .map_err(map_filesystem_error)
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for identity-bound rename/move.
    pub(crate) fn rename_file(
        &self,
        path: &str,
        destination: &str,
        expected_sha256: &str,
    ) -> Result<(), CodingEditError> {
        self.filesystem
            .move_file_if_sha256(path, destination, expected_sha256)
            .map_err(map_filesystem_error)
    }

    #[allow(dead_code)] // schema41 internal semantic surface; retained for structured directory creation.
    pub(crate) fn mkdir(&self, path: &str) -> Result<(), CodingEditError> {
        self.filesystem
            .create_directory(path)
            .map(|_| ())
            .map_err(map_filesystem_error)
    }

    pub(crate) fn apply_patch_preconditions(
        &self,
        expected: &BTreeMap<String, String>,
    ) -> Result<(), CodingEditError> {
        self.verify_expected_files(expected)
    }

    pub(crate) fn apply_patch(
        &self,
        patch: &str,
        expected: &BTreeMap<String, String>,
    ) -> Result<Vec<String>, CodingEditError> {
        self.apply_patch_with_expected(patch, Some(expected))
    }

    pub(crate) fn apply_patch_to_current(
        &self,
        patch: &str,
    ) -> Result<Vec<String>, CodingEditError> {
        self.apply_patch_with_expected(patch, None)
    }

    fn apply_patch_with_expected(
        &self,
        patch: &str,
        expected: Option<&BTreeMap<String, String>>,
    ) -> Result<Vec<String>, CodingEditError> {
        if let Some(expected) = expected {
            self.verify_expected_files(expected)?;
        }
        let mut operations = parse_patch(patch)?;
        if operations.is_empty() {
            return Err(CodingEditError::PatchConflict);
        }
        if operations.len() != 1 {
            return Err(CodingEditError::MultiFilePatchUnsupported);
        }
        match operations.pop().expect("one parsed patch operation") {
            PatchOperation::Update {
                path,
                destination,
                hunks,
            } => {
                if destination.is_some() {
                    return Err(CodingEditError::MultiFilePatchUnsupported);
                }
                let bytes = self.read_file(&path)?;
                let identity = match expected {
                    Some(expected) => expected
                        .get(&path)
                        .ok_or(CodingEditError::FileChanged)?
                        .clone(),
                    None => sha256_hex(&bytes),
                };
                self.require_identity(&bytes, &identity)?;
                let updated = apply_text_hunks_preserving_format(&bytes, &hunks)?;
                self.filesystem
                    .replace_file_if_sha256(&path, &identity, &updated)
                    .map_err(map_filesystem_error)?;
                Ok(vec![path])
            }
            PatchOperation::Add { path, content } => {
                self.filesystem
                    .validate_new_file_path(&path)
                    .map_err(map_filesystem_error)?;
                self.filesystem
                    .create_file_for_edit(&path, &content)
                    .map_err(map_filesystem_error)?;
                Ok(vec![path])
            }
            PatchOperation::Delete { path } => {
                let bytes = self.read_file(&path)?;
                let identity = match expected {
                    Some(expected) => expected
                        .get(&path)
                        .ok_or(CodingEditError::FileChanged)?
                        .clone(),
                    None => sha256_hex(&bytes),
                };
                self.require_identity(&bytes, &identity)?;
                self.filesystem
                    .delete_file_if_sha256(&path, &identity)
                    .map_err(map_filesystem_error)?;
                Ok(vec![path])
            }
        }
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>, CodingEditError> {
        self.filesystem
            .read_bytes_bounded(path, super::service::MAX_INTERNAL_FILE_BYTES)
            .map_err(map_filesystem_error)
    }

    fn require_identity(&self, bytes: &[u8], expected_sha256: &str) -> Result<(), CodingEditError> {
        if expected_sha256.len() != 64
            || !expected_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(CodingEditError::InvalidPath);
        }
        if !sha256_hex(bytes).eq_ignore_ascii_case(expected_sha256) {
            return Err(CodingEditError::FileChanged);
        }
        Ok(())
    }
}

pub(crate) fn normalize_patch_text(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

pub(crate) fn apply_text_hunks_preserving_format(
    bytes: &[u8],
    hunks: &[(String, String)],
) -> Result<Vec<u8>, CodingEditError> {
    let decoded = std::str::from_utf8(bytes).map_err(|_| CodingEditError::PatchConflict)?;
    let (bom, body) = decoded
        .strip_prefix('\u{feff}')
        .map_or(("", decoded), |body| ("\u{feff}", body));
    let line_ending = if body.find("\r\n").is_some_and(|crlf| {
        body.find('\n')
            .is_none_or(|lf| crlf <= lf.saturating_sub(1))
    }) {
        "\r\n"
    } else {
        "\n"
    };
    let mut text = body.replace("\r\n", "\n").replace('\r', "\n");
    for (old, new) in hunks {
        let (needle, replacement) = if text.match_indices(old.as_str()).next().is_none() {
            match old.strip_suffix('\n') {
                Some(without_final_newline)
                    if !without_final_newline.is_empty()
                        && text.ends_with(without_final_newline) =>
                {
                    (
                        without_final_newline,
                        new.strip_suffix('\n').unwrap_or(new.as_str()),
                    )
                }
                _ => (old.as_str(), new.as_str()),
            }
        } else {
            (old.as_str(), new.as_str())
        };
        let count = text.match_indices(needle).count();
        if count == 0 {
            return Err(CodingEditError::PatchConflict);
        }
        if count > 1 {
            return Err(CodingEditError::AmbiguousMatch);
        }
        text = text.replacen(needle, replacement, 1);
    }
    if line_ending == "\r\n" {
        text = text.replace('\n', "\r\n");
    }
    Ok(format!("{bom}{text}").into_bytes())
}

fn map_filesystem_error(error: FilesystemError) -> CodingEditError {
    match error {
        FilesystemError::NotFound => CodingEditError::NotFound,
        FilesystemError::InvalidArgument | FilesystemError::OutsideAuthority => {
            CodingEditError::InvalidPath
        }
        FilesystemError::AlreadyExists | FilesystemError::FileChanged => {
            CodingEditError::FileChanged
        }
        FilesystemError::LimitExceeded
        | FilesystemError::Cancelled
        | FilesystemError::Io
        | FilesystemError::Unsupported => CodingEditError::Io,
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
            operations.push(PatchOperation::Delete {
                path: path.to_string(),
            });
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn workspace(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "localbridge-edit-{label}-{}-{nonce}",
            std::process::id()
        ));
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
        let after = service
            .replace_exact("a.txt", &identity, "before", "after")
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "after\n");
        assert_eq!(after, sha256_hex(b"after\n"));
        assert_eq!(
            service.replace_exact("a.txt", &identity, "after", "again"),
            Err(CodingEditError::FileChanged)
        );
        fs::write(&path, b"x x").unwrap();
        let identity = sha256_hex(b"x x");
        assert_eq!(
            service.search_replace("a.txt", &identity, "x", "y"),
            Err(CodingEditError::AmbiguousMatch)
        );
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
        let mut expected = BTreeMap::new();
        expected.insert("a.txt".into(), sha256_hex(b"before\ncontext\n"));
        let changed = service
            .apply_patch(
                "*** Begin Patch\n*** Update File: a.txt\n@@\n-before\n+after\n context\n*** End Patch",
                &expected,
            )
            .unwrap();
        assert_eq!(changed, vec!["a.txt"]);
        assert_eq!(
            fs::read_to_string(root.join("a.txt")).unwrap(),
            "after\ncontext\n"
        );
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
    fn apply_patch_preserves_utf8_bom_and_crlf() {
        let root = workspace("patch-format");
        let before = b"\xef\xbb\xbfbefore\r\ncontext\r\n";
        fs::write(root.join("a.txt"), before).unwrap();
        let service = CodingEditService::new(&root).unwrap();
        let mut expected = BTreeMap::new();
        expected.insert("a.txt".into(), sha256_hex(before));

        service
            .apply_patch(
                "*** Begin Patch\n*** Update File: a.txt\n@@\n-before\n+after\n context\n*** End Patch",
                &expected,
            )
            .unwrap();

        assert_eq!(
            fs::read(root.join("a.txt")).unwrap(),
            b"\xef\xbb\xbfafter\r\ncontext\r\n"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn apply_patch_preserves_missing_final_newline() {
        let root = workspace("patch-no-final-newline");
        fs::write(root.join("a.txt"), b"before").unwrap();
        let service = CodingEditService::new(&root).unwrap();
        let mut expected = BTreeMap::new();
        expected.insert("a.txt".into(), sha256_hex(b"before"));

        service
            .apply_patch(
                "*** Begin Patch\n*** Update File: a.txt\n@@\n-before\n+after\n*** End Patch",
                &expected,
            )
            .unwrap();

        assert_eq!(fs::read(root.join("a.txt")).unwrap(), b"after");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_final_newline_fallback_only_matches_the_file_end() {
        let root = workspace("patch-no-final-newline-boundary");
        fs::write(root.join("a.txt"), b"before suffix").unwrap();
        let service = CodingEditService::new(&root).unwrap();
        let mut expected = BTreeMap::new();
        expected.insert("a.txt".into(), sha256_hex(b"before suffix"));

        assert_eq!(
            service.apply_patch(
                "*** Begin Patch\n*** Update File: a.txt\n@@\n-before\n+after\n*** End Patch",
                &expected,
            ),
            Err(CodingEditError::PatchConflict)
        );
        assert_eq!(fs::read(root.join("a.txt")).unwrap(), b"before suffix");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn multi_file_patch_is_rejected_before_any_file_changes() {
        let root = workspace("patch-single-file-boundary");
        let first = b"first-before\n";
        let second = b"second-before\n";
        fs::write(root.join("a.txt"), first).unwrap();
        fs::write(root.join("b.txt"), second).unwrap();
        let service = CodingEditService::new(&root).unwrap();
        let mut expected = BTreeMap::new();
        expected.insert("a.txt".into(), sha256_hex(first));
        expected.insert("b.txt".into(), sha256_hex(second));
        let result = service.apply_patch(
            "*** Begin Patch\n*** Update File: a.txt\n@@\n-first-before\n+first-after\n*** Update File: b.txt\n@@\n-second-before\n+second-after\n*** End Patch",
            &expected,
        );

        assert_eq!(result, Err(CodingEditError::MultiFilePatchUnsupported));
        assert_eq!(fs::read(root.join("a.txt")).unwrap(), first);
        assert_eq!(fs::read(root.join("b.txt")).unwrap(), second);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn concurrent_existing_writer_handle_fails_closed_before_any_overwrite() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
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
    fn workspace_hard_link_cannot_patch_an_outside_file_object() {
        let container = workspace("hardlink-container");
        let root = container.join("workspace");
        let outside = container.join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        let outside_file = outside.join("shared.txt");
        fs::write(&outside_file, b"outside\n").unwrap();
        fs::hard_link(&outside_file, root.join("alias.txt")).unwrap();

        let service = CodingEditService::new(&root).unwrap();
        assert_eq!(
            service.replace_exact("alias.txt", &sha256_hex(b"outside\n"), "outside", "changed",),
            Err(CodingEditError::InvalidPath)
        );
        assert_eq!(fs::read(&outside_file).unwrap(), b"outside\n");
        let _ = fs::remove_dir_all(container);
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
        let results = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            results.iter().filter(|result| result.is_ok()).count(),
            1,
            "{results:?}"
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == Err(CodingEditError::FileChanged))
                .count(),
            1,
            "{results:?}"
        );
        let final_bytes = fs::read(root.join("new.txt")).unwrap();
        assert!(final_bytes == b"first" || final_bytes == b"second");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shared_filesystem_blocks_deterministic_coding_edit_ancestor_swap() {
        fn create_junction(link: &Path, target: &Path) {
            let output = std::process::Command::new("cmd")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let root = workspace("ancestor-swap");
        let outside = workspace("ancestor-swap-outside");
        let safe = root.join("safe");
        let parent = safe.join("parent");
        let outside_parent = outside.join("parent");
        let displaced = root.join("safe-original");
        fs::create_dir_all(&parent).unwrap();
        fs::create_dir_all(&outside_parent).unwrap();
        fs::write(parent.join("a.txt"), b"inside\n").unwrap();
        fs::write(outside_parent.join("a.txt"), b"outside\n").unwrap();

        let authority = crate::workspace::WorkspaceResolver::active_workspace(&root).unwrap();
        let checked = authority.resolve_existing("safe/parent/a.txt").unwrap();
        fs::rename(&safe, &displaced).unwrap();
        create_junction(&safe, &outside);
        assert_ne!(
            fs::canonicalize(safe.join("parent/a.txt")).unwrap(),
            checked
        );
        fs::remove_dir(&safe).unwrap();
        fs::rename(&displaced, &safe).unwrap();

        let service = CodingEditService::new(&root).unwrap();
        service
            .filesystem
            .replace_file_if_sha256_with_test_hook(
                "safe/parent/a.txt",
                &sha256_hex(b"inside\n"),
                b"updated\n",
                || assert!(fs::rename(&safe, &displaced).is_err()),
            )
            .unwrap();
        assert_eq!(fs::read(parent.join("a.txt")).unwrap(), b"updated\n");
        assert_eq!(
            fs::read(outside_parent.join("a.txt")).unwrap(),
            b"outside\n"
        );

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
