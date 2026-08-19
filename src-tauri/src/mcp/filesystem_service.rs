use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::path_authority::{PathAuthority, PathAuthorityError, PathAuthorityScope};

pub(crate) const MAX_FILESYSTEM_READ_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_FILESYSTEM_ENTRIES: usize = 100_000;
pub(crate) const MAX_FILESYSTEM_RESULTS: usize = 10_000;
pub(crate) const MAX_FILESYSTEM_DEPTH: u32 = 64;

static TEMP_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilesystemError {
    InvalidArgument,
    NotFound,
    OutsideAuthority,
    AlreadyExists,
    LimitExceeded,
    Io,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FilesystemEntry {
    pub path: String,
    pub kind: &'static str,
    pub size: u64,
    pub modified_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FilesystemListResult {
    pub entries: Vec<FilesystemEntry>,
    pub scanned_entries: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FilesystemStatResult {
    pub path: String,
    pub kind: &'static str,
    pub size: u64,
    pub modified_ms: Option<u64>,
    pub calculated_size: bool,
    pub scanned_entries: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FilesystemReadResult {
    pub path: String,
    pub offset: u64,
    pub total_bytes: u64,
    pub returned_bytes: usize,
    pub eof: bool,
    pub encoding: &'static str,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FilesystemMutationResult {
    pub path: String,
    pub destination: Option<String>,
    pub bytes: u64,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FilesystemHashResult {
    pub path: String,
    pub algorithm: &'static str,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct FilesystemSearchOptions {
    pub recursive: bool,
    pub max_depth: u32,
    pub max_entries: usize,
    pub max_results: usize,
    pub pattern: String,
    pub kind: Option<String>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub modified_after_ms: Option<u64>,
    pub modified_before_ms: Option<u64>,
    pub sort_by: String,
    pub sort_order: String,
}

impl Default for FilesystemSearchOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            max_depth: 16,
            max_entries: 10_000,
            max_results: 1_000,
            pattern: "*".into(),
            kind: None,
            min_size: None,
            max_size: None,
            modified_after_ms: None,
            modified_before_ms: None,
            sort_by: "path".into(),
            sort_order: "asc".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FilesystemService {
    authority: PathAuthority,
}

impl FilesystemService {
    pub(crate) fn active_workspace(root: &Path) -> Result<Self, FilesystemError> {
        Ok(Self {
            authority: PathAuthority::active_workspace(root).map_err(map_path_error)?,
        })
    }

    pub(crate) fn broker_administrator() -> Self {
        Self {
            authority: PathAuthority::broker_administrator(),
        }
    }

    pub(crate) fn list(
        &self,
        path: &str,
        recursive: bool,
        max_depth: u32,
        max_entries: usize,
    ) -> Result<FilesystemListResult, FilesystemError> {
        validate_walk_bounds(max_depth, max_entries)?;
        let root = self.authority.resolve_existing(path).map_err(map_path_error)?;
        if !root.is_dir() || metadata_is_reparse(&fs::symlink_metadata(&root).map_err(|_| FilesystemError::Io)?) {
            return Err(FilesystemError::InvalidArgument);
        }
        let depth = if recursive { max_depth } else { 1 };
        self.walk(&root, depth, max_entries)
    }

    pub(crate) fn stat(
        &self,
        path: &str,
        calculate_size: bool,
        max_depth: u32,
        max_entries: usize,
    ) -> Result<FilesystemStatResult, FilesystemError> {
        let target = self.authority.resolve_existing(path).map_err(map_path_error)?;
        let metadata = fs::symlink_metadata(&target).map_err(|_| FilesystemError::Io)?;
        if metadata_is_reparse(&metadata) {
            return Err(FilesystemError::OutsideAuthority);
        }
        let mut size = metadata.len();
        let mut scanned_entries = 0usize;
        let mut truncated = false;
        if calculate_size && metadata.is_dir() {
            validate_walk_bounds(max_depth, max_entries)?;
            let walked = self.walk(&target, max_depth, max_entries)?;
            size = walked
                .entries
                .iter()
                .filter(|entry| entry.kind == "file")
                .map(|entry| entry.size)
                .sum();
            scanned_entries = walked.scanned_entries;
            truncated = walked.truncated;
        }
        Ok(FilesystemStatResult {
            path: self.display_path(&target)?,
            kind: metadata_kind(&metadata),
            size,
            modified_ms: modified_ms(&metadata),
            calculated_size: calculate_size && metadata.is_dir(),
            scanned_entries,
            truncated,
        })
    }

    pub(crate) fn read(
        &self,
        path: &str,
        offset: u64,
        max_bytes: usize,
    ) -> Result<FilesystemReadResult, FilesystemError> {
        if max_bytes == 0 || max_bytes > MAX_FILESYSTEM_READ_BYTES {
            return Err(FilesystemError::LimitExceeded);
        }
        let target = self.authority.resolve_existing(path).map_err(map_path_error)?;
        let metadata = fs::symlink_metadata(&target).map_err(|_| FilesystemError::Io)?;
        if !metadata.is_file() || metadata_is_reparse(&metadata) {
            return Err(FilesystemError::InvalidArgument);
        }
        let total_bytes = metadata.len();
        if offset > total_bytes {
            return Err(FilesystemError::InvalidArgument);
        }
        let mut file = File::open(&target).map_err(|_| FilesystemError::Io)?;
        file.seek(SeekFrom::Start(offset)).map_err(|_| FilesystemError::Io)?;
        let remaining = total_bytes.saturating_sub(offset).min(max_bytes as u64) as usize;
        let mut bytes = vec![0u8; remaining];
        file.read_exact(&mut bytes).map_err(|_| FilesystemError::Io)?;
        let (encoding, content) = match std::str::from_utf8(&bytes) {
            Ok(text) => ("utf8", text.to_string()),
            Err(_) => ("base64", STANDARD.encode(&bytes)),
        };
        Ok(FilesystemReadResult {
            path: self.display_path(&target)?,
            offset,
            total_bytes,
            returned_bytes: bytes.len(),
            eof: offset.saturating_add(bytes.len() as u64) >= total_bytes,
            encoding,
            content,
        })
    }

    pub(crate) fn write(
        &self,
        path: &str,
        content: &[u8],
        overwrite: bool,
    ) -> Result<FilesystemMutationResult, FilesystemError> {
        if content.len() > MAX_FILESYSTEM_READ_BYTES {
            return Err(FilesystemError::LimitExceeded);
        }
        let target = self.authority.resolve_missing_leaf(path).map_err(map_path_error)?;
        let existing = fs::symlink_metadata(&target).ok();
        if let Some(metadata) = &existing {
            if metadata_is_reparse(metadata) || !metadata.is_file() {
                return Err(FilesystemError::OutsideAuthority);
            }
            if !overwrite {
                return Err(FilesystemError::AlreadyExists);
            }
            self.authority
                .revalidate_opened_path(&target)
                .map_err(map_path_error)?;
        }
        let parent = self
            .authority
            .revalidate_parent(&target)
            .map_err(map_path_error)?;
        let temp = sibling_temp_path(&parent, &target)?;
        let result = (|| {
            write_new_synced(&temp, content)?;
            let final_parent = self
                .authority
                .revalidate_parent(&target)
                .map_err(map_path_error)?;
            if final_parent != parent {
                return Err(FilesystemError::OutsideAuthority);
            }
            if target.exists() {
                self.authority
                    .revalidate_opened_path(&target)
                    .map_err(map_path_error)?;
            }
            atomic_replace(&temp, &target, overwrite)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result?;
        let final_target = self
            .authority
            .revalidate_opened_path(&target)
            .map_err(map_path_error)?;
        Ok(FilesystemMutationResult {
            path: self.display_path(&final_target)?,
            destination: None,
            bytes: content.len() as u64,
            changed: true,
        })
    }

    pub(crate) fn search(
        &self,
        path: &str,
        options: &FilesystemSearchOptions,
    ) -> Result<FilesystemListResult, FilesystemError> {
        validate_walk_bounds(options.max_depth, options.max_entries)?;
        if options.max_results == 0 || options.max_results > MAX_FILESYSTEM_RESULTS {
            return Err(FilesystemError::LimitExceeded);
        }
        if options.pattern.is_empty()
            || !matches!(options.sort_by.as_str(), "path" | "size" | "modified")
            || !matches!(options.sort_order.as_str(), "asc" | "desc")
            || options.kind.as_deref().is_some_and(|kind| !matches!(kind, "file" | "directory"))
        {
            return Err(FilesystemError::InvalidArgument);
        }
        let depth = if options.recursive { options.max_depth } else { 1 };
        let mut walked = self.list(path, options.recursive, depth, options.max_entries)?;
        walked.entries.retain(|entry| {
            let name = Path::new(&entry.path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            wildcard_match(&options.pattern, name)
                && options.kind.as_deref().is_none_or(|kind| {
                    (kind == "file" && entry.kind == "file")
                        || (kind == "directory" && entry.kind == "directory")
                })
                && options.min_size.is_none_or(|min| entry.size >= min)
                && options.max_size.is_none_or(|max| entry.size <= max)
                && options
                    .modified_after_ms
                    .is_none_or(|min| entry.modified_ms.is_some_and(|value| value >= min))
                && options
                    .modified_before_ms
                    .is_none_or(|max| entry.modified_ms.is_some_and(|value| value <= max))
        });
        match options.sort_by.as_str() {
            "size" => walked.entries.sort_by_key(|entry| (entry.size, entry.path.clone())),
            "modified" => walked
                .entries
                .sort_by_key(|entry| (entry.modified_ms.unwrap_or(0), entry.path.clone())),
            _ => walked.entries.sort_by(|a, b| a.path.cmp(&b.path)),
        }
        if options.sort_order == "desc" {
            walked.entries.reverse();
        }
        if walked.entries.len() > options.max_results {
            walked.entries.truncate(options.max_results);
            walked.truncated = true;
        }
        Ok(walked)
    }

    pub(crate) fn copy(
        &self,
        source: &str,
        destination: &str,
        recursive: bool,
        overwrite: bool,
        max_depth: u32,
        max_entries: usize,
    ) -> Result<FilesystemMutationResult, FilesystemError> {
        validate_walk_bounds(max_depth, max_entries)?;
        let source_path = self.authority.resolve_existing(source).map_err(map_path_error)?;
        let metadata = fs::symlink_metadata(&source_path).map_err(|_| FilesystemError::Io)?;
        if metadata_is_reparse(&metadata) {
            return Err(FilesystemError::OutsideAuthority);
        }
        let destination_path = self
            .authority
            .resolve_missing_leaf(destination)
            .map_err(map_path_error)?;
        if metadata.is_file() {
            let bytes = self.copy_file_atomic(&source_path, &destination_path, overwrite)?;
            let destination_final = self
                .authority
                .resolve_existing(destination)
                .map_err(map_path_error)?;
            if sha256_file(&source_path)? != sha256_file(&destination_final)? {
                return Err(FilesystemError::Io);
            }
            return Ok(FilesystemMutationResult {
                path: self.display_path(&source_path)?,
                destination: Some(self.display_path(&destination_final)?),
                bytes,
                changed: true,
            });
        }
        if !metadata.is_dir() || !recursive || destination_path.exists() {
            return Err(if destination_path.exists() {
                FilesystemError::AlreadyExists
            } else {
                FilesystemError::InvalidArgument
            });
        }
        self.authority
            .revalidate_parent(&destination_path)
            .map_err(map_path_error)?;
        fs::create_dir(&destination_path).map_err(|_| FilesystemError::Io)?;
        let copied = self.copy_directory_tree(
            &source_path,
            &destination_path,
            max_depth,
            max_entries,
        );
        if copied.is_err() {
            let _ = fs::remove_dir_all(&destination_path);
        }
        let bytes = copied?;
        let destination_final = self
            .authority
            .resolve_existing(destination)
            .map_err(map_path_error)?;
        if tree_manifest(&source_path, max_depth, max_entries)?
            != tree_manifest(&destination_final, max_depth, max_entries)?
        {
            return Err(FilesystemError::Io);
        }
        Ok(FilesystemMutationResult {
            path: self.display_path(&source_path)?,
            destination: Some(self.display_path(&destination_final)?),
            bytes,
            changed: true,
        })
    }

    fn copy_file_atomic(
        &self,
        source: &Path,
        target: &Path,
        overwrite: bool,
    ) -> Result<u64, FilesystemError> {
        let source = self
            .authority
            .revalidate_opened_path(source)
            .map_err(map_path_error)?;
        let source_metadata = fs::symlink_metadata(&source).map_err(|_| FilesystemError::Io)?;
        if !source_metadata.is_file() || metadata_is_reparse(&source_metadata) {
            return Err(FilesystemError::InvalidArgument);
        }
        if let Ok(metadata) = fs::symlink_metadata(target) {
            if metadata_is_reparse(&metadata) || !metadata.is_file() {
                return Err(FilesystemError::OutsideAuthority);
            }
            if !overwrite {
                return Err(FilesystemError::AlreadyExists);
            }
            self.authority
                .revalidate_opened_path(target)
                .map_err(map_path_error)?;
        }
        let parent = self.authority.revalidate_parent(target).map_err(map_path_error)?;
        let temp = sibling_temp_path(&parent, target)?;
        let result = (|| {
            let mut input = File::open(&source).map_err(|_| FilesystemError::Io)?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(0);
            }
            let mut output = options.open(&temp).map_err(|_| FilesystemError::Io)?;
            let bytes = std::io::copy(&mut input, &mut output).map_err(|_| FilesystemError::Io)?;
            output.sync_all().map_err(|_| FilesystemError::Io)?;
            drop(output);
            let final_parent = self.authority.revalidate_parent(target).map_err(map_path_error)?;
            if final_parent != parent {
                return Err(FilesystemError::OutsideAuthority);
            }
            if target.exists() {
                self.authority
                    .revalidate_opened_path(target)
                    .map_err(map_path_error)?;
            }
            atomic_replace(&temp, target, overwrite)?;
            Ok(bytes)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    pub(crate) fn move_path(
        &self,
        source: &str,
        destination: &str,
        recursive: bool,
        overwrite: bool,
        max_depth: u32,
        max_entries: usize,
    ) -> Result<FilesystemMutationResult, FilesystemError> {
        validate_walk_bounds(max_depth, max_entries)?;
        let source_path = self.authority.resolve_existing(source).map_err(map_path_error)?;
        let source_metadata = fs::symlink_metadata(&source_path).map_err(|_| FilesystemError::Io)?;
        if metadata_is_reparse(&source_metadata) {
            return Err(FilesystemError::OutsideAuthority);
        }
        let destination_path = self
            .authority
            .resolve_missing_leaf(destination)
            .map_err(map_path_error)?;
        if source_metadata.is_dir() && !recursive {
            return Err(FilesystemError::InvalidArgument);
        }
        if destination_path.exists() && !overwrite {
            return Err(FilesystemError::AlreadyExists);
        }
        if source_metadata.is_dir() && destination_path.exists() {
            return Err(FilesystemError::AlreadyExists);
        }
        self.authority
            .revalidate_opened_path(&source_path)
            .map_err(map_path_error)?;
        self.authority
            .revalidate_parent(&destination_path)
            .map_err(map_path_error)?;
        match rename_path(&source_path, &destination_path, overwrite) {
            Ok(()) => {
                let final_destination = self
                    .authority
                    .revalidate_opened_path(&destination_path)
                    .map_err(map_path_error)?;
                return Ok(FilesystemMutationResult {
                    path: source.replace('\\', "/"),
                    destination: Some(self.display_path(&final_destination)?),
                    bytes: if source_metadata.is_file() { source_metadata.len() } else { 0 },
                    changed: true,
                });
            }
            Err(error) if is_cross_volume(&error) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(FilesystemError::AlreadyExists);
            }
            Err(_) => return Err(FilesystemError::Io),
        }
        let copied = self.copy(
            source,
            destination,
            recursive,
            overwrite,
            max_depth,
            max_entries,
        )?;
        let destination_final = self
            .authority
            .resolve_existing(destination)
            .map_err(map_path_error)?;
        let verified = if source_metadata.is_file() {
            sha256_file(&source_path)? == sha256_file(&destination_final)?
        } else {
            tree_manifest(&source_path, max_depth, max_entries)?
                == tree_manifest(&destination_final, max_depth, max_entries)?
        };
        if !verified {
            return Err(FilesystemError::Io);
        }
        self.delete(source, recursive, max_depth, max_entries)?;
        Ok(copied)
    }

    pub(crate) fn delete(
        &self,
        path: &str,
        recursive: bool,
        max_depth: u32,
        max_entries: usize,
    ) -> Result<FilesystemMutationResult, FilesystemError> {
        validate_walk_bounds(max_depth, max_entries)?;
        let target = self.authority.resolve_existing(path).map_err(map_path_error)?;
        if self.authority.scope() == PathAuthorityScope::ActiveWorkspace
            && self.authority.canonical_root() == Some(target.as_path())
        {
            return Err(FilesystemError::OutsideAuthority);
        }
        let metadata = fs::symlink_metadata(&target).map_err(|_| FilesystemError::Io)?;
        if metadata_is_reparse(&metadata) {
            return Err(FilesystemError::OutsideAuthority);
        }
        self.authority
            .revalidate_opened_path(&target)
            .map_err(map_path_error)?;
        self.authority
            .revalidate_parent(&target)
            .map_err(map_path_error)?;
        if metadata.is_dir() {
            if recursive {
                let walked = self.walk(&target, max_depth, max_entries)?;
                if walked.truncated {
                    return Err(FilesystemError::LimitExceeded);
                }
                fs::remove_dir_all(&target).map_err(|_| FilesystemError::Io)?;
            } else {
                fs::remove_dir(&target).map_err(|error| {
                    if error.kind() == std::io::ErrorKind::DirectoryNotEmpty {
                        FilesystemError::InvalidArgument
                    } else {
                        FilesystemError::Io
                    }
                })?;
            }
        } else if metadata.is_file() {
            fs::remove_file(&target).map_err(|_| FilesystemError::Io)?;
        } else {
            return Err(FilesystemError::Unsupported);
        }
        Ok(FilesystemMutationResult {
            path: path.replace('\\', "/"),
            destination: None,
            bytes: metadata.len(),
            changed: true,
        })
    }

    pub(crate) fn hash(&self, path: &str) -> Result<FilesystemHashResult, FilesystemError> {
        let target = self.authority.resolve_existing(path).map_err(map_path_error)?;
        let metadata = fs::symlink_metadata(&target).map_err(|_| FilesystemError::Io)?;
        if !metadata.is_file() || metadata_is_reparse(&metadata) {
            return Err(FilesystemError::InvalidArgument);
        }
        Ok(FilesystemHashResult {
            path: self.display_path(&target)?,
            algorithm: "sha256",
            sha256: sha256_file(&target)?,
            bytes: metadata.len(),
        })
    }

    pub(crate) fn create_directory(
        &self,
        path: &str,
    ) -> Result<FilesystemMutationResult, FilesystemError> {
        let target = self.authority.resolve_missing_leaf(path).map_err(map_path_error)?;
        if target.exists() {
            return Err(FilesystemError::AlreadyExists);
        }
        self.authority
            .revalidate_parent(&target)
            .map_err(map_path_error)?;
        fs::create_dir(&target).map_err(|_| FilesystemError::Io)?;
        let final_target = match self.authority.revalidate_opened_path(&target) {
            Ok(value) => value,
            Err(error) => {
                let _ = fs::remove_dir(&target);
                return Err(map_path_error(error));
            }
        };
        Ok(FilesystemMutationResult {
            path: self.display_path(&final_target)?,
            destination: None,
            bytes: 0,
            changed: true,
        })
    }

    pub(crate) fn remove_empty_directory(
        &self,
        path: &str,
    ) -> Result<FilesystemMutationResult, FilesystemError> {
        let target = self.authority.resolve_existing(path).map_err(map_path_error)?;
        if self.authority.scope() == PathAuthorityScope::ActiveWorkspace
            && self.authority.canonical_root() == Some(target.as_path())
        {
            return Err(FilesystemError::OutsideAuthority);
        }
        let metadata = fs::symlink_metadata(&target).map_err(|_| FilesystemError::Io)?;
        if !metadata.is_dir() || metadata_is_reparse(&metadata) {
            return Err(FilesystemError::InvalidArgument);
        }
        if fs::read_dir(&target)
            .map_err(|_| FilesystemError::Io)?
            .next()
            .is_some()
        {
            return Err(FilesystemError::InvalidArgument);
        }
        self.authority
            .revalidate_opened_path(&target)
            .map_err(map_path_error)?;
        self.authority
            .revalidate_parent(&target)
            .map_err(map_path_error)?;
        fs::remove_dir(&target).map_err(|_| FilesystemError::Io)?;
        Ok(FilesystemMutationResult {
            path: path.replace('\\', "/"),
            destination: None,
            bytes: 0,
            changed: true,
        })
    }

    fn walk(
        &self,
        root: &Path,
        max_depth: u32,
        max_entries: usize,
    ) -> Result<FilesystemListResult, FilesystemError> {
        let mut entries = Vec::new();
        let mut stack = vec![(root.to_path_buf(), 0u32)];
        let mut scanned_entries = 0usize;
        let mut truncated = false;
        while let Some((directory, depth)) = stack.pop() {
            if depth >= max_depth {
                continue;
            }
            let mut children = fs::read_dir(&directory)
                .map_err(|_| FilesystemError::Io)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| FilesystemError::Io)?;
            children.sort_by_key(|entry| entry.file_name());
            for child in children {
                if scanned_entries >= max_entries {
                    truncated = true;
                    break;
                }
                scanned_entries += 1;
                let path = child.path();
                let metadata = fs::symlink_metadata(&path).map_err(|_| FilesystemError::Io)?;
                let reparse = metadata_is_reparse(&metadata);
                if !reparse {
                    self.authority
                        .revalidate_opened_path(&path)
                        .map_err(map_path_error)?;
                }
                let kind = if reparse { "reparse" } else { metadata_kind(&metadata) };
                entries.push(FilesystemEntry {
                    path: self.display_path(&path)?,
                    kind,
                    size: metadata.len(),
                    modified_ms: modified_ms(&metadata),
                });
                if !reparse && metadata.is_dir() && depth + 1 < max_depth {
                    stack.push((path, depth + 1));
                }
            }
            if truncated {
                break;
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(FilesystemListResult {
            entries,
            scanned_entries,
            truncated,
        })
    }

    fn copy_directory_tree(
        &self,
        source: &Path,
        destination: &Path,
        max_depth: u32,
        max_entries: usize,
    ) -> Result<u64, FilesystemError> {
        let walked = self.walk(source, max_depth, max_entries)?;
        if walked.truncated {
            return Err(FilesystemError::LimitExceeded);
        }
        let source_display = self.display_path(source)?;
        let destination_display = self.display_path(destination)?;
        let mut bytes = 0u64;
        for entry in walked.entries.iter().filter(|entry| entry.kind == "directory") {
            let relative = relative_from_display(&source_display, &entry.path)?;
            let target = join_display(&destination_display, &relative);
            self.create_directory(&target)?;
        }
        for entry in walked.entries.iter().filter(|entry| entry.kind == "file") {
            let relative = relative_from_display(&source_display, &entry.path)?;
            let source_file = join_display(&source_display, &relative);
            let destination_file = join_display(&destination_display, &relative);
            let source_path = self
                .authority
                .resolve_existing(&source_file)
                .map_err(map_path_error)?;
            let destination_path = self
                .authority
                .resolve_missing_leaf(&destination_file)
                .map_err(map_path_error)?;
            bytes = bytes.saturating_add(self.copy_file_atomic(
                &source_path,
                &destination_path,
                false,
            )?);
        }
        if walked.entries.iter().any(|entry| entry.kind == "reparse") {
            return Err(FilesystemError::OutsideAuthority);
        }
        Ok(bytes)
    }

    fn display_path(&self, path: &Path) -> Result<String, FilesystemError> {
        self.authority.display_path(path).map_err(map_path_error)
    }
}

fn validate_walk_bounds(max_depth: u32, max_entries: usize) -> Result<(), FilesystemError> {
    if max_depth == 0
        || max_depth > MAX_FILESYSTEM_DEPTH
        || max_entries == 0
        || max_entries > MAX_FILESYSTEM_ENTRIES
    {
        return Err(FilesystemError::LimitExceeded);
    }
    Ok(())
}

fn map_path_error(error: PathAuthorityError) -> FilesystemError {
    match error {
        PathAuthorityError::InvalidPath => FilesystemError::InvalidArgument,
        PathAuthorityError::NotFound => FilesystemError::NotFound,
        PathAuthorityError::OutsideAuthority => FilesystemError::OutsideAuthority,
    }
}

fn metadata_kind(metadata: &Metadata) -> &'static str {
    if metadata.is_file() {
        "file"
    } else if metadata.is_dir() {
        "directory"
    } else if metadata.file_type().is_symlink() {
        "symlink"
    } else {
        "other"
    }
}

#[cfg(windows)]
fn metadata_is_reparse(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn modified_ms(metadata: &Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|value| value.as_millis().min(u64::MAX as u128) as u64)
}

fn sibling_temp_path(parent: &Path, target: &Path) -> Result<PathBuf, FilesystemError> {
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or(FilesystemError::InvalidArgument)?;
    let generation = TEMP_GENERATION.fetch_add(1, Ordering::Relaxed);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(parent.join(format!(
        ".{name}.localbridge-{}-{generation:x}-{nonce:x}.tmp",
        std::process::id()
    )))
}

fn write_new_synced(path: &Path, content: &[u8]) -> Result<(), FilesystemError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let mut file = options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            FilesystemError::AlreadyExists
        } else {
            FilesystemError::Io
        }
    })?;
    if file.write_all(content).and_then(|_| file.sync_all()).is_err() {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(FilesystemError::Io);
    }
    Ok(())
}

#[cfg(windows)]
fn atomic_replace(temp: &Path, target: &Path, overwrite: bool) -> Result<(), FilesystemError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let temp = temp
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let flags = MOVEFILE_WRITE_THROUGH
        | if overwrite {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    if unsafe { MoveFileExW(temp.as_ptr(), target.as_ptr(), flags) } == 0 {
        let error = std::io::Error::last_os_error();
        return Err(if error.kind() == std::io::ErrorKind::AlreadyExists {
            FilesystemError::AlreadyExists
        } else {
            FilesystemError::Io
        });
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(temp: &Path, target: &Path, overwrite: bool) -> Result<(), FilesystemError> {
    if !overwrite && target.exists() {
        return Err(FilesystemError::AlreadyExists);
    }
    fs::rename(temp, target).map_err(|_| FilesystemError::Io)
}

#[cfg(windows)]
fn rename_path(source: &Path, destination: &Path, overwrite: bool) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MoveFileExW};
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let flags = if overwrite { MOVEFILE_REPLACE_EXISTING } else { 0 };
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn rename_path(source: &Path, destination: &Path, overwrite: bool) -> std::io::Result<()> {
    if !overwrite && destination.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "destination exists",
        ));
    }
    fs::rename(source, destination)
}

fn is_cross_volume(error: &std::io::Error) -> bool {
    #[cfg(windows)]
    {
        error.raw_os_error() == Some(17)
    }
    #[cfg(not(windows))]
    {
        error.raw_os_error() == Some(18)
    }
}

fn sha256_file(path: &Path) -> Result<String, FilesystemError> {
    let mut file = File::open(path).map_err(|_| FilesystemError::Io)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| FilesystemError::Io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

type TreeManifestEntry = (String, &'static str, u64, Option<String>);

fn tree_manifest(
    root: &Path,
    max_depth: u32,
    max_entries: usize,
) -> Result<Vec<TreeManifestEntry>, FilesystemError> {
    validate_walk_bounds(max_depth, max_entries)?;
    let root = fs::canonicalize(root).map_err(|_| FilesystemError::NotFound)?;
    let mut manifest = Vec::new();
    let mut stack = vec![(root.clone(), 0u32)];
    let mut scanned = 0usize;
    while let Some((directory, depth)) = stack.pop() {
        if depth >= max_depth {
            continue;
        }
        let mut children = fs::read_dir(&directory)
            .map_err(|_| FilesystemError::Io)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| FilesystemError::Io)?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            if scanned >= max_entries {
                return Err(FilesystemError::LimitExceeded);
            }
            scanned += 1;
            let path = child.path();
            let metadata = fs::symlink_metadata(&path).map_err(|_| FilesystemError::Io)?;
            if metadata_is_reparse(&metadata) {
                return Err(FilesystemError::OutsideAuthority);
            }
            let relative = path
                .strip_prefix(&root)
                .map_err(|_| FilesystemError::OutsideAuthority)?
                .to_string_lossy()
                .replace('\\', "/");
            if metadata.is_file() {
                manifest.push((relative, "file", metadata.len(), Some(sha256_file(&path)?)));
            } else if metadata.is_dir() {
                manifest.push((relative, "directory", 0, None));
                if depth + 1 < max_depth {
                    stack.push((path, depth + 1));
                }
            } else {
                return Err(FilesystemError::Unsupported);
            }
        }
    }
    manifest.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(manifest)
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase().into_bytes();
    let value = value.to_ascii_lowercase().into_bytes();
    let (mut p, mut v, mut star, mut checkpoint) = (0usize, 0usize, None, 0usize);
    while v < value.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            checkpoint = v;
        } else if let Some(star_index) = star {
            p = star_index + 1;
            checkpoint += 1;
            v = checkpoint;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

fn relative_from_display(root: &str, child: &str) -> Result<String, FilesystemError> {
    if root == "." {
        return Ok(child.to_string());
    }
    child
        .strip_prefix(root)
        .and_then(|value| value.strip_prefix('/'))
        .map(str::to_string)
        .ok_or(FilesystemError::OutsideAuthority)
}

fn join_display(root: &str, relative: &str) -> String {
    if root == "." {
        relative.to_string()
    } else if relative.is_empty() {
        root.to_string()
    } else {
        format!("{}/{}", root.trim_end_matches('/'), relative)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "localbridge-filesystem-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn bounded_binary_read_uses_base64_and_offsets() {
        let root = workspace("read");
        fs::write(root.join("binary.bin"), [0xff, 0x00, 0x80, 0x41]).unwrap();
        let service = FilesystemService::active_workspace(&root).unwrap();
        let first = service.read("binary.bin", 0, 2).unwrap();
        assert_eq!(first.encoding, "base64");
        assert_eq!(STANDARD.decode(first.content).unwrap(), [0xff, 0x00]);
        assert!(!first.eof);
        let second = service.read("binary.bin", 2, 2).unwrap();
        assert_eq!(STANDARD.decode(second.content).unwrap(), [0x80, 0x41]);
        assert!(second.eof);
        assert_eq!(service.read("binary.bin", 0, MAX_FILESYSTEM_READ_BYTES + 1), Err(FilesystemError::LimitExceeded));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn atomic_overwrite_and_sha256_are_stable() {
        let root = workspace("write");
        fs::write(root.join("a.txt"), b"old").unwrap();
        let service = FilesystemService::active_workspace(&root).unwrap();
        assert_eq!(service.write("a.txt", b"new", false), Err(FilesystemError::AlreadyExists));
        service.write("a.txt", b"new", true).unwrap();
        assert_eq!(fs::read(root.join("a.txt")).unwrap(), b"new");
        assert_eq!(service.hash("a.txt").unwrap().sha256, "11507a0e2f5e69d5dfa40a62a1bd7b6ee57e6bcd85c67c9b8431b36fff21c437");
        assert!(!fs::read_dir(&root).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().contains(".localbridge-")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn filename_search_and_list_bounds_are_enforced() {
        let root = workspace("search");
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("alpha.txt"), b"a").unwrap();
        fs::write(root.join("beta.bin"), b"bb").unwrap();
        fs::write(root.join("sub").join("gamma.txt"), b"ccc").unwrap();
        let service = FilesystemService::active_workspace(&root).unwrap();
        let found = service.search(".", &FilesystemSearchOptions { pattern: "*.txt".into(), ..Default::default() }).unwrap();
        assert_eq!(found.entries.iter().map(|entry| entry.path.as_str()).collect::<Vec<_>>(), vec!["alpha.txt", "sub/gamma.txt"]);
        let bounded = service.list(".", true, 8, 2).unwrap();
        assert_eq!(bounded.scanned_entries, 2);
        assert!(bounded.truncated);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn internal_directory_changes_share_same_authority() {
        let root = workspace("directories");
        let service = FilesystemService::active_workspace(&root).unwrap();
        service.create_directory("child").unwrap();
        assert!(root.join("child").is_dir());
        service.remove_empty_directory("child").unwrap();
        assert!(!root.join("child").exists());
        assert_eq!(service.remove_empty_directory("."), Err(FilesystemError::OutsideAuthority));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn copy_streams_files_larger_than_public_read_bound() {
        let root = workspace("large-copy");
        let payload = vec![0x5a; MAX_FILESYSTEM_READ_BYTES + 8192];
        fs::write(root.join("large.bin"), &payload).unwrap();
        let service = FilesystemService::active_workspace(&root).unwrap();
        let copied = service
            .copy("large.bin", "large-copy.bin", false, false, 8, 100)
            .unwrap();
        assert_eq!(copied.bytes, payload.len() as u64);
        assert_eq!(fs::read(root.join("large-copy.bin")).unwrap(), payload);
        assert_eq!(
            service.hash("large.bin").unwrap().sha256,
            service.hash("large-copy.bin").unwrap().sha256
        );
        fs::remove_dir_all(root).unwrap();
    }
}
