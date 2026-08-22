use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::filesystem::service::{
    FilesystemCancellation, FilesystemError, FilesystemMutationResult, FilesystemSearchOptions,
    FilesystemService,
};

use super::{
    AdministratorFilesystemAction, AdministratorFilesystemEntry, AdministratorFilesystemErrorCode,
    AdministratorFilesystemKind, AdministratorFilesystemResult, AdministratorFilesystemSortBy,
    AdministratorFilesystemSortOrder, AdministratorFilesystemSpec, AdministratorWorkspacePathField,
    PrivilegedFilesystemAction, PrivilegedFilesystemResult, PrivilegedFilesystemSpec,
};

pub(crate) fn run_privileged_filesystem(
    spec: PrivilegedFilesystemSpec,
) -> Result<PrivilegedFilesystemResult, ()> {
    spec.validate().map_err(|_| ())?;
    let action = spec.action;
    let path = spec.path.clone();
    let service = FilesystemService::broker_administrator();
    match action {
        PrivilegedFilesystemAction::ReadFile => {
            let result = service
                .read(&path, 0, super::MAX_PRIVILEGED_FILE_BYTES)
                .map_err(|_| ())?;
            if !result.eof || result.returned_bytes > super::MAX_PRIVILEGED_FILE_BYTES {
                return Err(());
            }
            let bytes = match result.encoding.to_string().as_str() {
                "base64" => STANDARD.decode(result.content).map_err(|_| ())?,
                "utf8" => result.content.into_bytes(),
                _ => return Err(()),
            };
            Ok(PrivilegedFilesystemResult {
                action,
                path,
                destination: None,
                content_base64: Some(STANDARD.encode(&bytes)),
                bytes: bytes.len() as u32,
            })
        }
        PrivilegedFilesystemAction::WriteFile => {
            let encoded = spec.content_base64.as_deref().ok_or(())?;
            let bytes = STANDARD.decode(encoded).map_err(|_| ())?;
            if bytes.len() > super::MAX_PRIVILEGED_FILE_BYTES {
                return Err(());
            }
            service.write(&path, &bytes, true).map_err(|_| ())?;
            Ok(PrivilegedFilesystemResult {
                action,
                path,
                destination: None,
                content_base64: None,
                bytes: bytes.len() as u32,
            })
        }
        PrivilegedFilesystemAction::CreateDirectory => {
            create_legacy_directory_all(&service, &path)?;
            Ok(PrivilegedFilesystemResult {
                action,
                path,
                destination: None,
                content_base64: None,
                bytes: 0,
            })
        }
        PrivilegedFilesystemAction::Rename => {
            let destination = spec.destination.clone().ok_or(())?;
            let metadata = service.stat(&path, false, 1, 1).map_err(|_| ())?;
            service
                .move_path(
                    &path,
                    &destination,
                    metadata.kind == "directory",
                    false,
                    64,
                    100_000,
                )
                .map_err(|_| ())?;
            Ok(PrivilegedFilesystemResult {
                action,
                path,
                destination: Some(destination),
                content_base64: None,
                bytes: 0,
            })
        }
        PrivilegedFilesystemAction::Delete => {
            service
                .delete(&path, spec.recursive, 64, 100_000)
                .map_err(|_| ())?;
            Ok(PrivilegedFilesystemResult {
                action,
                path,
                destination: None,
                content_base64: None,
                bytes: 0,
            })
        }
    }
}

fn create_legacy_directory_all(service: &FilesystemService, path: &str) -> Result<(), ()> {
    let mut cursor = std::path::PathBuf::from(path);
    let mut missing = Vec::new();
    loop {
        let current = cursor.to_str().ok_or(())?;
        match service.stat(current, false, 1, 1) {
            Ok(result) if result.kind == "directory" => break,
            Ok(_) => return Err(()),
            Err(FilesystemError::NotFound) => {
                missing.push(cursor.file_name().ok_or(())?.to_os_string());
                if !cursor.pop() {
                    return Err(());
                }
            }
            Err(_) => return Err(()),
        }
    }
    for component in missing.into_iter().rev() {
        cursor.push(component);
        let current = cursor.to_str().ok_or(())?;
        match service.create_directory(current) {
            Ok(_) | Err(FilesystemError::AlreadyExists) => {}
            Err(_) => return Err(()),
        }
    }
    Ok(())
}

