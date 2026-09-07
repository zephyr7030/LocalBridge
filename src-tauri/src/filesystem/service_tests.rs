use super::*;

#[cfg(windows)]
fn create_junction(link: &Path, target: &Path) {
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
    assert_eq!(
        service.read("binary.bin", 0, MAX_FILESYSTEM_READ_BYTES + 1),
        Err(FilesystemError::LimitExceeded)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn atomic_overwrite_and_sha256_are_stable() {
    let root = workspace("write");
    fs::write(root.join("a.txt"), b"old").unwrap();
    let service = FilesystemService::active_workspace(&root).unwrap();
    assert_eq!(
        service.write("a.txt", b"new", false),
        Err(FilesystemError::AlreadyExists)
    );
    service.write("a.txt", b"new", true).unwrap();
    assert_eq!(fs::read(root.join("a.txt")).unwrap(), b"new");
    assert_eq!(
        service.hash("a.txt").unwrap().sha256,
        "11507a0e2f5e69d5dfa40a62a1bd7b6ee57e6bcd85c67c9b8431b36fff21c437"
    );
    assert!(!fs::read_dir(&root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".localbridge-")
    }));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_write_accepts_multi_megabyte_content_without_shell_limits() {
    let root = workspace("large-native-write");
    let service = FilesystemService::active_workspace(&root).unwrap();
    let content = vec![b'x'; 2 * 1024 * 1024];
    let result = service.write("large.txt", &content, false).unwrap();
    assert_eq!(result.bytes, content.len() as u64);
    assert_eq!(
        fs::metadata(root.join("large.txt")).unwrap().len(),
        content.len() as u64
    );
    assert_eq!(
        service.write(
            "too-large.txt",
            &vec![0; MAX_INTERNAL_FILE_BYTES + 1],
            false
        ),
        Err(FilesystemError::LimitExceeded)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn filename_search_and_list_bounds_are_enforced() {
    let root = workspace("search");
    fs::create_dir(root.join("sub")).unwrap();
    fs::write(root.join("alpha.txt"), b"a").unwrap();
    fs::write(root.join("beta.bin"), b"bb").unwrap();
    fs::write(root.join("sub").join("gamma.txt"), b"ccc").unwrap();
    for index in 0..32 {
        fs::write(root.join(format!("noise-{index:02}.bin")), b"x").unwrap();
    }
    let service = FilesystemService::active_workspace(&root).unwrap();
    let found = service
        .search(
            ".",
            &FilesystemSearchOptions {
                pattern: "*.txt".into(),
                recursive: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        found
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha.txt", "sub/gamma.txt"]
    );
    let bounded = service.list(".", true, 8, 2).unwrap();
    assert_eq!(bounded.scanned_entries, 2);
    assert!(bounded.truncated);
    assert_eq!(bounded.entries.len(), 2);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn content_search_is_literal_bounded_and_reports_skipped_files() {
    let root = workspace("content-search");
    fs::create_dir(root.join("sub")).unwrap();
    fs::write(root.join("alpha.txt"), b"Needle here\nsecond needle\n").unwrap();
    fs::write(root.join("sub/nested.txt"), b"nested NEEDLE\n").unwrap();
    fs::write(root.join("binary.bin"), b"needle\0binary").unwrap();
    fs::write(root.join("large.txt"), vec![b'x'; 32]).unwrap();
    let service = FilesystemService::active_workspace(&root).unwrap();

    let found = service
        .search_content(
            ".",
            &FilesystemContentSearchOptions {
                pattern: "needle".into(),
                case_sensitive: false,
                recursive: true,
                max_file_bytes: 30,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(
        found
            .matches
            .iter()
            .map(|item| (item.path.as_str(), item.line, item.column))
            .collect::<Vec<_>>(),
        vec![
            ("alpha.txt", 1, 1),
            ("alpha.txt", 2, 8),
            ("sub/nested.txt", 1, 8)
        ]
    );
    assert_eq!(found.skipped_binary_files, 1);
    assert_eq!(found.skipped_oversized_files, 1);
    assert!(found.truncated);

    let single_file = service
        .search_content(
            "alpha.txt",
            &FilesystemContentSearchOptions {
                pattern: "Needle".into(),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(single_file.matches.len(), 1);
    assert_eq!(single_file.scanned_files, 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn non_recursive_list_does_not_treat_child_contents_as_root_truncation() {
    let root = workspace("non-recursive-list");
    fs::create_dir(root.join(".coding-tools")).unwrap();
    fs::write(root.join(".coding-tools/private.txt"), b"hidden by depth").unwrap();
    fs::create_dir(root.join("LocalBridge")).unwrap();
    for index in 0..13 {
        fs::write(root.join(format!("entry-{index:02}.txt")), b"x").unwrap();
    }
    let service = FilesystemService::active_workspace(&root).unwrap();

    let listed = service.list(".", false, 8, 20).unwrap();
    assert_eq!(listed.scanned_entries, 15);
    assert_eq!(listed.entries.len(), 15);
    assert!(!listed.truncated);

    let found = service
        .search(
            ".",
            &FilesystemSearchOptions {
                pattern: "LocalBridge".into(),
                max_entries: 20,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(found.entries.len(), 1);
    assert_eq!(found.entries[0].path, "LocalBridge");
    assert_eq!(found.scanned_entries, 15);
    assert!(!found.truncated);

    let recursive = service
        .search(
            ".",
            &FilesystemSearchOptions {
                pattern: "LocalBridge".into(),
                recursive: true,
                max_depth: 1,
                max_entries: 20,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(recursive.entries.len(), 1);
    assert_eq!(recursive.entries[0].path, "LocalBridge");
    assert!(
        recursive.truncated,
        "deeper content is truncated without discarding root siblings"
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn active_workspace_volume_root_can_be_enumerated() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let volume_root = manifest
        .ancestors()
        .last()
        .expect("Cargo manifest path has a volume root");
    let service = FilesystemService::active_workspace(volume_root).unwrap();

    let listed = service
        .list(".", false, 1, MAX_FILESYSTEM_ENTRIES)
        .expect("active workspace volume root must be listable");
    let absolute = service
        .list(
            volume_root.to_string_lossy().as_ref(),
            false,
            1,
            MAX_FILESYSTEM_ENTRIES,
        )
        .expect("absolute active workspace volume root must be listable");

    assert!(
        listed.entries.iter().any(|entry| {
            volume_root.join(&entry.path) == manifest
                || manifest.starts_with(volume_root.join(&entry.path))
        }),
        "volume-root listing omitted the repository ancestor: {listed:#?}"
    );
    assert!(
        absolute.entries.iter().any(|entry| {
            volume_root.join(&entry.path) == manifest
                || manifest.starts_with(volume_root.join(&entry.path))
        }),
        "absolute volume-root listing omitted the repository ancestor: {absolute:#?}"
    );

    let repository_ancestor = manifest
        .strip_prefix(volume_root)
        .unwrap()
        .components()
        .next()
        .unwrap()
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    let searched = service
        .search(
            ".",
            &FilesystemSearchOptions {
                pattern: repository_ancestor.clone(),
                max_depth: 1,
                max_entries: MAX_FILESYSTEM_ENTRIES,
                ..Default::default()
            },
        )
        .expect("active workspace volume root must be searchable");
    assert!(
        searched
            .entries
            .iter()
            .any(|entry| entry.path.eq_ignore_ascii_case(&repository_ancestor)),
        "volume-root search omitted {repository_ancestor}: {searched:#?}"
    );
}

#[test]
fn tree_manifest_fails_closed_at_the_scan_bound() {
    let root = workspace("manifest-bound");
    for index in 0..16 {
        fs::write(root.join(format!("entry-{index:02}.txt")), b"x").unwrap();
    }
    assert_eq!(
        tree_manifest(&root, 8, 2, &FilesystemCancellation::default()),
        Err(FilesystemError::LimitExceeded)
    );
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
    assert_eq!(
        service.remove_empty_directory("."),
        Err(FilesystemError::OutsideAuthority)
    );
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

#[test]
fn internal_whole_file_read_is_bounded_before_allocation() {
    let root = workspace("internal-read-limit");
    let path = root.join("large.bin");
    let file = File::create(&path).unwrap();
    file.set_len(MAX_INTERNAL_FILE_BYTES as u64 + 1).unwrap();
    drop(file);
    let service = FilesystemService::active_workspace(&root).unwrap();
    assert_eq!(
        service.read_all_bytes("large.bin"),
        Err(FilesystemError::LimitExceeded)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn recursive_copy_depth_limit_never_returns_partial_success() {
    let root = workspace("copy-depth");
    fs::create_dir_all(root.join("source/sub")).unwrap();
    fs::write(root.join("source/sub/secret.txt"), b"secret").unwrap();
    let service = FilesystemService::active_workspace(&root).unwrap();
    let listed = service.list("source", true, 1, 100).unwrap();
    assert!(listed.truncated);
    assert_eq!(
        service.copy("source", "destination", true, false, 1, 100),
        Err(FilesystemError::LimitExceeded)
    );
    assert!(!root.join("destination").exists());
    assert_eq!(
        fs::read(root.join("source/sub/secret.txt")).unwrap(),
        b"secret"
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn recursive_delete_limit_preflight_failure_changes_nothing() {
    let root = workspace("delete-preflight");
    fs::create_dir(root.join("limit")).unwrap();
    fs::write(root.join("limit/a.txt"), b"a").unwrap();
    fs::write(root.join("limit/b.txt"), b"b").unwrap();
    let service = FilesystemService::active_workspace(&root).unwrap();
    assert_eq!(
        service.delete("limit", true, 8, 1),
        Err(FilesystemError::LimitExceeded)
    );
    assert_eq!(fs::read(root.join("limit/a.txt")).unwrap(), b"a");
    assert_eq!(fs::read(root.join("limit/b.txt")).unwrap(), b"b");

    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn delete_junction_removes_entry_without_touching_referent() {
    let root = workspace("delete-junction-entry");
    let target = root.join("target");
    let link = root.join("link");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep.txt"), b"keep").unwrap();
    create_junction(&link, &target);
    let service = FilesystemService::active_workspace(&root).unwrap();
    service.delete("link", true, 8, 100).unwrap();
    assert!(!link.exists());
    assert_eq!(fs::read(target.join("keep.txt")).unwrap(), b"keep");
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn recursive_delete_treats_junction_as_leaf() {
    let root = workspace("recursive-delete-junction");
    let target = root.join("target");
    let parent = root.join("parent");
    let link = parent.join("link");
    fs::create_dir(&target).unwrap();
    fs::create_dir(&parent).unwrap();
    fs::write(target.join("keep.txt"), b"keep").unwrap();
    fs::write(parent.join("ordinary.txt"), b"ordinary").unwrap();
    create_junction(&link, &target);
    let service = FilesystemService::active_workspace(&root).unwrap();
    service.delete("parent", true, 8, 100).unwrap();
    assert!(!parent.exists());
    assert_eq!(fs::read(target.join("keep.txt")).unwrap(), b"keep");
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn move_junction_renames_entry_without_touching_referent() {
    let root = workspace("move-junction-entry");
    let target = root.join("target");
    let link = root.join("link");
    let moved = root.join("moved-link");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep.txt"), b"keep").unwrap();
    create_junction(&link, &target);
    let service = FilesystemService::active_workspace(&root).unwrap();
    service
        .move_path("link", "moved-link", false, false, 8, 100)
        .unwrap();
    assert!(!link.exists());
    assert!(metadata_is_reparse(&fs::symlink_metadata(&moved).unwrap()));
    assert_eq!(fs::read(target.join("keep.txt")).unwrap(), b"keep");
    fs::remove_dir(&moved).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn validated_parent_handle_closes_the_deterministic_check_then_swap_gap() {
    let root = workspace("toctou-parent");
    let outside = workspace("toctou-parent-outside");
    let safe = root.join("safe");
    let parent = safe.join("parent");
    let outside_parent = outside.join("parent");
    let displaced = root.join("safe-original");
    let target = parent.join("target.txt");
    fs::create_dir_all(&parent).unwrap();
    fs::create_dir(&outside_parent).unwrap();
    let service = FilesystemService::active_workspace(&root).unwrap();

    // A path-only validation result has no lifetime: an ancestor of the
    // checked parent can be replaced by an outside junction immediately
    // after validation.
    let checked = service.authority.revalidate_parent(&target).unwrap();
    assert_eq!(
        checked,
        service.authority.resolve_existing("safe/parent").unwrap()
    );
    fs::rename(&safe, &displaced).unwrap();
    create_junction(&safe, &outside);
    assert_ne!(fs::canonicalize(safe.join("parent")).unwrap(), checked);
    fs::remove_dir(&safe).unwrap();
    fs::rename(&displaced, &safe).unwrap();

    // The mutation primitive pins the whole directory chain without
    // FILE_SHARE_DELETE. The same ancestor swap is rejected while the
    // chain lives.
    let parent_handle = service.open_mutation_parent(&target).unwrap();
    assert!(fs::rename(&safe, &displaced).is_err());

    let temp = parent.join("commit.tmp");
    fs::write(&temp, b"inside").unwrap();
    use windows_sys::Win32::Storage::FileSystem::DELETE;
    let temp_handle = service
        .authority
        .open_validated_handle(&temp, DELETE)
        .unwrap();
    rename_handle_to_path(temp_handle.raw_handle(), &target, false).unwrap();
    drop(temp_handle);
    drop(parent_handle);

    assert_eq!(fs::read(&target).unwrap(), b"inside");
    assert!(!outside.join("target.txt").exists());
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[cfg(windows)]
#[test]
fn validated_source_handle_keeps_move_and_delete_bound_to_the_opened_object() {
    use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_GENERIC_READ};

    let root = workspace("toctou-source");
    let source = root.join("source.txt");
    let displaced = root.join("displaced.txt");
    let moved = root.join("moved.txt");
    fs::write(&source, b"original").unwrap();
    let service = FilesystemService::active_workspace(&root).unwrap();

    let source_handle = service
        .authority
        .open_validated_handle(&source, DELETE | FILE_GENERIC_READ)
        .unwrap();
    assert!(fs::rename(&source, &displaced).is_err());
    rename_handle_to_path(source_handle.raw_handle(), &moved, false).unwrap();
    drop(source_handle);
    assert_eq!(fs::read(&moved).unwrap(), b"original");

    let delete_handle = service
        .authority
        .open_validated_handle(&moved, DELETE)
        .unwrap();
    assert!(fs::rename(&moved, &displaced).is_err());
    delete_raw_handle(delete_handle.raw_handle()).unwrap();
    drop(delete_handle);
    assert!(!moved.exists());
    assert!(!displaced.exists());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn validated_shared_read_handle_blocks_ancestor_swap_and_reads_opened_object() {
    let root = workspace("read-handle-swap");
    let outside = workspace("read-handle-swap-outside");
    let safe = root.join("safe");
    let parent = safe.join("parent");
    let outside_parent = outside.join("parent");
    let displaced = root.join("safe-original");
    fs::create_dir_all(&parent).unwrap();
    fs::create_dir_all(&outside_parent).unwrap();
    fs::write(parent.join("read.txt"), b"inside").unwrap();
    fs::write(outside_parent.join("read.txt"), b"outside").unwrap();
    let service = FilesystemService::active_workspace(&root).unwrap();
    let bytes = service
        .read_all_bytes_with_test_hook("safe/parent/read.txt", || {
            assert!(fs::rename(&safe, &displaced).is_err());
        })
        .unwrap();
    assert_eq!(bytes, b"inside");
    assert_eq!(
        fs::read(outside_parent.join("read.txt")).unwrap(),
        b"outside"
    );
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[cfg(windows)]
#[test]
fn bounded_validated_read_blocks_ancestor_swap_and_enforces_limit() {
    let root = workspace("bounded-read-handle-swap");
    let outside = workspace("bounded-read-handle-swap-outside");
    let safe = root.join("safe");
    let parent = safe.join("parent");
    let outside_parent = outside.join("parent");
    let displaced = root.join("safe-original");
    fs::create_dir_all(&parent).unwrap();
    fs::create_dir_all(&outside_parent).unwrap();
    fs::write(parent.join("read.txt"), b"inside").unwrap();
    fs::write(outside_parent.join("read.txt"), b"outside-secret").unwrap();
    let service = FilesystemService::active_workspace(&root).unwrap();
    let bytes = service
        .read_bytes_bounded_with_test_hook("safe/parent/read.txt", 6, || {
            assert!(fs::rename(&safe, &displaced).is_err());
        })
        .unwrap();
    assert_eq!(bytes, b"inside");
    assert_eq!(
        service.read_bytes_bounded("safe/parent/read.txt", 5),
        Err(FilesystemError::LimitExceeded)
    );
    assert_eq!(
        fs::read(outside_parent.join("read.txt")).unwrap(),
        b"outside-secret"
    );
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[cfg(windows)]
#[test]
fn sensitive_mutations_reject_an_outside_workspace_junction() {
    let root = workspace("toctou-junction");
    let outside = workspace("toctou-junction-outside");
    fs::write(root.join("copy-source.txt"), b"copy").unwrap();
    fs::write(root.join("move-source.txt"), b"move").unwrap();
    fs::write(outside.join("outside.txt"), b"outside").unwrap();
    let link = root.join("escape");
    create_junction(&link, &outside);
    let service = FilesystemService::active_workspace(&root).unwrap();

    assert_eq!(
        service.write("escape/write.txt", b"blocked", true),
        Err(FilesystemError::OutsideAuthority)
    );
    assert_eq!(
        service.copy("copy-source.txt", "escape/copied.txt", false, false, 8, 100,),
        Err(FilesystemError::OutsideAuthority)
    );
    assert_eq!(
        service.move_path("move-source.txt", "escape/moved.txt", false, false, 8, 100,),
        Err(FilesystemError::OutsideAuthority)
    );
    assert_eq!(
        service.delete("escape/outside.txt", false, 8, 100),
        Err(FilesystemError::OutsideAuthority)
    );

    assert!(!outside.join("write.txt").exists());
    assert!(!outside.join("copied.txt").exists());
    assert!(!outside.join("moved.txt").exists());
    assert_eq!(fs::read(outside.join("outside.txt")).unwrap(), b"outside");
    assert!(root.join("move-source.txt").exists());

    fs::remove_dir(&link).unwrap();
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[cfg(windows)]
#[test]
fn cross_volume_move_copies_verifies_then_deletes_the_opened_source() {
    use std::path::{Component, Prefix};

    fn drive(path: &Path) -> Option<u8> {
        match path.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => Some(letter),
                _ => None,
            },
            _ => None,
        }
    }

    let source_root = workspace("cross-volume-source");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let destination_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!(
            "schema43-cross-volume-{}-{nonce}",
            std::process::id()
        ));
    if drive(&source_root) == drive(&destination_root) {
        fs::remove_dir_all(source_root).unwrap();
        return;
    }
    fs::create_dir_all(&destination_root).unwrap();

    let source_file = source_root.join("source.bin");
    let destination_file = destination_root.join("destination.bin");
    let payload = vec![0x5a; MAX_FILESYSTEM_READ_BYTES + 4096];
    fs::write(&source_file, &payload).unwrap();

    let source_directory = source_root.join("tree");
    fs::create_dir(&source_directory).unwrap();
    fs::create_dir(source_directory.join("nested")).unwrap();
    fs::write(source_directory.join("a.txt"), b"alpha").unwrap();
    fs::write(source_directory.join("nested").join("b.txt"), b"beta").unwrap();
    let destination_directory = destination_root.join("tree-moved");

    let service = FilesystemService::broker_administrator();
    service
        .move_path(
            source_file.to_str().unwrap(),
            destination_file.to_str().unwrap(),
            false,
            false,
            8,
            100,
        )
        .unwrap();
    assert!(!source_file.exists());
    assert_eq!(fs::read(&destination_file).unwrap(), payload);

    service
        .move_path(
            source_directory.to_str().unwrap(),
            destination_directory.to_str().unwrap(),
            true,
            false,
            8,
            100,
        )
        .unwrap();
    assert!(!source_directory.exists());
    assert_eq!(
        fs::read(destination_directory.join("a.txt")).unwrap(),
        b"alpha"
    );
    assert_eq!(
        fs::read(destination_directory.join("nested").join("b.txt")).unwrap(),
        b"beta"
    );

    fs::remove_dir_all(source_root).unwrap();
    fs::remove_dir_all(destination_root).unwrap();
}

#[test]
fn cross_volume_move_source_locks_block_concurrent_writers() {
    use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_GENERIC_READ};

    let root = workspace("move-write-lock");
    let source_file = root.join("source.txt");
    fs::write(&source_file, b"stable").unwrap();
    let service = FilesystemService::active_workspace(&root).unwrap();
    let root_lock = service
        .authority
        .open_move_root_validated_handle(&source_file, DELETE | FILE_GENERIC_READ)
        .unwrap();
    assert!(OpenOptions::new().write(true).open(&source_file).is_err());
    drop(root_lock);
    assert!(OpenOptions::new().write(true).open(&source_file).is_ok());

    let tree = root.join("tree");
    fs::create_dir(&tree).unwrap();
    let child = tree.join("child.txt");
    fs::write(&child, b"stable-child").unwrap();
    let mut scanned = 0usize;
    let locks = service
        .lock_cross_volume_move_tree(&tree, 0, 8, 100, &mut scanned)
        .unwrap();
    assert!(OpenOptions::new().write(true).open(&child).is_err());
    let injected = tree.join("new-child.txt");
    let injected_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&injected)
        .unwrap();
    drop(injected_file);
    drop(locks);
    assert!(OpenOptions::new().write(true).open(&child).is_ok());
    fs::remove_file(injected).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn cross_volume_move_delete_preflight_rejects_post_verify_new_child_without_deleting_source() {
    let root = workspace("move-post-verify-child");
    let tree = root.join("tree");
    fs::create_dir(&tree).unwrap();
    fs::write(tree.join("original.txt"), b"original").unwrap();
    let service = FilesystemService::active_workspace(&root).unwrap();
    let mut locked_scanned = 0usize;
    let locks = service
        .lock_cross_volume_move_tree(&tree, 0, 8, 100, &mut locked_scanned)
        .unwrap();
    let expected = tree_manifest(&tree, 8, 100, &service.cancellation).unwrap();
    fs::write(tree.join("injected.txt"), b"injected").unwrap();
    let mut expected_remaining = expected
        .into_iter()
        .map(|entry| (entry.0.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut scanned = 0usize;
    let mut pending = Vec::new();
    assert_eq!(
        service.preflight_directory_delete_exact(
            &tree,
            Path::new(""),
            0,
            8,
            &mut scanned,
            100,
            &mut expected_remaining,
            &mut pending,
        ),
        Err(FilesystemError::FileChanged)
    );
    drop(pending);
    drop(locks);
    assert_eq!(fs::read(tree.join("original.txt")).unwrap(), b"original");
    assert_eq!(fs::read(tree.join("injected.txt")).unwrap(), b"injected");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn workspace_path_guard_keeps_validated_ancestor_from_being_rebound() {
    let root = workspace("broker-workspace-guard");
    let safe = root.join("safe");
    let displaced = root.join("safe-old");
    fs::create_dir(&safe).unwrap();
    fs::write(safe.join("source.txt"), b"inside").unwrap();
    let service = FilesystemService::active_workspace(&root).unwrap();
    let guard = service
        .pin_workspace_path("safe/source.txt", false, false)
        .unwrap();
    assert!(fs::rename(&safe, &displaced).is_err());
    drop(guard);
    fs::rename(&safe, &displaced).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn filesystem_cancelled_before_recursive_copy_never_creates_destination() {
    let root = workspace("cancel-copy-before-start");
    fs::create_dir_all(root.join("source/sub")).unwrap();
    fs::write(root.join("source/sub/data.txt"), b"payload").unwrap();
    let cancellation = FilesystemCancellation::default();
    cancellation.cancel();
    let service = FilesystemService::active_workspace(&root)
        .unwrap()
        .with_cancellation(cancellation);
    assert_eq!(
        service.copy("source", "destination", true, false, 8, 100),
        Err(FilesystemError::Cancelled)
    );
    assert!(!root.join("destination").exists());
    assert_eq!(
        fs::read(root.join("source/sub/data.txt")).unwrap(),
        b"payload"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn filesystem_midstream_copy_cancellation_stops_on_next_chunk() {
    struct CancellingReader {
        chunks: usize,
        cancellation: FilesystemCancellation,
    }
    impl Read for CancellingReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.chunks == 0 {
                return Ok(0);
            }
            let len = buffer.len().min(64 * 1024);
            buffer[..len].fill(b'x');
            self.chunks -= 1;
            if self.chunks == 2 {
                self.cancellation.cancel();
            }
            Ok(len)
        }
    }
    let cancellation = FilesystemCancellation::default();
    let mut reader = CancellingReader {
        chunks: 4,
        cancellation: cancellation.clone(),
    };
    let mut output = Vec::new();
    assert_eq!(
        copy_with_cancellation(&mut reader, &mut output, &cancellation),
        Err(FilesystemError::Cancelled)
    );
    assert_eq!(output.len(), 2 * 64 * 1024);
}