pub(crate) fn run_administrator_filesystem(
    spec: AdministratorFilesystemSpec,
) -> Result<AdministratorFilesystemResult, AdministratorFilesystemErrorCode> {
    run_administrator_filesystem_with_cancellation(spec, FilesystemCancellation::default())
}

pub(crate) fn run_administrator_filesystem_with_cancellation(
    spec: AdministratorFilesystemSpec,
    cancellation: FilesystemCancellation,
) -> Result<AdministratorFilesystemResult, AdministratorFilesystemErrorCode> {
    spec.validate()
        .map_err(|_| AdministratorFilesystemErrorCode::InvalidArgument)?;
    let _workspace_guards = administrator_workspace_guards(&spec)?;
    let service = FilesystemService::broker_administrator().with_cancellation(cancellation);
    match spec.action {
        AdministratorFilesystemAction::List => {
            let result = service
                .list(
                    administrator_path(&spec)?,
                    spec.recursive,
                    spec.max_depth,
                    spec.max_entries as usize,
                )
                .map_err(administrator_filesystem_error)?;
            Ok(AdministratorFilesystemResult::Entries {
                action: spec.action,
                entries: result
                    .entries
                    .into_iter()
                    .map(administrator_entry)
                    .collect(),
                scanned_entries: u32::try_from(result.scanned_entries)
                    .map_err(|_| AdministratorFilesystemErrorCode::LimitExceeded)?,
                truncated: result.truncated,
            })
        }
        AdministratorFilesystemAction::Stat => {
            let result = service
                .stat(
                    administrator_path(&spec)?,
                    spec.calculate_size,
                    spec.max_depth,
                    spec.max_entries as usize,
                )
                .map_err(administrator_filesystem_error)?;
            Ok(AdministratorFilesystemResult::Stat {
                path: result.path,
                kind: result.kind.to_string(),
                size: result.size,
                modified_ms: result.modified_ms,
                calculated_size: result.calculated_size,
                scanned_entries: u32::try_from(result.scanned_entries)
                    .map_err(|_| AdministratorFilesystemErrorCode::LimitExceeded)?,
                truncated: result.truncated,
            })
        }
        AdministratorFilesystemAction::Read => {
            let result = service
                .read(
                    administrator_path(&spec)?,
                    spec.offset,
                    spec.max_bytes as usize,
                )
                .map_err(administrator_filesystem_error)?;
            Ok(AdministratorFilesystemResult::Read {
                path: result.path,
                offset: result.offset,
                total_bytes: result.total_bytes,
                returned_bytes: u32::try_from(result.returned_bytes)
                    .map_err(|_| AdministratorFilesystemErrorCode::LimitExceeded)?,
                eof: result.eof,
                encoding: result.encoding.to_string(),
                content: result.content,
            })
        }
        AdministratorFilesystemAction::Write => {
            let bytes = STANDARD
                .decode(
                    spec.content_base64
                        .as_deref()
                        .ok_or(AdministratorFilesystemErrorCode::InvalidArgument)?,
                )
                .map_err(|_| AdministratorFilesystemErrorCode::InvalidArgument)?;
            let result = service
                .write(administrator_path(&spec)?, &bytes, spec.overwrite)
                .map_err(administrator_filesystem_error)?;
            Ok(administrator_mutation(spec.action, result))
        }
        AdministratorFilesystemAction::Search => {
            let options = FilesystemSearchOptions {
                recursive: spec.recursive,
                max_depth: spec.max_depth,
                max_entries: spec.max_entries as usize,
                max_results: spec.max_results as usize,
                pattern: spec
                    .pattern
                    .clone()
                    .ok_or(AdministratorFilesystemErrorCode::InvalidArgument)?,
                kind: spec.kind.map(|kind| match kind {
                    AdministratorFilesystemKind::File => "file".to_string(),
                    AdministratorFilesystemKind::Directory => "directory".to_string(),
                }),
                min_size: spec.min_size,
                max_size: spec.max_size,
                modified_after_ms: spec.modified_after_ms,
                modified_before_ms: spec.modified_before_ms,
                sort_by: match spec.sort_by {
                    AdministratorFilesystemSortBy::Path => "path",
                    AdministratorFilesystemSortBy::Size => "size",
                    AdministratorFilesystemSortBy::Modified => "modified",
                }
                .to_string(),
                sort_order: match spec.sort_order {
                    AdministratorFilesystemSortOrder::Asc => "asc",
                    AdministratorFilesystemSortOrder::Desc => "desc",
                }
                .to_string(),
            };
            let result = service
                .search(administrator_path(&spec)?, &options)
                .map_err(administrator_filesystem_error)?;
            Ok(AdministratorFilesystemResult::Entries {
                action: spec.action,
                entries: result
                    .entries
                    .into_iter()
                    .map(administrator_entry)
                    .collect(),
                scanned_entries: u32::try_from(result.scanned_entries)
                    .map_err(|_| AdministratorFilesystemErrorCode::LimitExceeded)?,
                truncated: result.truncated,
            })
        }
        AdministratorFilesystemAction::Copy | AdministratorFilesystemAction::Move => {
            let source = spec
                .source
                .as_deref()
                .ok_or(AdministratorFilesystemErrorCode::InvalidArgument)?;
            let destination = spec
                .destination
                .as_deref()
                .ok_or(AdministratorFilesystemErrorCode::InvalidArgument)?;
            let result = if spec.action == AdministratorFilesystemAction::Copy {
                service.copy(
                    source,
                    destination,
                    spec.recursive,
                    spec.overwrite,
                    spec.max_depth,
                    spec.max_entries as usize,
                )
            } else {
                service.move_path(
                    source,
                    destination,
                    spec.recursive,
                    spec.overwrite,
                    spec.max_depth,
                    spec.max_entries as usize,
                )
            }
            .map_err(administrator_filesystem_error)?;
            Ok(administrator_mutation(spec.action, result))
        }
        AdministratorFilesystemAction::Delete => {
            let result = service
                .delete(
                    administrator_path(&spec)?,
                    spec.recursive,
                    spec.max_depth,
                    spec.max_entries as usize,
                )
                .map_err(administrator_filesystem_error)?;
            Ok(administrator_mutation(spec.action, result))
        }
        AdministratorFilesystemAction::Hash => {
            let result = service
                .hash(administrator_path(&spec)?)
                .map_err(administrator_filesystem_error)?;
            Ok(AdministratorFilesystemResult::Hash {
                path: result.path,
                algorithm: result.algorithm.to_string(),
                sha256: result.sha256,
                bytes: result.bytes,
            })
        }
    }
}

fn administrator_workspace_guards(
    spec: &AdministratorFilesystemSpec,
) -> Result<Vec<crate::filesystem::service::WorkspacePathGuard>, AdministratorFilesystemErrorCode> {
    let Some(root) = spec.workspace_root.as_deref() else {
        return Ok(Vec::new());
    };
    let identity = spec
        .workspace_identity
        .as_deref()
        .ok_or(AdministratorFilesystemErrorCode::InvalidArgument)?;
    let authority =
        crate::workspace::WorkspaceResolver::active_workspace(std::path::Path::new(root))
            .map_err(|_| AdministratorFilesystemErrorCode::OutsideAuthority)?;
    authority
        .matches_workspace_identity_token(identity)
        .map_err(|_| AdministratorFilesystemErrorCode::OutsideAuthority)?;
    let workspace =
        FilesystemService::from_authority(authority).map_err(administrator_filesystem_error)?;
    let mut guards = Vec::with_capacity(spec.workspace_fields.len());
    for field in &spec.workspace_fields {
        let (path, allow_missing_leaf, allow_target_delete) = match field {
            AdministratorWorkspacePathField::Path => (
                spec.path
                    .as_deref()
                    .ok_or(AdministratorFilesystemErrorCode::InvalidArgument)?,
                spec.action == AdministratorFilesystemAction::Write,
                spec.action == AdministratorFilesystemAction::Delete,
            ),
            AdministratorWorkspacePathField::Source => (
                spec.source
                    .as_deref()
                    .ok_or(AdministratorFilesystemErrorCode::InvalidArgument)?,
                false,
                spec.action == AdministratorFilesystemAction::Move,
            ),
            AdministratorWorkspacePathField::Destination => (
                spec.destination
                    .as_deref()
                    .ok_or(AdministratorFilesystemErrorCode::InvalidArgument)?,
                true,
                false,
            ),
        };
        guards.push(
            workspace
                .pin_workspace_path(path, allow_missing_leaf, allow_target_delete)
                .map_err(administrator_filesystem_error)?,
        );
    }
    Ok(guards)
}

fn administrator_path(
    spec: &AdministratorFilesystemSpec,
) -> Result<&str, AdministratorFilesystemErrorCode> {
    spec.path
        .as_deref()
        .ok_or(AdministratorFilesystemErrorCode::InvalidArgument)
}

fn administrator_filesystem_error(error: FilesystemError) -> AdministratorFilesystemErrorCode {
    match error {
        FilesystemError::InvalidArgument => AdministratorFilesystemErrorCode::InvalidArgument,
        FilesystemError::NotFound => AdministratorFilesystemErrorCode::NotFound,
        FilesystemError::OutsideAuthority => AdministratorFilesystemErrorCode::OutsideAuthority,
        FilesystemError::AlreadyExists => AdministratorFilesystemErrorCode::AlreadyExists,
        FilesystemError::FileChanged => AdministratorFilesystemErrorCode::AlreadyExists,
        FilesystemError::LimitExceeded => AdministratorFilesystemErrorCode::LimitExceeded,
        FilesystemError::Cancelled => AdministratorFilesystemErrorCode::Cancelled,
        FilesystemError::Unsupported => AdministratorFilesystemErrorCode::Unsupported,
        FilesystemError::Io => AdministratorFilesystemErrorCode::Io,
    }
}

fn administrator_entry(
    entry: crate::filesystem::service::FilesystemEntry,
) -> AdministratorFilesystemEntry {
    AdministratorFilesystemEntry {
        path: entry.path,
        kind: entry.kind.to_string(),
        size: entry.size,
        modified_ms: entry.modified_ms,
    }
}

fn administrator_mutation(
    action: AdministratorFilesystemAction,
    result: FilesystemMutationResult,
) -> AdministratorFilesystemResult {
    AdministratorFilesystemResult::Mutation {
        action,
        path: result.path,
        destination: result.destination,
        bytes: result.bytes,
        changed: result.changed,
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "localbridge-privileged-filesystem-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn create_junction(link: &std::path::Path, target: &std::path::Path) {
        let output = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "mklink /J failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn administrator_spec(action: AdministratorFilesystemAction) -> AdministratorFilesystemSpec {
        AdministratorFilesystemSpec {
            action,
            path: None,
            source: None,
            destination: None,
            workspace_root: None,
            workspace_identity: None,
            workspace_fields: Vec::new(),
            recursive: false,
            max_depth: 16,
            max_entries: 100,
            max_results: 100,
            offset: 0,
            max_bytes: 65_536,
            content_base64: None,
            pattern: None,
            kind: None,
            min_size: None,
            max_size: None,
            modified_after_ms: None,
            modified_before_ms: None,
            sort_by: AdministratorFilesystemSortBy::Path,
            sort_order: AdministratorFilesystemSortOrder::Asc,
            overwrite: false,
            calculate_size: false,
        }
    }

    #[test]
    fn administrator_filesystem_rejects_junction_alias_to_localbridge_control_plane() {
        let root = temp_root();
        let protected = root.join("LocalBridge");
        let alias = root.join("innocent-admin-data");
        fs::create_dir(&protected).unwrap();
        let policy = protected.join("runtime-policy.toml");
        fs::write(&policy, b"protected-sentinel").unwrap();
        create_junction(&alias, &protected);

        let alias_policy = alias.join("runtime-policy.toml");
        let mut write = administrator_spec(AdministratorFilesystemAction::Write);
        write.path = Some(alias_policy.to_string_lossy().into_owned());
        write.content_base64 = Some(STANDARD.encode(b"mutated"));
        write.overwrite = true;
        assert_eq!(
            run_administrator_filesystem(write),
            Err(AdministratorFilesystemErrorCode::OutsideAuthority)
        );

        let mut delete = administrator_spec(AdministratorFilesystemAction::Delete);
        delete.path = Some(alias_policy.to_string_lossy().into_owned());
        assert_eq!(
            run_administrator_filesystem(delete),
            Err(AdministratorFilesystemErrorCode::OutsideAuthority)
        );

        let moved = root.join("moved-policy.toml");
        let mut move_source = administrator_spec(AdministratorFilesystemAction::Move);
        move_source.source = Some(alias_policy.to_string_lossy().into_owned());
        move_source.destination = Some(moved.to_string_lossy().into_owned());
        assert_eq!(
            run_administrator_filesystem(move_source),
            Err(AdministratorFilesystemErrorCode::OutsideAuthority)
        );

        let safe_source = root.join("safe-source.txt");
        fs::write(&safe_source, b"safe").unwrap();
        let mut move_destination = administrator_spec(AdministratorFilesystemAction::Move);
        move_destination.source = Some(safe_source.to_string_lossy().into_owned());
        move_destination.destination =
            Some(alias.join("innocent.txt").to_string_lossy().into_owned());
        assert_eq!(
            run_administrator_filesystem(move_destination),
            Err(AdministratorFilesystemErrorCode::OutsideAuthority)
        );

        assert_eq!(fs::read(&policy).unwrap(), b"protected-sentinel");
        assert!(safe_source.is_file());
        assert!(!moved.exists());
        assert!(!protected.join("innocent.txt").exists());
        fs::remove_dir(&alias).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn structured_privileged_filesystem_supports_binary_roundtrip_rename_and_delete() {
        let root = temp_root();
        let dir = root.join("admin-dir").join("nested");
        let file = dir.join("payload.bin");
        let renamed = dir.join("renamed.bin");
        let payload = b"schema33\0binary";

        run_privileged_filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::CreateDirectory,
            path: dir.to_string_lossy().into_owned(),
            destination: None,
            content_base64: None,
            recursive: false,
        })
        .unwrap();
        run_privileged_filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::CreateDirectory,
            path: dir.to_string_lossy().into_owned(),
            destination: None,
            content_base64: None,
            recursive: false,
        })
        .unwrap();
        run_privileged_filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::WriteFile,
            path: file.to_string_lossy().into_owned(),
            destination: None,
            content_base64: Some(STANDARD.encode(payload)),
            recursive: false,
        })
        .unwrap();
        let read = run_privileged_filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::ReadFile,
            path: file.to_string_lossy().into_owned(),
            destination: None,
            content_base64: None,
            recursive: false,
        })
        .unwrap();
        assert_eq!(
            STANDARD.decode(read.content_base64.unwrap()).unwrap(),
            payload
        );
        run_privileged_filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::Rename,
            path: file.to_string_lossy().into_owned(),
            destination: Some(renamed.to_string_lossy().into_owned()),
            content_base64: None,
            recursive: false,
        })
        .unwrap();
        assert!(renamed.is_file());
        run_privileged_filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::Delete,
            path: dir.to_string_lossy().into_owned(),
            destination: None,
            content_base64: None,
            recursive: true,
        })
        .unwrap();
        assert!(!dir.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn administrator_workspace_binding_rejects_pre_broker_ancestor_reparse_swap() {
        use std::process::Command;

        let root = temp_root();
        let workspace = root.join("workspace");
        let outside = root.join("outside");
        let destination = root.join("copied.txt");
        let safe = workspace.join("safe");
        fs::create_dir_all(&safe).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(safe.join("source.txt"), b"inside").unwrap();
        fs::write(outside.join("source.txt"), b"outside").unwrap();
        let workspace_identity = crate::workspace::WorkspaceResolver::active_workspace(&workspace)
            .unwrap()
            .workspace_identity_token()
            .unwrap();
        fs::remove_dir_all(&safe).unwrap();
        let status = Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&safe)
            .arg(&outside)
            .status()
            .unwrap();
        assert!(status.success());

        let result = run_administrator_filesystem(AdministratorFilesystemSpec {
            action: AdministratorFilesystemAction::Copy,
            path: None,
            source: Some(safe.join("source.txt").to_string_lossy().into_owned()),
            destination: Some(destination.to_string_lossy().into_owned()),
            workspace_root: Some(workspace.to_string_lossy().into_owned()),
            workspace_identity: Some(workspace_identity),
            workspace_fields: vec![AdministratorWorkspacePathField::Source],
            recursive: false,
            max_depth: 16,
            max_entries: 100,
            max_results: 100,
            offset: 0,
            max_bytes: 65_536,
            content_base64: None,
            pattern: None,
            kind: None,
            min_size: None,
            max_size: None,
            modified_after_ms: None,
            modified_before_ms: None,
            sort_by: AdministratorFilesystemSortBy::Path,
            sort_order: AdministratorFilesystemSortOrder::Asc,
            overwrite: false,
            calculate_size: false,
        });
        assert_eq!(
            result,
            Err(AdministratorFilesystemErrorCode::OutsideAuthority)
        );
        assert!(!destination.exists());

        fs::remove_dir(&safe).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn administrator_mutation_rejects_non_sensitive_hardlink_alias_to_control_plane_object() {
        let root = temp_root();
        let protected = root.join("runtime-policy.toml");
        let alias = root.join("notes.txt");
        let moved = root.join("moved.txt");
        let ordinary = root.join("ordinary.txt");
        fs::write(&protected, b"protected-sentinel").unwrap();
        fs::hard_link(&protected, &alias).unwrap();
        fs::write(&ordinary, b"old").unwrap();

        let mut write = administrator_spec(AdministratorFilesystemAction::Write);
        write.path = Some(alias.to_string_lossy().into_owned());
        write.content_base64 = Some(STANDARD.encode(b"mutated"));
        write.overwrite = true;
        assert_eq!(
            run_administrator_filesystem(write),
            Err(AdministratorFilesystemErrorCode::OutsideAuthority)
        );
        assert_eq!(fs::read(&protected).unwrap(), b"protected-sentinel");
        assert_eq!(fs::read(&alias).unwrap(), b"protected-sentinel");

        let mut delete = administrator_spec(AdministratorFilesystemAction::Delete);
        delete.path = Some(alias.to_string_lossy().into_owned());
        assert_eq!(
            run_administrator_filesystem(delete),
            Err(AdministratorFilesystemErrorCode::OutsideAuthority)
        );
        assert!(protected.exists());
        assert!(alias.exists());

        let mut move_alias = administrator_spec(AdministratorFilesystemAction::Move);
        move_alias.source = Some(alias.to_string_lossy().into_owned());
        move_alias.destination = Some(moved.to_string_lossy().into_owned());
        assert_eq!(
            run_administrator_filesystem(move_alias),
            Err(AdministratorFilesystemErrorCode::OutsideAuthority)
        );
        assert!(protected.exists());
        assert!(alias.exists());
        assert!(!moved.exists());

        let mut normal_write = administrator_spec(AdministratorFilesystemAction::Write);
        normal_write.path = Some(ordinary.to_string_lossy().into_owned());
        normal_write.content_base64 = Some(STANDARD.encode(b"new"));
        normal_write.overwrite = true;
        run_administrator_filesystem(normal_write).unwrap();
        assert_eq!(fs::read(&ordinary).unwrap(), b"new");

        fs::remove_dir_all(root).unwrap();
    }
}
